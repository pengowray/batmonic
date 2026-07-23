//! Power Spectral Density estimation using Welch's method.
//!
//! Computes averaged periodograms over overlapping Hann-windowed segments,
//! with peak detection and bandwidth analysis (-6 dB, -10 dB).

use crate::dsp::fft::{hann_window, plan_fft_forward};

/// Hann window power correction factor: sum of squared window values.
fn hann_power_sum(size: usize) -> f64 {
    let w = hann_window(size);
    w.iter().map(|&v| (v as f64) * (v as f64)).sum()
}

// ── Result types ────────────────────────────────────────────────────────────

/// Result of a PSD computation.
#[derive(Clone, Debug)]
pub struct PsdResult {
    /// Power spectral density in dB per bin (length = nfft/2 + 1).
    /// Bin 0 = DC, bin N = Nyquist.
    pub power_db: Vec<f64>,
    /// Frequency resolution in Hz per bin.
    pub freq_resolution: f64,
    /// Sample rate of the source audio.
    pub sample_rate: u32,
    /// NFFT size used.
    pub nfft: usize,
    /// Number of frames averaged.
    pub frame_count: usize,
    /// All detected peaks, sorted by power (strongest first).
    pub peaks: Vec<PsdPeak>,
    /// Optional frequency range used for peak detection (Hz).
    pub peak_freq_range: Option<(f64, f64)>,
}

/// Peak frequency and bandwidth analysis from a PSD.
#[derive(Clone, Debug)]
pub struct PsdPeak {
    /// Peak frequency in Hz.
    pub freq_hz: f64,
    /// Peak power in dB.
    pub power_db: f64,
    /// Bin index of the peak.
    pub bin_index: usize,
    /// -6 dB bandwidth: (low_hz, high_hz). None if the peak doesn't drop 6 dB.
    pub bw_6db: Option<(f64, f64)>,
    /// -10 dB bandwidth: (low_hz, high_hz). None if the peak doesn't drop 10 dB.
    pub bw_10db: Option<(f64, f64)>,
}

// ── Computation ─────────────────────────────────────────────────────────────

/// Compute PSD using Welch's method (synchronous).
///
/// - `samples`: mono f32 audio
/// - `sample_rate`: Hz
/// - `nfft`: FFT size (e.g. 256, 512, 1024, 2048, 4096)
///
/// Uses 50% overlap and Hann window.
pub fn compute_psd(
    samples: &[f32],
    sample_rate: u32,
    nfft: usize,
    peak_freq_range: Option<(f64, f64)>,
) -> PsdResult {
    let n_bins = nfft / 2 + 1;
    let hop = nfft / 2;
    let window = hann_window(nfft);
    let power_norm = hann_power_sum(nfft) * sample_rate as f64;

    let fft = plan_fft_forward(nfft);
    let mut input = fft.make_input_vec();
    let mut spectrum = fft.make_output_vec();

    let mut accum = vec![0.0f64; n_bins];
    let mut frame_count = 0usize;

    let mut pos = 0usize;
    while pos + nfft <= samples.len() {
        let frame = &samples[pos..pos + nfft];

        // Apply window
        for (inp, (&s, &w)) in input.iter_mut().zip(frame.iter().zip(window.iter())) {
            *inp = s * w;
        }

        // FFT
        fft.process(&mut input, &mut spectrum).expect("FFT failed");

        // Accumulate |X[k]|²
        for (acc, c) in accum.iter_mut().zip(spectrum.iter()) {
            *acc += (c.re as f64) * (c.re as f64) + (c.im as f64) * (c.im as f64);
        }

        frame_count += 1;
        pos += hop;
    }

    // Average and normalize to PSD, convert to dB
    let power_db: Vec<f64> = if frame_count > 0 {
        accum
            .iter()
            .enumerate()
            .map(|(i, &sum)| {
                let mut psd = sum / (frame_count as f64 * power_norm);
                // Double non-DC, non-Nyquist bins (one-sided spectrum)
                if i > 0 && i < n_bins - 1 {
                    psd *= 2.0;
                }
                if psd > 0.0 {
                    10.0 * psd.log10()
                } else {
                    -200.0
                }
            })
            .collect()
    } else {
        vec![-200.0; n_bins]
    };

    let freq_resolution = sample_rate as f64 / nfft as f64;
    let peaks = find_peaks(&power_db, freq_resolution, peak_freq_range);

    PsdResult {
        power_db,
        freq_resolution,
        sample_rate,
        nfft,
        frame_count,
        peaks,
        peak_freq_range,
    }
}

