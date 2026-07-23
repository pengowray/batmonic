use crate::dsp::fft::{plan_fft_forward, plan_fft_inverse};
use crate::dsp::pitch_shift::PitchFactor;
use std::f32::consts::PI;

const FFT_SIZE: usize = 4096;
const HOP: usize = 1024; // 75% overlap

/// Phase-vocoder pitch shift via direct spectral bin shifting, with
/// Laroche–Dolson identity phase locking.
///
/// Shifts frequency content by remapping STFT bins each frame. Synthesis phase is
/// derived per frame WITHOUT a propagating accumulator, so it (a) has no phase
/// discontinuities at streaming chunk boundaries and (b) keeps the noise floor
/// INCOHERENT — a true propagating accumulator was tried (commit 1dc2590) but
/// tonalised the noise into an audible "musical noise" buzz that scaled with gain.
///
/// To avoid the warble that a plain stateless per-bin phase scaling produces on
/// strong tonal content, each frame's spectral peaks are found and every bin is
/// phase-locked to its nearest peak: its synthesis phase is the peak's scaled
/// phase plus the bin's own analysis-phase offset from that peak. This keeps the
/// phase coherence within each sinusoid (less warble) while remaining stateless
/// (no buzz). See the synthesis loop for detail.
///
/// See [`PitchFactor`] for the sign/bypass convention (positive = down,
/// negative = up, `|value| <= 1` = bypass).
pub fn phase_vocoder_pitch_shift(samples: &[f32], factor: PitchFactor) -> Vec<f32> {
    if samples.is_empty() {
        return Vec::new();
    }

    if factor.is_bypass() {
        return samples.to_vec();
    }

    let abs_factor = factor.magnitude() as f32;

    // pitch_factor: multiply frequencies by this amount
    // shift_down factor=10 → pitch_factor = 0.1 (divide freq by 10)
    // shift_up factor=-10 → pitch_factor = 10 (multiply freq by 10)
    let pitch_factor = if factor.is_up() {
        abs_factor
    } else {
        1.0 / abs_factor
    };

    if samples.len() < FFT_SIZE {
        return samples.to_vec();
    }

    let n_bins = FFT_SIZE / 2 + 1;
    let fft_f = FFT_SIZE as f32;
    let original_len = samples.len();

    // Pad input so the STFT frames fully cover every sample.
    // Without padding, the last (samples.len() % HOP) samples get no STFT frame,
    // producing a brief silence/click at streaming chunk boundaries.
    let padded_len = if !(samples.len() - FFT_SIZE).is_multiple_of(HOP) {
        let n = (samples.len() - FFT_SIZE) / HOP + 2; // one extra frame
        (n - 1) * HOP + FFT_SIZE
    } else {
        samples.len()
    };
    let samples = if padded_len > samples.len() {
        let mut padded = samples.to_vec();
        padded.resize(padded_len, 0.0);
        padded
    } else {
        samples.to_vec()
    };
    let samples = &samples[..];

    let n_frames = (samples.len().saturating_sub(FFT_SIZE)) / HOP + 1;
    if n_frames == 0 {
        return vec![0.0; original_len];
    }

    let out_len = (n_frames - 1) * HOP + FFT_SIZE;

    // Hann window
    let hann: Vec<f32> = (0..FFT_SIZE)
        .map(|i| {
            let x = PI * i as f32 / fft_f;
            x.sin().powi(2)
        })
        .collect();

    // FFT plans from the shared thread-local planner (no per-call allocation).
    let fft_forward = plan_fft_forward(FFT_SIZE);
    let fft_inverse = plan_fft_inverse(FFT_SIZE);

    let mut output = vec![0.0f32; out_len];
    let mut window_sum = vec![0.0f32; out_len];

    let mut fft_in = vec![0.0f32; FFT_SIZE];
    let mut spectrum = fft_forward.make_output_vec();
    let mut ifft_out = vec![0.0f32; FFT_SIZE];

    // Per-frame scratch for analysis.
    let mut mag = vec![0.0f32; n_bins];
    let mut phase = vec![0.0f32; n_bins];
    // Peak-locking scratch: the spectral peaks of the frame, and for each bin the
    // peak that governs its "region of influence" (Laroche–Dolson).
    let mut peaks: Vec<usize> = Vec::new();
    let mut peak_of = vec![0usize; n_bins];

    for frame in 0..n_frames {
        let offset = frame * HOP;
        if offset + FFT_SIZE > samples.len() {
            break;
        }

        // Window the input frame
        for i in 0..FFT_SIZE {
            fft_in[i] = samples[offset + i] * hann[i];
        }

        // Forward FFT (skip the frame on the impossible size-mismatch error
        // rather than panicking in the audio thread).
        if fft_forward.process(&mut fft_in, &mut spectrum).is_err() {
            continue;
        }

        // Extract magnitude and phase per bin.
        for k in 0..n_bins {
            let re = spectrum[k].re;
            let im = spectrum[k].im;
            mag[k] = (re * re + im * im).sqrt();
            phase[k] = im.atan2(re);
        }

        // ── Peak-locking (Laroche–Dolson identity phase locking) ─────────────
        // Find spectral peaks (local magnitude maxima above a small fraction of
        // the frame peak) and assign every bin to its nearest peak. The synthesis
        // phase below is then locked to the governing peak's scaled phase plus the
        // bin's analysis-phase offset from that peak. This preserves phase
        // coherence WITHIN each sinusoid (killing the per-bin-scaling warble)
        // while staying STATELESS across frames/chunks — so the noise floor stays
        // incoherent and there is no propagating "musical noise" buzz (the failure
        // mode of a synthesis-phase accumulator, commit 1dc2590).
        peaks.clear();
        let max_mag = mag.iter().copied().fold(0.0f32, f32::max);
        let peak_thresh = max_mag * 1e-3;
        for k in 1..n_bins - 1 {
            if mag[k] > peak_thresh && mag[k] > mag[k - 1] && mag[k] >= mag[k + 1] {
                peaks.push(k);
            }
        }
        if peaks.is_empty() {
            // No clear peaks (near-silence): each bin governs itself, reducing to
            // the plain stateless nearest-bin behaviour.
            for (k, p) in peak_of.iter_mut().enumerate() {
                *p = k;
            }
        } else {
            // Assign each bin to the nearest peak (boundary at the midpoint between
            // consecutive peaks).
            let mut pi = 0usize;
            for (k, p) in peak_of.iter_mut().enumerate() {
                while pi + 1 < peaks.len()
                    && (peaks[pi + 1] as i32 - k as i32).abs() < (peaks[pi] as i32 - k as i32).abs()
                {
                    pi += 1;
                }
                *p = peaks[pi];
            }
        }

        // Shift bins: for each output bin j, read from source bin j / pitch_factor.
        for (j, spec_bin) in spectrum.iter_mut().enumerate().take(n_bins) {
            let source = j as f32 / pitch_factor;
            let s_idx = source as usize;
            let s_frac = source - s_idx as f32;

            let (m, src_phase) = if s_idx + 1 < n_bins {
                // Interpolate magnitude; take nearest bin phase (avoids wrapping issues)
                (
                    mag[s_idx] * (1.0 - s_frac) + mag[s_idx + 1] * s_frac,
                    if s_frac < 0.5 {
                        phase[s_idx]
                    } else {
                        phase[s_idx + 1]
                    },
                )
            } else if s_idx < n_bins {
                (mag[s_idx] * (1.0 - s_frac), phase[s_idx])
            } else {
                (0.0, 0.0)
            };

            // Identity phase locking: anchor to the governing peak's scaled phase
            // and add this bin's analysis-phase offset from that peak. At the peak
            // itself this is peak_phase * pitch_factor (frequency-correct); away
            // from it the intra-sinusoid relationship (src_phase - peak_phase) is
            // preserved instead of independently scaled.
            let kp = peak_of[s_idx.min(n_bins - 1)];
            let out_phase = phase[kp] * pitch_factor + (src_phase - phase[kp]);
            spec_bin.re = m * out_phase.cos();
            spec_bin.im = m * out_phase.sin();
        }

        // DC and Nyquist bins must be real for realfft inverse
        spectrum[0].im = 0.0;
        spectrum[n_bins - 1].im = 0.0;

        // Inverse FFT (skip this frame's overlap-add on the impossible error).
        if fft_inverse.process(&mut spectrum, &mut ifft_out).is_err() {
            continue;
        }

        // Normalize + overlap-add
        let norm = 1.0 / fft_f;
        let out_offset = frame * HOP;
        for i in 0..FFT_SIZE {
            let j = out_offset + i;
            if j < out_len {
                output[j] += ifft_out[i] * norm * hann[i];
                window_sum[j] += hann[i] * hann[i];
            }
        }
    }

    // Normalize by window overlap sum
    for i in 0..out_len {
        if window_sum[i] > 1e-6 {
            output[i] /= window_sum[i];
        }
    }

    // Fade in over the first HOP samples to avoid onset click from incomplete
    // window overlap at the start of each chunk
    let fade_len = HOP.min(out_len);
    for (i, sample) in output.iter_mut().enumerate().take(fade_len) {
        *sample *= i as f32 / fade_len as f32;
    }

    // Truncate to original input length
    output.truncate(original_len);
    while output.len() < original_len {
        output.push(0.0);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bypass_small_factor() {
        let input: Vec<f32> = (0..1000).map(|i| (i as f32 * 0.1).sin()).collect();
        assert_eq!(
            phase_vocoder_pitch_shift(&input, PitchFactor::from_signed(1.0)),
            input
        );
        assert_eq!(
            phase_vocoder_pitch_shift(&input, PitchFactor::from_signed(-1.0)),
            input
        );
        assert_eq!(
            phase_vocoder_pitch_shift(&input, PitchFactor::from_signed(0.5)),
            input
        );
    }

    #[test]
    fn test_empty_input() {
        assert!(phase_vocoder_pitch_shift(&[], PitchFactor::from_signed(10.0)).is_empty());
    }

    #[test]
    fn test_preserves_length_down() {
        let input: Vec<f32> = (0..8192).map(|i| (i as f32 * 0.01).sin()).collect();
        let output = phase_vocoder_pitch_shift(&input, PitchFactor::from_signed(10.0));
        assert_eq!(output.len(), input.len());
    }

    #[test]
    fn test_preserves_length_up() {
        let input: Vec<f32> = (0..8192).map(|i| (i as f32 * 0.01).sin()).collect();
        let output = phase_vocoder_pitch_shift(&input, PitchFactor::from_signed(-10.0));
        assert_eq!(output.len(), input.len());
    }

    #[test]
    fn test_nonzero_output_down() {
        // 40 kHz tone at 192 kHz sample rate, shifted down by 10
        let sr = 192000.0f32;
        let freq = 40000.0f32;
        let n = 16384;
        let input: Vec<f32> = (0..n)
            .map(|i| (2.0 * PI * freq * i as f32 / sr).sin())
            .collect();
        let output = phase_vocoder_pitch_shift(&input, PitchFactor::from_signed(10.0));
        let peak = output.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(peak > 0.01, "Output should not be silent, peak={}", peak);
    }

    #[test]
    fn test_nonzero_output_up() {
        let input: Vec<f32> = (0..16384).map(|i| (i as f32 * 0.01).sin()).collect();
        let output = phase_vocoder_pitch_shift(&input, PitchFactor::from_signed(-5.0));
        let peak = output.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(peak > 0.01, "Output should not be silent, peak={}", peak);
    }

    /// Dominant frequency (Hz) via the peak FFT bin of a Hann-windowed slice
    /// taken from the middle of the signal (avoids the head fade + edges).
    fn dominant_freq(samples: &[f32], sr: f32) -> f32 {
        use realfft::RealFftPlanner;
        let n = FFT_SIZE.min(samples.len());
        let start = (samples.len() - n) / 2;
        let mut buf: Vec<f32> = samples[start..start + n].to_vec();
        for (i, s) in buf.iter_mut().enumerate() {
            *s *= (PI * i as f32 / n as f32).sin().powi(2);
        }
        let mut planner = RealFftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(n);
        let mut spec = fft.make_output_vec();
        fft.process(&mut buf, &mut spec).unwrap();
        let (mut peak_bin, mut peak_mag) = (0usize, 0.0f32);
        for (k, c) in spec.iter().enumerate() {
            let m = c.re * c.re + c.im * c.im;
            if m > peak_mag {
                peak_mag = m;
                peak_bin = k;
            }
        }
        peak_bin as f32 * sr / n as f32
    }

    #[test]
    fn test_shifts_to_correct_frequency_down() {
        // 24 kHz tone @ 192 kHz, shift DOWN 8x -> expect ~3 kHz. (24 kHz lands on
        // an exact bin so the remap target is clean.)
        let sr = 192000.0f32;
        let input: Vec<f32> = (0..16384)
            .map(|i| (2.0 * PI * 24000.0 * i as f32 / sr).sin())
            .collect();
        let out = phase_vocoder_pitch_shift(&input, PitchFactor::from_signed(8.0));
        let measured = dominant_freq(&out, sr);
        // Generous tolerance: the stateless nearest-bin synthesis scales phase by
        // pitch_factor, which is only approximately frequency-accurate (the
        // "warble"). Down-shift (the bat use case) is the more accurate direction.
        let tol = 3000.0 * 0.05;
        assert!(
            (measured - 3000.0).abs() < tol,
            "expected ~3000 Hz, got {measured} Hz"
        );
    }

    #[test]
    fn test_shifts_to_correct_frequency_up() {
        // 3 kHz tone @ 192 kHz, shift UP 8x (factor -8) -> expect ~24 kHz.
        let sr = 192000.0f32;
        let input: Vec<f32> = (0..16384)
            .map(|i| (2.0 * PI * 3000.0 * i as f32 / sr).sin())
            .collect();
        let out = phase_vocoder_pitch_shift(&input, PitchFactor::from_signed(-8.0));
        let measured = dominant_freq(&out, sr);
        // Wider tolerance for up-shift: scaling phase by a large factor (8) is
        // the least frequency-accurate case of the nearest-bin synthesis.
        let tol = 24000.0 * 0.05;
        assert!(
            (measured - 24000.0).abs() < tol,
            "expected ~24000 Hz, got {measured} Hz"
        );
    }
}
