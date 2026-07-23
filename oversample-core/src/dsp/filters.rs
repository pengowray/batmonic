use crate::dsp::fft::{hann_window, plan_fft_forward, plan_fft_inverse};

/// How the multi-band EQ splits the spectrum, relative to the selected
/// `[freq_low, freq_high]` range. The boolean accessors replace the magic
/// `band_mode {<=2, >=3, >=4}` comparisons that were previously duplicated
/// across the filter functions and `harmonics_band_bounds`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum BandMode {
    /// Below + selected only — everything at/above `freq_low` uses the selected gain.
    TwoBand,
    /// Below + selected + above.
    #[default]
    ThreeBand,
    /// Below + selected + harmonics + above.
    FourBand,
}

impl BandMode {
    /// Whether the spectrum above `freq_high` gets its own "above" gain band
    /// rather than being folded into the selected band. (Was `band_mode >= 3`.)
    pub fn has_above_band(self) -> bool {
        !matches!(self, BandMode::TwoBand)
    }

    /// Whether a distinct harmonics band exists between the selected range and
    /// twice `freq_high`. (Was `band_mode >= 4`.)
    pub fn has_harmonics(self) -> bool {
        matches!(self, BandMode::FourBand)
    }
}

pub fn harmonics_band_bounds(
    freq_low: f64,
    freq_high: f64,
    band_mode: BandMode,
) -> Option<(f64, f64)> {
    if !band_mode.has_harmonics() || freq_high <= 0.0 || freq_low >= freq_high {
        return None;
    }

    let band_low = (freq_low * 2.0).max(freq_high);
    let band_high = freq_high * 2.0;
    if band_low < band_high {
        Some((band_low, band_high))
    } else {
        None
    }
}

/// Apply a multi-band EQ filter in the frequency domain using overlap-add FFT processing.
///
/// Bands are defined relative to the "selected" frequency range [freq_low, freq_high]:
/// - Below: 0 to freq_low
/// - Selected: freq_low to freq_high
/// - Harmonics: max(freq_high, freq_low*2) to freq_high*2 (only `BandMode::FourBand`)
/// - Above: everything above (`BandMode::ThreeBand` and up)
///
/// In 2-band mode (`BandMode::TwoBand`), everything at or above freq_low uses db_selected.
pub fn apply_eq_filter(
    samples: &[f32],
    sample_rate: u32,
    freq_low: f64,
    freq_high: f64,
    db_below: f64,
    db_selected: f64,
    db_harmonics: f64,
    db_above: f64,
    band_mode: BandMode,
) -> Vec<f32> {
    if samples.is_empty() {
        return Vec::new();
    }

    let fft_size = 4096;
    let hop_size = fft_size / 2;
    let len = samples.len();

    let window = hann_window(fft_size);

    // Build per-bin gain table
    let num_bins = fft_size / 2 + 1;
    let freq_per_bin = sample_rate as f64 / fft_size as f64;
    let harmonics_bounds = harmonics_band_bounds(freq_low, freq_high, band_mode);

    let gains: Vec<f32> = (0..num_bins)
        .map(|i| {
            let freq = i as f64 * freq_per_bin;
            let db = if freq < freq_low {
                db_below
            } else if freq <= freq_high {
                db_selected
            } else if !band_mode.has_above_band() {
                // 2-band: everything above uses selected
                db_selected
            } else if let Some((harmonics_lower, harmonics_upper)) = harmonics_bounds {
                if freq >= harmonics_lower && freq <= harmonics_upper {
                    db_harmonics
                } else {
                    db_above
                }
            } else {
                db_above
            };
            10.0_f64.powf(db / 20.0) as f32
        })
        .collect();

    let (fft_fwd, fft_inv) = (plan_fft_forward(fft_size), plan_fft_inverse(fft_size));

    // Overlap-add output buffer
    let mut output = vec![0.0f32; len];
    let mut window_sum = vec![0.0f32; len];

    // Pre-allocate FFT buffers once and reuse across frames
    let mut frame = fft_fwd.make_input_vec();
    let mut spectrum = fft_fwd.make_output_vec();
    let mut time_out = fft_inv.make_output_vec();

    let mut pos = 0;
    while pos < len {
        // Zero and fill frame in-place (no allocation)
        frame.fill(0.0);
        for (i, &w) in window.iter().enumerate() {
            if pos + i < len {
                frame[i] = samples[pos + i] * w;
            }
        }

        // Forward FFT
        fft_fwd
            .process(&mut frame, &mut spectrum)
            .expect("FFT forward failed");

        // Apply per-bin gains
        for (bin, gain) in gains.iter().enumerate() {
            if bin < spectrum.len() {
                spectrum[bin] *= *gain;
            }
        }

        // Inverse FFT
        fft_inv
            .process(&mut spectrum, &mut time_out)
            .expect("FFT inverse failed");

        // Normalize (realfft inverse doesn't normalize)
        let norm = 1.0 / fft_size as f32;

        // Overlap-add with window
        for i in 0..fft_size {
            if pos + i < len {
                output[pos + i] += time_out[i] * norm * window[i];
                window_sum[pos + i] += window[i] * window[i];
            }
        }

        pos += hop_size;
    }

    // Normalize by window sum to avoid amplitude changes
    for i in 0..len {
        if window_sum[i] > 1e-6 {
            output[i] /= window_sum[i];
        }
    }

    output
}

