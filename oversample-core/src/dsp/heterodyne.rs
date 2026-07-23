use crate::dsp::filters::lowpass_filter;
use std::f64::consts::PI;

/// Simulate a heterodyne bat detector by mixing (multiplying) the input signal
/// with a local oscillator, then low-pass filtering to extract the difference
/// frequency. This shifts ultrasonic frequencies down into the audible range.
///
/// The heterodyne principle: if the input signal has frequency f_in and the
/// local oscillator has frequency f_lo, multiplication produces two components:
///   - f_in + f_lo  (sum, removed by low-pass filter)
///   - |f_in - f_lo| (difference, the audible output)
pub fn heterodyne_mix(samples: &[f32], sample_rate: u32, lo_freq: f64, cutoff_hz: f64) -> Vec<f32> {
    let sr = sample_rate as f64;
    let angular_freq = 2.0 * PI * lo_freq;

    // Step 1: Generate local oscillator and multiply with input (ring modulation)
    let mixed: Vec<f32> = samples
        .iter()
        .enumerate()
        .map(|(i, &sample)| {
            let t = i as f64 / sr;
            let lo_sample = (angular_freq * t).cos() as f32;
            sample * lo_sample
        })
        .collect();

    // Step 2: Cascaded low-pass filter to remove the sum frequency component.
    // 4 passes of a single-pole IIR (EMA) give a -24 dB/octave ASYMPTOTIC
    // rolloff. NOTE: this is NOT a 4th-order Butterworth — the four real poles
    // sit at the same cutoff (a "synchronously-tuned" cascade), so the knee is
    // gentle, the effective -3 dB point falls below `cutoff_hz`, and there is no
    // maximally-flat passband. Only the far-stopband slope matches Butterworth.
    let mut filtered = mixed;
    for _ in 0..4 {
        filtered = lowpass_filter(&filtered, cutoff_hz, sample_rate);
    }
    filtered
}

/// Stateful real-time heterodyne processor for live mic monitoring.
/// Maintains oscillator phase and cascaded LP filter states between
/// consecutive audio buffers to avoid clicks and transients.
pub struct RealtimeHet {
    phase: f64,
    lp_state: [f32; 4],
}

impl Default for RealtimeHet {
    fn default() -> Self {
        Self::new()
    }
}

impl RealtimeHet {
    pub fn new() -> Self {
        Self {
            phase: 0.0,
            lp_state: [0.0; 4],
        }
    }

    /// Process `input` through heterodyne (ring modulation + 4-pass LP) and
    /// write result into `output`. Both slices must have the same length.
    pub fn process(
        &mut self,
        input: &[f32],
        output: &mut [f32],
        sample_rate: u32,
        lo_freq: f64,
        cutoff_hz: f64,
    ) {
        let sr = sample_rate as f64;
        let phase_inc = 2.0 * PI * lo_freq / sr;
        let dt = 1.0 / sr;
        let rc = 1.0 / (2.0 * PI * cutoff_hz);
        let alpha = (dt / (rc + dt)) as f32;

        for (i, &sample) in input.iter().enumerate() {
            // Ring modulation with continuous phase
            let lo = (self.phase + phase_inc * i as f64).cos() as f32;
            let mut val = sample * lo;

            // 4-pass cascaded single-pole LP filter
            for s in self.lp_state.iter_mut() {
                val = alpha * val + (1.0 - alpha) * *s;
                *s = val;
            }

            output[i] = val;
        }

        // Advance phase, keep in [0, 2π) to avoid precision loss
        self.phase = (self.phase + phase_inc * input.len() as f64) % (2.0 * PI);
    }

    /// Reset state (call when HET params change significantly or mic restarts)
    pub fn reset(&mut self) {
        self.phase = 0.0;
        self.lp_state = [0.0; 4];
    }
}

/// Comb heterodyne — mix the input against N evenly-spaced carriers and sum
/// the (lowpass-filtered) difference tones. Lets the user cover a wide
/// ultrasonic range without retuning, at the cost of overlapping difference
/// images when the carrier spacing is tighter than 2× cutoff. Spacing equal
/// to roughly 2× cutoff gives near-seamless coverage with minimal overlap.
pub fn heterodyne_comb_mix(
    samples: &[f32],
    sample_rate: u32,
    carrier_freqs: &[f64],
    cutoff_hz: f64,
) -> Vec<f32> {
    if carrier_freqs.is_empty() || samples.is_empty() {
        return samples.to_vec();
    }
    let mut acc = vec![0.0f32; samples.len()];
    for &lo in carrier_freqs {
        let band = heterodyne_mix(samples, sample_rate, lo, cutoff_hz);
        for (a, b) in acc.iter_mut().zip(band.iter()) {
            *a += *b;
        }
    }
    // Normalize by carrier count so the perceived loudness doesn't scale
    // with N (and so flipping count up/down isn't a volume jump).
    let scale = 1.0 / carrier_freqs.len() as f32;
    for s in acc.iter_mut() {
        *s *= scale;
    }
    acc
}

/// Stateful real-time comb heterodyne processor. Tracks a phase + 4-pass LP
/// state per carrier and sums the resulting difference tones. Output is
/// scaled by 1/N so the level is comparable to single-carrier heterodyne.
pub struct RealtimeCombHet {
    /// Per-carrier oscillator phase and cascaded LP state.
    voices: Vec<RealtimeHet>,
}

