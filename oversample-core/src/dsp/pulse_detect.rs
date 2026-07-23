use crate::dsp::zc_divide::{cascaded_lp, smooth_envelope};
use crate::types::{AudioData, SpectrogramData};

#[derive(Clone, Debug)]
pub struct DetectedPulse {
    pub index: usize,        // 1-based pulse number
    pub start_time: f64,     // seconds
    pub end_time: f64,       // seconds
    pub peak_time: f64,      // time of peak energy within pulse
    pub peak_freq: f64,      // dominant frequency (Hz) from spectrogram
    pub snr_db: f64,         // signal-to-noise ratio relative to noise floor
    pub peak_amplitude: f64, // peak envelope level (linear)
}

impl DetectedPulse {
    pub fn duration_ms(&self) -> f64 {
        (self.end_time - self.start_time) * 1000.0
    }
}

#[derive(Clone, Debug)]
pub struct PulseDetectionParams {
    pub min_pulse_duration_ms: f64,
    pub max_pulse_duration_ms: f64,
    pub min_gap_ms: f64,
    pub threshold_db: f64,
    /// Bandpass low frequency (Hz). 0 = no highpass.
    pub bandpass_low_hz: f64,
    /// Bandpass high frequency (Hz). 0 = no lowpass (use Nyquist).
    pub bandpass_high_hz: f64,
}

impl Default for PulseDetectionParams {
    fn default() -> Self {
        Self {
            min_pulse_duration_ms: 0.3,
            max_pulse_duration_ms: 50.0,
            min_gap_ms: 3.0,
            threshold_db: 6.0,
            bandpass_low_hz: 0.0,
            bandpass_high_hz: 0.0,
        }
    }
}

/// Detect individual pulses (bat calls) in an audio recording.
///
/// Uses energy envelope with Schmitt trigger thresholding, bandpassed to the
/// given frequency range. Peak frequency for each pulse is extracted from the
/// pre-computed spectrogram.
pub fn detect_pulses(
    audio: &AudioData,
    spectrogram: &SpectrogramData,
    params: &PulseDetectionParams,
) -> Vec<DetectedPulse> {
    // Borrow the resident mono mix instead of materializing the whole file
    // again via read_region (the spectrogram path borrows `samples` the same
    // way) — that copy was a full-length f32 allocation that could OOM the WASM
    // heap on long recordings.
    let samples: &[f32] = &audio.samples;
    let sr = audio.sample_rate;
    if samples.len() < 2 {
        return Vec::new();
    }

    // Step 1: Bandpass filter to focus frequency range
    let filtered = bandpass(samples, sr, params.bandpass_low_hz, params.bandpass_high_hz);

    // Step 2: Compute energy envelope (~0.25ms window for bat calls)
    let env_window = ((sr as f64 * 0.00025) as usize).max(1);
    let envelope = smooth_envelope(&filtered, env_window);

    // Step 3: Estimate noise floor (10th percentile of envelope)
    let noise_floor = estimate_noise_floor(&envelope);
    if noise_floor <= 0.0 {
        return Vec::new();
    }

    // Step 4: Schmitt trigger pulse detection
    let threshold_high = noise_floor * 10f64.powf(params.threshold_db / 20.0) as f32;
    let hysteresis_db = params.threshold_db - 3.0;
    let threshold_low = noise_floor * 10f64.powf(hysteresis_db.max(0.0) / 20.0) as f32;

    let min_gap_samples = ((sr as f64 * params.min_gap_ms / 1000.0) as usize).max(1);
    let min_dur_samples = ((sr as f64 * params.min_pulse_duration_ms / 1000.0) as usize).max(1);
    let max_dur_samples = ((sr as f64 * params.max_pulse_duration_ms / 1000.0) as usize).max(1);

    let raw_pulses = detect_raw_pulses(&envelope, threshold_high, threshold_low, min_gap_samples);

    // Step 5: Filter by duration and build results
    let mut pulses = Vec::new();
    let mut index = 1usize;

    for (start_sample, end_sample, peak_sample, peak_amp) in raw_pulses {
        let dur = end_sample - start_sample;
        if dur < min_dur_samples || dur > max_dur_samples {
            continue;
        }

        let start_time = start_sample as f64 / sr as f64;
        let end_time = end_sample as f64 / sr as f64;
        let peak_time = peak_sample as f64 / sr as f64;

        // Step 6: Find peak frequency from spectrogram
        let peak_freq = find_peak_frequency(spectrogram, start_time, end_time);

        // Step 7: Compute SNR
        let snr_db = if noise_floor > 0.0 {
            20.0 * (peak_amp as f64 / noise_floor as f64).log10()
        } else {
            0.0
        };

        pulses.push(DetectedPulse {
            index,
            start_time,
            end_time,
            peak_time,
            peak_freq,
            snr_db,
            peak_amplitude: peak_amp as f64,
        });
        index += 1;
    }

    pulses
}

