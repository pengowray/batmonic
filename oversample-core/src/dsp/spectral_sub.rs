//! Spectral subtraction noise reduction: learn a broadband noise floor from
//! a segment of audio, then attenuate frequency bins near or below that floor
//! during playback. Complements the notch filter (which targets discrete tonal
//! noise) by handling broadband hiss, wind, and ambient noise.

use crate::dsp::fft::{hann_window, plan_fft_forward, plan_fft_inverse};
use realfft::num_complex::Complex;
use serde::{Deserialize, Serialize};

/// A learned noise floor spectrum for spectral subtraction.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NoiseFloor {
    /// Per-bin mean magnitude (linear scale). Length = fft_size / 2 + 1.
    pub bin_magnitudes: Vec<f64>,
    /// FFT size used to compute this noise floor.
    pub fft_size: usize,
    /// Sample rate of the audio this was learned from.
    pub sample_rate: u32,
    /// Duration of audio analyzed (seconds).
    pub analysis_duration_secs: f64,
    /// Number of STFT frames averaged.
    pub frame_count: u64,
}

// ── Noise floor learning ────────────────────────────────────────────────────

/// Pick FFT size based on sample rate (same logic as notch detection).
fn fft_size_for_rate(sample_rate: u32) -> usize {
    if sample_rate >= 192_000 {
        8192
    } else {
        4096
    }
}

/// Async version that yields periodically via a caller-supplied future.
pub async fn learn_noise_floor_async<F, Fut>(
    samples: &[f32],
    sample_rate: u32,
    analysis_duration_secs: f64,
    yield_now: F,
) -> Option<NoiseFloor>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let fft_size = fft_size_for_rate(sample_rate);
    let hop_size = fft_size / 2;
    let num_bins = fft_size / 2 + 1;

    let max_samples = (analysis_duration_secs * sample_rate as f64) as usize;
    let analysis = &samples[..samples.len().min(max_samples)];

    if analysis.len() < fft_size {
        return None;
    }

    let window = hann_window(fft_size);

    let fft = plan_fft_forward(fft_size);
    let mut input = fft.make_input_vec();
    let mut spectrum = fft.make_output_vec();

    // Welford's online mean per bin
    let mut mean = vec![0.0f64; num_bins];
    let mut count = 0u64;

    let mut pos = 0;
    let mut frame_count = 0u32;
    let yield_interval = 50;

    while pos + fft_size <= analysis.len() {
        for (inp, (&s, &w)) in input
            .iter_mut()
            .zip(analysis[pos..pos + fft_size].iter().zip(window.iter()))
        {
            *inp = s * w;
        }

        fft.process(&mut input, &mut spectrum).expect("FFT failed");

        count += 1;
        for (i, c) in spectrum.iter().enumerate() {
            let mag = c.norm() as f64;
            mean[i] += (mag - mean[i]) / count as f64;
        }

        pos += hop_size;
        frame_count += 1;
        if frame_count.is_multiple_of(yield_interval) {
            yield_now().await;
        }
    }

    if count < 2 {
        return None;
    }

    let actual_duration = pos as f64 / sample_rate as f64;

    Some(NoiseFloor {
        bin_magnitudes: mean,
        fft_size,
        sample_rate,
        analysis_duration_secs: actual_duration,
        frame_count: count,
    })
}

// ── Spectral subtraction application ────────────────────────────────────────

