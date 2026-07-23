use crate::audio::source::{ChannelView, DEFAULT_ANALYSIS_WINDOW_SECS};
use crate::dsp::bit_analysis::{self, BitAnalysis, BitCaution};
use crate::dsp::lsb_autocorr::{self, LsbAutocorrResult, LsbVerdict};
use crate::dsp::pipistrelle::{self, PipistrelleResult, PipistrelleVerdict};
use crate::dsp::wsnr;
use crate::state::store_fields::*;
use crate::state::{AppState, RightSidebarTab};
use leptos::prelude::*;
use oversample_core::bit_depth_certainty::{self, BitDepthCertainty, CertaintyLevel};
use oversample_core::device_hint::{
    self, DeviceHint, HintConfidence, MetadataMatch, PIPISTRELLE_FAMILY,
};
use oversample_core::dsp::audiomoth::{self, AudioMothResult};
use oversample_core::dsp::effective_nyquist::{self, EffectiveNyquistResult};
use std::sync::Arc;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::spawn_local;

#[component]
pub(crate) fn AnalysisPanel() -> impl IntoView {
    let state = expect_context::<AppState>();

    // Async analysis results — None means "not yet computed" or "computing"
    let analysis: RwSignal<Option<BitAnalysis>> = RwSignal::new(None);
    let wsnr_result: RwSignal<Option<wsnr::WsnrResult>> = RwSignal::new(None);
    let lsb_result: RwSignal<Option<LsbAutocorrResult>> = RwSignal::new(None);
    let pipistrelle_result: RwSignal<Option<PipistrelleResult>> = RwSignal::new(None);
    let nyquist_result: RwSignal<Option<EffectiveNyquistResult>> = RwSignal::new(None);
    let audiomoth_result: RwSignal<Option<AudioMothResult>> = RwSignal::new(None);
    let is_computing = RwSignal::new(false);
    let last_computed_idx: RwSignal<Option<usize>> = RwSignal::new(None);
    let compute_gen = RwSignal::new(0u32);
    // Whether the last analysis used full file (true) or first 30s (false).
    let analysis_is_full = RwSignal::new(false);
    // Whether the file is longer than the analysis window.
    let file_is_long = RwSignal::new(false);

    // Run analysis (default: first 30s; full_file=true for full scan)
    let run_analysis = move |full_file: bool| {
        let files = state.library.files().get_untracked();
        let idx = state.library.current_index().get_untracked();
        let file = idx.and_then(|i| files.get(i).cloned());
        let Some(file) = file else {
            return;
        };

        analysis.set(None);
        wsnr_result.set(None);
        lsb_result.set(None);
        pipistrelle_result.set(None);
        nyquist_result.set(None);
        audiomoth_result.set(None);
        is_computing.set(true);
        last_computed_idx.set(idx);
        analysis_is_full.set(full_file);
        compute_gen.update(|g| *g += 1);
        let generation = compute_gen.get_untracked();

        let sample_rate = file.audio.sample_rate;
        let bits_per_sample = file.audio.metadata.bits_per_sample;
        let is_float = file.audio.metadata.is_float;
        let total = file.audio.source.total_samples() as usize;
        if total == 0 {
            is_computing.set(false);
            return;
        }

        // ZC files have no meaningful continuous waveform — the audio
        // placeholder is silent. Skip the bit-depth / LSB / firmware /
        // wSNR analyses (they'd just measure the silent placeholder)
        // and let the view layer render a dedicated ZC stats section
        // from file.audio.metadata.zc_data instead.
        if file.audio.metadata.zc_data.is_some() {
            is_computing.set(false);
            return;
        }

        let max_samples = (DEFAULT_ANALYSIS_WINDOW_SECS * sample_rate as f64) as usize;
        let is_long = total > max_samples;
        file_is_long.set(is_long);

        let samples: Arc<Vec<f32>> = if full_file || !is_long {
            analysis_is_full.set(true);
            Arc::new(
                file.audio
                    .source
                    .read_region(ChannelView::MonoMix, 0, total),
            )
        } else {
            Arc::new(
                file.audio
                    .source
                    .read_region(ChannelView::MonoMix, 0, max_samples),
            )
        };
        let duration_secs = samples.len() as f64 / sample_rate as f64;

        spawn_local(async move {
            yield_to_browser().await;
            if compute_gen.try_get_untracked() != Some(generation) {
                return;
            }

            let bits_result =
                bit_analysis::analyze_bits(&samples, bits_per_sample, is_float, duration_secs);
            if compute_gen.try_get_untracked() != Some(generation) {
                return;
            }
            analysis.set(Some(bits_result));

            yield_to_browser().await;
            if compute_gen.try_get_untracked() != Some(generation) {
                return;
            }

            let wsnr_res = wsnr::analyze_wsnr(&samples, sample_rate);
            if compute_gen.try_get_untracked() != Some(generation) {
                return;
            }
            wsnr_result.set(Some(wsnr_res));

            yield_to_browser().await;
            if compute_gen.try_get_untracked() != Some(generation) {
                return;
            }

            let lsb_res = lsb_autocorr::analyze_lsb_autocorr(&samples, bits_per_sample, is_float);
            if compute_gen.try_get_untracked() != Some(generation) {
                return;
            }
            lsb_result.set(Some(lsb_res));

            yield_to_browser().await;
            if compute_gen.try_get_untracked() != Some(generation) {
                return;
            }

            let pipi_res = pipistrelle::detect(&samples, sample_rate, bits_per_sample, is_float);
            if compute_gen.try_get_untracked() != Some(generation) {
                return;
            }
            pipistrelle_result.set(Some(pipi_res));

            yield_to_browser().await;
            if compute_gen.try_get_untracked() != Some(generation) {
                return;
            }

            let nyq_res = effective_nyquist::detect(&samples, sample_rate);
            if compute_gen.try_get_untracked() != Some(generation) {
                return;
            }
            nyquist_result.set(Some(nyq_res));

            yield_to_browser().await;
            if compute_gen.try_get_untracked() != Some(generation) {
                return;
            }

            let lsb_is_zero_padded = matches!(
                lsb_result.get_untracked().as_ref().map(|l| &l.verdict),
                Some(oversample_core::dsp::lsb_autocorr::LsbVerdict::ZeroPaddedNBit { .. }),
            );
            let am_res = audiomoth::detect(
                &samples,
                sample_rate,
                bits_per_sample,
                is_float,
                lsb_is_zero_padded,
            );
            if compute_gen.try_get_untracked() != Some(generation) {
                return;
            }
            audiomoth_result.set(Some(am_res));

            is_computing.set(false);
        });
    };

    // Only compute expensive analysis when the Analysis tab is active
    Effect::new(move || {
        let tab = state.panels.right_tab().get();
        state.library.files().track(); // subscribe without cloning the Vec
        let idx = state.library.current_index().get();

        if tab != RightSidebarTab::Analysis {
            return;
        }

        // Already computed for this file
        if idx == last_computed_idx.get_untracked() && analysis.get_untracked().is_some() {
            return;
        }

        let files = state.library.files().get_untracked();
        let file = idx.and_then(|i| files.get(i).cloned());
        if file.is_none() {
            analysis.set(None);
            wsnr_result.set(None);
            lsb_result.set(None);
            pipistrelle_result.set(None);
            nyquist_result.set(None);
            audiomoth_result.set(None);
            last_computed_idx.set(None);
            is_computing.set(false);
            return;
        }

        run_analysis(false);
    });

    let xc_quality = Memo::new(move |_| {
        let files = state.library.files().get();
        let idx = state.library.current_index().get();
        idx.and_then(|i| files.get(i).cloned())
            .and_then(|file| file.xc_metadata)
            .and_then(|meta| {
                meta.iter()
                    .find(|(k, _)| k == "Quality")
                    .map(|(_, v)| v.clone())
            })
    });

    let report_text = Memo::new(move |_| {
        let files = state.library.files().get();
        let idx = state.library.current_index().get();
        let file = idx.and_then(|i| files.get(i).cloned());

        let mut report = "=== Audio Analysis ===\n".to_string();

        // ZC files: skip the audio-specific sections (their analyses
        // weren't run) and emit a dedicated ZC summary instead.
        if let Some(ref f) = file {
            if let Some(zc) = f.audio.metadata.zc_data.as_ref() {
                let md = &zc.metadata;
                let total = zc.times_s.len();
                let on = zc.on_dot_count();
                let off = total.saturating_sub(on);
                let off_pct = if total > 0 {
                    off as f64 * 100.0 / total as f64
                } else {
                    0.0
                };
                let mut freqs: Vec<f64> = zc
                    .freqs_hz
                    .iter()
                    .zip(&zc.off_mask)
                    .filter_map(|(&f, &of)| (!of && f > 0.0).then_some(f))
                    .collect();
                freqs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                let dur_text = crate::format_time::format_duration(zc.duration_secs(), 3);
                report.push_str("=== Anabat ZC Recording ===\n");
                report.push_str(&format!("  File type: {}\n", md.file_type));
                report.push_str(&format!(
                    "  Dots: {} total ({} ON, {} OFF — {:.1}%)\n",
                    total, on, off, off_pct
                ));
                if let (Some(&min), Some(&max)) = (freqs.first(), freqs.last()) {
                    let mean = freqs.iter().sum::<f64>() / freqs.len() as f64;
                    let median = freqs[freqs.len() / 2];
                    report.push_str(&format!(
                        "  Frequency: {:.1}\u{2013}{:.1} kHz, mean {:.1} kHz, median {:.1} kHz ({} ON dots with valid freq)\n",
                        min / 1000.0, max / 1000.0, mean / 1000.0, median / 1000.0, freqs.len(),
                    ));
                }
                report.push_str(&format!("  Duration: {}\n", dur_text));
                report.push_str(&format!(
                    "  divratio: {}, vres: {}, res1: {} Hz\n",
                    md.divratio, md.vres, md.res1
                ));
                if let Some(ts) = md.timestamp {
                    report.push_str(&format!(
                        "  Recorded: {:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:06}\n",
                        ts.year,
                        ts.month,
                        ts.day,
                        ts.hour,
                        ts.minute,
                        ts.second,
                        ts.microseconds_total
                    ));
                }
                if !md.location.is_empty() {
                    report.push_str(&format!("  Location: {}\n", md.location));
                }
                if !md.species.is_empty() {
                    report.push_str(&format!("  Species:  {}\n", md.species));
                }
                if !md.tape.is_empty() {
                    report.push_str(&format!("  Tape:     {}\n", md.tape));
                }
                if !md.date.is_empty() {
                    report.push_str(&format!("  Date:     {}\n", md.date));
                }
                if !md.spec.is_empty() {
                    report.push_str(&format!("  Spec:     {}\n", md.spec));
                }
                if !md.note1.is_empty() {
                    report.push_str(&format!("  Note 1:   {}\n", md.note1));
                }
                if !md.note2.is_empty() {
                    report.push_str(&format!("  Note 2:   {}\n", md.note2));
                }
                if !md.id_code.is_empty() {
                    report.push_str(&format!("  ID:       {}\n", md.id_code));
                }
                if !md.gps.is_empty() {
                    report.push_str(&format!("  GPS:      {}\n", md.gps));
                }
                if !md.guano.is_empty() {
                    report.push_str("\nGUANO metadata:\n");
                    for (k, v) in &md.guano {
                        report.push_str(&format!("  {} = {}\n", k, v));
                    }
                }
                return report;
            }
        }

        if let Some(ref f) = file {
            let meta = &f.audio.metadata;
            let sr = f.audio.sample_rate;
            let sr_text = if sr % 1000 == 0 {
                format!("{} kHz", sr / 1000)
            } else {
                format!("{:.1} kHz", sr as f64 / 1000.0)
            };
            let ch_text = match f.audio.channels {
                1 => "Mono".to_string(),
                2 => "Stereo".to_string(),
                n => format!("{} ch", n),
            };
            let bit_text = if meta.is_float {
                format!("{}-bit float", meta.bits_per_sample)
            } else {
                format!("{}-bit", meta.bits_per_sample)
            };
            let total_samples = f.audio.source.total_samples() as usize;
            let dur_text = crate::format_time::format_duration(f.audio.duration_secs, 3);
            report.push_str(&format!(
                "\nFile\n  Sample rate: {}\n  Channels: {}\n  Bit depth: {}\n  Duration: {}\n  Samples: {}\n",
                sr_text, ch_text, bit_text, dur_text, total_samples
            ));

            // Signal stats — scan first 30s only for large files
            let max_scan = (DEFAULT_ANALYSIS_WINDOW_SECS * f.audio.sample_rate as f64) as usize;
            let scan_len = total_samples.min(max_scan);
            let smp = f
                .audio
                .source
                .read_region(ChannelView::MonoMix, 0, scan_len);
            let len = smp.len();
            if len > 0 {
                let mut smin = f32::INFINITY;
                let mut smax = f32::NEG_INFINITY;
                let mut sum = 0.0f64;
                let mut sum_sq = 0.0f64;
                for &s in smp.iter() {
                    if s < smin {
                        smin = s;
                    }
                    if s > smax {
                        smax = s;
                    }
                    sum += s as f64;
                    sum_sq += (s as f64) * (s as f64);
                }
                let dc_bias = sum / len as f64;
                let rms = (sum_sq / len as f64).sqrt();
                let min_db = if smin.abs() > 0.0 {
                    format!("{:.1} dB", 20.0 * (smin.abs() as f64).log10())
                } else {
                    "-\u{221e} dB".into()
                };
                let max_db = if smax.abs() > 0.0 {
                    format!("{:.1} dB", 20.0 * (smax.abs() as f64).log10())
                } else {
                    "-\u{221e} dB".into()
                };
                let rms_db = if rms > 0.0 {
                    format!("{:.1} dB", 20.0 * rms.log10())
                } else {
                    "-\u{221e} dB".into()
                };
                let dc_db = if dc_bias.abs() > 0.0 {
                    format!("{:.1} dB", 20.0 * dc_bias.abs().log10())
                } else {
                    "-\u{221e} dB".into()
                };
                report.push_str(&format!(
                    "\nSignal\n  Min: {:.4} ({})\n  Max: {:.4} ({})\n  RMS: {}\n  DC bias: {}\n",
                    smin, min_db, smax, max_db, rms_db, dc_db
                ));
            }
        }

        // wSNR
        if let Some(ref w) = wsnr_result.get() {
            let grade = w.grade.label();
            report.push_str(&format!(
                "\nRecording Quality (wSNR): {}\n  SNR: {:.1} dB(ISO/ITU)\n  Signal: {:.1} dB (ISO 226)\n  Noise: {:.1} dB (ITU-R 468)\n",
                grade, w.snr_db, w.signal_db, w.noise_db
            ));
            if let Some(xc) = xc_quality.get() {
                report.push_str(&format!("  XC quality (metadata): {}\n", xc.trim()));
            }
            for msg in &w.warnings {
                report.push_str(&format!("  \u{26a0} {}\n", msg));
            }
        }

        // Tiered effective-bit-depth verdict (composed from bit / lsb / pip / am)
        if let (Some(ref a), Some(ref lsb), Some(ref f)) =
            (analysis.get(), lsb_result.get(), file.as_ref())
        {
            let pip = pipistrelle_result.get();
            let am = audiomoth_result.get();
            let bdc = bit_depth_certainty::compose(
                f.audio.metadata.bits_per_sample,
                f.audio.metadata.is_float,
                f.audio.metadata.format,
                a,
                lsb,
                pip.as_ref(),
                am.as_ref(),
            );
            report.push_str("\nBit Depth\n");
            report.push_str(&format!("  {}\n", bdc.headline));
            report.push_str(&format!("  ({})\n", bdc.certainty.label()));
            for fact in &bdc.facts {
                let prefix = match fact.certainty {
                    CertaintyLevel::Certain => "  ✓ ",
                    CertaintyLevel::HighConfidence => "  ● ",
                    CertaintyLevel::Suggestive => "  ○ ",
                };
                report.push_str(&format!("{}{}\n", prefix, fact.statement));
            }
        }

        // Bit analysis (detailed per-bit breakdown)
        if let Some(ref a) = analysis.get() {
            let total = a.total_samples;
            let pos_total = a.positive_total;
            let neg_total = a.negative_total;
            let zero_total = a.zero_total;
            let non_silent = pos_total + neg_total;
            let is_asymmetric = non_silent > 0
                && a.bit_cautions
                    .first()
                    .map(|c| c.iter().any(|x| matches!(x, BitCaution::SignBitSkewed)))
                    .unwrap_or(false);

            let pos_pct = if total > 0 {
                format!("{:.0}%", pos_total as f64 / total as f64 * 100.0)
            } else {
                "0%".into()
            };
            let neg_pct = if total > 0 {
                format!("{:.0}%", neg_total as f64 / total as f64 * 100.0)
            } else {
                "0%".into()
            };
            let zero_pct = if total > 0 {
                format!("{:.0}%", zero_total as f64 / total as f64 * 100.0)
            } else {
                "0%".into()
            };
            let split_label = if is_asymmetric {
                "Asymmetric"
            } else {
                "Sample split"
            };
            let split_line = format!(
                "  {}: {}+ {}− {}silence\n",
                split_label, pos_pct, neg_pct, zero_pct
            );

            report.push_str("\nBit Usage\n");

            if !a.is_float {
                let zero_padding = a.bits_per_sample - a.effective_bits;
                let effective_depth = a
                    .bits_per_sample
                    .saturating_sub(a.headroom_bits)
                    .saturating_sub(zero_padding);
                let headroom_db = a.headroom_bits as f64 * 20.0 * 2f64.log10();
                report.push_str(&format!(
                    "  Effective bit depth: {} bits\n",
                    effective_depth
                ));
                report.push_str(&format!(
                    "  Entropy estimate: ~{:.1} bits\n",
                    a.effective_bits_f64
                ));
                if a.headroom_bits > 0 {
                    report.push_str(&format!(
                        "  Headroom: {} bits ({:.1} dB)\n",
                        a.headroom_bits, headroom_db
                    ));
                }
                if zero_padding > 0 {
                    report.push_str(&format!("  Zero padding: {} bits\n", zero_padding));
                }
                report.push_str(&format!("  {}\n", a.summary));
                if let Some(ref vc) = a.value_coverage {
                    report.push_str(&format!(
                        "  Value coverage: {:.1}% ({} of {})\n",
                        vc.coverage_pct, vc.unique_count, vc.value_space
                    ));
                    let ceiled = vc.resolution_bits.ceil() as u16;
                    report.push_str(&format!(
                        "  Value resolution: ~{:.1} bits ({}-bit)\n",
                        vc.resolution_bits, ceiled
                    ));
                }
            } else {
                report.push_str(&format!("  {}\n", a.summary));
                report.push_str(&format!(
                    "  Entropy estimate: ~{:.1} bits\n",
                    a.effective_bits_f64
                ));
            }
            {
                let nf_db = a.noise_floor_db;
                let nf_bits = -nf_db / (20.0 * 2f64.log10());
                report.push_str(&format!(
                    "  Noise floor: {:.1} dBFS (~{:.1} bits)\n",
                    nf_db, nf_bits
                ));
            }
            report.push_str(&split_line);

            for w in &a.warnings {
                report.push_str(&format!("  ! {}\n", w));
            }
            let caution_list: Vec<String> = a
                .bit_cautions
                .iter()
                .enumerate()
                .filter(|(_, cs)| !cs.is_empty())
                .map(|(i, cs)| {
                    let label = bit_analysis::bit_label(i, a.bits_per_sample, a.is_float);
                    let names: Vec<&str> = cs
                        .iter()
                        .map(|c| match c {
                            BitCaution::SignBitSkewed => "asymmetric distribution",
                            BitCaution::Always1 => "always 1",
                            BitCaution::OnlyInFade => "only in fade",
                            BitCaution::VeryLowUsage => "very low usage",
                        })
                        .collect();
                    format!("{} ({})", label, names.join(", "))
                })
                .collect();
            if !caution_list.is_empty() {
                report.push_str(&format!("  Cautions: {}\n", caution_list.join("; ")));
            }
        }

        // LSB autocorrelation
        if let Some(ref l) = lsb_result.get() {
            report.push_str("\nLSB Autocorrelation\n");
            let verdict_text = match &l.verdict {
                LsbVerdict::NotApplicable => "Not applicable",
                LsbVerdict::ZeroPaddedNBit { .. } => "Zero-padded (whole file)",
                LsbVerdict::QuietSectionZeroPadded { .. } => {
                    "Zero-padded in quiet sections only (noise-gated)"
                }
                LsbVerdict::DspPaddedLowBitDepth { .. } => "DSP-padded / IIR residue",
                LsbVerdict::ConsistentWithClaimedBitDepth => "Consistent with claimed bit depth",
                LsbVerdict::Inconclusive => "Inconclusive (noise floor too high)",
            };
            report.push_str(&format!("  Verdict: {}\n", verdict_text));
            if !matches!(l.verdict, LsbVerdict::NotApplicable) {
                report.push_str(&format!(
                    "  Quietest window: stdev {:.1} LSB at sample {}\n",
                    l.quietest_window_stdev, l.quietest_window_idx
                ));
                report.push_str(&format!(
                    "  Low {}-bit: chi\u{00B2}={:.0}, lag-1 ACF={:+.3}, lag-256 ACF={:+.3}, nonzero {:.1}%\n",
                    l.n_low, l.quiet_lsb_chi2, l.quiet_lsb_lag1_acf, l.quiet_lsb_lag256_acf,
                    l.quiet_lsb_nonzero_frac * 100.0,
                ));
                if l.gcd_nonzero > 1 {
                    report.push_str(&format!("  GCD of nonzero samples: {}\n", l.gcd_nonzero));
                }
            }
            if !l.explanation.is_empty() {
                report.push_str(&format!("  {}\n", l.explanation));
            }
        }

        // Mic firmware signatures (Pipistrelle family)
        if let Some(ref p) = pipistrelle_result.get() {
            report.push_str("\nFirmware fingerprint (Pipistrelle family)\n");
            let verdict_text = match p.verdict {
                PipistrelleVerdict::Match => "match",
                PipistrelleVerdict::Possible => "possible",
                PipistrelleVerdict::NoMatch => "no match",
                PipistrelleVerdict::NotApplicable => "not applicable",
            };
            report.push_str(&format!("  Verdict: {}\n", verdict_text));
            if !matches!(p.verdict, PipistrelleVerdict::NotApplicable) && !p.per_preset.is_empty() {
                if let Some(db) = p.best_db_cut {
                    report.push_str(&format!(
                        "  Best preset: dBcut={}  residual={:.2}%  in-range={:.1}%\n",
                        db,
                        p.best_normalized_residual * 100.0,
                        p.best_in_range_frac * 100.0,
                    ));
                    report.push_str(&format!(
                        "  Near-integer recovered samples: {}/{} = {:.1}% (uniform baseline 20%)\n",
                        p.best_near_integer_match,
                        p.best_near_integer_total,
                        p.best_near_integer_frac * 100.0,
                    ));
                    report.push_str(&format!(
                        "  Mean |fractional part| of recovered: {:.3} (uniform baseline 0.25)\n",
                        p.best_mean_abs_frac,
                    ));
                }
                let scores: Vec<String> = p
                    .per_preset
                    .iter()
                    .map(|s| format!("dBcut={}: {:.2}%", s.db_cut, s.normalized_residual * 100.0))
                    .collect();
                report.push_str(&format!(
                    "  All presets (residual): {}\n",
                    scores.join("; ")
                ));
                report.push_str(&format!(
                    "  Windows analyzed: {} (skipped {} as silent)\n",
                    p.windows_used, p.windows_skipped_silent,
                ));
            }
            if !p.explanation.is_empty() {
                report.push_str(&format!("  {}\n", p.explanation));
            }
        }

        // Device hints + metadata comparison
        if let (Some(ref f), Some(ref bit), Some(ref lsb), Some(ref pip)) = (
            file.as_ref(),
            analysis.get(),
            lsb_result.get(),
            pipistrelle_result.get(),
        ) {
            let nyq = nyquist_result.get();
            let am = audiomoth_result.get();
            let hints = device_hint::infer_device_hints(
                f.audio.sample_rate,
                f.audio.metadata.bits_per_sample,
                f.audio.metadata.is_float,
                bit,
                lsb,
                pip,
                nyq.as_ref(),
                am.as_ref(),
            );
            let guano = f.audio.metadata.guano.as_ref().map(|g| g.fields.clone());
            let xc = f.xc_metadata.clone();
            let m = device_hint::compare_to_metadata(&hints, xc.as_deref(), guano.as_deref());
            if !hints.is_empty() || !matches!(m, MetadataMatch::NoClaim) {
                report.push_str("\nDevice hints\n");
                for h in &hints {
                    let conf = match h.confidence {
                        HintConfidence::Strong => "strong",
                        HintConfidence::Likely => "likely",
                        HintConfidence::Possible => "possible",
                    };
                    report.push_str(&format!("  [{}] {}\n", conf, h.label));
                    if !h.candidates.is_empty() {
                        report
                            .push_str(
                                &format!("       candidates: {}\n", h.candidates.join(", "),),
                            );
                    }
                }
                match m {
                    MetadataMatch::NoClaim => {}
                    MetadataMatch::ClaimNoAnalysis { claim } => {
                        report.push_str(&format!(
                            "  metadata says: {} (no analysis hints to compare)\n",
                            claim
                        ));
                    }
                    MetadataMatch::Match {
                        claim,
                        matched_candidate,
                    } => {
                        report.push_str(&format!(
                            "  metadata: {} \u{2014} matches ({})\n",
                            claim, matched_candidate
                        ));
                    }
                    MetadataMatch::Mismatch {
                        claim,
                        hint_summary,
                    } => {
                        report.push_str(&format!(
                            "  ! metadata: {} \u{2014} does NOT match analysis ({})\n",
                            claim, hint_summary
                        ));
                    }
                }
            }
        }

        report
    });

    view! {
        <div class="sidebar-panel">
            // Copy report button
            {move || {
                let has_file = {
                    let files = state.library.files().get();
                    let idx = state.library.current_index().get();
                    idx.and_then(|i| files.get(i)).is_some()
                };
                if has_file {
                    let text = report_text.get();
                    let on_copy = move |_: web_sys::MouseEvent| {
                        super::copy_to_clipboard(&text);
                    };
                    view! {
                        <div class="copy-report-row">
                            <button class="copy-report-btn" on:click=on_copy title="Copy full analysis report to clipboard">"Copy report"</button>
                        </div>
                    }.into_any()
                } else {
                    view! { <span></span> }.into_any()
                }
            }}
            // File info + signal stats
            {move || {
                let files = state.library.files().get();
                let idx = state.library.current_index().get();
                let file = idx.and_then(|i| files.get(i).cloned());
                match file.as_ref() {
                    None => view! {
                        <div class="sidebar-panel-empty">"No file selected"</div>
                    }.into_any(),
                    Some(f) if f.audio.metadata.zc_data.is_some() => {
                        render_zc_file_info(f).into_any()
                    }
                    Some(f) => {
                        let meta = &f.audio.metadata;
                        let sr = f.audio.sample_rate;
                        let sr_text = if sr % 1000 == 0 {
                            format!("{} kHz", sr / 1000)
                        } else {
                            format!("{:.1} kHz", sr as f64 / 1000.0)
                        };
                        let ch_text = match f.audio.channels {
                            1 => "Mono".to_string(),
                            2 => "Stereo".to_string(),
                            n => format!("{} ch", n),
                        };
                        let bit_text = if meta.is_float {
                            format!("{}-bit float", meta.bits_per_sample)
                        } else {
                            format!("{}-bit", meta.bits_per_sample)
                        };
                        let total_samples = f.audio.source.total_samples() as usize;
                        let dur_text = crate::format_time::format_duration(f.audio.duration_secs, 3);
                        let samples_text = format!("{}", total_samples);

                        // Signal stats — scan first 30s only for large files
                        let max_scan = (DEFAULT_ANALYSIS_WINDOW_SECS * f.audio.sample_rate as f64) as usize;
                        let scan_len = total_samples.min(max_scan);
                        let samples = f.audio.source.read_region(ChannelView::MonoMix, 0, scan_len);
                        let len = samples.len();
                        let (sig_min, sig_max, dc_bias, rms) = if len > 0 {
                            let mut smin = f32::INFINITY;
                            let mut smax = f32::NEG_INFINITY;
                            let mut sum = 0.0f64;
                            let mut sum_sq = 0.0f64;
                            for &s in samples.iter() {
                                if s < smin { smin = s; }
                                if s > smax { smax = s; }
                                sum += s as f64;
                                sum_sq += (s as f64) * (s as f64);
                            }
                            (smin, smax, sum / len as f64, (sum_sq / len as f64).sqrt())
                        } else {
                            (0.0f32, 0.0f32, 0.0f64, 0.0f64)
                        };
                        let min_db = if sig_min.abs() > 0.0 { format!("{:.1} dB", 20.0 * (sig_min.abs() as f64).log10()) } else { "-\u{221E} dB".into() };
                        let max_db = if sig_max.abs() > 0.0 { format!("{:.1} dB", 20.0 * (sig_max.abs() as f64).log10()) } else { "-\u{221E} dB".into() };
                        let rms_db = if rms > 0.0 { format!("{:.1} dB", 20.0 * rms.log10()) } else { "-\u{221E} dB".into() };
                        let dc_db = if dc_bias.abs() > 0.0 { format!("{:.1} dB", 20.0 * dc_bias.abs().log10()) } else { "-\u{221E} dB".into() };
                        let dc_raw_tooltip = format!("{:.6} (raw)", dc_bias);
                        // DC relative to RMS: gives perceptual sense of DC severity
                        let dc_rms_ratio = if rms > 0.0 { dc_bias.abs() / rms } else { 0.0 };
                        // Warning: notable DC if |dc| > 1% of full scale OR dc/rms > 5%, gated on N
                        let dc_notable = len > 10_000 && (dc_bias.abs() > 0.01 || dc_rms_ratio > 0.05);
                        let dc_warning = if dc_notable {
                            Some(format!("DC offset: {} \u{2014} {:.0}% of RMS level", dc_db, dc_rms_ratio * 100.0))
                        } else {
                            None
                        };

                        view! {
                            <div class="setting-group">
                                <div class="setting-group-title">"File"</div>
                                <div class="analysis-stats">
                                    <div class="analysis-stat">
                                        <span class="analysis-stat-value">{sr_text}</span>
                                        <span class="analysis-stat-label">"Sample rate"</span>
                                    </div>
                                    <div class="analysis-stat">
                                        <span class="analysis-stat-value">{ch_text}</span>
                                        <span class="analysis-stat-label">"Channels"</span>
                                    </div>
                                    <div class="analysis-stat">
                                        <span class="analysis-stat-value">{bit_text}</span>
                                        <span class="analysis-stat-label">"Bit depth"</span>
                                    </div>
                                    <div class="analysis-stat">
                                        <span class="analysis-stat-value">{dur_text}</span>
                                        <span class="analysis-stat-label">"Duration"</span>
                                    </div>
                                    <div class="analysis-stat">
                                        <span class="analysis-stat-value">{samples_text}</span>
                                        <span class="analysis-stat-label">"Samples"</span>
                                    </div>
                                </div>
                            </div>
                            <div class="setting-group">
                                <div class="setting-group-title">"Signal"</div>
                                <div class="analysis-stats">
                                    <div class="analysis-stat">
                                        <span class="analysis-stat-value">{format!("{:.4}", sig_min)}</span>
                                        <span class="analysis-stat-label" title=min_db>"Min"</span>
                                    </div>
                                    <div class="analysis-stat">
                                        <span class="analysis-stat-value">{format!("{:.4}", sig_max)}</span>
                                        <span class="analysis-stat-label" title=max_db>"Max"</span>
                                    </div>
                                    <div class="analysis-stat">
                                        <span class="analysis-stat-value">{rms_db}</span>
                                        <span class="analysis-stat-label">"RMS"</span>
                                    </div>
                                    <div class="analysis-stat">
                                        <span class="analysis-stat-value">{dc_db}</span>
                                        <span class="analysis-stat-label" title=dc_raw_tooltip>"DC bias"</span>
                                    </div>
                                </div>
                                {dc_warning.map(|w| view! { <div class="analysis-warning">{w}</div> })}
                            </div>
                        }.into_any()
                    }
                }
            }}
            // Computing indicator + analysis scope badge
            {move || {
                if is_computing.get() {
                    view! { <div class="sidebar-panel-empty">"Computing analysis\u{2026}"</div> }.into_any()
                } else if file_is_long.get() && !analysis_is_full.get() && analysis.get().is_some() {
                    view! {
                        <div class="analysis-scope-row">
                            <span class="analysis-scope-badge">"First 30s"</span>
                            <button
                                class="analysis-full-btn"
                                on:click=move |_| run_analysis(true)
                            >
                                "Analyze full file"
                            </button>
                        </div>
                    }.into_any()
                } else if analysis_is_full.get() && file_is_long.get() && analysis.get().is_some() {
                    view! {
                        <div class="analysis-scope-row">
                            <span class="analysis-scope-badge analysis-scope-full">"Full file"</span>
                        </div>
                    }.into_any()
                } else {
                    view! { <span></span> }.into_any()
                }
            }}
            // wSNR section
            {move || {
                match wsnr_result.get().as_ref() {
                    None => view! { <span></span> }.into_any(),
                    Some(w) => {
                        let grade_class = match w.grade {
                            wsnr::WsnrGrade::A => "wsnr-grade wsnr-grade-a",
                            wsnr::WsnrGrade::B => "wsnr-grade wsnr-grade-b",
                            wsnr::WsnrGrade::C => "wsnr-grade wsnr-grade-c",
                            wsnr::WsnrGrade::D => "wsnr-grade wsnr-grade-d",
                            wsnr::WsnrGrade::E => "wsnr-grade wsnr-grade-e",
                        };
                        let grade_label = w.grade.label().to_string();
                        let snr_text = format!("{:.1} dB(ISO/ITU)", w.snr_db);
                        let signal_text = format!("Signal: {:.1} dB (ISO 226)", w.signal_db);
                        let noise_text = format!("Noise: {:.1} dB (ITU-R 468)", w.noise_db);

                        let xc_comparison = xc_quality.get().map(|xc_q| {
                            let xc_letter = xc_q.trim().to_uppercase();
                            let xc_badge_class = match xc_letter.as_str() {
                                "A" => "wsnr-grade-sm wsnr-grade-a",
                                "B" => "wsnr-grade-sm wsnr-grade-b",
                                "C" => "wsnr-grade-sm wsnr-grade-c",
                                "D" => "wsnr-grade-sm wsnr-grade-d",
                                _ => "wsnr-grade-sm wsnr-grade-e",
                            };
                            //let computed = grade_label.clone();
                            let note = "";
                            /*
                            let note = if xc_letter == computed {
                                "(matches)".to_string()
                            } else {
                                format!("(estimated: {})", computed)
                            };
                            */
                            (xc_letter, xc_badge_class.to_string(), note)
                        });

                        let warnings: Vec<_> = w.warnings.iter().map(|msg| {
                            let msg = msg.clone();
                            view! { <div class="wsnr-warning">{msg}</div> }
                        }).collect();

                        view! {
                            <div class="setting-group">
                                <div class="setting-group-title">"Recording Quality"</div>
                                <div class="wsnr-result">
                                    <div class="wsnr-detail">Estimated wSNR:</div>
                                    <div class="wsnr-header">
                                        <span class=grade_class>{grade_label}</span>
                                        <span class="wsnr-snr">{snr_text}</span>
                                    </div>
                                    <div class="wsnr-detail">{signal_text}</div>
                                    <div class="wsnr-detail">{noise_text}</div>
                                    {if !warnings.is_empty() {
                                        view! { <div class="wsnr-warnings">{warnings}</div> }.into_any()
                                    } else {
                                        view! { <span></span> }.into_any()
                                    }}
                                    {xc_comparison.map(|(letter, badge_class, note)| view! {
                                        <div class="wsnr-comparison">
                                            "Metadata: "
                                            <span class=badge_class>{letter}</span>
                                            " (XC grade)" {note}
                                        </div>
                                    })}
                                </div>
                            </div>
                        }.into_any()
                    }
                }
            }}
            // Bit analysis section
            {move || {
                match analysis.get().as_ref() {
                    None => view! { <span></span> }.into_any(),
                    Some(a) => {
                        let bits = a.bits_per_sample as usize;
                        let cols = 4usize;
                        let total = a.total_samples;
                        let summary = a.summary.clone();
                        let warnings = a.warnings.clone();
                        let is_float = a.is_float;
                        let bits_per_sample = a.bits_per_sample;
                        let effective_bits = a.effective_bits;
                        let effective_bits_f64 = a.effective_bits_f64;
                        let headroom_bits = a.headroom_bits;
                        let noise_floor_db = a.noise_floor_db;
                        let value_coverage = a.value_coverage.clone();

                        // Positive/negative/zero split grids
                        let pos_total = a.positive_total;
                        let neg_total = a.negative_total;
                        let zero_total = a.zero_total;
                        let pos_counts = a.positive_counts.clone();
                        let neg_counts = a.negative_counts.clone();

                        let non_silent = pos_total + neg_total;
                        let is_asymmetric = non_silent > 0 && a.bit_cautions.first()
                            .map(|c| c.iter().any(|x| matches!(x, BitCaution::SignBitSkewed)))
                            .unwrap_or(false);

                        let pos_pct_f = if total > 0 { pos_total as f64 / total as f64 * 100.0 } else { 0.0 };
                        let neg_pct_f = if total > 0 { neg_total as f64 / total as f64 * 100.0 } else { 0.0 };
                        let zero_pct_f = if total > 0 { zero_total as f64 / total as f64 * 100.0 } else { 0.0 };
                        let pos_pct = format!("{:.0}%", pos_pct_f);
                        let neg_pct = format!("{:.0}%", neg_pct_f);
                        let zero_pct = format!("{:.0}%", zero_pct_f);
                        let pos_tooltip = format!("{} samples above 0", pos_total);
                        let neg_tooltip = format!("{} samples below 0", neg_total);
                        let zero_tooltip = format!("{} samples exactly zero", zero_total);

                        // Sample distribution line — always shown; prefixed "Asymmetric:" if skewed
                        let split_label = if is_asymmetric { "Asymmetric:" } else { "Sample split:" };
                        let split_text = format!("{} {} +, {} \u{2212}, {} silence",
                            split_label, pos_pct, neg_pct, zero_pct);
                        let split_tooltip = format!("{} positive, {} negative, {} silence",
                            pos_total, neg_total, zero_total);

                        // Integer-only bit depth stats (computed once for both display and ordering)
                        let zero_padding = if !is_float { bits_per_sample - effective_bits } else { 0 };
                        let effective_depth = if !is_float {
                            bits_per_sample.saturating_sub(headroom_bits).saturating_sub(zero_padding)
                        } else {
                            0
                        };
                        let headroom_db = headroom_bits as f64 * 20.0 * 2f64.log10();

                        let entropy_text = format!("Entropy estimate: ~{:.1} bits", effective_bits_f64);
                        let entropy_tooltip = format!("Estimated effective bit depth (Shannon entropy sum); nominal: {}-bit", bits_per_sample);

                        let warning_items: Vec<_> = warnings.iter().map(|w| {
                            let w = w.clone();
                            view! { <div class="bit-warning">{w}</div> }
                        }).collect();

                        let make_sign_grid = |sign_counts: &[usize], sign_total: usize, polarity: &str| -> Vec<_> {
                            (0..bits).map(|idx| {
                                let count = sign_counts[idx];
                                let label = bit_analysis::bit_label(idx, bits_per_sample, is_float);
                                let is_sign_bit = idx == 0;
                                // Sign bit is always 0% or 100% by definition — keep grey
                                if is_sign_bit {
                                    let value_text = if sign_total > 0 && count == sign_total {
                                        "100%".to_string()
                                    } else if sign_total > 0 {
                                        "0%".to_string()
                                    } else {
                                        "\u{2013}".to_string()
                                    };
                                    let sign_tooltip = if polarity == "positive" {
                                        "Sign bit: always 0 for positive samples".to_string()
                                    } else {
                                        "Sign bit: always 1 for negative samples".to_string()
                                    };
                                    return view! {
                                        <div class="bit-cell unused" title=sign_tooltip>
                                            <span class="bit-label">{label}</span>
                                            <span class="bit-value">{value_text}</span>
                                        </div>
                                    };
                                }
                                let used = count > 0;
                                // Zero-count non-sign bits are zero-padded (red); used bits get normal coloring
                                let cell_class = if sign_total > 0 && count == sign_total {
                                    "bit-cell used full"
                                } else if used {
                                    "bit-cell used"
                                } else {
                                    "bit-cell zero-padded"
                                };
                                let value_text = if count == 0 {
                                    "\u{2013}".to_string()
                                } else if sign_total > 0 {
                                    let pct = count as f64 / sign_total as f64 * 100.0;
                                    if count == sign_total {
                                        "100%".to_string()
                                    } else if pct >= 99.9 {
                                        "~100%".to_string()
                                    } else if pct >= 1.0 {
                                        format!("{:.0}%", pct)
                                    } else if count > 99 {
                                        "99+".into()
                                    } else {
                                        format!("{}", count)
                                    }
                                } else {
                                    "\u{2013}".to_string()
                                };
                                let tooltip = if sign_total > 0 {
                                    let pct = count as f64 / sign_total as f64 * 100.0;
                                    let missing = sign_total - count;
                                    if missing > 0 && pct >= 99.5 {
                                        format!("Bit {} is set in {} / {} {} samples ({:.1}%) — all but {}", label, count, sign_total, polarity, pct, missing)
                                    } else {
                                        format!("Bit {} is set in {} / {} {} samples ({:.1}%)", label, count, sign_total, polarity, pct)
                                    }
                                } else {
                                    format!("Bit {}: no {} samples", label, polarity)
                                };
                                view! {
                                    <div class=cell_class title=tooltip>
                                        <span class="bit-label">{label}</span>
                                        <span class="bit-value">{value_text}</span>
                                    </div>
                                }
                            }).collect()
                        };

                        let pos_grid = make_sign_grid(&pos_counts, pos_total, "positive");
                        let neg_grid = make_sign_grid(&neg_counts, neg_total, "negative");

                        let nf_bits = -noise_floor_db / (20.0 * 2f64.log10());
                        let noise_floor_text = format!("Noise floor: {:.1} dBFS (~{:.1} bits)", noise_floor_db, nf_bits);
                        let noise_floor_tooltip = "Minimum RMS level of 512-sample windows above digital silence (−80 dBFS); converted to equivalent bit depth at 6 dB/bit".to_string();

                        // Compose the tiered effective-bit-depth verdict from
                        // all the analyses we have so far. Lead with the
                        // strongest single claim; fold the existing stats.
                        let lsb_ref = lsb_result.get();
                        let pip_ref = pipistrelle_result.get();
                        let am_ref = audiomoth_result.get();
                        let bit_clone = a.clone();
                        let format_str: &'static str = {
                            let files = state.library.files().get();
                            let idx = state.library.current_index().get();
                            idx.and_then(|i| files.get(i).map(|f| f.audio.metadata.format))
                                .unwrap_or("WAV")
                        };
                        let bdc_opt: Option<BitDepthCertainty> = lsb_ref.as_ref().map(|lsb| {
                            bit_depth_certainty::compose(
                                bits_per_sample,
                                is_float,
                                format_str,
                                &bit_clone,
                                lsb,
                                pip_ref.as_ref(),
                                am_ref.as_ref(),
                            )
                        });
                        let headline_class = match bdc_opt.as_ref().map(|b| b.certainty) {
                            Some(CertaintyLevel::Certain) => "bit-depth-stat bit-depth-primary",
                            Some(CertaintyLevel::HighConfidence) => "bit-depth-stat bit-depth-primary",
                            _ => "bit-depth-stat",
                        };
                        let headline_text = bdc_opt.as_ref()
                            .map(|b| b.headline.clone())
                            .unwrap_or_else(|| {
                                if is_float {
                                    format!("{}-bit float", bits_per_sample)
                                } else {
                                    format!("Effective bit depth: {} bits", effective_depth)
                                }
                            });
                        let certainty_label = bdc_opt.as_ref()
                            .map(|b| b.certainty.label().to_string());
                        let fact_views: Vec<_> = bdc_opt.as_ref()
                            .map(|b| b.facts.iter().map(|f| {
                                let cls = match f.certainty {
                                    CertaintyLevel::Certain => "bit-depth-stat",
                                    CertaintyLevel::HighConfidence => "bit-depth-stat",
                                    CertaintyLevel::Suggestive => "wsnr-detail",
                                };
                                let prefix = match f.certainty {
                                    CertaintyLevel::Certain => "\u{2713} ",
                                    CertaintyLevel::HighConfidence => "\u{25CF} ",
                                    CertaintyLevel::Suggestive => "\u{25CB} ",
                                };
                                let text = format!("{}{}", prefix, f.statement);
                                view! { <div class=cls>{text}</div> }
                            }).collect())
                            .unwrap_or_default();

                        view! {
                            <div class="setting-group">
                                <div class="setting-group-title" title="Combines value-coverage counting, LSB stride analysis, autocorrelation tests and firmware-family inverse-filter detectors into a single best-guess effective bit depth, ranked by certainty.">
                                    "Bit depth"
                                </div>
                                // Headline (strongest single claim)
                                <div class=headline_class>{headline_text}</div>
                                {certainty_label.map(|c| view! {
                                    <div class="wsnr-detail" style="font-style: italic;">
                                        {format!("({})", c)}
                                    </div>
                                })}
                                // Supporting facts, strongest-first, visible by default
                                {fact_views}
                                <div class=if is_asymmetric { "bit-warning" } else { "bit-depth-stat" } title=split_tooltip>{split_text}</div>
                                // Existing detail stats — fold so they don't dominate.
                                <details class="bit-depth-stat">
                                    <summary>"Bit-usage details"</summary>
                                    {if !is_float {
                                        let summary_class = if effective_bits < bits_per_sample { "bit-warning" } else { "bit-depth-stat" };
                                        view! {
                                            <div>
                                                <div class="bit-depth-stat">{format!("Per-bit-analysis effective depth: {} bits", effective_depth)}</div>
                                                <div class="bit-depth-stat" title=entropy_tooltip>{entropy_text}</div>
                                                {if headroom_bits > 0 {
                                                    view! { <div class="bit-depth-stat">{format!("Headroom: {} bit{} ({:.1} dB)", headroom_bits, if headroom_bits == 1 { "" } else { "s" }, headroom_db)}</div> }.into_any()
                                                } else { view! { <span></span> }.into_any() }}
                                                {if zero_padding > 0 {
                                                    view! { <div class="bit-depth-stat">{format!("Zero padding: {} bit{}", zero_padding, if zero_padding == 1 { "" } else { "s" })}</div> }.into_any()
                                                } else { view! { <span></span> }.into_any() }}
                                                {match value_coverage {
                                                    Some(ref vc) => {
                                                        let coverage_text = format!("Value coverage: {:.1}% ({} of {})",
                                                            vc.coverage_pct, vc.unique_count, vc.value_space);
                                                        let coverage_tooltip = format!("{} distinct sample values observed out of {} possible for {}-bit audio",
                                                            vc.unique_count, vc.value_space, bits_per_sample);
                                                        let resolution_text = format!("Value resolution: ~{:.1} bits ", vc.resolution_bits);
                                                        let resolution_tooltip = format!("log\u{2082}({}) = {:.2}", vc.unique_count, vc.resolution_bits);
                                                        let ceiled = vc.resolution_bits.ceil() as u16;
                                                        let suffix_text = format!("({}-bit)", ceiled);
                                                        view! {
                                                            <div>
                                                                <div class="bit-depth-stat" title=coverage_tooltip>{coverage_text}</div>
                                                                <div class="bit-depth-stat" title=resolution_tooltip>{resolution_text}{suffix_text}</div>
                                                            </div>
                                                        }.into_any()
                                                    }
                                                    None => view! { <span></span> }.into_any(),
                                                }}
                                                <div class=summary_class>{summary}</div>
                                            </div>
                                        }.into_any()
                                    } else {
                                        view! {
                                            <div>
                                                <div class="bit-depth-stat">{summary}</div>
                                                <div class="bit-depth-stat" title=entropy_tooltip>{entropy_text}</div>
                                            </div>
                                        }.into_any()
                                    }}
                                    <div class="bit-depth-stat" title=noise_floor_tooltip>{noise_floor_text}</div>
                                </details>
                                {warning_items}
                                <div class="bit-sign-header" title=pos_tooltip>{format!("Samples above zero ({})", pos_pct)}</div>
                                <div class="bit-grid" style=format!("grid-template-columns: repeat({}, 1fr);", cols)>
                                    {pos_grid}
                                </div>
                                <div class="bit-sign-header" title=neg_tooltip>{format!("Samples below zero ({})", neg_pct)}</div>
                                <div class="bit-grid" style=format!("grid-template-columns: repeat({}, 1fr);", cols)>
                                    {neg_grid}
                                </div>
                                <div class="bit-sign-header" title=zero_tooltip>{format!("Silence ({})", zero_pct)}</div>
                            </div>
                        }.into_any()
                    }
                }
            }}
            // LSB Autocorrelation section
            {move || {
                match lsb_result.get().as_ref() {
                    None => view! { <span></span> }.into_any(),
                    Some(l) if matches!(l.verdict, LsbVerdict::NotApplicable) => {
                        view! { <span></span> }.into_any()
                    }
                    Some(l) => render_lsb_section(l).into_any()
                }
            }}
            // Mic Signatures section (Pipistrelle-family firmware detection)
            {move || {
                match pipistrelle_result.get().as_ref() {
                    None => view! { <span></span> }.into_any(),
                    Some(p) if matches!(p.verdict, PipistrelleVerdict::NotApplicable | PipistrelleVerdict::NoMatch) => {
                        view! { <span></span> }.into_any()
                    }
                    Some(p) => render_pipistrelle_section(p).into_any(),
                }
            }}
            // Device hints + metadata comparison
            {move || {
                let files = state.library.files().get();
                let idx = state.library.current_index().get();
                let file = idx.and_then(|i| files.get(i).cloned());
                let (Some(file), Some(bit), Some(lsb), Some(pip)) = (
                    file,
                    analysis.get(),
                    lsb_result.get(),
                    pipistrelle_result.get(),
                ) else { return view! { <span></span> }.into_any() };
                let sr = file.audio.sample_rate;
                let bits = file.audio.metadata.bits_per_sample;
                let is_float = file.audio.metadata.is_float;
                let nyq_opt = nyquist_result.get();
                let am_opt = audiomoth_result.get();
                let hints = device_hint::infer_device_hints(
                    sr, bits, is_float, &bit, &lsb, &pip, nyq_opt.as_ref(), am_opt.as_ref(),
                );
                let guano = file.audio.metadata.guano.as_ref().map(|g| g.fields.clone());
                let xc = file.xc_metadata.clone();
                let metadata_match = device_hint::compare_to_metadata(
                    &hints,
                    xc.as_deref(),
                    guano.as_deref(),
                );
                if hints.is_empty() && matches!(metadata_match, MetadataMatch::NoClaim) {
                    return view! { <span></span> }.into_any();
                }
                render_device_hints_section(hints, metadata_match).into_any()
            }}
        </div>
    }
}