/// Bandpass filter samples to the given frequency range.
fn bandpass(samples: &[f32], sample_rate: u32, low_hz: f64, high_hz: f64) -> Vec<f32> {
    let nyquist = sample_rate as f64 / 2.0;
    let mut result = samples.to_vec();

    // Highpass via subtracting lowpass
    if low_hz > 0.0 && low_hz < nyquist {
        let lp = cascaded_lp(samples, low_hz, sample_rate, 4);
        for (r, l) in result.iter_mut().zip(lp.iter()) {
            *r -= *l;
        }
    }

    // Lowpass
    if high_hz > 0.0 && high_hz < nyquist {
        result = cascaded_lp(&result, high_hz, sample_rate, 4);
    }

    result
}

/// Estimate noise floor as the 10th percentile of envelope values.
fn estimate_noise_floor(envelope: &[f32]) -> f32 {
    if envelope.is_empty() {
        return 0.0;
    }
    // Sample up to 10000 evenly-spaced values to avoid sorting the entire envelope
    let step = (envelope.len() / 10_000).max(1);
    let mut sampled: Vec<f32> = envelope.iter().step_by(step).copied().collect();
    sampled.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let idx = sampled.len() / 10; // 10th percentile
    sampled[idx].max(1e-10) // tiny minimum to avoid division by zero
}

/// Raw pulse detection using Schmitt trigger on envelope.
/// Returns Vec of (start_sample, end_sample, peak_sample, peak_amplitude).
fn detect_raw_pulses(
    envelope: &[f32],
    threshold_high: f32,
    threshold_low: f32,
    min_gap_samples: usize,
) -> Vec<(usize, usize, usize, f32)> {
    let mut pulses: Vec<(usize, usize, usize, f32)> = Vec::new();
    let mut in_pulse = false;
    let mut pulse_start = 0usize;
    let mut peak_sample = 0usize;
    let mut peak_amp = 0.0f32;

    for (i, &env) in envelope.iter().enumerate() {
        if !in_pulse {
            if env >= threshold_high {
                in_pulse = true;
                pulse_start = i;
                peak_sample = i;
                peak_amp = env;
            }
        } else {
            if env > peak_amp {
                peak_amp = env;
                peak_sample = i;
            }
            if env < threshold_low {
                // Pulse ended
                let pulse_end = i;

                // Try to merge with previous pulse if gap is too small
                if let Some(last) = pulses.last_mut() {
                    if pulse_start - last.1 < min_gap_samples {
                        // Merge: extend previous pulse
                        last.1 = pulse_end;
                        if peak_amp > last.3 {
                            last.2 = peak_sample;
                            last.3 = peak_amp;
                        }
                        in_pulse = false;
                        continue;
                    }
                }

                pulses.push((pulse_start, pulse_end, peak_sample, peak_amp));
                in_pulse = false;
            }
        }
    }

    // Close any open pulse at end of signal
    if in_pulse {
        pulses.push((pulse_start, envelope.len(), peak_sample, peak_amp));
    }

    pulses
}

