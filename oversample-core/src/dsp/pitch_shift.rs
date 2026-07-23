use serde::{Deserialize, Serialize};

/// A sign-encoded pitch-shift factor (shared by PitchShift and PhaseVocoder).
///
/// The historical convention packs three things into one number, and is kept on
/// the wire/UI (stored values and slider positions are unchanged):
///
/// - magnitude > 1, positive sign → shift DOWN (divide frequencies; 10 ⇒ 50 kHz → 5 kHz)
/// - magnitude > 1, negative sign → shift UP   (multiply frequencies; -10 ⇒ 5 kHz → 50 kHz)
/// - `|value| <= 1` → bypass (returns input unchanged)
///
/// Call sites use the named accessors below instead of re-deriving
/// `abs()`/`< 0.0`/`<= 1.0`, so the direction can no longer be inverted by
/// accident. `#[serde(transparent)]` keeps the serialized form a bare number.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PitchFactor(f64);

impl PitchFactor {
    /// Wrap a raw sign-encoded factor (the historical convention).
    pub fn from_signed(value: f64) -> Self {
        Self(value)
    }
    /// Shift DOWN by `magnitude`× (e.g. 50 kHz → 5 kHz at 10×).
    pub fn down(magnitude: f64) -> Self {
        Self(magnitude.abs())
    }
    /// Shift UP by `magnitude`× (e.g. 5 kHz → 50 kHz at 10×).
    pub fn up(magnitude: f64) -> Self {
        Self(-magnitude.abs())
    }
    /// The raw sign-encoded value (for storage, display, and UI sliders).
    pub fn signed(self) -> f64 {
        self.0
    }
    /// Effective magnitude, clamped to ≥ 1 (matches the old `abs().max(1.0)`).
    pub fn magnitude(self) -> f64 {
        self.0.abs().max(1.0)
    }
    /// True when `|value| <= 1`, i.e. the shift is a no-op.
    pub fn is_bypass(self) -> bool {
        self.0.abs() <= 1.0
    }
    /// True when the factor shifts frequencies UP (negative sign).
    pub fn is_up(self) -> bool {
        self.0 < 0.0
    }
}

impl From<PitchFactor> for f64 {
    fn from(f: PitchFactor) -> f64 {
        f.0
    }
}

/// Pitch-shift audio by `factor` while preserving original duration.
/// See [`PitchFactor`] for the sign/bypass convention.
pub fn pitch_shift_realtime(samples: &[f32], factor: PitchFactor) -> Vec<f32> {
    if samples.is_empty() {
        return samples.to_vec();
    }

    if factor.is_bypass() {
        return samples.to_vec();
    }

    let abs_factor = factor.magnitude();
    let shift_up = factor.is_up();

    // Step 1: resample to change frequencies
    let resampled = if shift_up {
        resample_compress(samples, abs_factor) // shorter, higher freq
    } else {
        resample_stretch(samples, abs_factor) // longer, lower freq
    };

    // Step 2: OLA to restore original duration
    // Shift down: resampled is longer → compress with analysis_hop > synthesis_hop
    // Shift up:   resampled is shorter → stretch with analysis_hop < synthesis_hop
    let window_size: usize = 2048;
    let synthesis_hop = window_size / 2;
    let analysis_hop = if shift_up {
        (synthesis_hop as f64 / abs_factor).max(1.0) as usize
    } else {
        (synthesis_hop as f64 * abs_factor) as usize
    };

    let out_len = samples.len();
    let mut output = vec![0.0f32; out_len];
    let mut window_sum = vec![0.0f32; out_len];

    // Hann window
    let hann: Vec<f32> = (0..window_size)
        .map(|i| {
            let x = std::f32::consts::PI * i as f32 / window_size as f32;
            x.sin().powi(2)
        })
        .collect();

    let mut read_pos = 0usize;
    let mut write_pos = 0usize;

    while read_pos + window_size <= resampled.len() && write_pos + window_size <= out_len {
        for i in 0..window_size {
            output[write_pos + i] += resampled[read_pos + i] * hann[i];
            window_sum[write_pos + i] += hann[i];
        }
        read_pos += analysis_hop;
        write_pos += synthesis_hop;
    }

    // Normalize by window overlap sum
    for i in 0..out_len {
        if window_sum[i] > 0.001 {
            output[i] /= window_sum[i];
        }
    }

    output
}

/// Resample by stretching: output is longer, frequencies lower.
pub fn resample_stretch(samples: &[f32], factor: f64) -> Vec<f32> {
    let out_len = (samples.len() as f64 * factor) as usize;
    let mut output = Vec::with_capacity(out_len);

    for i in 0..out_len {
        let src_pos = i as f64 / factor;
        let idx = src_pos as usize;
        let frac = (src_pos - idx as f64) as f32;

        let s0 = samples[idx.min(samples.len() - 1)];
        let s1 = samples[(idx + 1).min(samples.len() - 1)];
        output.push(s0 + frac * (s1 - s0));
    }

    output
}

/// Resample by compressing: output is shorter, frequencies higher.
pub fn resample_compress(samples: &[f32], factor: f64) -> Vec<f32> {
    let out_len = (samples.len() as f64 / factor) as usize;
    let mut output = Vec::with_capacity(out_len);

    for i in 0..out_len {
        let src_pos = i as f64 * factor;
        let idx = src_pos as usize;
        let frac = (src_pos - idx as f64) as f32;

        let s0 = samples[idx.min(samples.len() - 1)];
        let s1 = samples[(idx + 1).min(samples.len() - 1)];
        output.push(s0 + frac * (s1 - s0));
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resample_stretch_doubles_length() {
        let input: Vec<f32> = (0..100).map(|i| (i as f32 / 100.0).sin()).collect();
        let output = resample_stretch(&input, 2.0);
        assert_eq!(output.len(), 200);
    }

    #[test]
    fn test_resample_compress_halves_length() {
        let input: Vec<f32> = (0..100).map(|i| (i as f32 / 100.0).sin()).collect();
        let output = resample_compress(&input, 2.0);
        assert_eq!(output.len(), 50);
    }

    #[test]
    fn test_pitch_shift_down_preserves_length() {
        let input: Vec<f32> = (0..4096).map(|i| (i as f32 / 100.0).sin()).collect();
        let output = pitch_shift_realtime(&input, PitchFactor::from_signed(10.0));
        assert_eq!(output.len(), input.len());
    }

    #[test]
    fn test_pitch_shift_up_preserves_length() {
        let input: Vec<f32> = (0..4096).map(|i| (i as f32 / 100.0).sin()).collect();
        let output = pitch_shift_realtime(&input, PitchFactor::from_signed(-10.0));
        assert_eq!(output.len(), input.len());
    }

    #[test]
    fn test_pitch_shift_bypass() {
        let input: Vec<f32> = vec![1.0, 2.0, 3.0, 4.0];
        assert_eq!(
            pitch_shift_realtime(&input, PitchFactor::from_signed(0.0)),
            input
        );
        assert_eq!(
            pitch_shift_realtime(&input, PitchFactor::from_signed(1.0)),
            input
        );
        assert_eq!(
            pitch_shift_realtime(&input, PitchFactor::from_signed(-1.0)),
            input
        );
    }

    #[test]
    fn test_pitch_shift_empty() {
        assert!(pitch_shift_realtime(&[], PitchFactor::from_signed(10.0)).is_empty());
        assert!(pitch_shift_realtime(&[], PitchFactor::from_signed(-10.0)).is_empty());
    }
}