// ============================================================================
// Helper renderers for the analysis subsections.
// Pulled out of the main view so each can be edited without grappling with
// the giant `view! { ... }` block.
// ============================================================================

/// Render the analysis panel's File-info area for an Anabat .zc file.
/// The audio side of these recordings is a silent placeholder, so the
/// normal File / Signal / wSNR / Bit-Usage / LSB / Mic-Signatures
/// sections don't apply — we replace them with a ZC-specific view that
/// surfaces the dot count, frequency range, header metadata, and any
/// embedded GUANO key/values.
fn render_zc_file_info(f: &crate::state::LoadedFile) -> impl IntoView {
    let Some(zc) = f.audio.metadata.zc_data.as_ref() else {
        return view! { <span></span> }.into_any();
    };
    let md = &zc.metadata;

    // ── Top-level facts ────────────────────────────────────────────────
    let total = zc.times_s.len();
    let on = zc.on_dot_count();
    let off = total.saturating_sub(on);
    let duration = zc.duration_secs();
    let dur_text = crate::format_time::format_duration(duration, 3);

    // Compute frequency stats over ON dots with valid frequency.
    let mut freqs: Vec<f64> = zc
        .freqs_hz
        .iter()
        .zip(&zc.off_mask)
        .filter_map(|(&f, &off)| (!off && f > 0.0).then_some(f))
        .collect();
    freqs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let freq_range = if freqs.is_empty() {
        None
    } else {
        let min = freqs[0];
        let max = freqs[freqs.len() - 1];
        let mean = freqs.iter().sum::<f64>() / freqs.len() as f64;
        let median = freqs[freqs.len() / 2];
        Some((min, max, mean, median))
    };

    // (Header-metadata rows moved to the Info/Metadata tab —
    // see `zc_header_section()` in metadata_panel.rs.)

    let guano_views: Vec<_> = md
        .guano
        .iter()
        .map(|(k, v)| {
            view! {
                <div class="bit-depth-stat" style="margin-left: 1em;">
                    {format!("{} = {}", k, v)}
                </div>
            }
        })
        .collect();
    let has_guano = !md.guano.is_empty();

    let off_pct = if total > 0 {
        off as f64 * 100.0 / total as f64
    } else {
        0.0
    };
    let dot_summary = format!("{} dots ({} ON, {} OFF — {:.1}%)", total, on, off, off_pct,);
    let freq_lines: Vec<_> = freq_range
        .map(|(min, max, mean, median)| {
            vec![
            view! {
                <div class="bit-depth-stat bit-depth-primary">
                    {format!("Frequency range: {:.1} – {:.1} kHz (mean {:.1} kHz)",
                        min / 1000.0, max / 1000.0, mean / 1000.0)}
                </div>
            }.into_any(),
            view! {
                <div class="bit-depth-stat">
                    {format!("Median {:.1} kHz \u{2014} sorted across {} valid-frequency ON dots",
                        median / 1000.0, freqs.len())}
                </div>
            }.into_any(),
        ]
        })
        .unwrap_or_default();

    let divratio = md.divratio;
    let divratio_explanation = format!(
        "divratio = {} \u{2014} the recorder's frequency-divider ratio. \
         Dot frequency is computed as divratio × 10\u{2076} / (\u{0394}t in \u{00B5}s).",
        divratio,
    );
    let file_type_text = format!(
        "ZC file type {} \u{2014} {}",
        md.file_type,
        if md.file_type >= 132 {
            "modern (v132+) with embedded timestamp and GUANO"
        } else {
            "legacy (pre-v132) without embedded timestamp"
        },
    );

    view! {
        <div>
            <div class="setting-group">
                <div class="setting-group-title" title="Anabat zero-crossing recording — no continuous waveform, just dot timing.">
                    "ZC Recording"
                </div>
                <div class="bit-depth-stat bit-depth-primary">{dot_summary}</div>
                {freq_lines}
                <div class="bit-depth-stat">
                    {format!("Duration: {}", dur_text)}
                </div>
                <div class="wsnr-detail">{file_type_text}</div>
                <div class="bit-depth-stat" title=divratio_explanation>
                    {format!("divratio: {}, vres: {}, res1: {} Hz",
                        md.divratio, md.vres, md.res1)}
                </div>
            </div>

            {if has_guano {
                view! {
                    <div class="setting-group">
                        <div class="setting-group-title" title="GUANO key/value metadata embedded in the .zc file (Anabat Insight, Roostlogger, etc. all write this).">
                            "GUANO metadata"
                        </div>
                        {guano_views}
                    </div>
                }.into_any()
            } else { view! { <span></span> }.into_any() }}
        </div>
    }.into_any()
}