/// Fast IIR-based multi-band EQ using cascaded lowpass band-splitting.
/// Lower latency than FFT version, suitable for live parameter changes.
/// Band transitions are softer (~24 dB/octave) compared to FFT's brick-wall.
pub fn apply_eq_filter_fast(
    samples: &[f32],
    sample_rate: u32,
    freq_low: f64,
    freq_high: f64,
    db_below: f64,
    db_selected: f64,
    db_harmonics: f64,
    db_above: f64,
    band_mode: BandMode,
) -> Vec<f32> {
    if samples.is_empty() {
        return Vec::new();
    }

    let gain_below = 10.0_f64.powf(db_below / 20.0) as f32;
    let gain_selected = 10.0_f64.powf(db_selected / 20.0) as f32;
    let harmonics_bounds = harmonics_band_bounds(freq_low, freq_high, band_mode);
    let gain_harmonics = if harmonics_bounds.is_some() {
        10.0_f64.powf(db_harmonics / 20.0) as f32
    } else {
        0.0
    };
    let gain_above = if band_mode.has_above_band() {
        10.0_f64.powf(db_above / 20.0) as f32
    } else {
        gain_selected
    };

    // Split at freq_low: below vs rest
    let lp_low = cascaded_lowpass(samples, freq_low, sample_rate, 4);
    let hp_low: Vec<f32> = samples
        .iter()
        .zip(lp_low.iter())
        .map(|(s, l)| s - l)
        .collect();

    // Split rest at freq_high: selected vs upper
    let lp_high = cascaded_lowpass(&hp_low, freq_high, sample_rate, 4);

    let len = samples.len();
    let mut output = vec![0.0f32; len];

    // Below band
    for i in 0..len {
        output[i] += lp_low[i] * gain_below;
    }

    // Selected band
    for i in 0..len {
        output[i] += lp_high[i] * gain_selected;
    }

    if let Some((harmonics_lower, harmonics_upper)) = harmonics_bounds {
        // Split the upper region around the harmonics band so widened selections map to 2x bounds.
        let hp_high: Vec<f32> = hp_low
            .iter()
            .zip(lp_high.iter())
            .map(|(s, l)| s - l)
            .collect();
        let lp_harm_upper = cascaded_lowpass(&hp_high, harmonics_upper, sample_rate, 4);
        let hp_harm_lower: Vec<f32> = if harmonics_lower > freq_high {
            let lp_harm_lower = cascaded_lowpass(&hp_high, harmonics_lower, sample_rate, 4);
            hp_high
                .iter()
                .zip(lp_harm_lower.iter())
                .map(|(s, l)| s - l)
                .collect()
        } else {
            hp_high.clone()
        };
        for i in 0..len {
            let harmonics = hp_harm_lower[i] - (hp_high[i] - lp_harm_upper[i]);
            let above = hp_high[i] - hp_harm_lower[i] + (hp_high[i] - lp_harm_upper[i]);
            output[i] += harmonics * gain_harmonics + above * gain_above;
        }
    } else {
        // Above band (or selected dB in 2-band mode): hp_high = hp_low - lp_high; inline
        for i in 0..len {
            output[i] += (hp_low[i] - lp_high[i]) * gain_above;
        }
    }

    output
}

fn lowpass_filter_inplace(buf: &mut [f32], cutoff_hz: f64, sample_rate: u32) {
    if buf.is_empty() {
        return;
    }
    let dt = 1.0 / sample_rate as f64;
    let rc = 1.0 / (2.0 * std::f64::consts::PI * cutoff_hz);
    let alpha = (dt / (rc + dt)) as f32;
    let mut prev = buf[0];
    for s in buf[1..].iter_mut() {
        let y = alpha * *s + (1.0 - alpha) * prev;
        *s = y;
        prev = y;
    }
}

