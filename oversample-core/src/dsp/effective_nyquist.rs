//! Detect the *effective* Nyquist frequency of a recording — the
//! highest frequency at which the file actually carries signal energy.
//!
//! Catches:
//! - **Upsampled** files: the spectrum is essentially silent above the
//!   original Nyquist (interpolation noise floor only).
//! - **Aggressive anti-aliasing** in the recorder: the spectrum rolls
//!   off well before the file's claimed Nyquist.
//!
//! Doesn't catch oversampled-then-decimated recordings (the output
//! spectrum is fully populated up to the output Nyquist).

use crate::dsp::psd::{self, PsdResult};

#[derive(Clone, Debug, PartialEq)]
pub enum EffectiveNyquistVerdict {
    /// Spectrum is populated all the way to the file's Nyquist.
    /// `claimed_nyquist_hz` is `sample_rate / 2`.
    FullBandwidth { claimed_nyquist_hz: f64 },
    /// Spectrum drops off well below the claimed Nyquist. `effective_hz`
    /// is the highest frequency with meaningful energy.
    BandLimited {
        claimed_nyquist_hz: f64,
        effective_hz: f64,
        /// Drop-off ratio: effective / claimed. < 0.95 triggers this verdict.
        ratio: f64,
    },
    /// Recording too short / format not handled.
    NotApplicable,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EffectiveNyquistResult {
    pub verdict: EffectiveNyquistVerdict,
    pub explanation: String,
    pub claimed_nyquist_hz: f64,
    pub effective_hz: f64,
    /// In-band (low-frequency) noise floor in dB.
    pub low_band_floor_db: f64,
    /// Above-effective-Nyquist noise floor in dB.
    pub upper_band_floor_db: f64,
    /// Drop-off in dB between low-band and upper-band floors.
    pub drop_db: f64,
}

/// Number of equal-width bands the spectrum is divided into for the
/// per-band peak scan.
const N_BANDS: usize = 32;
/// A band is considered "populated" if its peak power is within this
/// many dB of the loudest band's peak.
const POPULATED_GAP_DB: f64 = 50.0;
/// Search NFFT for the PSD.
const NFFT: usize = 4096;
/// Treat anything below this ratio as band-limited.
const BAND_LIMITED_RATIO: f64 = 0.95;
/// Below this absolute PSD level (dB) we treat the band as "essentially
/// digital silence" — only numerical noise from float arithmetic, not
/// real ADC/thermal noise. The threshold matches typical float32 noise
/// floors after FFT and Hann windowing.
const PRECISION_FLOOR_DB: f64 = -100.0;

pub fn detect(samples: &[f32], sample_rate: u32) -> EffectiveNyquistResult {
    let claimed_nyquist = sample_rate as f64 / 2.0;
    let default = EffectiveNyquistResult {
        verdict: EffectiveNyquistVerdict::NotApplicable,
        explanation: String::new(),
        claimed_nyquist_hz: claimed_nyquist,
        effective_hz: claimed_nyquist,
        low_band_floor_db: f64::NAN,
        upper_band_floor_db: f64::NAN,
        drop_db: 0.0,
    };

    if samples.len() < NFFT * 4 {
        return EffectiveNyquistResult {
            explanation: "Recording too short for effective-Nyquist analysis".into(),
            ..default
        };
    }

    let psd: PsdResult = psd::compute_psd(samples, sample_rate, NFFT, None);
    let n_bins = psd.power_db.len(); // = NFFT/2 + 1
    if n_bins < N_BANDS * 2 {
        return EffectiveNyquistResult {
            explanation: "PSD bin count too low".into(),
            ..default
        };
    }

    // Divide the spectrum (excluding DC) into N_BANDS equal-width bands
    // and find the MAX power in each. We look at peaks, not noise floor,
    // because pure-tone signals can have arbitrarily low floors.
    let band_size = n_bins / N_BANDS;
    let mut band_peaks: Vec<f64> = Vec::with_capacity(N_BANDS);
    for b in 0..N_BANDS {
        let lo = (b * band_size).max(1);
        let hi = ((b + 1) * band_size).min(n_bins);
        let peak = psd.power_db[lo..hi]
            .iter()
            .cloned()
            .fold(f64::NEG_INFINITY, f64::max);
        band_peaks.push(peak);
    }

    // Global peak across all bands.
    let global_peak = band_peaks.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let populated_threshold = global_peak - POPULATED_GAP_DB;

    // Walk down from the top band, find the highest "populated" one.
    let effective_band = (0..N_BANDS)
        .rev()
        .find(|&b| band_peaks[b] > populated_threshold)
        .unwrap_or(0);
    let effective_hz = ((effective_band + 1) * band_size) as f64 * psd.freq_resolution;
    let ratio = (effective_hz / claimed_nyquist).min(1.0);

    // For diagnostics: median-of-band-peaks in lower vs upper halves.
    let split = effective_band + 1;
    let low_band_floor = if split > 0 {
        let mut lower = band_peaks[..split].to_vec();
        lower.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        lower[lower.len() / 2]
    } else {
        f64::NAN
    };
    let upper_band_floor = if split < N_BANDS {
        let mut upper = band_peaks[split..].to_vec();
        upper.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        upper[upper.len() / 2]
    } else {
        low_band_floor
    };
    let drop_db = low_band_floor - upper_band_floor;

    // Only flag as "band-limited" (suggesting upsampling / hardware bandwidth
    // limit) when the upper-band peaks are near the precision floor — that's
    // what distinguishes a digital cutoff from natural content bandlimiting
    // (e.g. bat calls don't extend up to Nyquist, but the noise still does).
    let upper_band_at_precision_floor = upper_band_floor < PRECISION_FLOOR_DB;
    let (verdict, explanation) = if ratio < BAND_LIMITED_RATIO && upper_band_at_precision_floor {
        (
            EffectiveNyquistVerdict::BandLimited {
                claimed_nyquist_hz: claimed_nyquist,
                effective_hz,
                ratio,
            },
            format!(
                "Spectrum cuts off at {:.1} kHz ({:.0}% of the claimed Nyquist of \
                 {:.1} kHz). Above that the spectrum is essentially digital \
                 silence ({:.0} dB \u{2014} only numerical noise from float \
                 arithmetic) \u{2014} the recording was almost certainly \
                 upsampled from a lower-rate source, or has hardware bandwidth \
                 limiting.",
                effective_hz / 1000.0,
                ratio * 100.0,
                claimed_nyquist / 1000.0,
                upper_band_floor,
            ),
        )
    } else {
        (
            EffectiveNyquistVerdict::FullBandwidth {
                claimed_nyquist_hz: claimed_nyquist,
            },
            format!(
                "Spectrum has content / noise floor up to within {:.0}% of the \
                 claimed Nyquist of {:.1} kHz (top-band median {:.0} dB).",
                ratio * 100.0,
                claimed_nyquist / 1000.0,
                upper_band_floor,
            ),
        )
    };
    let _ = drop_db; // kept in struct for diagnostic display

    EffectiveNyquistResult {
        verdict,
        explanation,
        claimed_nyquist_hz: claimed_nyquist,
        effective_hz,
        low_band_floor_db: low_band_floor,
        upper_band_floor_db: upper_band_floor,
        drop_db,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    #[test]
    fn detects_upsampled_signal() {
        // A 96 kHz signal upsampled to 384 kHz: only frequencies up to
        // ~48 kHz have content; the rest is interpolation silence.
        let fs = 384_000u32;
        let n = 16_384;
        let signal: Vec<f32> = (0..n)
            .map(|i| {
                let t = i as f32 / fs as f32;
                // Several tones, all below 48 kHz
                (2.0 * PI * 10_000.0 * t).sin() * 0.3
                    + (2.0 * PI * 25_000.0 * t).sin() * 0.2
                    + (2.0 * PI * 42_000.0 * t).sin() * 0.1
            })
            .collect();
        let r = detect(&signal, fs);
        assert!(
            matches!(r.verdict, EffectiveNyquistVerdict::BandLimited { .. }),
            "expected BandLimited, got {:?}",
            r,
        );
        // Effective Nyquist should be near 42 kHz (the highest tone)
        assert!(
            r.effective_hz < 60_000.0,
            "effective_hz = {}",
            r.effective_hz
        );
    }

    #[test]
    fn accepts_full_bandwidth_white_noise() {
        let fs = 384_000u32;
        let n = 16_384;
        let mut state: u32 = 0x42;
        let signal: Vec<f32> = (0..n)
            .map(|_| {
                state = state.wrapping_mul(1664525).wrapping_add(1013904223);
                ((state >> 16) as i32 - 32768) as f32 / 32768.0 * 0.3
            })
            .collect();
        let r = detect(&signal, fs);
        assert!(
            matches!(r.verdict, EffectiveNyquistVerdict::FullBandwidth { .. }),
            "expected FullBandwidth on white noise, got {:?}",
            r,
        );
    }
}
