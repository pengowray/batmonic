//! Heuristics that map low-level analysis results (`bit_analysis`,
//! `lsb_autocorr`, `pipistrelle::detect`) into device / firmware
//! candidates. These are *hints*, not assertions — most are based on
//! signatures shared by multiple devices, so we list candidates rather
//! than naming one.
//!
//! Used by the UI to surface things like "12-bit zero-padded
//! WAV — consistent with Wildlife Acoustics Song Meter Mini Bat,
//! AudioMoth (12-bit modes), and similar 12-bit-ADC recorders".

use crate::dsp::audiomoth::{AudioMothResult, AudioMothVerdict};
use crate::dsp::bit_analysis::BitAnalysis;
use crate::dsp::effective_nyquist::{EffectiveNyquistResult, EffectiveNyquistVerdict};
use crate::dsp::lsb_autocorr::{LsbAutocorrResult, LsbVerdict};
use crate::dsp::pipistrelle::{PipistrelleResult, PipistrelleVerdict};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HintConfidence {
    /// Multiple signatures coincide; the recording almost certainly came
    /// from one of the listed devices.
    Strong,
    /// One distinctive signature matches — likely but not conclusive.
    Likely,
    /// Weak hint (e.g. sample-rate-only). Many devices may share this.
    Possible,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DeviceHint {
    /// Short user-facing label.
    pub label: String,
    pub confidence: HintConfidence,
    /// Devices / families this hint suggests, in rough likelihood order.
    pub candidates: Vec<String>,
    /// Longer detail (suitable for tooltip / `<details>` body).
    pub detail: String,
}

/// Pipistrelle-family member devices. Used for the user-facing list when
/// the firmware signature matches.
pub const PIPISTRELLE_FAMILY: &[&str] = &[
    "Pipistrelle (Omenie)",
    "Pippyg",
    "Pipmini",
    "Pipistrelle USB Microphone",
    "Batsynth",
];

pub fn infer_device_hints(
    sample_rate: u32,
    bits_per_sample: u16,
    is_float: bool,
    bit: &BitAnalysis,
    lsb: &LsbAutocorrResult,
    pip: &PipistrelleResult,
    nyq: Option<&EffectiveNyquistResult>,
    am: Option<&AudioMothResult>,
) -> Vec<DeviceHint> {
    let mut hints = Vec::new();

    // 1. Pipistrelle-family firmware signature.
    //
    // The Pipistrelle integer-multiple test isn't unique to Pipistrelle — it
    // also fires on AudioMoth, Pettersson D1000 quiet-zero, and SM Mini Bat
    // fw2+ files, all of which share the "12-bit ADC + DSP" structure. To
    // avoid double-attribution, only surface a Pipistrelle hint when no
    // mutually-exclusive firmware family has already been identified.
    let conflicting_family_detected =
        // AudioMoth detector fired → it's that family (low-Hz HPF), not Pipistrelle
        // (which has a ~3.8 kHz HPF + biquad).
        matches!(am.map(|a| &a.verdict),
                 Some(AudioMothVerdict::Match) | Some(AudioMothVerdict::Possible))
        // Whole-file zero-pad signature → it's a literal 12-bit ADC zero-padded
        // recorder (SM Mini Bat fw1, etc.), not Pipistrelle.
        || matches!(lsb.verdict, LsbVerdict::ZeroPaddedNBit { .. })
        // Quiet-section noise-gate signature → Pettersson D1000 family.
        || matches!(lsb.verdict, LsbVerdict::QuietSectionZeroPadded { .. });

    if !conflicting_family_detected {
        // When Pipistrelle's integer-multiple test fires *without* a
        // mutually-exclusive family confirming itself, the file could be
        // genuine Pipistrelle OR AudioMoth (with a custom HPF) OR
        // Pettersson D1000 (without a clear quiet section) OR any other
        // 12-bit-ADC + DSP firmware. We surface the hint with all those
        // candidates so metadata-comparison can resolve. The label
        // accurately reflects the ambiguity.
        let broader_candidates: Vec<String> = PIPISTRELLE_FAMILY
            .iter()
            .map(|s| s.to_string())
            .chain([
                "AudioMoth (with custom HPF cutoff)".to_string(),
                "Pettersson D1000 / D1000X / D500x".to_string(),
                "Wildlife Acoustics Song Meter Mini Bat (firmware 2+)".to_string(),
                "Elekon Batlogger M / M2".to_string(),
                "Echo Meter Touch (some firmware versions)".to_string(),
                "Other 12-bit-ADC + DSP ultrasonic recorders".to_string(),
            ])
            .collect();

        match pip.verdict {
            PipistrelleVerdict::Match => {
                hints.push(DeviceHint {
                    label: format!(
                        "12-bit ADC + Pipistrelle-style DSP firmware (dBcut={})",
                        pip.best_db_cut.unwrap_or(0),
                    ),
                    confidence: HintConfidence::Strong,
                    candidates: broader_candidates,
                    detail: "The Pipistrelle firmware's inverse-filter chain (HPF + \
                        biquad band-cut, dBcut=12 preset) reconstructs the signal to \
                        within ~2% RMS error and every recovered ADC value lands in \
                        the 12-bit range [0, 4095]. This signature is shared by \
                        Pipistrelle's exact firmware and by other 12-bit-ADC + \
                        DSP firmware in the same family; metadata comparison usually \
                        resolves which one this file actually is."
                        .into(),
                });
            }
            PipistrelleVerdict::Possible => {
                hints.push(DeviceHint {
                    label: format!(
                        "Possible 12-bit ADC + Pipistrelle-style DSP firmware (dBcut={})",
                        pip.best_db_cut.unwrap_or(0),
                    ),
                    confidence: HintConfidence::Possible,
                    candidates: broader_candidates,
                    detail: "Pipistrelle inverse-filter residual is in a range \
                        that's consistent with the Pipistrelle / AudioMoth / \
                        D1000X family of 12-bit firmware, but not strong enough \
                        to be conclusive."
                        .into(),
                });
            }
            _ => {}
        }
    }

    // 1b. AudioMoth-family firmware signature (12-bit ADC + low-Hz HPF,
    // shared across several brands' firmware).
    if let Some(am) = am {
        match am.verdict {
            AudioMothVerdict::Match => {
                hints.push(DeviceHint {
                    label: am
                        .best
                        .as_ref()
                        .map(|b| {
                            format!(
                        "12-bit ADC + low-frequency HPF firmware (fitted {} Hz cutoff, G = {:.0})",
                        b.cutoff_hz, b.gain_total,
                    )
                        })
                        .unwrap_or_else(|| "12-bit ADC + low-frequency HPF firmware".into()),
                    confidence: HintConfidence::Strong,
                    candidates: vec![
                        "AudioMoth (default 8 or 48 Hz DC blocker)".into(),
                        "Wildlife Acoustics Song Meter Mini Bat (firmware 2+)".into(),
                        "Other 12-bit-ADC recorders with HPF DC blocking".into(),
                    ],
                    detail: "After inverse-filtering the AudioMoth-style HPF, sample \
                        differences land cleanly on integer multiples of \
                        sampleMultiplier × Butterworth_design_gain. The signature is \
                        shared by AudioMoth's open-source firmware and at least some \
                        Wildlife Acoustics Song Meter firmware versions that follow a \
                        similar 12-bit-ADC + low-Hz-HPF design."
                        .into(),
                });
            }
            AudioMothVerdict::Possible => {
                hints.push(DeviceHint {
                    label: am.best.as_ref().map(|b| format!(
                        "Possible 12-bit ADC + low-frequency HPF firmware ({} Hz cutoff, G = {:.0})",
                        b.cutoff_hz, b.gain_total,
                    )).unwrap_or_else(|| "Possible AudioMoth-family firmware".into()),
                    confidence: HintConfidence::Possible,
                    candidates: vec![
                        "AudioMoth".into(),
                        "Wildlife Acoustics Song Meter Mini Bat (firmware 2+)".into(),
                    ],
                    detail: "Weak integer-multiple signature \u{2014} possible \
                        AudioMoth-family firmware but not conclusive.".into(),
                });
            }
            _ => {}
        }
    }

    // 2. LSB-based hints (zero-padding signatures).
    if !is_float {
        if let LsbVerdict::ZeroPaddedNBit {
            padding_bits,
            effective_bits,
        } = lsb.verdict
        {
            let (label, candidates, detail) = match padding_bits {
                4 => (
                    format!(
                        "12-bit ADC zero-padded into a {}-bit container",
                        bits_per_sample,
                    ),
                    vec![
                        "Wildlife Acoustics Song Meter Mini Bat".into(),
                        "Wildlife Acoustics Echo Meter Touch (some firmware versions)".into(),
                        "AudioMoth (some firmware versions)".into(),
                        "Teensy-based DIY recorders (e.g. Tensy ActiveRecorder)".into(),
                        "Other 12-bit-ADC bat recorders".into(),
                    ],
                    "Every sample is a multiple of 16, so the low 4 bits are \
                     literally always zero. The recorder's ADC is 12-bit; the \
                     extra 4 bits are unused."
                        .to_string(),
                ),
                1 => (
                    format!(
                        "15-bit ADC (1 bit zero-padded into the {}-bit container)",
                        bits_per_sample,
                    ),
                    vec![
                        "Pettersson u384 USB microphone".into(),
                        "Pettersson 256".into(),
                        "Other devices that drop the LSB at the ADC interface".into(),
                    ],
                    "Every sample is even, so the LSB is always zero. This is \
                     the Pettersson u384's documented behaviour, but other \
                     1-bit-shift designs would look the same."
                        .to_string(),
                ),
                n => (
                    format!(
                        "{}-bit effective depth ({} bit{} zero-padded)",
                        effective_bits,
                        n,
                        if n == 1 { "" } else { "s" },
                    ),
                    vec!["Recorder with N-bit ADC \u{2192} larger container".into()],
                    format!(
                        "Samples are all multiples of {}; the low {} bit{} \
                         literally always zero.",
                        1u32 << n,
                        n,
                        if n == 1 { "" } else { "s" },
                    ),
                ),
            };
            hints.push(DeviceHint {
                label,
                confidence: HintConfidence::Strong,
                candidates,
                detail,
            });
        } else if let LsbVerdict::QuietSectionZeroPadded {
            effective_bits_in_quiet,
            padding_bits,
        } = lsb.verdict
        {
            hints.push(DeviceHint {
                label: format!(
                    "Noise-gated quiet sections (low {} bits zero when silent; full \
                     {}-bit otherwise)",
                    padding_bits, bits_per_sample,
                ),
                confidence: HintConfidence::Strong,
                candidates: vec![
                    "Pettersson D1000 / D1000X / D500x".into(),
                    "AudioMoth (some firmware versions)".into(),
                    "Other recorders with DSP noise-gate / AGC behaviour".into(),
                ],
                detail: format!(
                    "In quiet sections the low {} bits are literally zero, but loud \
                     sections have non-zero values throughout. The recorder is still \
                     {}-bit at the ADC; its firmware just stops feeding noise into the \
                     LSBs below some threshold (saves storage, makes silence look \
                     cleaner). Effective bit depth drops to ~{}-bit during quiet \
                     periods.",
                    padding_bits, bits_per_sample, effective_bits_in_quiet,
                ),
            });
        } else if let LsbVerdict::DspPaddedLowBitDepth {
            effective_bits_guess,
        } = lsb.verdict
        {
            hints.push(DeviceHint {
                label: format!(
                    "~{}-bit ADC with on-device DSP (filter residue in LSBs)",
                    effective_bits_guess,
                ),
                confidence: HintConfidence::Likely,
                candidates: vec![
                    "AudioMoth (12-bit ADC + DC-block / gain stage)".into(),
                    "Pipistrelle-family firmware".into(),
                    "Other DIY / open-source recorders with on-chip filtering".into(),
                ],
                detail: "Low bits show statistical structure (chi\u{00B2} test, \
                    autocorrelation) inconsistent with analog noise but consistent \
                    with deterministic fixed-point filter residue \u{2014} suggests \
                    a lower-bit-depth ADC followed by on-device DSP."
                    .into(),
            });
        }
    }

    // 3. Effective-Nyquist hint — catches files claimed at high sample
    // rate but actually band-limited (upsampled, or aggressive AAF).
    if let Some(n) = nyq {
        if let EffectiveNyquistVerdict::BandLimited {
            effective_hz,
            ratio,
            claimed_nyquist_hz,
        } = n.verdict
        {
            hints.push(DeviceHint {
                label: format!(
                    "Effective bandwidth \u{2248} {:.0} kHz \u{2014} only {:.0}% of the \
                     claimed {:.0} kHz Nyquist",
                    effective_hz / 1000.0,
                    ratio * 100.0,
                    claimed_nyquist_hz / 1000.0,
                ),
                confidence: HintConfidence::Likely,
                candidates: vec![
                    "File upsampled from a lower-rate source".into(),
                    "Recorder with aggressive anti-aliasing filter".into(),
                ],
                detail: n.explanation.clone(),
            });
        }
    }

    // 4. Sample-rate fingerprints (weak hints).
    if let Some(h) = sample_rate_hint(sample_rate) {
        hints.push(h);
    }

    // 4. Tighten: if effective_bits derived from `bit_analysis.value_coverage`
    // is much smaller than `bits_per_sample`, surface that even without an
    // LSB verdict.
    if !is_float {
        if let Some(vc) = &bit.value_coverage {
            let ceiled = vc.resolution_bits.ceil() as u16;
            let is_notable =
                (bits_per_sample == 16 && ceiled <= 12) || (bits_per_sample == 24 && ceiled <= 16);
            // Only emit if no zero-pad hint already covered this case.
            let already_covered = hints
                .iter()
                .any(|h| h.label.contains("zero-padded") || h.label.contains("zero-pad"));
            if is_notable && !already_covered {
                hints.push(DeviceHint {
                    label: format!(
                        "Value coverage \u{2248} {}-bit despite a {}-bit container",
                        ceiled, bits_per_sample,
                    ),
                    confidence: HintConfidence::Likely,
                    candidates: vec![
                        "Recorder using a lower-bit-depth ADC".into(),
                        "File downsampled in bit depth before saving".into(),
                    ],
                    detail: format!(
                        "Only {} distinct sample values appeared, out of the \
                         {} possible at {}-bit. This is consistent with the \
                         signal genuinely being ~{}-bit at some stage of the \
                         capture pipeline.",
                        vc.unique_count, vc.value_space, bits_per_sample, ceiled,
                    ),
                });
            }
        }
    }

    hints
}

fn sample_rate_hint(sample_rate: u32) -> Option<DeviceHint> {
    let (label, candidates, detail) = match sample_rate {
        312_500 => (
            "Sample rate 312 500 Hz",
            vec!["Elekon Batlogger M / M2".into()],
            "312 500 Hz is the Elekon Batlogger's native sample rate.",
        ),
        333_333 | 375_000 | 400_000 => (
            "Sample rate is an Avisoft variant",
            vec!["Avisoft UltraSoundGate (various models)".into()],
            "These rates are characteristic of the Avisoft UltraSoundGate family \
             (e.g. 116Hme).",
        ),
        38_400 => (
            "Sample rate 38 400 Hz",
            vec!["Wildlife Acoustics EM3".into()],
            "38 400 Hz is the EM3's full-spectrum WAV rate.",
        ),
        _ => return None,
    };
    Some(DeviceHint {
        label: label.into(),
        confidence: HintConfidence::Possible,
        candidates,
        detail: detail.into(),
    })
}

/// Compare hint candidates against any device claim in XC sidecar / GUANO
/// metadata. Returns a verdict suitable for showing in the UI.
#[derive(Clone, Debug, PartialEq)]
pub enum MetadataMatch {
    /// No claim found in metadata.
    NoClaim,
    /// A claim was found, but the analysis didn't produce any device hints.
    ClaimNoAnalysis { claim: String },
    /// Claim and at least one hint candidate share a recognisable substring.
    Match {
        claim: String,
        matched_candidate: String,
    },
    /// Claim is present and conflicts with all hint candidates.
    Mismatch { claim: String, hint_summary: String },
}

/// Best-effort device claim from metadata. Prefers GUANO `Make`+`Model`
/// (most authoritative), falls back to XC `dvc`/`mic`.
pub fn extract_device_claim(
    xc_metadata: Option<&[(String, String)]>,
    guano: Option<&[(String, String)]>,
) -> Option<String> {
    if let Some(g) = guano {
        let make = g
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("Make"))
            .map(|(_, v)| v.trim())
            .unwrap_or("");
        let model = g
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("Model"))
            .map(|(_, v)| v.trim())
            .unwrap_or("");
        let combined = match (make.is_empty(), model.is_empty()) {
            (false, false) => format!("{} {}", make, model),
            (true, false) => model.to_string(),
            (false, true) => make.to_string(),
            (true, true) => String::new(),
        };
        if !combined.is_empty() {
            return Some(combined);
        }
    }
    if let Some(xc) = xc_metadata {
        let dvc = xc
            .iter()
            .find(|(k, _)| k == "dvc")
            .map(|(_, v)| v.trim())
            .unwrap_or("");
        let mic = xc
            .iter()
            .find(|(k, _)| k == "mic")
            .map(|(_, v)| v.trim())
            .unwrap_or("");
        if !dvc.is_empty() && !mic.is_empty() && !mic_subsumed_by_dvc(dvc, mic) {
            return Some(format!("{} ({})", dvc, mic));
        } else if !dvc.is_empty() {
            return Some(dvc.to_string());
        } else if !mic.is_empty() {
            return Some(mic.to_string());
        }
    }
    None
}