fn render_lsb_section(l: &LsbAutocorrResult) -> impl IntoView {
    // Human-readable lead line per verdict.
    let (verdict_text, verdict_class) = match &l.verdict {
        LsbVerdict::ZeroPaddedNBit {
            effective_bits,
            padding_bits,
        } => (
            format!(
                "Effective {}-bit \u{2014} low {} bit{} of every sample are literally zero",
                effective_bits,
                padding_bits,
                if *padding_bits == 1 { "" } else { "s" },
            ),
            "bit-depth-stat bit-depth-primary",
        ),
        LsbVerdict::QuietSectionZeroPadded {
            effective_bits_in_quiet,
            padding_bits,
        } => (
            format!(
                "Quiet sections drop to {}-bit (low {} bits zeroed in silence; \
                 full depth elsewhere) \u{2014} typical of noise-gated firmware",
                effective_bits_in_quiet, padding_bits,
            ),
            "bit-warning",
        ),
        LsbVerdict::DspPaddedLowBitDepth {
            effective_bits_guess,
        } => (
            format!(
                "Low bits look deterministic \u{2014} probably a ~{}-bit ADC plus \
                 on-device DSP padded into the {}-bit container",
                effective_bits_guess,
                l.n_low + 4,
            ),
            "bit-warning",
        ),
        LsbVerdict::ConsistentWithClaimedBitDepth => (
            "Low bits look like analog noise \u{2014} bit-depth claim is plausible".into(),
            "bit-depth-stat",
        ),
        LsbVerdict::Inconclusive => (
            format!(
                "Inconclusive \u{2014} even the quietest spot in the file is too \
                 noisy (stdev {:.1} LSB) for the LSB tests to discriminate",
                l.quietest_window_stdev,
            ),
            "wsnr-detail",
        ),
        LsbVerdict::NotApplicable => ("Not applicable".into(), "wsnr-detail"),
    };

    // Pattern length (if any short repeating period was found in the low bits).
    let pattern_line = l.low_bit_period.and_then(|p| {
        // Hide the trivial "period 1" for the zero-padded case; the lead line
        // already says the bits are literally zero.
        if p == 1 && matches!(l.verdict, LsbVerdict::ZeroPaddedNBit { .. }) {
            return None;
        }
        Some(format!(
            "Low bits repeat every {} sample{} ({:.0}% match)",
            p,
            if p == 1 { "" } else { "s" },
            l.low_bit_period_match * 100.0,
        ))
    });

    // Detail lines for the fold-down.
    let chi2_text = format!(
        "Low {}-bit stats (quietest 4096-sample window): chi\u{00B2} = {:.0}, lag-1 = {:+.3}, lag-256 = {:+.3}",
        l.n_low, l.quiet_lsb_chi2, l.quiet_lsb_lag1_acf, l.quiet_lsb_lag256_acf,
    );
    let chi2_tooltip = format!(
        "chi\u{00B2} (df=15): >37.7 is significant at p<0.001 \u{2014} large values mean \
         the low bits don't look uniform.\n\
         lag-1 / lag-256: autocorrelation of the signed-centered low bits. Real ADC \
         noise is uncorrelated (~0); DSP residue is correlated (further from 0).\n\
         Quietest window starts at sample {}.",
        l.quietest_window_idx,
    );
    let nf_text = format!(
        "Quietest window stdev: {:.1} LSB, {:.1}% of samples have nonzero low bits",
        l.quietest_window_stdev,
        l.quiet_lsb_nonzero_frac * 100.0,
    );
    let nf_tooltip = "Stdev of the quietest 4096-sample window in raw integer LSB units. \
        Noise floor above ~16 LSB dithers out any DSP signature in the low bits, so we \
        can't tell DSP-padded from true full-bit-depth above that.";
    let gcd_line = if l.gcd_nonzero > 1 {
        Some(format!(
            "All nonzero samples are multiples of {}",
            l.gcd_nonzero
        ))
    } else {
        None
    };
    let explanation = l.explanation.clone();

    view! {
        <div class="setting-group">
            <div class="setting-group-title" title="Tests whether the lowest few bits look like analog noise or like padding from a lower-bit-depth source.">
                "Low-bit analysis"
            </div>
            <div class=verdict_class>{verdict_text}</div>
            {pattern_line.map(|p| view! { <div class="bit-warning">{p}</div> })}
            <details class="bit-depth-stat">
                <summary>"Stats"</summary>
                <div class="bit-depth-stat" title=chi2_tooltip>{chi2_text}</div>
                <div class="bit-depth-stat" title=nf_tooltip>{nf_text}</div>
                {gcd_line.map(|g| view! { <div class="bit-depth-stat">{g}</div> })}
                {if !explanation.is_empty() {
                    view! { <div class="wsnr-detail">{explanation}</div> }.into_any()
                } else { view! { <span></span> }.into_any() }}
            </details>
        </div>
    }
}

