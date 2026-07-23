//! Walk a directory of `.wav` files, run bit / LSB-autocorrelation /
//! Pipistrelle-firmware analyses on each, read sibling `*.xc.json` XC
//! metadata if present (for the `dvc` / `mic` / `smp` fields), and emit
//! a TSV row per file.
//!
//! Used as a scratchpad while deciding which extra mic / firmware
//! signatures are worth detecting. Not part of the shipping app.
//!
//! Run: `cargo run -p oversample-core --release --example scan_mic_signatures -- \
//!         <dir> > out.tsv`

use oversample_core::audio::loader;
use oversample_core::device_hint::{self, HintConfidence, MetadataMatch};
use oversample_core::dsp::{audiomoth, bit_analysis, effective_nyquist, lsb_autocorr, pipistrelle};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

const MAX_SECONDS_TO_LOAD: f64 = 30.0;

struct LoadedMono {
    sample_rate: u32,
    channels: u32,
    bits_per_sample: u16,
    is_float: bool,
    mono: Vec<f32>,
}

fn main() {
    let dir = std::env::args()
        .nth(1)
        .expect("usage: scan_mic_signatures <dir>");
    let dir = PathBuf::from(dir);
    let mut wavs: Vec<PathBuf> = walk_wavs(&dir);
    wavs.sort();

    // Header
    println!(
        "filename\tdvc\tmic\tsmp_meta\tchannels\tbits\tsmp_actual\tdur_s\teffective_bits\theadroom\tnoise_floor_dbfs\tlsb_verdict\tlsb_gcd_nonzero\tlsb_chi2\tlsb_lag1\tlsb_lag256\tlsb_nonzero_pct\tlsb_quiet_stdev\tlsb_period\tlsb_period_match\tpipi_verdict\tpipi_best_dbcut\tpipi_residual_pct\tpipi_in_range_pct\tpipi_near_int_pct\tpipi_mean_abs_frac\tpipi_windows_used\tpipi_windows_skipped\tnyq_eff_khz\tnyq_ratio\tnyq_drop_db\tnyq_upper_db\tam_verdict\tam_cutoff\tam_osdiv\tam_gain\tam_mean_abs_frac\tam_near_int_pct\ttop_hint_label\ttop_hint_confidence\tmeta_match"
    );

    for wav in &wavs {
        let stem = wav.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let xc_json = wav.with_file_name(format!("{}.xc.json", stem));
        let (dvc, mic, smp_meta) = read_xc_sidecar(&xc_json);

        let loaded = match load_mono(wav, MAX_SECONDS_TO_LOAD) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("skip {}: {}", wav.display(), e);
                continue;
            }
        };
        let bits = loaded.bits_per_sample;
        let is_float = loaded.is_float;
        let mono = &loaded.mono;
        let sample_rate = loaded.sample_rate;
        let dur = mono.len() as f64 / sample_rate as f64;

        let bit = bit_analysis::analyze_bits(mono, bits, is_float, dur);
        let lsb = lsb_autocorr::analyze_lsb_autocorr(mono, bits, is_float);
        let pip = pipistrelle::detect(mono, sample_rate, bits, is_float);
        let nyq = effective_nyquist::detect(mono, sample_rate);
        let lsb_zero_padded =
            matches!(lsb.verdict, lsb_autocorr::LsbVerdict::ZeroPaddedNBit { .. });
        let am = audiomoth::detect(mono, sample_rate, bits, is_float, lsb_zero_padded);

        let lsb_verdict = match &lsb.verdict {
            lsb_autocorr::LsbVerdict::NotApplicable => "n/a".to_string(),
            lsb_autocorr::LsbVerdict::ZeroPaddedNBit {
                effective_bits,
                padding_bits,
            } => {
                format!("zero-padded({}b, eff {}b)", padding_bits, effective_bits)
            }
            lsb_autocorr::LsbVerdict::QuietSectionZeroPadded {
                effective_bits_in_quiet,
                padding_bits,
            } => {
                format!(
                    "quiet-zero({}b, eff-in-quiet {}b)",
                    padding_bits, effective_bits_in_quiet
                )
            }
            lsb_autocorr::LsbVerdict::DspPaddedLowBitDepth {
                effective_bits_guess,
            } => {
                format!("dsp-padded(eff ~{}b)", effective_bits_guess)
            }
            lsb_autocorr::LsbVerdict::ConsistentWithClaimedBitDepth => "consistent".into(),
            lsb_autocorr::LsbVerdict::Inconclusive => "inconclusive".into(),
        };

        let pipi_verdict = match pip.verdict {
            pipistrelle::PipistrelleVerdict::Match => "match",
            pipistrelle::PipistrelleVerdict::Possible => "possible",
            pipistrelle::PipistrelleVerdict::NoMatch => "no-match",
            pipistrelle::PipistrelleVerdict::NotApplicable => "n/a",
        };

        // Infer hints + metadata comparison
        let hints = device_hint::infer_device_hints(
            sample_rate,
            bits,
            is_float,
            &bit,
            &lsb,
            &pip,
            Some(&nyq),
            Some(&am),
        );
        let xc_pairs = vec![
            ("dvc".to_string(), dvc.clone()),
            ("mic".to_string(), mic.clone()),
        ];
        let mm = device_hint::compare_to_metadata(&hints, Some(&xc_pairs), None);
        let top_hint = hints.first();
        let top_label = top_hint.map(|h| h.label.clone()).unwrap_or_default();
        let top_conf = match top_hint.map(|h| &h.confidence) {
            Some(HintConfidence::Strong) => "strong",
            Some(HintConfidence::Likely) => "likely",
            Some(HintConfidence::Possible) => "possible",
            None => "",
        };
        let mm_str = match mm {
            MetadataMatch::NoClaim => "no-claim".to_string(),
            MetadataMatch::ClaimNoAnalysis { .. } => "claim-no-hints".to_string(),
            MetadataMatch::Match {
                matched_candidate, ..
            } => format!("match: {}", matched_candidate),
            MetadataMatch::Mismatch { .. } => "MISMATCH".to_string(),
        };

        let period_str = lsb
            .low_bit_period
            .map(|p| p.to_string())
            .unwrap_or_default();

        let am_verdict = match am.verdict {
            audiomoth::AudioMothVerdict::Match => "match",
            audiomoth::AudioMothVerdict::Possible => "possible",
            audiomoth::AudioMothVerdict::NoMatch => "no-match",
            audiomoth::AudioMothVerdict::NotApplicable => "n/a",
        };
        let (am_cutoff, am_osdiv, am_gain, am_mf, am_nip) = match am.best.as_ref() {
            Some(b) => (
                b.cutoff_hz.to_string(),
                b.os_times_div.to_string(),
                format!("{:.3}", b.gain_total),
                format!("{:.4}", b.mean_abs_frac),
                format!("{:.1}", b.near_integer_frac * 100.0),
            ),
            None => ("".into(), "".into(), "".into(), "".into(), "".into()),
        };

        println!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{:.2}\t{}\t{}\t{:.1}\t{}\t{}\t{:.1}\t{:+.4}\t{:+.4}\t{:.1}\t{:.1}\t{}\t{:.2}\t{}\t{}\t{:.2}\t{:.1}\t{:.1}\t{:.3}\t{}\t{}\t{:.1}\t{:.3}\t{:.1}\t{:.1}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            stem,
            tsv(&dvc),
            tsv(&mic),
            smp_meta.as_deref().unwrap_or(""),
            loaded.channels,
            bits,
            sample_rate,
            dur,
            bit.effective_bits,
            bit.headroom_bits,
            bit.noise_floor_db,
            lsb_verdict,
            lsb.gcd_nonzero,
            lsb.quiet_lsb_chi2,
            lsb.quiet_lsb_lag1_acf,
            lsb.quiet_lsb_lag256_acf,
            lsb.quiet_lsb_nonzero_frac * 100.0,
            lsb.quietest_window_stdev,
            period_str,
            lsb.low_bit_period_match,
            pipi_verdict,
            pip.best_db_cut.map(|v| v.to_string()).unwrap_or_default(),
            pip.best_normalized_residual * 100.0,
            pip.best_in_range_frac * 100.0,
            pip.best_near_integer_frac * 100.0,
            pip.best_mean_abs_frac,
            pip.windows_used,
            pip.windows_skipped_silent,
            nyq.effective_hz / 1000.0,
            (nyq.effective_hz / nyq.claimed_nyquist_hz).min(1.0),
            nyq.drop_db,
            nyq.upper_band_floor_db,
            am_verdict,
            am_cutoff,
            am_osdiv,
            am_gain,
            am_mf,
            am_nip,
            tsv(&top_label),
            top_conf,
            tsv(&mm_str),
        );
    }
}

