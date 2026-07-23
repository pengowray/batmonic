//! Pipistrelle-family firmware signature detection.
//!
//! Phil Atkin / Omenie's open-source RP2040 bat-detector firmware ships in
//! several products that all share the same DSP pipeline: the **Pipistrelle**
//! handheld scanning heterodyne detector, **Pipmini** (mini variant),
//! **Pippyg** (passive detector), the **Pipistrelle USB Microphone**, and
//! **Batsynth** (echolocation synthesiser). All sample the RP2040's 12-bit
//! ADC at 384 kHz, then high-pass and band-cut the stream in fixed-point
//! before packaging into a 16-bit USB Audio Class container. The lower bits
//! of the 16-bit output are *not* analog noise; they are deterministic IIR
//! residue from the firmware's fixed-point math.
//!
//! On silent / short-circuit recordings this is directly visible in the LSB
//! statistics (see `lsb_autocorr` module). On real recordings, analog noise
//! dithers the LSBs into apparent uniformity and statistical tests become
//! inconclusive. But the firmware DSP is fully deterministic and stably
//! invertible, so we can run the inverse filter chain and check whether the
//! recovered "ADC input" lands on integers in the 12-bit range. If it does,
//! the recording almost certainly came from this firmware family; if not,
//! it didn't.
//!
//! Firmware reference: `analog_microphone.hpp` from the pipistrelle-usb-mic
//! source. Filter chain (HPF → biquad):
//!
//! ```text
//!   outFIX = (v - 2048) << 16  -  dFIX
//!   dFIX  += outFIX >> 4                              // 1-pole HPF, pole 15/16
//!   HPF[n] = outFIX >> 12
//!   iy    = (b0*ix + b1*(ix1-iy1) + b2*ix2 - a2*iy2) >> 13   // biquad band-cut ~24 kHz
//! ```
//!
//! Note that `a1 = b1` in this biquad (custom coupled topology, not standard
//! Direct Form II). Five coefficient presets exist, indexed by `dBcut` of
//! 3, 6, 9, 12, 32 dB.

/// Biquad coefficient preset matching the firmware's `filterInit(dBcut)`.
/// `a1 = b1` by construction (see `kib1*(ix1-iy1)` in the firmware source).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PipistrelleCoeffs {
    pub db_cut: u8,
    pub b0: f64,
    pub b1: f64,
    pub b2: f64,
    pub a2: f64,
}

/// All five `filterInit` presets from the upstream firmware.
pub const PRESETS: &[PipistrelleCoeffs] = &[
    PipistrelleCoeffs {
        db_cut: 3,
        b0: 0.99192746,
        b1: -1.79668601,
        b2: 0.95279146,
        a2: 0.94471892,
    },
    PipistrelleCoeffs {
        db_cut: 6,
        b0: 0.98369852,
        b1: -1.78737325,
        b2: 0.95094035,
        a2: 0.93463886,
    },
    PipistrelleCoeffs {
        db_cut: 9,
        b0: 0.97509378,
        b1: -1.77642980,
        b2: 0.94769998,
        a2: 0.92279376,
    },
    PipistrelleCoeffs {
        db_cut: 12,
        b0: 0.91071516,
        b1: -1.59205495,
        b2: 0.81251270,
        a2: 0.72322786,
    },
    PipistrelleCoeffs {
        db_cut: 32,
        b0: 0.99209049,
        b1: -1.83314419,
        b2: 0.99209049,
        a2: 0.98418098,
    },
];