pub fn cascaded_lowpass(samples: &[f32], cutoff: f64, sample_rate: u32, passes: usize) -> Vec<f32> {
    let mut result = samples.to_vec();
    for _ in 0..passes {
        lowpass_filter_inplace(&mut result, cutoff, sample_rate);
    }
    result
}

/// Split a signal into three brick-wall frequency bands via overlap-add FFT.
/// Returns (below freq_low, between freq_low..=freq_high, above freq_high).
pub fn split_three_bands_fft(
    samples: &[f32],
    sample_rate: u32,
    freq_low: f64,
    freq_high: f64,
) -> (Vec<f32>, Vec<f32>, Vec<f32>) {
    use realfft::num_complex::Complex;

    let len = samples.len();
    if len == 0 {
        return (Vec::new(), Vec::new(), Vec::new());
    }

    let fft_size = 4096usize;
    let hop_size = fft_size / 2;
    let window = hann_window(fft_size);
    let num_bins = fft_size / 2 + 1;
    let freq_per_bin = sample_rate as f64 / fft_size as f64;

    let (fft_fwd, fft_inv) = (plan_fft_forward(fft_size), plan_fft_inverse(fft_size));

    let mut below = vec![0.0f32; len];
    let mut middle = vec![0.0f32; len];
    let mut above = vec![0.0f32; len];
    let mut window_sum = vec![0.0f32; len];

    let mut frame = fft_fwd.make_input_vec();
    let mut spectrum = fft_fwd.make_output_vec();
    let mut scratch = fft_fwd.make_output_vec();
    let mut time_out = fft_inv.make_output_vec();
    let norm = 1.0 / fft_size as f32;
    let zero = Complex::new(0.0f32, 0.0f32);

    let lo_bin = (freq_low / freq_per_bin).round() as usize;
    let hi_bin = (freq_high / freq_per_bin).round() as usize;

    let mut pos = 0;
    while pos < len {
        frame.fill(0.0);
        for (i, &w) in window.iter().enumerate() {
            if pos + i < len {
                frame[i] = samples[pos + i] * w;
            }
        }

        fft_fwd
            .process(&mut frame, &mut spectrum)
            .expect("FFT forward failed");

        let emit = |target: &mut [f32],
                    bin_gate: &dyn Fn(usize) -> bool,
                    scratch: &mut [Complex<f32>],
                    time_out: &mut [f32]| {
            for bin in 0..num_bins {
                scratch[bin] = if bin_gate(bin) { spectrum[bin] } else { zero };
            }
            fft_inv
                .process(scratch, time_out)
                .expect("FFT inverse failed");
            for i in 0..fft_size {
                if pos + i < len {
                    target[pos + i] += time_out[i] * norm * window[i];
                }
            }
        };

        emit(&mut below, &|bin| bin < lo_bin, &mut scratch, &mut time_out);
        emit(
            &mut middle,
            &|bin| bin >= lo_bin && bin <= hi_bin,
            &mut scratch,
            &mut time_out,
        );
        emit(&mut above, &|bin| bin > hi_bin, &mut scratch, &mut time_out);

        for i in 0..fft_size {
            if pos + i < len {
                window_sum[pos + i] += window[i] * window[i];
            }
        }

        pos += hop_size;
    }

    for i in 0..len {
        if window_sum[i] > 1e-6 {
            let inv = 1.0 / window_sum[i];
            below[i] *= inv;
            middle[i] *= inv;
            above[i] *= inv;
        }
    }

    (below, middle, above)
}

/// Simple single-pole IIR low-pass filter (first-order exponential moving average).
///
/// Transfer function: y[n] = alpha * x[n] + (1 - alpha) * y[n-1]
/// where alpha = dt / (rc + dt), rc = 1 / (2 * PI * cutoff_hz)
///
/// For production use, upgrade to a higher-order Butterworth or Chebyshev filter
/// for sharper rolloff.
pub fn lowpass_filter(samples: &[f32], cutoff_hz: f64, sample_rate: u32) -> Vec<f32> {
    if samples.is_empty() {
        return Vec::new();
    }

    let dt = 1.0 / sample_rate as f64;
    let rc = 1.0 / (2.0 * std::f64::consts::PI * cutoff_hz);
    let alpha = (dt / (rc + dt)) as f32;

    let mut output = Vec::with_capacity(samples.len());
    let mut prev = samples[0];
    output.push(prev);

    for &sample in &samples[1..] {
        let filtered = alpha * sample + (1.0 - alpha) * prev;
        output.push(filtered);
        prev = filtered;
    }

    output
}