fn mic_subsumed_by_dvc(dvc: &str, mic: &str) -> bool {
    let dvc_l = dvc.to_lowercase();
    let mic_l = mic.to_lowercase();
    dvc_l.contains(&mic_l) || mic_l.contains(&dvc_l)
}

pub fn compare_to_metadata(
    hints: &[DeviceHint],
    xc_metadata: Option<&[(String, String)]>,
    guano: Option<&[(String, String)]>,
) -> MetadataMatch {
    let Some(claim) = extract_device_claim(xc_metadata, guano) else {
        return MetadataMatch::NoClaim;
    };
    if hints.is_empty() {
        return MetadataMatch::ClaimNoAnalysis { claim };
    }
    let claim_l = claim.to_lowercase();
    let claim_keywords: Vec<&str> = claim_l
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| s.len() >= 3 && !is_noise_word(s))
        .collect();

    // First pass: any-confidence Match.
    for hint in hints {
        for candidate in &hint.candidates {
            if matches_candidate(&claim_l, &claim_keywords, candidate) {
                return MetadataMatch::Match {
                    claim,
                    matched_candidate: candidate.clone(),
                };
            }
        }
    }

    // No matching candidate — but a Mismatch is only meaningful when at
    // least one hint is Strong-confidence. Lower-confidence hints don't
    // contradict the metadata, they just describe characteristics of the
    // file that *could* belong to the claimed device too. Surface them
    // as "no comparison" rather than "mismatch".
    let strong_hints: Vec<&DeviceHint> = hints
        .iter()
        .filter(|h| h.confidence == HintConfidence::Strong)
        .collect();
    if strong_hints.is_empty() {
        return MetadataMatch::ClaimNoAnalysis { claim };
    }
    let hint_summary = strong_hints
        .iter()
        .map(|h| h.label.clone())
        .collect::<Vec<_>>()
        .join("; ");
    MetadataMatch::Mismatch {
        claim,
        hint_summary,
    }
}

