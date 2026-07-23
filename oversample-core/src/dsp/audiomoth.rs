//! AudioMoth firmware signature detection.
//!
//! Reverse-engineered from upstream [AudioMoth-Firmware-Basic][fw]
//! (`src/butterworth.c`, `src/digitalfilter.c`, `src/main.c`).
//!
//! [fw]: https://github.com/OpenAcousticDevices/AudioMoth-Firmware-Basic
//!
//! ## How AudioMoth shapes the output stream
//!
//! 1. The EFM32's 12-bit ADC samples at a high "raw" rate, with optional
//!    hardware oversampling `OS ∈ {1, 2, 4, ..., 128}` producing a single
//!    signed int16 per oversample period.
//! 2. The firmware sums `D = sampleRateDivider` consecutive raw samples
//!    (`D ∈ {1, 2, 4, 6, 8, 10, 12, 16}`). The sum is an integer (sum of
//!    integers). The output sample rate is `raw_rate / D`.
//! 3. The summed integer is multiplied by `sampleMultiplier =
//!    16 / (OS × D)` — this is the firmware's "additional gain" that
//!    normalises the summed value back toward int16 range.
//! 4. The result is fed into a 1-pole Butterworth high-pass filter
//!    designed at the output sample rate, with cutoff 48 Hz (default)
//!    or 8 Hz (the "low DC blocking filter" option).
//! 5. Output is clipped to int16 range and cast to `int16_t`.
//!
//! ## The detection trick
//!
//! The HPF code from the firmware is:
//!
//! ```text
//!   y[n] = (s[n] - s[n-1]) · G + yc0 · y[n-1]
//! ```
//!
//! where `s[n]` is the integer sum and `G = sampleMultiplier · butterworth_design_gain`.
//! Rearranging:
//!
//! ```text
//!   y[n] - yc0 · y[n-1]  =  G · (s[n] - s[n-1])
//! ```
//!
//! The right-hand side is `G` times an integer. So when we receive
//! quantised `y_q[n] = round(y[n])` and compute
//!
//! ```text
//!   D_recovered[n] = y_q[n] - yc0 · y_q[n-1]
//!                  = G · (s[n] - s[n-1]) + noise
//! ```
//!
//! where `noise = ε[n] - yc0 · ε[n-1]` has support `[-(1+yc0)/2, (1+yc0)/2]`
//! ≈ `[-1, 1]` for the typical `yc0 ≈ 0.999`. Dividing by `G` shrinks the
//! noise envelope to `[-1/G, 1/G]`. For `G ≥ 4` this does **not wrap mod 1**,
//! and the integer-clustering test gives a clean discriminator.
//!
//! Valid `G` values per AudioMoth config (must be `16 / (OS × D)` with
//! `OS × D ∈ {1, 2, 4, 8, 16}`):
//!
//! | G | useful? | mean(\|frac\|) for AudioMoth | uniform baseline |
//! |---|---|---|---|
//! | 16 | YES, very strong | ~0.02 | 0.25 |
//! | 8 | YES, strong | ~0.04 | 0.25 |
//! | 4 | YES, good | ~0.08 | 0.25 |
//! | 2 | marginal — noise wraps | ~0.18 | 0.25 |
//! | 1 | no signature — gain = 1 | 0.25 | 0.25 |
//!
//! We search over `G ∈ {2, 4, 8, 16}` and over `fc ∈ {48, 8}` Hz, picking
//! the candidate with the lowest mean fractional part.

use std::f64::consts::PI;

/// AudioMoth's two firmware cutoff options (Hz).
pub const DEFAULT_CUTOFF_HZ: u32 = 48;
pub const LOW_CUTOFF_HZ: u32 = 8;

/// `OS × sampleRateDivider` values that produce a clean integer-multiple
/// signature (with corresponding `G_total ∈ {16, 8, 4, 2}`).
const OS_DIV_CANDIDATES: &[u32] = &[1, 2, 4, 8];

