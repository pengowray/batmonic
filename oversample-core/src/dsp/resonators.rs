// SPDX-License-Identifier: GPL-3.0-only OR MIT OR Apache-2.0
//! Thin adapter over the [`resonators`] crate — Alexandre François's Resonate
//! algorithm.
//!
//! The upstream crate implements the paper faithfully; this module only
//! reshapes its output into the project's [`SpectrogramColumn`] layout and
//! scales magnitudes to match STFT brightness so existing gain / floor_db
//! controls behave identically in Spectrogram and Resonators views.
//!
//! # Layout
//!
//! For compatibility with the existing spectrogram pipeline, we build a
//! linear-frequency bank of `num_bins = fft_size / 2 + 1` resonators covering
//! 0..Nyquist with `f_k = k · (sr/2) / (num_bins - 1)`. Downstream code
//! (row→freq mapping, tile blit, freq markers) needs no special cases.
//!
//! # References
//!
//! - Algorithm: <https://alexandrefrancois.org/Resonate/>
//! - C++ reference: <https://github.com/alexandrefrancois/noFFT>
//! - Rust reference (this crate): <https://github.com/jhartquist/resonators>

use crate::types::SpectrogramColumn;
use resonators::{ResonatorBank, ResonatorConfig, alpha_from_tau};

/// Frequency-bin spacing for a resonator bank.
///
/// Output always has `fft_size/2 + 1` rows (matching STFT so the rest of the
/// rendering pipeline stays linear); this enum only affects where the actual
/// resonators sit in frequency space. `Log` bins are resampled to the linear
/// output rows by nearest-bin-in-log-space mapping, which draws bat harmonics
/// as clean stripes while keeping the axis / overlay code unchanged.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum ResonatorLayout {
    /// Evenly-spaced linear bins from 0 to Nyquist — same layout as the STFT.
    #[default]
    Linear,
    /// Log-spaced bins from `LOG_MIN_FREQ_HZ` to Nyquist. Gives more detail
    /// at low frequencies and concentrates bins where harmonic bat calls
    /// actually live.
    Log,
}

impl ResonatorLayout {
    pub fn label(self) -> &'static str {
        match self {
            Self::Linear => "Linear",
            Self::Log => "Log",
        }
    }

    pub const ALL: &'static [ResonatorLayout] = &[Self::Linear, Self::Log];
}

/// Lowest frequency for log-spaced layouts. Below this the display shows the
/// lowest log bin's magnitude (no subsonic resonators).
pub const LOG_MIN_FREQ_HZ: f32 = 20.0;

/// Recommended warm-up samples for a given bandwidth.
///
/// Returns ≈5τ samples, where τ = 1/(2π·bandwidth) is the EMA time constant.
/// At 5τ the EMA has converged to within ~1% of steady state.
pub fn warmup_samples(sample_rate: u32, bandwidth_hz: f32) -> usize {
    let bw = bandwidth_hz.max(1.0);
    let tau_secs = 1.0 / (std::f32::consts::TAU * bw);
    (5.0 * tau_secs * sample_rate as f32).ceil().max(256.0) as usize
}