/// Async version that yields periodically via a caller-supplied future.
///
/// `yield_now` is called every 64 frames to let the caller yield to the browser
/// (or do nothing in non-wasm contexts). `is_cancelled` is checked after each
/// yield; return `true` to abort early.
pub async fn compute_psd_async<F, Fut>(
    samples: &[f32],
    sample_rate: u32,
    nfft: usize,
    peak_freq_range: Option<(f64, f64)>,
    yield_now: F,
    is_cancelled: &dyn Fn() -> bool,
) -> Option<PsdResult>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let n_bins = nfft / 2 + 1;
    let hop = nfft / 2;
    let window = hann_window(nfft);
    let power_norm = hann_power_sum(nfft) * sample_rate as f64;

    let fft = plan_fft_forward(nfft);
    let mut input = fft.make_input_vec();
    let mut spectrum = fft.make_output_vec();

    let mut accum = vec![0.0f64; n_bins];
    let mut frame_count = 0usize;

    let yield_interval = 64;
    let mut pos = 0usize;
    while pos + nfft <= samples.len() {
        let frame = &samples[pos..pos + nfft];

        for (inp, (&s, &w)) in input.iter_mut().zip(frame.iter().zip(window.iter())) {
            *inp = s * w;
        }
        fft.process(&mut input, &mut spectrum).expect("FFT failed");

        for (acc, c) in accum.iter_mut().zip(spectrum.iter()) {
            *acc += (c.re as f64) * (c.re as f64) + (c.im as f64) * (c.im as f64);
        }

        frame_count += 1;
        pos += hop;

        if frame_count.is_multiple_of(yield_interval) {
            yield_now().await;

            if is_cancelled() {
                return None;
            }
        }
    }

    let power_db: Vec<f64> = if frame_count > 0 {
        accum
            .iter()
            .enumerate()
            .map(|(i, &sum)| {
                let mut psd = sum / (frame_count as f64 * power_norm);
                if i > 0 && i < n_bins - 1 {
                    psd *= 2.0;
                }
                if psd > 0.0 {
                    10.0 * psd.log10()
                } else {
                    -200.0
                }
            })
            .collect()
    } else {
        vec![-200.0; n_bins]
    };

    let freq_resolution = sample_rate as f64 / nfft as f64;
    let peaks = find_peaks(&power_db, freq_resolution, peak_freq_range);

    Some(PsdResult {
        power_db,
        freq_resolution,
        sample_rate,
        nfft,
        frame_count,
        peaks,
        peak_freq_range,
    })
}

// ── Peak detection ──────────────────────────────────────────────────────────

/// Maximum number of peaks to return.
const MAX_PEAKS: usize = 8;

/// Minimum prominence (dB) a local maximum must have relative to the valleys
/// on either side to be counted as a peak.
const MIN_PROMINENCE_DB: f64 = 3.0;

/// Find all significant local maxima in the PSD, sorted by power (strongest first).
/// If `freq_range` is Some, only bins within that Hz range are considered for peaks.
fn find_peaks(
    power_db: &[f64],
    freq_resolution: f64,
    freq_range: Option<(f64, f64)>,
) -> Vec<PsdPeak> {
    if power_db.len() < 3 {
        return Vec::new();
    }

    let n = power_db.len();

    // Compute bin range from frequency range
    let (min_bin, max_bin) = if let Some((lo, hi)) = freq_range {
        let lo_bin = ((lo / freq_resolution).floor() as usize).max(2);
        let hi_bin = ((hi / freq_resolution).ceil() as usize).min(n - 2);
        (lo_bin, hi_bin)
    } else {
        (2, n - 2)
    };

    // Find local maxima (bins where power[i] > both neighbours)
    let mut candidates: Vec<(usize, f64)> = Vec::new();
    for i in min_bin..=max_bin {
        if power_db[i] > power_db[i - 1] && power_db[i] >= power_db[i + 1] {
            candidates.push((i, power_db[i]));
        }
    }

    if candidates.is_empty() {
        // Fallback: global max within range
        let (peak_bin, &peak_power) = power_db[min_bin..=max_bin]
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(i, v)| (i + min_bin, v))
            .unwrap_or((0, &-200.0));
        if peak_bin > 0 {
            candidates.push((peak_bin, peak_power));
        }
    }

    // Filter by prominence: for each candidate, walk left and right to find
    // the lowest valley before reaching a higher peak.
    let mut peaks: Vec<(usize, f64, f64)> = Vec::new(); // (bin, power, prominence)
    for &(bin, power) in &candidates {
        // Walk left to find minimum before a higher peak
        let mut left_min = power;
        for j in (1..bin).rev() {
            left_min = left_min.min(power_db[j]);
            if power_db[j] > power {
                break;
            }
        }
        // Walk right
        let mut right_min = power;
        for &val in &power_db[(bin + 1)..n] {
            right_min = right_min.min(val);
            if val > power {
                break;
            }
        }
        let prominence = power - left_min.max(right_min);
        if prominence >= MIN_PROMINENCE_DB {
            peaks.push((bin, power, prominence));
        }
    }

    // If no peaks passed prominence filter, use the single strongest candidate
    if peaks.is_empty() {
        if let Some(&(bin, power)) = candidates
            .iter()
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
        {
            peaks.push((bin, power, 0.0));
        }
    }

    // Sort by power descending, take top MAX_PEAKS
    peaks.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    peaks.truncate(MAX_PEAKS);

    peaks
        .iter()
        .map(|&(bin, power, _)| {
            let freq_hz = bin as f64 * freq_resolution;
            let bw_6db = find_bandwidth(power_db, bin, power, 6.0, freq_resolution);
            let bw_10db = find_bandwidth(power_db, bin, power, 10.0, freq_resolution);
            PsdPeak {
                freq_hz,
                power_db: power,
                bin_index: bin,
                bw_6db,
                bw_10db,
            }
        })
        .collect()
}