/// Apply spectral subtraction noise reduction via overlap-add STFT.
///
/// - `noise_floor`: the learned noise floor spectrum
/// - `strength`: 0.0 = no reduction, 1.0 = full subtraction, >1.0 = over-subtraction
/// - `floor_factor`: minimum residual as a fraction of original magnitude (prevents musical noise)
/// - `harmonic_suppression`: 0.0–1.0, propagates noise floor to 2x and 3x harmonic bins
pub fn apply_spectral_subtraction(
    samples: &[f32],
    sample_rate: u32,
    noise_floor: &NoiseFloor,
    strength: f64,
    floor_factor: f64,
    harmonic_suppression: f64,
) -> Vec<f32> {
    if samples.is_empty() || strength <= 0.0 {
        return samples.to_vec();
    }

    // Skip if sample rate doesn't match the learned floor — the per-bin
    // frequencies wouldn't line up, so subtraction would be wrong. Fail SAFE
    // (return the input untouched) but warn so the no-op is diagnosable rather
    // than silent.
    if noise_floor.sample_rate != sample_rate {
        log::warn!(
            "spectral subtraction skipped: noise floor learned at {} Hz but audio is {} Hz",
            noise_floor.sample_rate,
            sample_rate
        );
        return samples.to_vec();
    }

    let fft_size = noise_floor.fft_size;
    let hop_size = fft_size / 2;
    let len = samples.len();

    let window = hann_window(fft_size);
    let num_bins = fft_size / 2 + 1;

    // Build enhanced noise floor with harmonic propagation
    let effective_floor = if harmonic_suppression > 0.0 {
        let mut enhanced = noise_floor.bin_magnitudes.clone();
        for b in 0..num_bins {
            let mag = noise_floor.bin_magnitudes[b];
            if mag < 1e-20 {
                continue;
            }
            for multiplier in [2usize, 3usize] {
                let harmonic_bin = b * multiplier;
                if harmonic_bin < num_bins {
                    enhanced[harmonic_bin] += harmonic_suppression * mag;
                }
            }
        }
        enhanced
    } else {
        noise_floor.bin_magnitudes.clone()
    };

    let (fft_fwd, fft_inv) = (plan_fft_forward(fft_size), plan_fft_inverse(fft_size));

    let mut output = vec![0.0f32; len];
    let mut window_sum = vec![0.0f32; len];

    let mut frame = fft_fwd.make_input_vec();
    let mut spectrum = fft_fwd.make_output_vec();
    let mut time_out = fft_inv.make_output_vec();

    let mut pos = 0;
    while pos < len {
        // Fill windowed frame
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

        // Spectral subtraction per bin
        for (bin, c) in spectrum.iter_mut().enumerate() {
            let mag = c.norm() as f64;
            let noise_mag = if bin < num_bins {
                effective_floor[bin] * strength
            } else {
                0.0
            };

            let clean_mag = (mag - noise_mag).max(floor_factor * mag);

            // Preserve original phase, apply cleaned magnitude
            if mag > 1e-20 {
                let scale = (clean_mag / mag) as f32;
                *c = Complex::new(c.re * scale, c.im * scale);
            }
        }

        // Inverse FFT
        fft_inv
            .process(&mut spectrum, &mut time_out)
            .expect("FFT inverse failed");

        // Normalize + overlap-add
        let norm = 1.0 / fft_size as f32;
        for i in 0..fft_size {
            if pos + i < len {
                output[pos + i] += time_out[i] * norm * window[i];
                window_sum[pos + i] += window[i] * window[i];
            }
        }

        pos += hop_size;
    }

    // Normalize by window sum
    for i in 0..len {
        if window_sum[i] > 1e-6 {
            output[i] /= window_sum[i];
        }
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_floor(fft_size: usize, sample_rate: u32, magnitude: f64) -> NoiseFloor {
        NoiseFloor {
            bin_magnitudes: vec![magnitude; fft_size / 2 + 1],
            fft_size,
            sample_rate,
            analysis_duration_secs: 0.5,
            frame_count: 1,
        }
    }

    #[test]
    fn fft_size_for_rate_picks_8192_above_192k() {
        assert_eq!(fft_size_for_rate(44_100), 4096);
        assert_eq!(fft_size_for_rate(96_000), 4096);
        assert_eq!(fft_size_for_rate(192_000), 8192);
        assert_eq!(fft_size_for_rate(384_000), 8192);
    }

    #[test]
    fn strength_zero_is_passthrough() {
        let floor = synthetic_floor(4096, 44_100, 1.0);
        let samples = vec![0.5f32; 4096];
        let out = apply_spectral_subtraction(&samples, 44_100, &floor, 0.0, 0.1, 0.0);
        assert_eq!(out, samples);
    }

    #[test]
    fn mismatched_sample_rate_is_passthrough() {
        // Floor learned at 44.1k, audio at 48k → must not be applied.
        let floor = synthetic_floor(4096, 44_100, 1.0);
        let samples = vec![0.5f32; 4096];
        let out = apply_spectral_subtraction(&samples, 48_000, &floor, 1.0, 0.1, 0.0);
        assert_eq!(out, samples);
    }

    #[test]
    fn empty_input_is_empty_output() {
        let floor = synthetic_floor(4096, 44_100, 1.0);
        let out = apply_spectral_subtraction(&[], 44_100, &floor, 1.0, 0.1, 0.0);
        assert!(out.is_empty());
    }

    #[test]
    fn aggressive_floor_attenuates_signal() {
        // Build a floor whose magnitude exceeds the test signal magnitude in every bin —
        // the floor_factor (here 0.1) should clamp output to 10% of original.
        let sr = 44_100u32;
        let fft_size = 4096;
        let floor = synthetic_floor(fft_size, sr, 1e6); // unrealistically loud "noise" floor
        let n = fft_size * 4;
        let samples: Vec<f32> = (0..n)
            .map(|i| 0.3 * (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / sr as f32).sin())
            .collect();
        let out = apply_spectral_subtraction(&samples, sr, &floor, 1.0, 0.1, 0.0);
        // Steady-state region (skip windowing transients at the head/tail).
        let head = fft_size;
        let tail = out.len() - fft_size;
        let in_rms: f64 = (samples[head..tail]
            .iter()
            .map(|&s| (s as f64).powi(2))
            .sum::<f64>()
            / (tail - head) as f64)
            .sqrt();
        let out_rms: f64 = (out[head..tail]
            .iter()
            .map(|&s| (s as f64).powi(2))
            .sum::<f64>()
            / (tail - head) as f64)
            .sqrt();
        // floor_factor=0.1 means output magnitude is capped at 10% per bin, so total
        // RMS should drop by roughly the same factor (within a few percent slack).
        assert!(
            out_rms < in_rms * 0.2,
            "spectral subtraction should attenuate; in_rms={in_rms}, out_rms={out_rms}",
        );
    }

    #[test]
    fn zero_floor_with_unity_strength_is_near_passthrough() {
        // Floor of zero everywhere means nothing to subtract — output should track input
        // closely. (Window overlap-add introduces tiny boundary effects; check the middle.)
        let sr = 44_100u32;
        let fft_size = 4096;
        let floor = synthetic_floor(fft_size, sr, 0.0);
        let n = fft_size * 4;
        let samples: Vec<f32> = (0..n)
            .map(|i| 0.3 * (2.0 * std::f32::consts::PI * 1500.0 * i as f32 / sr as f32).sin())
            .collect();
        let out = apply_spectral_subtraction(&samples, sr, &floor, 1.0, 0.1, 0.0);
        let head = fft_size;
        let tail = out.len() - fft_size;
        let diff_rms: f64 = (samples[head..tail]
            .iter()
            .zip(out[head..tail].iter())
            .map(|(a, b)| ((a - b) as f64).powi(2))
            .sum::<f64>()
            / (tail - head) as f64)
            .sqrt();
        assert!(
            diff_rms < 0.05,
            "expected near-passthrough, diff_rms={diff_rms}"
        );
    }
}