fn matches_candidate(claim_l: &str, claim_keywords: &[&str], candidate: &str) -> bool {
    let cand_l = candidate.to_lowercase();
    if claim_l.contains(&cand_l) || cand_l.contains(claim_l) {
        return true;
    }
    // Normalise both to alphanumeric-only, so "Audio Moth" matches
    // "AudioMoth" and "WA SongMeter" matches "WA Song Meter".
    let strip = |s: &str| -> String { s.chars().filter(|c| c.is_alphanumeric()).collect() };
    let claim_join = strip(claim_l);
    let cand_join = strip(&cand_l);
    if claim_join.len() >= 5 && cand_join.contains(&claim_join) {
        return true;
    }
    if cand_join.len() >= 5 && claim_join.contains(&cand_join) {
        return true;
    }
    for kw in claim_keywords {
        // Word-level match
        if cand_l
            .split(|c: char| !c.is_alphanumeric())
            .any(|w| w == *kw)
        {
            return true;
        }
        // Substring-of-a-word match (handles "moth" matching "audiomoth")
        if kw.len() >= 4 && cand_join.contains(*kw) {
            return true;
        }
    }
    false
}

fn is_noise_word(s: &str) -> bool {
    matches!(
        s,
        "the"
            | "and"
            | "with"
            | "for"
            | "from"
            | "any"
            | "all"
            | "wav"
            | "wave"
            | "ultrasound"
            | "ultrasonic"
            | "microphone"
            | "mic"
            | "recorder"
            | "detector"
            | "device"
            | "built"
            | "internal"
            | "ext"
            | "version"
            | "ver"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claim(make: &str, model: &str) -> Vec<(String, String)> {
        let mut v = Vec::new();
        if !make.is_empty() {
            v.push(("Make".into(), make.into()));
        }
        if !model.is_empty() {
            v.push(("Model".into(), model.into()));
        }
        v
    }

    #[test]
    fn extract_prefers_guano() {
        let g = claim("Wildlife Acoustics", "Song Meter Mini Bat");
        let x = vec![("dvc".into(), "Other".into())];
        assert_eq!(
            extract_device_claim(Some(&x), Some(&g)).as_deref(),
            Some("Wildlife Acoustics Song Meter Mini Bat"),
        );
    }

    #[test]
    fn extract_falls_back_to_xc() {
        let x = vec![
            ("dvc".into(), "Pettersson D500x".into()),
            ("mic".into(), "Advanced electret".into()),
        ];
        assert_eq!(
            extract_device_claim(Some(&x), None).as_deref(),
            Some("Pettersson D500x (Advanced electret)"),
        );
    }

    #[test]
    fn extract_collapses_redundant_mic() {
        let x = vec![
            ("dvc".into(), "Pettersson Ultrasound Microphone u384".into()),
            ("mic".into(), "Pettersson Ultrasound Microphone u384".into()),
        ];
        assert_eq!(
            extract_device_claim(Some(&x), None).as_deref(),
            Some("Pettersson Ultrasound Microphone u384"),
        );
    }

    #[test]
    fn compare_matches_song_meter_mini() {
        let hints = vec![DeviceHint {
            label: "12-bit ADC zero-padded into 16-bit".into(),
            confidence: HintConfidence::Strong,
            candidates: vec!["Wildlife Acoustics Song Meter Mini Bat".into()],
            detail: String::new(),
        }];
        let g = claim("Wildlife Acoustics", "Song Meter Mini Bat 2");
        match compare_to_metadata(&hints, None, Some(&g)) {
            MetadataMatch::Match { .. } => {}
            other => panic!("expected match, got {:?}", other),
        }
    }

    #[test]
    fn compare_reports_mismatch() {
        let hints = vec![DeviceHint {
            label: "12-bit ADC zero-padded into 16-bit".into(),
            confidence: HintConfidence::Strong,
            candidates: vec!["Wildlife Acoustics Song Meter Mini Bat".into()],
            detail: String::new(),
        }];
        let g = claim("Pettersson", "D500x");
        match compare_to_metadata(&hints, None, Some(&g)) {
            MetadataMatch::Mismatch { .. } => {}
            other => panic!("expected mismatch, got {:?}", other),
        }
    }
}