impl Default for RealtimeCombHet {
    fn default() -> Self {
        Self::new()
    }
}

impl RealtimeCombHet {
    pub fn new() -> Self {
        Self { voices: Vec::new() }
    }

    /// Resize the voice bank to match the requested carrier count. New voices
    /// start with zero phase; existing voices keep their state to avoid
    /// clicks when the count changes during monitoring.
    fn ensure_n(&mut self, n: usize) {
        if self.voices.len() < n {
            self.voices.resize_with(n, RealtimeHet::new);
        } else if self.voices.len() > n {
            self.voices.truncate(n);
        }
    }

    pub fn process(
        &mut self,
        input: &[f32],
        output: &mut [f32],
        sample_rate: u32,
        carrier_freqs: &[f64],
        cutoff_hz: f64,
    ) {
        let n = carrier_freqs.len();
        if n == 0 || input.is_empty() {
            for o in output.iter_mut() {
                *o = 0.0;
            }
            return;
        }
        self.ensure_n(n);

        // Process the first carrier into the output buffer, then accumulate
        // the rest. Avoids a separate zero-fill pass.
        let mut scratch = vec![0.0f32; input.len()];
        self.voices[0].process(input, output, sample_rate, carrier_freqs[0], cutoff_hz);
        for (i, &lo) in carrier_freqs.iter().enumerate().skip(1) {
            self.voices[i].process(input, &mut scratch, sample_rate, lo, cutoff_hz);
            for (o, s) in output.iter_mut().zip(scratch.iter()) {
                *o += *s;
            }
        }
        let scale = 1.0 / n as f32;
        for s in output.iter_mut() {
            *s *= scale;
        }
    }

    pub fn reset(&mut self) {
        for v in &mut self.voices {
            v.reset();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_heterodyne_shifts_frequency_down() {
        let sample_rate = 192_000u32;
        let input_freq = 45_000.0; // 45 kHz bat call
        let lo_freq = 44_000.0; // Tuned to 44 kHz
                                // Expected difference: 1 kHz (audible)

        let duration = 0.05; // 50ms
        let num_samples = (sample_rate as f64 * duration) as usize;

        let input: Vec<f32> = (0..num_samples)
            .map(|i| {
                let t = i as f64 / sample_rate as f64;
                (2.0 * std::f64::consts::PI * input_freq * t).sin() as f32
            })
            .collect();

        let output = heterodyne_mix(&input, sample_rate, lo_freq, 15_000.0);
        assert_eq!(output.len(), input.len());

        // Verify the output has energy (is not all zeros)
        let rms: f64 =
            (output.iter().map(|s| (*s as f64).powi(2)).sum::<f64>() / output.len() as f64).sqrt();
        assert!(
            rms > 0.01,
            "Output should have significant energy, got RMS={rms}"
        );

        // Use zero-crossing on the output to verify it's near 1 kHz
        let zc = crate::dsp::zero_crossing::zero_crossing_frequency(&output, sample_rate);
        let error = (zc.estimated_frequency_hz - 1000.0).abs();
        assert!(
            error < 200.0,
            "Expected ~1000 Hz difference tone, got {} Hz",
            zc.estimated_frequency_hz
        );
    }

    #[test]
    fn test_heterodyne_empty_input() {
        let output = heterodyne_mix(&[], 192_000, 45_000.0, 15_000.0);
        assert!(output.is_empty());
    }

    #[test]
    fn test_comb_heterodyne_picks_up_tone_near_any_carrier() {
        // 3 carriers spread across 30/50/70 kHz with a 6 kHz lowpass cutoff.
        // A 53 kHz tone is 3 kHz from the 50 kHz carrier (inside the band)
        // and 17/23 kHz from the others (well outside).
        let sample_rate = 192_000u32;
        let input_freq = 53_000.0;
        let carriers = [30_000.0, 50_000.0, 70_000.0];
        let cutoff = 6_000.0;

        let duration = 0.05;
        let num_samples = (sample_rate as f64 * duration) as usize;
        let input: Vec<f32> = (0..num_samples)
            .map(|i| {
                let t = i as f64 / sample_rate as f64;
                (2.0 * std::f64::consts::PI * input_freq * t).sin() as f32
            })
            .collect();

        let output = heterodyne_comb_mix(&input, sample_rate, &carriers, cutoff);
        assert_eq!(output.len(), input.len());

        // Skip the LP startup transient before measuring RMS / ZC.
        let warmup = (sample_rate as f64 * 0.005) as usize;
        let tail = &output[warmup..];

        let rms: f64 =
            (tail.iter().map(|s| (*s as f64).powi(2)).sum::<f64>() / tail.len() as f64).sqrt();
        assert!(
            rms > 0.005,
            "Comb output should pick up the tone, got RMS={rms}"
        );

        let zc = crate::dsp::zero_crossing::zero_crossing_frequency(tail, sample_rate);
        let error = (zc.estimated_frequency_hz - 3000.0).abs();
        assert!(
            error < 400.0,
            "Expected ~3000 Hz difference tone from the 50 kHz carrier, got {} Hz",
            zc.estimated_frequency_hz
        );
    }

    #[test]
    fn test_comb_heterodyne_empty_carriers_is_passthrough() {
        let input = vec![0.5f32, -0.5, 0.25, -0.25];
        let output = heterodyne_comb_mix(&input, 48_000, &[], 5_000.0);
        assert_eq!(output, input);
    }
}
