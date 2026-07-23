use crate::dsp::filters::lowpass_filter;

/// Bandpass-filter samples to the ultrasonic range (15 kHz – Nyquist).
/// Shared by both `zc_divide` and `zc_rate_per_bin`.
fn bandpass_ultrasonic(samples: &[f32], sample_rate: u32) -> Vec<f32> {
    // High-pass at 15 kHz via subtracting lowpass from original
    let lp = cascaded_lp(samples, 15_000.0, sample_rate, 4);
    let filtered: Vec<f32> = samples.iter().zip(lp.iter()).map(|(s, l)| s - l).collect();

    // Low-pass at 150 kHz (only matters if sample rate > 300 kHz)
    let nyquist = sample_rate as f64 / 2.0;
    if nyquist > 150_000.0 {
        cascaded_lp(&filtered, 150_000.0, sample_rate, 4)
    } else {
        filtered
    }
}

/// Compute an adaptive Schmitt trigger threshold from the filtered signal.
/// Uses the peak amplitude: threshold = peak * fraction, with a tiny minimum
/// so that pure silence doesn't trigger.
fn adaptive_threshold(filtered: &[f32]) -> (f32, f32) {
    let peak = filtered.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
    // Trigger at 5% of peak — catches quiet calls while rejecting noise floor.
    // Minimum of 1e-5 so pure digital silence never arms.
    let threshold_high = (peak * 0.05).max(1e-5);
    let threshold_low = threshold_high * 0.4;
    (threshold_high, threshold_low)
}

/// Simulate a zero-crossing frequency division bat detector.
///
/// Real FD detectors work by:
/// 1. Bandpass filtering the input to the ultrasonic range (15-150 kHz)
/// 2. Using a Schmitt trigger (hysteresis comparator) to reject noise crossings
/// 3. Dividing the crossing rate by `division_factor`
/// 4. Outputting a short pulse at each divided crossing
///
/// The output amplitude tracks the input envelope so that louder bat calls
/// produce louder clicks, matching the behavior of analog FD detectors.
pub fn zc_divide(
    samples: &[f32],
    sample_rate: u32,
    division_factor: u32,
    skip_bandpass: bool,
) -> Vec<f32> {
    if samples.len() < 2 || division_factor == 0 {
        return vec![0.0; samples.len()];
    }

    let filtered = if skip_bandpass {
        samples.to_vec()
    } else {
        bandpass_ultrasonic(samples, sample_rate)
    };

    // Envelope follower (~1ms window)
    let env_samples = ((sample_rate as f64 * 0.001) as usize).max(1);
    let envelope = smooth_envelope(&filtered, env_samples);

    // Adaptive threshold based on filtered signal peak
    let (threshold_high, threshold_low) = adaptive_threshold(&filtered);

    // Schmitt trigger zero-crossing detection with division
    let mut output = vec![0.0f32; samples.len()];
    let mut crossing_count: u32 = 0;
    let mut armed = false;
    let mut prev_positive = filtered[0] >= 0.0;

    // Click duration: ~0.15ms
    let click_len = ((sample_rate as f64 * 0.00015) as usize).max(2);
    let output_gain: f32 = 0.01;

    for i in 1..filtered.len() {
        let env = envelope[i];

        if env > threshold_high {
            armed = true;
        } else if env < threshold_low {
            armed = false;
            crossing_count = 0;
        }

        let curr_positive = filtered[i] >= 0.0;
        if armed && prev_positive != curr_positive {
            crossing_count += 1;
            if crossing_count >= division_factor {
                crossing_count = 0;
                let amp = (env / threshold_high).min(1.0) * output_gain;
                let end = (i + click_len).min(samples.len());
                for (k, out_sample) in output[i..end].iter_mut().enumerate() {
                    let phase = k as f64 / click_len as f64 * std::f64::consts::PI;
                    *out_sample = phase.sin() as f32 * amp;
                }
            }
        }
        prev_positive = curr_positive;
    }

    cascaded_lp(&output, 12_000.0, sample_rate, 2)
}