fn render_pipistrelle_section(p: &PipistrelleResult) -> impl IntoView {
    let family_tooltip = format!(
        "The Pipistrelle-family RP2040 firmware is open source and ships in \
         multiple devices: {}. We can only narrow this down to the family, not \
         the specific product.",
        PIPISTRELLE_FAMILY.join(", "),
    );

    let (lead_text, lead_class) = match p.verdict {
        PipistrelleVerdict::Match => (
            format!(
                "Pipistrelle-family firmware detected (dBcut={}, {:.2}% reconstruction error)",
                p.best_db_cut.unwrap_or(0),
                p.best_normalized_residual * 100.0,
            ),
            "bit-depth-stat bit-depth-primary",
        ),
        PipistrelleVerdict::Possible => (
            format!(
                "Possibly Pipistrelle-family firmware (dBcut={}, {:.2}% reconstruction error)",
                p.best_db_cut.unwrap_or(0),
                p.best_normalized_residual * 100.0,
            ),
            "bit-warning",
        ),
        PipistrelleVerdict::NoMatch => (
            "No Pipistrelle-family firmware signature".into(),
            "wsnr-detail",
        ),
        PipistrelleVerdict::NotApplicable => (
            "Pipistrelle-family detection skipped (wrong sample rate / format)".into(),
            "wsnr-detail",
        ),
    };

    // Caveat line for Possible matches — make sure the user understands this
    // is not conclusive.
    let caveat = matches!(p.verdict, PipistrelleVerdict::Possible).then(|| {
        "Note: many bandlimited 16-bit ultrasonic recordings give similarly-low \
         reconstruction errors, so a 'possible' verdict is far from conclusive."
            .to_string()
    });

    // Detail lines.
    let near_int_text = format!(
        "Recovered ADC samples landing within \u{00B1}0.1 of an integer: {}/{} = {:.1}% \
         (uniform baseline 20%)",
        p.best_near_integer_match,
        p.best_near_integer_total,
        p.best_near_integer_frac * 100.0,
    );
    let near_int_tooltip = "After inverse-filtering the firmware's DSP chain in float, \
        the recovered ADC values should be integers (modulo firmware truncation noise). \
        Real Pipistrelle recordings cluster on integers; unrelated signals look uniform.";
    let mean_frac_text = format!(
        "Mean |fractional part| = {:.3} (uniform baseline 0.25)",
        p.best_mean_abs_frac,
    );
    let in_range_text = format!(
        "{:.1}% of recovered values fall inside the 12-bit ADC range [0, 4095]",
        p.best_in_range_frac * 100.0,
    );
    let windows_text = format!(
        "{} window{} analysed, {} skipped (below silence gate)",
        p.windows_used,
        if p.windows_used == 1 { "" } else { "s" },
        p.windows_skipped_silent,
    );
    let preset_lines: Vec<_> = p
        .per_preset
        .iter()
        .map(|s| {
            let line = format!(
                "dBcut={}: residual {:.2}%, near-integer {:.1}%, in-range {:.1}%",
                s.db_cut,
                s.normalized_residual * 100.0,
                s.near_integer_frac * 100.0,
                s.in_range_frac * 100.0,
            );
            view! { <div class="bit-depth-stat">{line}</div> }
        })
        .collect();

    view! {
        <div class="setting-group">
            <div class="setting-group-title" title=family_tooltip>
                "Firmware fingerprint (Pipistrelle family)"
            </div>
            <div class=lead_class>{lead_text}</div>
            {caveat.map(|c| view! { <div class="wsnr-detail">{c}</div> })}
            <details class="bit-depth-stat">
                <summary>"Stats"</summary>
                <div class="bit-depth-stat" title=near_int_tooltip>{near_int_text}</div>
                <div class="bit-depth-stat">{mean_frac_text}</div>
                <div class="bit-depth-stat">{in_range_text}</div>
                <div class="wsnr-detail">{windows_text}</div>
                <details>
                    <summary>"Per-preset"</summary>
                    {preset_lines}
                </details>
            </details>
        </div>
    }
}