/// Find the dominant frequency in the spectrogram within a time range.
fn find_peak_frequency(spectrogram: &SpectrogramData, start_time: f64, end_time: f64) -> f64 {
    let columns = &spectrogram.columns;
    if columns.is_empty() {
        return 0.0;
    }

    let mut best_mag = 0.0f32;
    let mut best_bin = 0usize;

    for col in columns.iter() {
        if col.time_offset < start_time || col.time_offset > end_time {
            continue;
        }
        for (bin, &mag) in col.magnitudes.iter().enumerate() {
            if mag > best_mag {
                best_mag = mag;
                best_bin = bin;
            }
        }
    }

    best_bin as f64 * spectrogram.freq_resolution
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_ms_is_difference_times_1000() {
        let p = DetectedPulse {
            index: 1,
            start_time: 0.10,
            end_time: 0.105,
            peak_time: 0.102,
            peak_freq: 40_000.0,
            snr_db: 18.0,
            peak_amplitude: 0.5,
        };
        assert!((p.duration_ms() - 5.0).abs() < 1e-9);
    }

    #[test]
    fn estimate_noise_floor_empty_envelope_returns_zero() {
        assert_eq!(estimate_noise_floor(&[]), 0.0);
    }

    #[test]
    fn estimate_noise_floor_takes_tenth_percentile() {
        // 100 values: 0.0..=0.99 in 0.01 steps. 10th percentile ≈ 0.10.
        let env: Vec<f32> = (0..100).map(|i| i as f32 / 100.0).collect();
        let floor = estimate_noise_floor(&env);
        // Step-sampling + sort + index/10 → bucket 10 → 0.10 with f32 jitter.
        assert!((floor - 0.10).abs() < 0.02, "got {floor}");
    }

    #[test]
    fn estimate_noise_floor_clamps_to_tiny_positive_value() {
        // All zeros → 10th percentile is 0; result clamped to ≥ 1e-10.
        let env = vec![0.0f32; 50];
        let floor = estimate_noise_floor(&env);
        assert!(floor >= 1e-10);
        assert!(floor < 1e-5);
    }

    #[test]
    fn detect_raw_pulses_finds_single_burst() {
        // Envelope: flat low, then a bump that crosses high, then low again.
        let mut env = vec![0.01f32; 100];
        for i in 30..40 {
            env[i] = 1.0;
        }
        let pulses = detect_raw_pulses(&env, 0.5, 0.1, 1);
        assert_eq!(pulses.len(), 1);
        let (start, end, peak, amp) = pulses[0];
        assert_eq!(start, 30);
        assert!(end >= 40 && end <= 41);
        assert!(peak >= 30 && peak < 40);
        assert!((amp - 1.0).abs() < 1e-6);
    }

    #[test]
    fn detect_raw_pulses_merges_close_bursts() {
        // Two bursts separated by 2 samples below threshold, min_gap=5 → merge.
        let mut env = vec![0.01f32; 100];
        for i in 30..35 {
            env[i] = 1.0;
        }
        for i in 37..42 {
            env[i] = 0.8;
        }
        let pulses = detect_raw_pulses(&env, 0.5, 0.1, 5);
        assert_eq!(pulses.len(), 1, "close bursts should be merged");
    }

    #[test]
    fn detect_raw_pulses_keeps_well_separated_bursts() {
        let mut env = vec![0.01f32; 100];
        for i in 10..15 {
            env[i] = 1.0;
        }
        for i in 80..85 {
            env[i] = 1.0;
        }
        let pulses = detect_raw_pulses(&env, 0.5, 0.1, 5);
        assert_eq!(pulses.len(), 2);
    }

    #[test]
    fn detect_raw_pulses_closes_open_pulse_at_end_of_signal() {
        // Pulse that never falls back below low threshold — should still be reported.
        let mut env = vec![0.01f32; 100];
        for i in 90..100 {
            env[i] = 1.0;
        }
        let pulses = detect_raw_pulses(&env, 0.5, 0.1, 1);
        assert_eq!(pulses.len(), 1);
        assert_eq!(pulses[0].1, 100); // pulse_end = envelope length
    }
}