/// Lower bound on sample rate (Hz). AudioMoth has firmware modes from
/// 8 kHz upward; below this the recording isn't an AudioMoth.
const MIN_SAMPLE_RATE: u32 = 8_000;
/// Upper bound on sample rate (Hz). 384 kHz is the highest firmware mode.
const MAX_SAMPLE_RATE: u32 = 384_000;

const WARMUP: usize = 1024;
const WINDOW: usize = 8192;
const MAX_WINDOWS: usize = 6;
/// Minimum window stdev on the int16 scale to count as having signal.
const SIGNAL_STDEV_GATE: f64 = 30.0;
/// A recovered `D/G` value within this many ULP of an integer counts as
/// "near integer" for display purposes.
const NEAR_INTEGER_THRESHOLD: f64 = 0.1;

#[derive(Clone, Debug, PartialEq)]
pub enum AudioMothVerdict {
    /// Strong match: at least one (fc, G) preset gives mean|frac| well
    /// below the uniform baseline.
    Match,
    /// Some integer-clustering but not as tight as a clean AudioMoth.
    Possible,
    /// No (fc, G) preset clusters on integers.
    NoMatch,
    /// File format / sample rate outside the firmware's envelope.
    NotApplicable,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AudioMothScore {
    pub cutoff_hz: u32,
    /// `OS × sampleRateDivider`; determines `G_total = 16 / this × design_gain`.
    pub os_times_div: u32,
    /// Filter pole used (from Butterworth design at this fc, fs).
    pub yc0: f64,
    /// `sampleMultiplier × butterworth_design_gain`.
    pub gain_total: f64,
    /// Primary discriminator: mean of `|D/G - round(D/G)|`. Uniform
    /// baseline 0.25; tighter integer clustering is smaller.
    pub mean_abs_frac: f64,
    /// Fraction of `D/G` values within ±0.1 of an integer. Useful for
    /// display ("X / N samples landed near-integer").
    pub near_integer_frac: f64,
    pub near_integer_match: usize,
    pub near_integer_total: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AudioMothResult {
    pub verdict: AudioMothVerdict,
    pub explanation: String,
    pub best: Option<AudioMothScore>,
    pub per_candidate: Vec<AudioMothScore>,
    pub windows_used: usize,
    pub windows_skipped_silent: usize,
}

/// Compute the firmware's HPF pole `yc0 = (1 - tan(π·fc/fs)) / (1 + tan(π·fc/fs))`.
/// Mirrors `designFilter()` in upstream `digitalfilter.c`.
pub fn audiomoth_yc0(cutoff_hz: u32, sample_rate: u32) -> f64 {
    let alpha = cutoff_hz as f64 / sample_rate as f64;
    let t = (PI * alpha).tan();
    (1.0 - t) / (1.0 + t)
}

/// Compute the firmware's Butterworth design gain (the `gain` static set
/// by `designFilter()` before `setAdditionalGain` multiplies the
/// `sampleMultiplier` in). For a 1-pole HPF designed at `fc`, this is
/// the reciprocal of the magnitude response at z = -1 (Nyquist).
pub fn audiomoth_design_gain(cutoff_hz: u32, sample_rate: u32) -> f64 {
    let p = audiomoth_yc0(cutoff_hz, sample_rate);
    // topcoeffs = [-1, 1], botcoeffs = [-p, 1]
    // top(-1) = -1 + (-1)·1 = -2
    // bot(-1) = -p + (-1)·1 = -(1 + p)
    let hf_gain = (-2.0_f64) / (-(1.0 + p));
    1.0 / hf_gain.abs()
}

/// Same as `audiomoth_design_gain` but accepts a fractional cutoff in Hz
/// (used when we've solved for `yc0` numerically and converted back).
pub fn audiomoth_design_gain_f(cutoff_hz: f64, sample_rate: u32) -> f64 {
    let alpha = cutoff_hz / sample_rate as f64;
    let t = (PI * alpha).tan();
    let p = (1.0 - t) / (1.0 + t);
    let hf_gain = (-2.0_f64) / (-(1.0 + p));
    1.0 / hf_gain.abs()
}

/// Convert a recovered pole `yc0` back to the implied cutoff frequency.
/// Inverse of `yc0 = (1 - tan(π·fc/fs)) / (1 + tan(π·fc/fs))`.
pub fn yc0_to_cutoff_hz(yc0: f64, sample_rate: u32) -> f64 {
    let t = (1.0 - yc0) / (1.0 + yc0);
    t.atan() / PI * sample_rate as f64
}

#[derive(Clone, Debug)]
struct Yc0Search {
    #[allow(dead_code)]
    yc0: f64,
    mean_abs_frac: f64,
    near_integer_frac: f64,
    near_integer_match: usize,
    near_integer_total: usize,
}

/// Score a single `(yc0, gain_total)` pair on the supplied windows.
fn score_yc0(
    samples: &[f32],
    starts: &[usize],
    yc0: f64,
    gain_total: f64,
    to_y: impl Fn(f32) -> f64,
) -> Yc0Search {
    let mut sum_abs_frac = 0.0f64;
    let mut near_int = 0usize;
    let mut total = 0usize;
    for &start in starts {
        let seg = &samples[start..start + WINDOW];
        let mut prev_y: Option<f64> = None;
        for &s in seg {
            let y = to_y(s);
            if let Some(yp) = prev_y {
                let d = y - yc0 * yp;
                let scaled = d / gain_total;
                let frac = scaled - scaled.round();
                let af = frac.abs();
                sum_abs_frac += af;
                if af < NEAR_INTEGER_THRESHOLD {
                    near_int += 1;
                }
                total += 1;
            }
            prev_y = Some(y);
        }
    }
    let mean_abs_frac = if total > 0 {
        sum_abs_frac / total as f64
    } else {
        f64::NAN
    };
    let near_integer_frac = if total > 0 {
        near_int as f64 / total as f64
    } else {
        0.0
    };
    Yc0Search {
        yc0,
        mean_abs_frac,
        near_integer_frac,
        near_integer_match: near_int,
        near_integer_total: total,
    }
}

/// Detect AudioMoth firmware signature.
///
/// `lsb_is_zero_padded` should be set when the LSB autocorrelation
/// verdict was `ZeroPaddedNBit { .. }` for this file — in that case the
/// AudioMoth integer-multiple test will give false positives (the
/// recorder's output is *already* an integer multiple of some K, so any
/// `yc0 ≈ 1` matches trivially), so we short-circuit to `NotApplicable`.
pub fn detect(
    samples: &[f32],
    sample_rate: u32,
    bits_per_sample: u16,
    is_float: bool,
    lsb_is_zero_padded: bool,
) -> AudioMothResult {
    let default = AudioMothResult {
        verdict: AudioMothVerdict::NotApplicable,
        explanation: String::new(),
        best: None,
        per_candidate: Vec::new(),
        windows_used: 0,
        windows_skipped_silent: 0,
    };

    if is_float || bits_per_sample != 16 {
        return AudioMothResult {
            explanation: "AudioMoth detection only applies to 16-bit integer audio".into(),
            ..default
        };
    }
    if lsb_is_zero_padded {
        return AudioMothResult {
            explanation: "File is zero-padded (low bits literally zero) \u{2014} the \
                          AudioMoth integer-multiple test cannot discriminate from \
                          generic zero-padded recorders in this case"
                .into(),
            ..default
        };
    }
    if !(MIN_SAMPLE_RATE..=MAX_SAMPLE_RATE).contains(&sample_rate) {
        return AudioMothResult {
            explanation: format!(
                "Sample rate {} Hz is outside AudioMoth's firmware envelope \
                 ({}\u{2013}{} Hz)",
                sample_rate, MIN_SAMPLE_RATE, MAX_SAMPLE_RATE,
            ),
            ..default
        };
    }
    if samples.len() < WARMUP + WINDOW * 2 {
        return AudioMothResult {
            explanation: "Recording too short for AudioMoth-signature analysis".into(),
            ..default
        };
    }

    let to_y = |s: f32| -> f64 { (s as f64) * 32768.0 };

    // Pick evenly-spaced windows.
    let usable = samples.len().saturating_sub(WARMUP).saturating_sub(WINDOW);
    let n_win = MAX_WINDOWS.min(1 + usable / WINDOW);
    let stride = if n_win > 1 { usable / (n_win - 1) } else { 0 };
    let starts: Vec<usize> = (0..n_win).map(|i| WARMUP + i * stride).collect();

    // Silence-gate the windows.
    let mut usable_starts = Vec::with_capacity(starts.len());
    let mut skipped = 0usize;
    for &start in &starts {
        let seg = &samples[start..start + WINDOW];
        let mean: f64 = seg.iter().map(|&s| to_y(s)).sum::<f64>() / seg.len() as f64;
        let var: f64 = seg
            .iter()
            .map(|&s| {
                let d = to_y(s) - mean;
                d * d
            })
            .sum::<f64>()
            / seg.len() as f64;
        if var.sqrt() >= SIGNAL_STDEV_GATE {
            usable_starts.push(start);
        } else {
            skipped += 1;
        }
    }

    if usable_starts.is_empty() {
        return AudioMothResult {
            verdict: AudioMothVerdict::NotApplicable,
            explanation: format!(
                "Every analysis window was below the silence gate (stdev < {:.0})",
                SIGNAL_STDEV_GATE,
            ),
            windows_skipped_silent: skipped,
            ..default
        };
    }

    let mut per_candidate = Vec::new();

    // Test the EXACT firmware (cutoff, OS×div) combinations. This is
    // strictly more discriminating than a free yc0 search — a generic
    // 12-bit-ADC recorder will fit at *some* yc0 trivially, but only
    // recordings made by AudioMoth's default-configured firmware will
    // match the specific (48 Hz or 8 Hz, G ∈ {2,4,8,16}) signature.
    for &cutoff in &[DEFAULT_CUTOFF_HZ, LOW_CUTOFF_HZ] {
        let yc0 = audiomoth_yc0(cutoff, sample_rate);
        let design_gain = audiomoth_design_gain(cutoff, sample_rate);
        for &os_div in OS_DIV_CANDIDATES {
            let sample_multiplier = 16.0 / os_div as f64;
            let gain_total = design_gain * sample_multiplier;
            if gain_total < 1.5 {
                continue;
            }
            let s = score_yc0(&samples, &usable_starts, yc0, gain_total, to_y);
            per_candidate.push(AudioMothScore {
                cutoff_hz: cutoff,
                os_times_div: os_div,
                yc0,
                gain_total,
                mean_abs_frac: s.mean_abs_frac,
                near_integer_frac: s.near_integer_frac,
                near_integer_match: s.near_integer_match,
                near_integer_total: s.near_integer_total,
            });
        }
    }

    let best = per_candidate
        .iter()
        .min_by(|a, b| {
            a.mean_abs_frac
                .partial_cmp(&b.mean_abs_frac)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .cloned();

    let (verdict, explanation) = match best.as_ref() {
        Some(s) if s.mean_abs_frac < 0.04 => (
            AudioMothVerdict::Match,
            format!(
                "Sample differences cluster on integer multiples of G = {:.3} \
                 (= {} \u{00D7} sampleMultiplier × design gain) with fitted HPF \
                 cutoff \u{2248} {} Hz. \
                 mean |D/G − round(D/G)| = {:.3} vs 0.25 uniform baseline; \
                 {}/{} = {:.1}% within \u{00B1}0.1. OS\u{00D7}div = {} \
                 \u{2014} strong AudioMoth-firmware signature.",
                s.gain_total,
                16.0 / s.os_times_div as f64,
                s.cutoff_hz,
                s.mean_abs_frac,
                s.near_integer_match,
                s.near_integer_total,
                s.near_integer_frac * 100.0,
                s.os_times_div,
            ),
        ),
        Some(s) if s.mean_abs_frac < 0.10 => (
            AudioMothVerdict::Possible,
            format!(
                "Some integer-clustering at G = {:.3} with fitted HPF cutoff \
                 \u{2248} {} Hz (OS\u{00D7}div = {}): mean |D/G − round(D/G)| \
                 = {:.3} vs 0.25 baseline \u{2014} possible AudioMoth-firmware \
                 signature.",
                s.gain_total, s.cutoff_hz, s.os_times_div, s.mean_abs_frac,
            ),
        ),
        Some(s) => (
            AudioMothVerdict::NoMatch,
            format!(
                "No AudioMoth signature. Best fit was mean |D/G − round(D/G)| \
                 = {:.3} at G = {:.3}, implied cutoff {} Hz.",
                s.mean_abs_frac, s.gain_total, s.cutoff_hz,
            ),
        ),
        None => (AudioMothVerdict::NoMatch, "No candidates tested".into()),
    };

    AudioMothResult {
        verdict,
        explanation,
        best,
        per_candidate,
        windows_used: usable_starts.len(),
        windows_skipped_silent: skipped,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Forward AudioMoth HPF, matching the firmware (gain ≈ 1) for given config.
    fn forward_audiomoth_hpf(
        raw_samples: &[i32],
        fs_output: u32,
        fc: u32,
        sample_rate_divider: u32,
        oversample_rate: u32,
    ) -> Vec<f64> {
        let yc0 = audiomoth_yc0(fc, fs_output);
        let design_gain = audiomoth_design_gain(fc, fs_output);
        let sample_multiplier = 16.0 / (oversample_rate as f64 * sample_rate_divider as f64);
        let total_gain = design_gain * sample_multiplier;

        let mut xv0 = 0.0;
        let mut xv1 = 0.0;
        let mut yv1 = 0.0;
        let mut out = Vec::with_capacity(raw_samples.len() / sample_rate_divider as usize);
        for chunk in raw_samples.chunks_exact(sample_rate_divider as usize) {
            let s: f64 = chunk.iter().map(|&v| v as f64).sum();
            xv0 = xv1;
            xv1 = s * total_gain;
            let y = xv1 - xv0 + yc0 * yv1;
            yv1 = y;
            out.push(y);
        }
        out
    }

    fn quantise_to_int16(samples: Vec<f64>) -> Vec<f32> {
        samples
            .into_iter()
            .map(|v| (v.round().clamp(-32768.0, 32767.0) / 32768.0) as f32)
            .collect()
    }

    #[test]
    fn yc0_formula_matches_firmware() {
        // At fs=384k, fc=48 → expected ≈ 0.99921
        let yc = audiomoth_yc0(48, 384_000);
        assert!((yc - 0.99921).abs() < 1e-4, "got yc0={}", yc);
        // At fs=48k, fc=48 → much smaller pole
        let yc = audiomoth_yc0(48, 48_000);
        assert!(yc < 0.995 && yc > 0.99, "got yc0={}", yc);
    }

    #[test]
    fn design_gain_close_to_one() {
        // Should be ~1 for all reasonable cutoffs (fc << fs).
        for &fs in &[16_000u32, 48_000, 96_000, 192_000, 384_000] {
            let g = audiomoth_design_gain(48, fs);
            assert!((g - 1.0).abs() < 0.01, "fs={} got gain={}", fs, g);
        }
    }

    /// The discriminator works at *any* sample rate when G = 16 (i.e., a
    /// single-sample divider with no oversampling) — that's the highest-
    /// rate AudioMoth output mode (384 kHz at sampleRateDivider=1).
    #[test]
    fn detect_accepts_high_rate_single_divider() {
        let fs_output = 384_000u32;
        let fc = 48u32;
        let n = 8 * WINDOW;
        // Synthesise raw 12-bit ADC stream
        let raw: Vec<i32> = (0..n)
            .map(|i| {
                let t = i as f64 / fs_output as f64;
                let s = 0.0
                    + 800.0 * (2.0 * PI * 18_000.0 * t).sin()
                    + 200.0 * (2.0 * PI * 31_000.0 * t).cos();
                (s.round() as i32).clamp(-2048, 2047)
            })
            .collect();
        let y = forward_audiomoth_hpf(&raw, fs_output, fc, 1, 1);
        let samples = quantise_to_int16(y);
        let r = detect(&samples, fs_output, 16, false, false);
        assert!(
            matches!(r.verdict, AudioMothVerdict::Match),
            "expected Match at G=16, got {:?}",
            r,
        );
        let best = r.best.as_ref().unwrap();
        assert_eq!(best.os_times_div, 1);
        assert_eq!(best.cutoff_hz, 48);
    }

    /// Standard AudioMoth config (48 kHz output, sampleRateDivider=8 from
    /// 384 kHz raw, oversample=1) → G=2. The test is marginal at G=2 (the
    /// noise envelope just touches the wrap boundary) so we accept either
    /// Match or Possible.
    #[test]
    fn detect_default_config_at_g2() {
        let fs_output = 48_000u32;
        let fc = 48u32;
        // Sampling at 384 kHz raw with D=8 gives 48 kHz output. We
        // simulate the chain by generating 384 kHz raw samples and the
        // forward filter sums them in 8s.
        let n = 8 * WINDOW * 8; // 8 windows worth of OUTPUT samples
        let raw: Vec<i32> = (0..n)
            .map(|i| {
                let t = i as f64 / 384_000.0;
                let s = 0.0
                    + 800.0 * (2.0 * PI * 4_000.0 * t).sin()
                    + 200.0 * (2.0 * PI * 12_000.0 * t).cos();
                (s.round() as i32).clamp(-2048, 2047)
            })
            .collect();
        let y = forward_audiomoth_hpf(&raw, fs_output, fc, 8, 1);
        let samples = quantise_to_int16(y);
        let r = detect(&samples, fs_output, 16, false, false);
        assert!(
            matches!(
                r.verdict,
                AudioMothVerdict::Match | AudioMothVerdict::Possible
            ),
            "expected Match or Possible at G=2, got {:?}",
            r,
        );
    }

    #[test]
    fn detect_rejects_white_noise() {
        let mut state: u32 = 0x12345678;
        let samples: Vec<f32> = (0..8 * WINDOW)
            .map(|_| {
                state = state.wrapping_mul(1664525).wrapping_add(1013904223);
                ((state >> 16) as i32 - 32768) as f32 / 32768.0 * 0.5
            })
            .collect();
        let r = detect(&samples, 192_000, 16, false, false);
        assert!(
            !matches!(r.verdict, AudioMothVerdict::Match),
            "false-positive on white noise: {:?}",
            r,
        );
    }

    #[test]
    fn detect_suppresses_when_zero_padded() {
        // Generate plausibly-AudioMoth-shaped data but pretend LSB analysis
        // already flagged it as zero-padded. Should return NotApplicable.
        let fs = 192_000u32;
        let samples: Vec<f32> = (0..8 * WINDOW)
            .map(|i| {
                let t = i as f64 / fs as f64;
                ((2.0 * PI * 12_000.0 * t).sin() * 0.3) as f32
            })
            .collect();
        let r = detect(&samples, fs, 16, false, true /* zero-padded */);
        assert!(matches!(r.verdict, AudioMothVerdict::NotApplicable));
    }

    #[test]
    fn detect_rejects_pure_tone() {
        // A pure analog-style tone (not generated through the AudioMoth HPF)
        // should NOT match.
        let fs = 192_000u32;
        let samples: Vec<f32> = (0..8 * WINDOW)
            .map(|i| {
                let t = i as f64 / fs as f64;
                (2.0 * PI * 12_000.0 * t).sin() as f32 * 0.5
            })
            .collect();
        let r = detect(&samples, fs, 16, false, false);
        assert!(
            !matches!(r.verdict, AudioMothVerdict::Match),
            "false-positive on pure tone: {:?}",
            r,
        );
    }
}