/// Compute zero-crossing rate per time bin for visualization.
///
/// Returns a Vec of (crossings_per_second, is_armed) per bin.
/// `bin_duration` is in seconds (e.g. 0.001 for 1ms bins).
/// Only counts crossings where the Schmitt trigger is armed (signal present).
pub fn zc_rate_per_bin(
    samples: &[f32],
    sample_rate: u32,
    bin_duration: f64,
    skip_bandpass: bool,
) -> Vec<(f64, bool)> {
    if samples.len() < 2 {
        return Vec::new();
    }

    let filtered = if skip_bandpass {
        samples.to_vec()
    } else {
        bandpass_ultrasonic(samples, sample_rate)
    };
    let env_samples = ((sample_rate as f64 * 0.001) as usize).max(1);
    let envelope = smooth_envelope(&filtered, env_samples);
    let (threshold_high, threshold_low) = adaptive_threshold(&filtered);

    let bin_samples = ((sample_rate as f64 * bin_duration) as usize).max(1);
    let num_bins = filtered.len().div_ceil(bin_samples);
    let mut bins = Vec::with_capacity(num_bins);

    let mut armed = false;
    let mut prev_positive = filtered[0] >= 0.0;
    let mut bin_crossings: usize = 0;
    let mut bin_armed = false;

    for i in 1..filtered.len() {
        let env = envelope[i];
        if env > threshold_high {
            armed = true;
        } else if env < threshold_low {
            armed = false;
        }

        let curr_positive = filtered[i] >= 0.0;
        if prev_positive != curr_positive && armed {
            bin_crossings += 1;
            bin_armed = true;
        }
        prev_positive = curr_positive;

        // End of bin?
        if (i % bin_samples) == 0 || i == filtered.len() - 1 {
            let actual_bin_dur = if i == filtered.len() - 1 {
                (i % bin_samples + 1) as f64 / sample_rate as f64
            } else {
                bin_duration
            };
            let rate = if actual_bin_dur > 0.0 {
                bin_crossings as f64 / (2.0 * actual_bin_dur)
            } else {
                0.0
            };
            bins.push((rate, bin_armed));
            bin_crossings = 0;
            bin_armed = false;
        }
    }

    bins
}

pub(crate) fn cascaded_lp(
    samples: &[f32],
    cutoff: f64,
    sample_rate: u32,
    passes: usize,
) -> Vec<f32> {
    let mut result = samples.to_vec();
    for _ in 0..passes {
        result = lowpass_filter(&result, cutoff, sample_rate);
    }
    result
}

pub(crate) fn smooth_envelope(samples: &[f32], window: usize) -> Vec<f32> {
    let mut env = vec![0.0f32; samples.len()];
    let attack = 1.0 / window as f32;
    let release = 1.0 / (window as f32 * 4.0);

    let mut current = 0.0f32;
    for (i, &s) in samples.iter().enumerate() {
        let abs = s.abs();
        if abs > current {
            current += attack * (abs - current);
        } else {
            current += release * (abs - current);
        }
        env[i] = current;
    }
    env
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    fn make_sine(freq: f64, sample_rate: u32, duration: f64) -> Vec<f32> {
        let n = (sample_rate as f64 * duration) as usize;
        (0..n)
            .map(|i| {
                let t = i as f64 / sample_rate as f64;
                (2.0 * PI * freq * t).sin() as f32
            })
            .collect()
    }

    #[test]
    fn test_ultrasonic_sine_produces_clicks() {
        let sr = 192_000;
        let input: Vec<f32> = make_sine(45_000.0, sr, 0.02)
            .iter()
            .map(|s| s * 0.8)
            .collect();
        let output = zc_divide(&input, sr, 10, false);
        assert_eq!(output.len(), input.len());

        let peak = output.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(peak > 0.001, "Should produce audible clicks, peak={peak}");
        assert!(peak < 0.1, "Should not be too loud, peak={peak}");
    }

    #[test]
    fn test_quiet_signal_still_detected() {
        let sr = 192_000;
        // Quiet signal at 0.005 amplitude — typical of real bat recordings
        let input: Vec<f32> = make_sine(45_000.0, sr, 0.02)
            .iter()
            .map(|s| s * 0.005)
            .collect();
        let output = zc_divide(&input, sr, 8, false);
        let peak = output.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(
            peak > 0.001,
            "Quiet bat calls should still produce clicks, peak={peak}"
        );
    }

    #[test]
    fn test_silence_produces_no_output() {
        let input = vec![0.0f32; 19200];
        let output = zc_divide(&input, 192_000, 10, false);
        let peak = output.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(peak < 0.001, "Silence should produce no clicks");
    }

    #[test]
    fn test_empty_input() {
        let output = zc_divide(&[], 192_000, 10, false);
        assert!(output.is_empty());
    }

    #[test]
    fn test_dc_signal_no_clicks() {
        let input = vec![1.0f32; 1000];
        let output = zc_divide(&input, 44100, 10, false);
        assert!(output.iter().all(|&s| s.abs() < 0.001));
    }

    #[test]
    fn test_zc_rate_bins() {
        let sr = 192_000;
        let input: Vec<f32> = make_sine(45_000.0, sr, 0.02)
            .iter()
            .map(|s| s * 0.5)
            .collect();
        let bins = zc_rate_per_bin(&input, sr, 0.001, false);
        assert!(!bins.is_empty());
        // Most bins should show ~45 kHz and be armed
        let armed_bins: Vec<_> = bins.iter().filter(|(_, armed)| *armed).collect();
        assert!(!armed_bins.is_empty(), "Should have armed bins");
        for &(rate, _) in &armed_bins {
            let error = (rate - 45_000.0).abs() / 45_000.0;
            assert!(error < 0.15, "Rate should be ~45kHz, got {rate:.0}");
        }
    }
}