fn render_device_hints_section(
    hints: Vec<DeviceHint>,
    metadata_match: MetadataMatch,
) -> impl IntoView {
    // Skip the section entirely if there's literally nothing to show.
    let any_strong_or_likely_hint = hints.iter().any(|h| {
        matches!(
            h.confidence,
            HintConfidence::Strong | HintConfidence::Likely
        )
    });
    let show_section =
        any_strong_or_likely_hint || !matches!(metadata_match, MetadataMatch::NoClaim);

    if !show_section {
        return view! { <span></span> }.into_any();
    }

    let hint_rows: Vec<_> = hints
        .iter()
        .map(|h| {
            let class = match h.confidence {
                HintConfidence::Strong => "bit-depth-stat bit-depth-primary",
                HintConfidence::Likely => "bit-warning",
                HintConfidence::Possible => "wsnr-detail",
            };
            let candidates = if h.candidates.is_empty() {
                String::new()
            } else {
                h.candidates.join(", ")
            };
            let detail = h.detail.clone();
            let label = h.label.clone();
            let candidates_view = if !candidates.is_empty() {
                view! {
                    <div class="wsnr-detail" style="margin-left: 1em;">
                        {format!("Candidates: {}", candidates)}
                    </div>
                }
                .into_any()
            } else {
                view! { <span></span> }.into_any()
            };
            view! {
                <div>
                    <div class=class title=detail>{label}</div>
                    {candidates_view}
                </div>
            }
        })
        .collect();

    let metadata_view = match metadata_match {
        MetadataMatch::NoClaim => view! { <span></span> }.into_any(),
        MetadataMatch::ClaimNoAnalysis { claim } => view! {
            <div class="wsnr-detail">
                {format!("File metadata says: {}", claim)}
                " — analysis did not produce any device fingerprints to compare against."
            </div>
        }.into_any(),
        MetadataMatch::Match { claim, matched_candidate } => view! {
            <div class="bit-depth-stat" title="Metadata-claimed device matches one of the analysis candidates.">
                "\u{2713} Metadata says "
                <strong>{claim}</strong>
                " — matches analysis ("
                {matched_candidate}
                ")"
            </div>
        }.into_any(),
        MetadataMatch::Mismatch { claim, hint_summary } => view! {
            <div class="bit-warning" title="Metadata-claimed device does NOT match what the analysis suggests. The file may have been mis-tagged, post-processed, or the analysis hint may be wrong.">
                "\u{26A0} Metadata says "
                <strong>{claim}</strong>
                " — but analysis suggests: "
                {hint_summary}
            </div>
        }.into_any(),
    };

    view! {
        <div class="setting-group">
            <div class="setting-group-title" title="Best-guess device candidates combining bit-depth analysis, low-bit signature, and firmware-fingerprint tests. Compared against any device claim in the file's XC or GUANO metadata.">
                "Device hints"
            </div>
            {hint_rows}
            {metadata_view}
        </div>
    }.into_any()
}

/// Yield once to the browser event loop via a zero-duration setTimeout.
async fn yield_to_browser() {
    let promise = js_sys::Promise::new(&mut |resolve, _reject| {
        let win = web_sys::window().unwrap();
        let cb = Closure::once_into_js(move || {
            let _ = resolve.call0(&JsValue::NULL);
        });
        let _ = win.set_timeout_with_callback_and_timeout_and_arguments_0(cb.unchecked_ref(), 0);
    });
    let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
}