/// Decimate (downsample) audio from `source_rate` to `target_rate`.
///
/// Applies a 4-pass cascaded lowpass anti-aliasing filter at `target_rate / 2` Hz,
/// then takes every M-th sample where M = source_rate / target_rate.
///
/// Returns the original samples unchanged if `target_rate >= source_rate`.
/// Rounds to the nearest integer decimation factor.
pub fn decimate(samples: &[f32], source_rate: u32, target_rate: u32) -> Vec<f32> {
    if target_rate >= source_rate || target_rate == 0 || samples.is_empty() {
        return samples.to_vec();
    }

    let factor = source_rate as f64 / target_rate as f64;
    let m = factor.round() as usize;
    if m <= 1 {
        return samples.to_vec();
    }

    // Anti-aliasing: lowpass at Nyquist of the target rate
    let nyquist = target_rate as f64 / 2.0;
    let filtered = cascaded_lowpass(samples, nyquist, source_rate, 4);

    // Take every M-th sample
    filtered.iter().step_by(m).copied().collect()
}

/// Compute the effective sample rate after decimation (integer-ratio rounded).
pub fn decimated_rate(source_rate: u32, target_rate: u32) -> u32 {
    if target_rate >= source_rate || target_rate == 0 {
        return source_rate;
    }
    let m = (source_rate as f64 / target_rate as f64).round() as u32;
    if m <= 1 {
        return source_rate;
    }
    source_rate / m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn harmonics_band_uses_doubled_low_when_it_exceeds_focus_high() {
        assert_eq!(
            harmonics_band_bounds(20_000.0, 30_000.0, BandMode::FourBand),
            Some((40_000.0, 60_000.0))
        );
    }

    #[test]
    fn harmonics_band_uses_focus_high_as_lower_floor() {
        assert_eq!(
            harmonics_band_bounds(20_000.0, 50_000.0, BandMode::FourBand),
            Some((50_000.0, 100_000.0))
        );
    }

    #[test]
    fn harmonics_band_is_disabled_for_invalid_ranges() {
        assert_eq!(
            harmonics_band_bounds(50_000.0, 50_000.0, BandMode::FourBand),
            None
        );
        assert_eq!(
            harmonics_band_bounds(20_000.0, 50_000.0, BandMode::ThreeBand),
            None
        );
    }

    #[test]
    fn test_lowpass_attenuates_high_frequency() {
        let sample_rate = 192_000u32;
        let num_samples = 19200; // 100ms

        // Generate a 50 kHz signal
        let high: Vec<f32> = (0..num_samples)
            .map(|i| {
                let t = i as f64 / sample_rate as f64;
                (2.0 * std::f64::consts::PI * 50_000.0 * t).sin() as f32
            })
            .collect();

        let filtered = lowpass_filter(&high, 10_000.0, sample_rate);

        let rms_in: f64 =
            (high.iter().map(|s| (*s as f64).powi(2)).sum::<f64>() / high.len() as f64).sqrt();
        let rms_out: f64 = (filtered.iter().map(|s| (*s as f64).powi(2)).sum::<f64>()
            / filtered.len() as f64)
            .sqrt();

        assert!(
            rms_out < rms_in * 0.3,
            "50 kHz should be attenuated by LPF at 10 kHz: in={rms_in}, out={rms_out}"
        );
    }

    #[test]
    fn test_lowpass_passes_low_frequency() {
        let sample_rate = 192_000u32;
        let num_samples = 19200;

        // Generate a 1 kHz signal (well below 10 kHz cutoff)
        let low: Vec<f32> = (0..num_samples)
            .map(|i| {
                let t = i as f64 / sample_rate as f64;
                (2.0 * std::f64::consts::PI * 1_000.0 * t).sin() as f32
            })
            .collect();

        let filtered = lowpass_filter(&low, 10_000.0, sample_rate);

        let rms_in: f64 =
            (low.iter().map(|s| (*s as f64).powi(2)).sum::<f64>() / low.len() as f64).sqrt();
        let rms_out: f64 = (filtered.iter().map(|s| (*s as f64).powi(2)).sum::<f64>()
            / filtered.len() as f64)
            .sqrt();

        assert!(
            rms_out > rms_in * 0.8,
            "1 kHz should pass through LPF at 10 kHz: in={rms_in}, out={rms_out}"
        );
    }

    #[test]
    fn test_empty_input() {
        let result = lowpass_filter(&[], 1000.0, 44100);
        assert!(result.is_empty());
    }
}