#[derive(Clone, Debug, PartialEq)]
pub enum PipistrelleVerdict {
    /// Strong match: the inverse filter recovered near-integer 12-bit ADC
    /// values across enough of the recording to be confident.
    Match,
    /// Weak match: inverse is plausible but not conclusive (e.g. small file,
    /// borderline residual). Worth flagging but not asserting.
    Possible,
    /// The inverse did not recover integer 12-bit values — the recording
    /// did not come from this firmware family (or the coefficient set we
    /// know about).
    NoMatch,
    /// Skipped (wrong format, too short, etc.).
    NotApplicable,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PipistrelleResult {
    pub verdict: PipistrelleVerdict,
    pub explanation: String,
    /// Best-matching coefficient preset (the `db_cut` value), if any preset
    /// was a plausible fit.
    pub best_db_cut: Option<u8>,
    /// **Primary discriminator.** Of the in-range recovered ADC values,
    /// what fraction land within ±0.1 of an integer. ~1.0 for a real
    /// pipistrelle recording (the inverse recovers the actual 12-bit ADC
    /// integers, modulo firmware truncation noise); ~0.20 for an unrelated
    /// 16-bit signal (uniform fractional part on [-0.5, +0.5]).
    pub best_near_integer_frac: f64,
    /// Numerator / denominator behind `best_near_integer_frac` — for
    /// user-facing display ("99% (8112/8192) of samples matched").
    pub best_near_integer_match: usize,
    pub best_near_integer_total: usize,
    /// Primary discriminator for `best`. `mean(|v - round(v)|)` over
    /// in-range recovered samples for the best-fit preset.
    pub best_mean_abs_frac: f64,
    /// Normalized forward residual for the best preset: after inverse
    /// filtering, rounding recovered ADC values to integers, and re-running
    /// the forward filter, this is the RMS of (predicted − actual) divided
    /// by the actual signal stdev. ~0 for a real pipistrelle recording
    /// (only firmware truncation noise); ~1 for an unrelated signal.
    /// Secondary metric — `best_near_integer_frac` is more discriminating.
    pub best_normalized_residual: f64,
    /// Fraction of recovered samples falling inside the 12-bit ADC range
    /// [0, 4095]. ~1.0 for pipistrelle; less for unrelated recordings.
    pub best_in_range_frac: f64,
    /// Per-preset diagnostic scores, for debugging / display.
    pub per_preset: Vec<PipistrelleScore>,
    /// Number of samples that were inverse-filtered (after warmup discard).
    pub samples_analyzed: usize,
    /// Number of windows averaged across.
    pub windows_used: usize,
    /// Number of windows that were skipped because their amplitude was
    /// below the silence-gate threshold (`SIGNAL_STDEV_GATE`).
    pub windows_skipped_silent: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PipistrelleScore {
    pub db_cut: u8,
    /// Fraction of in-range recovered samples within ±0.1 of an integer.
    pub near_integer_frac: f64,
    /// Numerator / denominator behind `near_integer_frac`.
    pub near_integer_match: usize,
    pub near_integer_total: usize,
    /// Mean of `|v - round(v)|` over in-range recovered samples — the
    /// primary verdict discriminator. Uniform baseline 0.25; real
    /// pipistrelle ~0.05-0.15.
    pub mean_abs_frac: f64,
    pub normalized_residual: f64,
    pub in_range_frac: f64,
}

const WINDOW: usize = 8192;
const WARMUP: usize = 1024;
/// Number of evenly-spaced windows to analyze across the file.
const MAX_WINDOWS: usize = 6;
/// Native ADC sample rate that the firmware's filter coefficients were
/// designed for. Detection on files at other rates (e.g. resampled offline)
/// produces unreliable verdicts.
pub const NATIVE_SAMPLE_RATE: u32 = 384_000;
/// Tolerance band around the native rate (±10%).
const SAMPLE_RATE_TOLERANCE: f64 = 0.10;
/// A recovered ADC value is considered "near-integer" if its distance to
/// the nearest integer is below this threshold. Used for the user-facing
/// display ("X / N samples landed near-integer") and as a soft diagnostic
/// — not the primary verdict discriminator, because the inverse biquad
/// amplifies int16 quantisation noise enough that even a real pipistrelle
/// recording produces a near-uniform distribution of fractional parts.
const NEAR_INTEGER_THRESHOLD: f64 = 0.1;
/// Minimum window stdev (on the int16 scale) to count as having enough
/// signal to drive the inverse-filter test. Quieter windows are skipped
/// because the inverse-filter output stays near zero and trivially
/// rounds to a constant integer. Real pipistrelle bat recordings
/// frequently have many quiet 8192-sample windows between calls; we
/// keep this gate fairly loose so we don't end up with zero usable
/// windows per file.
const SIGNAL_STDEV_GATE: f64 = 20.0;
/// Hard gate on sample rate. The firmware's biquad coefficients are
/// designed for the RP2040 12-bit ADC running at exactly 384 kHz, so we
/// want a tight gate. ±1 % covers normal clock drift and accidental
/// resampling tolerances without admitting the common Avisoft rates
/// (333/375/400 kHz) or other near-384k variants.
const SAMPLE_RATE_HARD_GATE: f64 = 0.01;
/// Forward-roundtrip normalised-residual thresholds for the verdict bands.
/// **Empirically calibrated against 45 known-pipistrelle-family recordings
/// (pippyg.com demos + direct-from-device files)**:
///
/// - Residuals on real recordings span 0.14 % to 7 % depending on signal
///   amplitude, quiet-section content and ambient noise.
/// - All 24 non-silent real-pipistrelle files give in_range = 100 % and
///   best_preset = dBcut 12 (the firmware default).
/// - For non-pipistrelle files at 384 kHz (AudioMoth, Pettersson D1000X),
///   residuals overlap heavily — many also give in_range=100 % with
///   dBcut=12 best. The discriminator is *NOT* unique to Pipistrelle on
///   its own. The `device_hint` layer applies the final disambiguation:
///   if `audiomoth::detect` or `lsb_autocorr::QuietSectionZeroPadded`
///   already fired, the file belongs to a *different* firmware family
///   and Pipistrelle's verdict should be suppressed at the hint level.
///
/// We therefore aim for high recall at the detector level (catch as many
/// genuine pipistrelle files as we can) and rely on `device_hint` to
/// suppress when conflicting detectors fire.
const RESIDUAL_MATCH_THRESHOLD: f64 = 0.02;
const RESIDUAL_POSSIBLE_THRESHOLD: f64 = 0.06;

/// Run all five coefficient presets on the recording and report the best fit.
/// `samples` are normalized f32 in [-1, 1]; `bits_per_sample` must be 16.
/// `sample_rate` is used only for a guard rail: files not at the firmware's
/// native ADC rate are flagged but still analyzed.
pub fn detect(
    samples: &[f32],
    sample_rate: u32,
    bits_per_sample: u16,
    is_float: bool,
) -> PipistrelleResult {
    let default = PipistrelleResult {
        verdict: PipistrelleVerdict::NotApplicable,
        explanation: String::new(),
        best_db_cut: None,
        best_near_integer_frac: 0.0,
        best_near_integer_match: 0,
        best_near_integer_total: 0,
        best_mean_abs_frac: f64::NAN,
        best_normalized_residual: f64::NAN,
        best_in_range_frac: 0.0,
        per_preset: Vec::new(),
        samples_analyzed: 0,
        windows_used: 0,
        windows_skipped_silent: 0,
    };

    if is_float || bits_per_sample != 16 {
        return PipistrelleResult {
            explanation: "Pipistrelle detection only applies to 16-bit integer audio".into(),
            ..default
        };
    }
    if samples.len() < WARMUP + WINDOW * 2 {
        return PipistrelleResult {
            explanation: "Recording too short for inverse-filter analysis".into(),
            ..default
        };
    }

    let rate_ratio = sample_rate as f64 / NATIVE_SAMPLE_RATE as f64;
    if (rate_ratio - 1.0).abs() > SAMPLE_RATE_HARD_GATE {
        return PipistrelleResult {
            explanation: format!(
                "Sample rate {} Hz is not within \u{00B1}{:.0}% of the firmware's \
                 native {} Hz \u{2014} cannot meaningfully test for this signature",
                sample_rate,
                SAMPLE_RATE_HARD_GATE * 100.0,
                NATIVE_SAMPLE_RATE,
            ),
            ..default
        };
    }
    let rate_mismatch = (rate_ratio - 1.0).abs() > SAMPLE_RATE_TOLERANCE;
    let rate_note = if rate_mismatch {
        format!(
            " (note: file sample rate {} Hz differs slightly from firmware's \
             native {} Hz)",
            sample_rate, NATIVE_SAMPLE_RATE
        )
    } else {
        String::new()
    };

    // Pick up to MAX_WINDOWS evenly-spaced 8192-sample windows.
    let usable = samples.len().saturating_sub(WARMUP).saturating_sub(WINDOW);
    let n_win = MAX_WINDOWS.min(1 + usable / WINDOW);
    let stride = if n_win > 1 { usable / (n_win - 1) } else { 0 };
    let window_starts: Vec<usize> = (0..n_win).map(|i| WARMUP + i * stride).collect();

    // f32 [-1,1] → int16-scale values
    let to_y = |s: f32| -> f64 { (s as f64) * 32768.0 };

    // Pre-classify each window as silent / usable based on its raw signal
    // stdev. Silent windows are skipped entirely; without enough signal,
    // the inverse-filter output hovers near zero and trivially rounds to
    // a constant 12-bit integer, producing a misleading "match".
    let mut usable_window_starts = Vec::with_capacity(window_starts.len());
    let mut windows_skipped_silent = 0usize;
    for &start in &window_starts {
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
            usable_window_starts.push(start);
        } else {
            windows_skipped_silent += 1;
        }
    }

    if usable_window_starts.is_empty() {
        return PipistrelleResult {
            verdict: PipistrelleVerdict::NotApplicable,
            explanation: format!(
                "Every analysis window was below the silence gate \
                 (stdev < {:.0} on the int16 scale){}",
                SIGNAL_STDEV_GATE, rate_note,
            ),
            windows_skipped_silent,
            ..default
        };
    }

    let mut per_preset = Vec::with_capacity(PRESETS.len());
    let mut total_samples = 0usize;

    for preset in PRESETS {
        let mut sum_resid_sq = 0.0f64;
        let mut sum_actual_sq = 0.0f64;
        let mut sum_in_range = 0usize;
        let mut sum_near_int_match = 0usize;
        let mut sum_near_int_total = 0usize;
        let mut sum_abs_frac = 0.0f64;
        let mut sum_n = 0usize;

        for &start in &usable_window_starts {
            let seg: Vec<f64> = samples[start..start + WINDOW]
                .iter()
                .map(|&s| to_y(s))
                .collect();
            let recovered = inverse_filter_chain(&seg, preset);
            // Discard the first portion — both the inverse biquad and the
            // integrator's initial state need to settle.
            let settle = 1024usize.min(recovered.len() / 4);
            let usable_v = &recovered[settle..];
            let usable_y = &seg[settle..];

            // For each *in-range* recovered value, measure distance to the
            // nearest integer. Real pipistrelle output recovers integers
            // (modulo the firmware's fixed-point truncation noise amplified
            // by the inverse biquad); unrelated 16-bit signals produce a
            // ~uniform fractional part on [-0.5, +0.5].
            for &v in usable_v {
                if (0.0..=4095.0).contains(&v) {
                    sum_in_range += 1;
                    sum_near_int_total += 1;
                    let frac = (v - v.round()).abs();
                    sum_abs_frac += frac;
                    if frac < NEAR_INTEGER_THRESHOLD {
                        sum_near_int_match += 1;
                    }
                }
            }

            // Forward roundtrip residual — kept as a secondary metric for
            // diagnostics. Round / clamp v to candidate 12-bit ADC, run
            // forward filter, compare to original y.
            let v_rounded: Vec<i32> = usable_v
                .iter()
                .map(|&v| (v.round() as i32).clamp(0, 4095))
                .collect();
            let predicted = forward_filter_chain(&v_rounded, preset);
            let fsettle = 256usize.min(predicted.len() / 4);
            for i in fsettle..predicted.len() {
                let diff = usable_y[i] - predicted[i];
                sum_resid_sq += diff * diff;
                sum_actual_sq += usable_y[i] * usable_y[i];
                sum_n += 1;
            }
        }

        let total_recovered: usize = usable_window_starts.len() * (WINDOW - 1024.min(WINDOW / 4));
        let in_range_frac = if total_recovered > 0 {
            sum_in_range as f64 / total_recovered as f64
        } else {
            0.0
        };
        let normalized_residual = if sum_actual_sq > 0.0 {
            (sum_resid_sq / sum_actual_sq).sqrt()
        } else {
            f64::INFINITY
        };
        let near_integer_frac = if sum_near_int_total > 0 {
            sum_near_int_match as f64 / sum_near_int_total as f64
        } else {
            0.0
        };
        let mean_abs_frac = if sum_near_int_total > 0 {
            sum_abs_frac / sum_near_int_total as f64
        } else {
            f64::NAN
        };
        per_preset.push(PipistrelleScore {
            db_cut: preset.db_cut,
            near_integer_frac,
            near_integer_match: sum_near_int_match,
            near_integer_total: sum_near_int_total,
            mean_abs_frac,
            normalized_residual,
            in_range_frac,
        });
        total_samples = total_samples.max(sum_n);
    }

    // Best preset = lowest forward-roundtrip residual. (Per-sample
    // integer-roundness is too insensitive: the inverse biquad amplifies
    // int16 quantisation noise enough that even the correct preset gives
    // ~uniform fractional parts on the recovered ADC stream.)
    let best = per_preset.iter().min_by(|a, b| {
        a.normalized_residual
            .partial_cmp(&b.normalized_residual)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Match criteria, calibrated empirically (see RESIDUAL_*_THRESHOLD docs):
    //
    // - `in_range == 100 %`: every recovered ADC value lands in [0, 4095].
    //   Real pipistrelle hits this exactly; many false-positive candidates
    //   (16-bit pro recorders with non-DC-centred signals) don't.
    // - `dBcut == 12`: the firmware default. All 24 known-good real-pipi
    //   files in the validation set win at dBcut=12. False positives often
    //   win at dBcut=3, 9, or 32.
    // - `residual` provides the strength dial.
    let in_range_pct = best.map(|s| s.in_range_frac).unwrap_or(0.0);

    let (
        verdict,
        explanation,
        best_db_cut,
        best_near_int,
        best_near_int_match,
        best_near_int_total,
        best_mean_frac,
        best_resid,
        best_ir,
    ) = match best {
        Some(s)
            if s.normalized_residual < RESIDUAL_MATCH_THRESHOLD
                && (in_range_pct - 1.0).abs() < 1e-9
                && s.db_cut == 12 =>
        {
            (
                PipistrelleVerdict::Match,
                format!(
                    "Inverse + forward roundtrip with the dBcut=12 firmware-default \
                     preset gives {:.2}% RMS error and every recovered ADC value lands \
                     in [0, 4095] \u{2014} strong Pipistrelle-family firmware signature{}",
                    s.normalized_residual * 100.0,
                    rate_note,
                ),
                Some(s.db_cut),
                s.near_integer_frac,
                s.near_integer_match,
                s.near_integer_total,
                s.mean_abs_frac,
                s.normalized_residual,
                s.in_range_frac,
            )
        }
        Some(s)
            if s.normalized_residual < RESIDUAL_POSSIBLE_THRESHOLD
                && s.in_range_frac > 0.99
                && s.db_cut == 12 =>
        {
            (
                PipistrelleVerdict::Possible,
                format!(
                    "Inverse + forward roundtrip with the dBcut=12 firmware-default \
                     preset gives {:.2}% RMS error \u{2014} possible Pipistrelle-family \
                     firmware, not conclusive{}",
                    s.normalized_residual * 100.0,
                    rate_note,
                ),
                Some(s.db_cut),
                s.near_integer_frac,
                s.near_integer_match,
                s.near_integer_total,
                s.mean_abs_frac,
                s.normalized_residual,
                s.in_range_frac,
            )
        }
        Some(s) => (
            PipistrelleVerdict::NoMatch,
            format!(
                "Best preset dBcut={} gives {:.1}% RMS error, in-range {:.1}% \
                 \u{2014} not consistent with Pipistrelle-family firmware{}",
                s.db_cut,
                s.normalized_residual * 100.0,
                s.in_range_frac * 100.0,
                rate_note,
            ),
            Some(s.db_cut),
            s.near_integer_frac,
            s.near_integer_match,
            s.near_integer_total,
            s.mean_abs_frac,
            s.normalized_residual,
            s.in_range_frac,
        ),
        None => (
            PipistrelleVerdict::NoMatch,
            format!("No preset produced a usable inverse{}", rate_note),
            None,
            0.0,
            0,
            0,
            f64::NAN,
            f64::NAN,
            0.0,
        ),
    };

    PipistrelleResult {
        verdict,
        explanation,
        best_db_cut,
        best_near_integer_frac: best_near_int,
        best_near_integer_match: best_near_int_match,
        best_near_integer_total: best_near_int_total,
        best_mean_abs_frac: best_mean_frac,
        best_normalized_residual: best_resid,
        best_in_range_frac: best_ir,
        per_preset,
        samples_analyzed: total_samples,
        windows_used: usable_window_starts.len(),
        windows_skipped_silent,
    }
}

/// Invert (biquad ∘ HPF) on a single window. Input `y` is the firmware's
/// USB Audio output scaled to int16 units; output is the recovered 12-bit
/// ADC value `v` (should be integer in [0, 4095] if the source is real
/// pipistrelle firmware).
///
/// Algorithm:
///
///  Inverse biquad — solve `iy = b0*ix + b1*ix1 - b1*iy1 + b2*ix2 - a2*iy2`
///  for `ix[n]` given the firmware-output sequence `iy[n]`:
///
///     ix[n] = (iy[n] + b1*iy[n-1] + a2*iy[n-2] - b1*ix[n-1] - b2*ix[n-2]) / b0
///
///  This inverse is stable because the biquad's numerator zeros lie inside
///  the unit circle for all five presets.
///
///  Inverse HPF — the forward HPF is `out = ((v-2048)<<16 - d) >> 12` with
///  `d[n+1] = d[n] + out << 4` (in firmware units). Recovering `v` is then
///  a simple integrator on `ix`:
///
///     d[n+1] = d[n] + 256 * ix[n]
///     v[n]   = (ix[n] * 4096 + d[n]) / 65536 + 2048
/// Forward filter (matches the firmware's `HPF + biquad` in float, without
/// the integer truncations). Used by `detect` to round-trip recovered ADC
/// values and check fit. `v` is a 12-bit ADC stream (integer in [0, 4095]);
/// the returned `iy` sequence is on the int16 scale.
pub fn forward_filter_chain(v: &[i32], c: &PipistrelleCoeffs) -> Vec<f64> {
    let mut d = 0.0f64;
    let mut hpf_out = Vec::with_capacity(v.len());
    for &vn in v {
        let u = ((vn - 2048) as f64) * 65536.0;
        let out = u - d;
        d += out / 16.0;
        hpf_out.push(out / 4096.0);
    }
    let mut ix1 = 0.0f64;
    let mut ix2 = 0.0f64;
    let mut iy1 = 0.0f64;
    let mut iy2 = 0.0f64;
    let mut iy_seq = Vec::with_capacity(v.len());
    for &ix in &hpf_out {
        let iy = c.b0 * ix + c.b1 * (ix1 - iy1) + c.b2 * ix2 - c.a2 * iy2;
        iy_seq.push(iy);
        ix2 = ix1;
        ix1 = ix;
        iy2 = iy1;
        iy1 = iy;
    }
    iy_seq
}

fn inverse_filter_chain(y: &[f64], c: &PipistrelleCoeffs) -> Vec<f64> {
    let n = y.len();
    let mut ix_prev2 = 0.0f64;
    let mut ix_prev1 = 0.0f64;
    let mut iy_prev2 = 0.0f64;
    let mut iy_prev1 = 0.0f64;
    let mut hpf_out = Vec::with_capacity(n);
    for &yn in y {
        let ix =
            (yn + c.b1 * iy_prev1 + c.a2 * iy_prev2 - c.b1 * ix_prev1 - c.b2 * ix_prev2) / c.b0;
        hpf_out.push(ix);
        ix_prev2 = ix_prev1;
        ix_prev1 = ix;
        iy_prev2 = iy_prev1;
        iy_prev1 = yn;
    }

    let mut d = 0.0f64;
    let mut recovered = Vec::with_capacity(n);
    for &ix in &hpf_out {
        let v = (ix * 4096.0 + d) / 65536.0 + 2048.0;
        recovered.push(v);
        d += 256.0 * ix;
    }
    recovered
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_recovers_integer_input() {
        // Synthesize a deterministic 12-bit "ADC" stream
        let n = 16384;
        let v: Vec<i32> = (0..n)
            .map(|i| {
                let t = i as f64 / 384_000.0;
                let s =
                    (1500.0 + 800.0 * (2.0 * std::f64::consts::PI * 12_000.0 * t).sin()).round();
                (s as i32).clamp(0, 4095)
            })
            .collect();

        for preset in PRESETS {
            let y = forward_filter_chain(&v, preset);
            let y_int16: Vec<f64> = y
                .iter()
                .map(|&x| x.round().clamp(-32768.0, 32767.0))
                .collect();
            let recovered = inverse_filter_chain(&y_int16, preset);
            // Skip startup transient
            let settle = 1024usize;
            let in_range_count = recovered[settle..]
                .iter()
                .filter(|&&r| (0.0..=4095.0).contains(&r))
                .count();
            let frac_in = in_range_count as f64 / (recovered.len() - settle) as f64;
            assert!(
                frac_in > 0.95,
                "preset dBcut={} only {:.2}% in range",
                preset.db_cut,
                frac_in * 100.0
            );
            let resid: f64 = recovered[settle..]
                .iter()
                .filter(|&&r| (0.0..=4095.0).contains(&r))
                .map(|&r| (r - r.round()).abs())
                .sum::<f64>()
                / in_range_count as f64;
            assert!(
                resid < 0.05,
                "preset dBcut={} residual {:.4} too high",
                preset.db_cut,
                resid
            );
        }
    }

    #[test]
    fn detect_rejects_white_noise() {
        // Random 16-bit signal that is NOT pipistrelle
        let mut state: u32 = 987654321;
        let samples: Vec<f32> = (0..65536)
            .map(|_| {
                state = state.wrapping_mul(1664525).wrapping_add(1013904223);
                (((state >> 16) & 0xFFFF) as i32 - 32768) as f32 / 32768.0
            })
            .collect();
        let r = detect(&samples, NATIVE_SAMPLE_RATE, 16, false);
        assert!(
            !matches!(r.verdict, PipistrelleVerdict::Match),
            "false-positive pipistrelle match on white noise: {:?}",
            r
        );
    }

    #[test]
    fn detect_accepts_synthetic_pipistrelle() {
        // Build a 12-bit "ADC" stream, run firmware-matching forward filter,
        // package as int16, then test that detect() picks up the signature.
        let preset = PRESETS[3]; // dBcut=12 is the default in the firmware
        let n = 32768;
        let v: Vec<i32> = (0..n)
            .map(|i| {
                let t = i as f64 / 384_000.0;
                let s = 2048.0
                    + 600.0 * (2.0 * std::f64::consts::PI * 18_000.0 * t).sin()
                    + 200.0 * (2.0 * std::f64::consts::PI * 31_000.0 * t).cos();
                (s.round() as i32).clamp(0, 4095)
            })
            .collect();
        let y = forward_filter_chain(&v, &preset);
        let samples: Vec<f32> = y
            .iter()
            .map(|&x| (x.round().clamp(-32768.0, 32767.0) / 32768.0) as f32)
            .collect();
        let r = detect(&samples, NATIVE_SAMPLE_RATE, 16, false);
        assert!(
            matches!(
                r.verdict,
                PipistrelleVerdict::Match | PipistrelleVerdict::Possible
            ),
            "synthetic pipistrelle stream not detected: {:?}",
            r
        );
        // Should have picked the dBcut=12 preset
        assert_eq!(r.best_db_cut, Some(12), "wrong preset selected: {:?}", r);
    }
}