fn tsv(s: &str) -> String {
    s.replace('\t', " ").replace('\n', " ")
}

fn walk_wavs(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return out;
    };
    for ent in entries.flatten() {
        let p = ent.path();
        if p.is_dir() {
            out.extend(walk_wavs(&p));
        } else if p
            .extension()
            .and_then(|s| s.to_str())
            .map(|s| s.eq_ignore_ascii_case("wav"))
            == Some(true)
        {
            out.push(p);
        }
    }
    out
}

fn read_xc_sidecar(p: &Path) -> (String, String, Option<String>) {
    let Ok(bytes) = fs::read(p) else {
        return (String::new(), String::new(), None);
    };
    let Ok(v): Result<Value, _> = serde_json::from_slice(&bytes) else {
        return (String::new(), String::new(), None);
    };
    let s = |k: &str| -> String {
        match v.get(k) {
            Some(Value::String(s)) => s.trim().to_string(),
            Some(Value::Number(n)) => n.to_string(),
            _ => String::new(),
        }
    };
    let smp = match v.get("smp") {
        Some(Value::String(s)) => Some(s.clone()),
        Some(Value::Number(n)) => Some(n.to_string()),
        _ => None,
    };
    (s("dvc"), s("mic"), smp)
}

fn load_mono(path: &Path, max_seconds: f64) -> Result<LoadedMono, String> {
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    let audio = loader::load_audio(&bytes)?;
    let total_samples_per_channel = audio.samples.len();
    let max_samples = (max_seconds * audio.sample_rate as f64) as usize;
    let want = total_samples_per_channel.min(max_samples);
    // `audio.samples` is already mono-mixed by the loader.
    let mono = audio.samples[..want].to_vec();
    Ok(LoadedMono {
        sample_rate: audio.sample_rate,
        channels: audio.channels,
        bits_per_sample: audio.metadata.bits_per_sample,
        is_float: audio.metadata.is_float,
        mono,
    })
}