/// Compute resonator columns over a slice of audio samples.
///
/// Parameters mirror `dsp::fft::compute_stft_columns`:
/// - `fft_size` determines `num_bins = fft_size/2 + 1` (frequency resolution).
/// - `hop_size` is the output column interval in samples.
/// - `col_start`/`col_count` select which columns to emit (0-based, counted
///   from sample 0 of the input slice). A fresh bank is built per call, so
///   the caller should pre-pad with warm-up samples and pass `col_start` =
///   the warm-up column count.
///
/// `bandwidth_hz` sets per-bin EMA bandwidth (uniform across all bins).
/// Smaller ⇒ sharper bins, slower tracking.
///
/// Output magnitudes are scaled by `fft_size * 0.5` to match the one-sided
/// STFT magnitude with Hann coherent gain, so existing brightness controls
/// work the same way in both views.
pub fn compute_resonator_columns(
    samples: &[f32],
    sample_rate: u32,
    fft_size: usize,
    hop_size: usize,
    col_start: usize,
    col_count: usize,
    bandwidth_hz: f32,
    layout: ResonatorLayout,
) -> Vec<SpectrogramColumn> {
    let output_bins = fft_size / 2 + 1;
    if samples.is_empty() || output_bins == 0 || col_count == 0 || hop_size == 0 {
        return vec![];
    }

    let sr_f = sample_rate as f32;
    let nyq = sr_f * 0.5;

    // Clamp bandwidth and convert to the library's alpha convention via tau.
    // `alpha_from_tau(tau, sr) = 1 - exp(-dt/tau)` — the library's
    // "alpha large = fast response" is the mirror of our prior scalar
    // impl's "alpha large = slow"; this conversion hides that from callers.
    let bw = bandwidth_hz.clamp(0.1, nyq * 0.99);
    let tau = 1.0 / (std::f32::consts::TAU * bw);
    let alpha = alpha_from_tau(tau, sr_f);

    // Build the resonator frequency list per chosen layout. Bin count equals
    // output_bins so log and linear have comparable compute cost and tile
    // detail — Log just distributes the same bins across a perceptually
    // useful axis.
    let bank_freqs: Vec<f32> = match layout {
        ResonatorLayout::Linear => {
            let denom = (output_bins - 1).max(1) as f32;
            (0..output_bins)
                .map(|k| (k as f32 * nyq / denom).max(0.01))
                .collect()
        }
        ResonatorLayout::Log => {
            let min = LOG_MIN_FREQ_HZ.max(0.01);
            let max = nyq.max(min * 2.0);
            if output_bins == 1 {
                vec![min]
            } else {
                let ratio = (max / min).powf(1.0 / (output_bins - 1) as f32);
                (0..output_bins)
                    .map(|k| min * ratio.powi(k as i32))
                    .collect()
            }
        }
    };
    let bank_bins = bank_freqs.len();

    // beta=1.0 disables the library's second-stage output EWMA so we get a
    // single-EWMA response matching the prior hand-rolled implementation,
    // which is what the bandwidth slider has been tuned against.
    let configs: Vec<ResonatorConfig> = bank_freqs
        .iter()
        .map(|&f| ResonatorConfig::new(f, alpha, 1.0))
        .collect();
    let mut bank = ResonatorBank::new(&configs, sr_f);

    // For Log layout, pre-compute a bank-bin index for each linear output
    // row so the per-frame loop is a cheap gather. For Linear the mapping
    // is the identity (bank_bins == output_bins).
    let row_to_bank: Option<Vec<usize>> = match layout {
        ResonatorLayout::Linear => None,
        ResonatorLayout::Log => Some(build_log_row_map(output_bins, &bank_freqs, nyq)),
    };

    let mag_scale = (fft_size as f32) * 0.5;
    let col_end = col_start + col_count;
    let total_samples = samples.len();
    let mut out: Vec<SpectrogramColumn> = Vec::with_capacity(col_count);

    // Stream hop-by-hop instead of calling `bank.resonate()` — that method
    // allocates a `Vec<Complex32>` the size of (n_frames * n_bins) up front
    // (~1 MB per baseline tile) which we'd then discard. Processing one hop
    // at a time and reading magnitudes directly from bank state avoids the
    // intermediate buffer entirely.
    let mut pos = 0usize;
    for frame in 0..col_end {
        let next = pos + hop_size;
        if next > total_samples {
            break;
        }
        bank.process_samples(&samples[pos..next]);
        pos = next;

        if frame < col_start {
            continue;
        }

        let mags: Vec<f32> = if let Some(map) = &row_to_bank {
            map.iter()
                .map(|&k| bank.magnitude(k) * mag_scale)
                .collect()
        } else {
            (0..bank_bins)
                .map(|k| bank.magnitude(k) * mag_scale)
                .collect()
        };

        // Library state reflects end of this hop.
        let time_offset = ((frame + 1) * hop_size) as f64 / sample_rate as f64;
        out.push(SpectrogramColumn { magnitudes: mags, time_offset });
    }

    out
}

/// For each linear output row (covering 0..Nyquist uniformly), pick the
/// closest bank bin in log-frequency distance. Rows below the lowest bank
/// frequency use the lowest bank bin.
fn build_log_row_map(output_bins: usize, bank_freqs: &[f32], nyq: f32) -> Vec<usize> {
    let denom = (output_bins - 1).max(1) as f32;
    let last_bank = bank_freqs.len() - 1;
    (0..output_bins)
        .map(|row| {
            let row_freq = (row as f32 * nyq / denom).max(0.01);
            if row_freq <= bank_freqs[0] {
                return 0;
            }
            if row_freq >= bank_freqs[last_bank] {
                return last_bank;
            }
            // Binary search for the first bank freq >= row_freq, then pick
            // whichever of idx-1 and idx is closer in log space.
            let idx = bank_freqs
                .binary_search_by(|&f| {
                    f.partial_cmp(&row_freq).unwrap_or(std::cmp::Ordering::Equal)
                })
                .unwrap_or_else(|i| i)
                .min(last_bank);
            if idx == 0 {
                return 0;
            }
            let prev = bank_freqs[idx - 1];
            let curr = bank_freqs[idx];
            let dprev = (row_freq / prev).ln().abs();
            let dcurr = (row_freq / curr).ln().abs();
            if dprev <= dcurr { idx - 1 } else { idx }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A pure tone should produce a peak at the matching bin.
    #[test]
    fn peak_at_tone_frequency() {
        let sr = 48_000u32;
        let fft_size = 256;
        let hop = 64;
        let num_bins = fft_size / 2 + 1;

        // 6 kHz sine, 1 s long.
        let f = 6_000.0f32;
        let samples: Vec<f32> = (0..sr as usize)
            .map(|i| (std::f32::consts::TAU * f * i as f32 / sr as f32).sin())
            .collect();

        let cols = compute_resonator_columns(
            &samples, sr, fft_size, hop, 0, 100, 200.0, ResonatorLayout::Linear,
        );
        assert!(!cols.is_empty());

        // Look at a column well past warm-up.
        let mid = &cols[cols.len() - 1];
        let (peak_bin, _peak_val) = mid
            .magnitudes
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .unwrap();

        let nyq = (sr as f32) / 2.0;
        let expected = (f / (nyq / (num_bins - 1) as f32)).round() as usize;
        assert!(
            (peak_bin as isize - expected as isize).abs() <= 1,
            "peak at bin {peak_bin}, expected {expected}"
        );
    }
}