/// Find bandwidth at `drop_db` below peak using linear interpolation.
fn find_bandwidth(
    power_db: &[f64],
    peak_bin: usize,
    peak_power: f64,
    drop_db: f64,
    freq_resolution: f64,
) -> Option<(f64, f64)> {
    let threshold = peak_power - drop_db;

    // Walk left from peak
    let low_freq = {
        let mut low_bin = None;
        for i in (1..peak_bin).rev() {
            if power_db[i] < threshold {
                // Interpolate between bin i and i+1
                let frac = if (power_db[i + 1] - power_db[i]).abs() > 1e-12 {
                    (threshold - power_db[i]) / (power_db[i + 1] - power_db[i])
                } else {
                    0.5
                };
                low_bin = Some((i as f64 + frac) * freq_resolution);
                break;
            }
        }
        low_bin
    };

    // Walk right from peak
    let high_freq = {
        let mut high_bin = None;
        for i in (peak_bin + 1)..power_db.len() {
            if power_db[i] < threshold {
                // Interpolate between bin i-1 and i
                let frac = if (power_db[i - 1] - power_db[i]).abs() > 1e-12 {
                    (threshold - power_db[i - 1]) / (power_db[i] - power_db[i - 1])
                } else {
                    0.5
                };
                high_bin = Some(((i - 1) as f64 + frac) * freq_resolution);
                break;
            }
        }
        high_bin
    };

    match (low_freq, high_freq) {
        (Some(lo), Some(hi)) => Some((lo, hi)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine_wave(freq_hz: f64, sample_rate: u32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| {
                (2.0 * std::f64::consts::PI * freq_hz * (i as f64) / sample_rate as f64).sin()
                    as f32
            })
            .collect()
    }

    #[test]
    fn peak_landed_on_pure_tone() {
        let sr = 48_000u32;
        let nfft = 4096;
        let tone_hz = 4_000.0;
        let samples = sine_wave(tone_hz, sr, sr as usize); // 1 second
        let result = compute_psd(&samples, sr, nfft, None);

        assert_eq!(result.power_db.len(), nfft / 2 + 1);
        assert_eq!(result.sample_rate, sr);
        assert_eq!(result.nfft, nfft);
        assert!(result.frame_count > 0);
        assert!(
            !result.peaks.is_empty(),
            "expected at least one detected peak"
        );

        // The strongest peak should be within one bin of the true tone frequency.
        let top = &result.peaks[0];
        let bin_hz = sr as f64 / nfft as f64;
        assert!(
            (top.freq_hz - tone_hz).abs() < bin_hz,
            "expected peak near {tone_hz} Hz, got {} Hz (bin width {})",
            top.freq_hz,
            bin_hz,
        );
    }

    #[test]
    fn short_input_returns_empty_frames() {
        // nfft larger than the input → no full frame, zero frames averaged.
        let result = compute_psd(&[0.1f32; 64], 44_100, 1024, None);
        assert_eq!(result.frame_count, 0);
        // Every bin should be the -200 dB "no data" sentinel.
        assert!(result.power_db.iter().all(|&db| db <= -199.0));
    }

    #[test]
    fn peak_search_respects_freq_range() {
        let sr = 48_000u32;
        let nfft = 4096;
        // Mix of 4 kHz and 12 kHz tones at equal amplitude.
        let s4: Vec<f32> = sine_wave(4_000.0, sr, sr as usize);
        let s12: Vec<f32> = sine_wave(12_000.0, sr, sr as usize);
        let samples: Vec<f32> = s4
            .iter()
            .zip(s12.iter())
            .map(|(a, b)| (a + b) * 0.5)
            .collect();

        // Restrict to 10-14 kHz — the 12 kHz tone should dominate the peak list.
        let result = compute_psd(&samples, sr, nfft, Some((10_000.0, 14_000.0)));
        let top = result.peaks.first().expect("peak in 10-14 kHz band");
        assert!(top.freq_hz > 10_000.0 && top.freq_hz < 14_000.0);
    }
}
