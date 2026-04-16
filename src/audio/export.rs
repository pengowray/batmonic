//! WAV export: process audio regions through the DSP pipeline and download as WAV files.

use leptos::prelude::*;
use wasm_bindgen::JsCast;

use crate::annotations::{Annotation, AnnotationKind, Region};
use crate::audio::microphone::encode_wav;
use crate::audio::playback::snapshot_params;
use crate::audio::source::{AudioSource, ChannelView};
use crate::audio::streaming_playback::{apply_dsp_mode, apply_filters, PlaybackParams, PV_MODE_BOOST_DB};
use crate::audio::playback::apply_gain;
use crate::state::{AppState, PlaybackMode, Selection};

/// Number of source samples per export chunk (same as streaming playback).
const CHUNK_SAMPLES: usize = 96_000;

/// Extra overlap samples prepended for IIR filter warmup.
const FILTER_WARMUP: usize = 4096;

/// Overlap samples for PV/PS crossfade (same as streaming_playback::PV_HQ_OVERLAP).
const PV_HQ_OVERLAP: usize = 8192;

/// Build PlaybackParams for exporting a region.
/// When `use_region_focus` is true and the region has frequency bounds,
/// those bounds drive the selection-based bandpass/heterodyne.
pub(crate) fn build_export_params(
    state: &AppState,
    region: Option<&Region>,
    use_region_focus: bool,
    sample_rate: u32,
) -> PlaybackParams {
    let selection = if use_region_focus {
        region.and_then(|r| {
            match (r.freq_low, r.freq_high) {
                (Some(lo), Some(hi)) => Some(Selection {
                    time_start: r.time_start,
                    time_end: r.time_end,
                    freq_low: Some(lo),
                    freq_high: Some(hi),
                }),
                _ => None,
            }
        })
    } else {
        state.selection.get_untracked()
    };
    snapshot_params(state, selection, sample_rate)
}

/// Process a time range through the DSP pipeline and return processed f32 samples.
pub(crate) fn process_region(
    source: &dyn AudioSource,
    sample_rate: u32,
    start_time: f64,
    end_time: f64,
    params: &PlaybackParams,
) -> Vec<f32> {
    let start_sample = (start_time * sample_rate as f64) as usize;
    let end_sample = ((end_time * sample_rate as f64) as usize).min(source.total_samples() as usize);

    let crossfade_mode = params.pv_hq
        && matches!(params.mode, PlaybackMode::PhaseVocoder | PlaybackMode::PitchShift);

    let mut all_samples: Vec<f32> = Vec::new();
    let mut pos = start_sample;
    let mut is_first_chunk = true;

    while pos < end_sample {
        let chunk_end = (pos + CHUNK_SAMPLES).min(end_sample);
        let warmup_start = pos.saturating_sub(FILTER_WARMUP);
        let warmup_len = pos - warmup_start;

        // In crossfade mode, extend the read past the nominal end
        let trailing_end = if crossfade_mode {
            (chunk_end + PV_HQ_OVERLAP).min(end_sample)
        } else if matches!(params.mode, PlaybackMode::PitchShift | PlaybackMode::PhaseVocoder) {
            (chunk_end + FILTER_WARMUP).min(end_sample)
        } else {
            chunk_end
        };
        let trailing_len = trailing_end - chunk_end;

        let chunk_with_warmup = source.read_region(
            ChannelView::MonoMix,
            warmup_start as u64,
            trailing_end - warmup_start,
        );
        let filtered = apply_filters(&chunk_with_warmup, sample_rate, params);
        let processed = apply_dsp_mode(&filtered, sample_rate, params);

        if crossfade_mode {
            // Trim warmup but keep trailing overlap
            let trim_start = warmup_len;
            let mut chunk_samples = if trim_start < processed.len() {
                processed[trim_start..].to_vec()
            } else {
                processed.to_vec()
            };

            let core_len = chunk_end - pos;

            // Hann fade-in on leading overlap (skip for first chunk)
            if !is_first_chunk {
                let fade_in_len = PV_HQ_OVERLAP.min(core_len).min(chunk_samples.len());
                for (i, sample) in chunk_samples.iter_mut().enumerate().take(fade_in_len) {
                    let t = i as f32 / fade_in_len as f32;
                    let w = 0.5 * (1.0 - (std::f32::consts::PI * t).cos());
                    *sample *= w;
                }
            }

            // Hann fade-out on trailing overlap
            if trailing_len > 0 {
                let fade_out_start = chunk_samples.len().saturating_sub(trailing_len);
                let fade_out_len = chunk_samples.len() - fade_out_start;
                for i in 0..fade_out_len {
                    let t = i as f32 / fade_out_len as f32;
                    let w = 0.5 * (1.0 + (std::f32::consts::PI * t).cos());
                    chunk_samples[fade_out_start + i] *= w;
                }
            }

            // Overlap-add into output buffer
            if is_first_chunk {
                all_samples.extend_from_slice(&chunk_samples);
            } else {
                // The overlap region is the last PV_HQ_OVERLAP samples of the
                // existing output, which fade out. Sum with this chunk's
                // fade-in region.
                let overlap = PV_HQ_OVERLAP.min(all_samples.len()).min(chunk_samples.len());
                let out_start = all_samples.len() - overlap;
                for i in 0..overlap {
                    all_samples[out_start + i] += chunk_samples[i];
                }
                // Append the non-overlapping tail
                if overlap < chunk_samples.len() {
                    all_samples.extend_from_slice(&chunk_samples[overlap..]);
                }
            }
        } else {
            // Standard mode: trim warmup and trailing
            let trim_start = warmup_len;
            let trim_end = processed.len().saturating_sub(trailing_len);
            let trimmed = if trim_start < trim_end {
                &processed[trim_start..trim_end]
            } else {
                &processed[..]
            };
            all_samples.extend_from_slice(trimmed);
        }

        is_first_chunk = false;
        pos = chunk_end;
    }

    // Apply gain (including PV compensatory boost)
    let pv_boost = if matches!(params.mode, PlaybackMode::PhaseVocoder) { PV_MODE_BOOST_DB } else { 0.0 };
    let gain_db = params.gain_db + pv_boost;
    apply_gain(&mut all_samples, gain_db);

    all_samples
}

/// Trigger a browser download of raw bytes.
pub(crate) fn trigger_browser_download(data: &[u8], filename: &str) {
    let array = js_sys::Uint8Array::new_with_length(data.len() as u32);
    array.copy_from(data);

    let parts = js_sys::Array::new();
    parts.push(&array.buffer());

    let blob = match web_sys::Blob::new_with_u8_array_sequence(&parts) {
        Ok(b) => b,
        Err(e) => {
            log::error!("Failed to create Blob: {:?}", e);
            return;
        }
    };

    let url = match web_sys::Url::create_object_url_with_blob(&blob) {
        Ok(u) => u,
        Err(e) => {
            log::error!("Failed to create object URL: {:?}", e);
            return;
        }
    };

    let window = web_sys::window().unwrap();
    let document = window.document().unwrap();
    let a: web_sys::HtmlAnchorElement = document
        .create_element("a").unwrap()
        .dyn_into().unwrap();
    a.set_href(&url);
    a.set_download(filename);
    a.set_attribute("style", "display:none").ok();
    document.body().unwrap().append_child(&a).ok();
    a.click();
    document.body().unwrap().remove_child(&a).ok();
    web_sys::Url::revoke_object_url(&url).ok();
}

/// Export a single region as a WAV file and trigger browser download.
pub(crate) fn export_one_region(
    source: &dyn AudioSource,
    sample_rate: u32,
    start_time: f64,
    end_time: f64,
    params: &PlaybackParams,
    filename: &str,
    source_filename: &str,
    source_guano: Option<&crate::audio::guano::GuanoMetadata>,
) {
    let samples = process_region(source, sample_rate, start_time, end_time, params);

    // Determine output sample rate — TimeExpansion slows playback by changing rate
    let output_rate = match params.mode {
        PlaybackMode::TimeExpansion => {
            let te = params.te_factor;
            (sample_rate as f64 / te) as u32
        }
        _ => sample_rate,
    };

    let mut wav_data = encode_wav(&samples, output_rate);

    // Build GUANO metadata for the export
    let guano = build_export_guano(
        output_rate, &samples, params, filename, source_filename, source_guano,
    );
    crate::audio::guano::append_guano_chunk(&mut wav_data, &guano.to_text());

    trigger_browser_download(&wav_data, filename);
}

/// Build GUANO metadata for an exported WAV file.
///
/// Field ordering: GUANO|Version first, then standard fields, then Oversample-specific.
/// For TE exports, Length is the "actual length" (WAV length / TE factor) per GUANO spec.
fn build_export_guano(
    output_rate: u32,
    samples: &[f32],
    params: &PlaybackParams,
    _filename: &str,
    source_filename: &str,
    source_guano: Option<&crate::audio::guano::GuanoMetadata>,
) -> crate::audio::guano::GuanoMetadata {
    use crate::audio::guano::GuanoMetadata;
    let version = env!("CARGO_PKG_VERSION");
    let wav_duration_secs = samples.len() as f64 / output_rate as f64;

    let mut g = GuanoMetadata::new();

    // ── GUANO namespace (must come first per spec) ──────────────────────
    g.add("GUANO|Version", "1.0");

    // ── Standard GUANO fields ───────────────────────────────────────────

    // Preserve source timestamp if available
    if let Some(src) = source_guano {
        for (k, v) in &src.fields {
            if k == "Timestamp" {
                g.add("Timestamp", v);
                break;
            }
        }
    }

    // TE factor and Length: per GUANO spec, Length is the "actual length"
    // which equals WAV length for direct recordings, but WAV length / TE
    // for time-expanded recordings.
    if let PlaybackMode::TimeExpansion = params.mode {
        let te = params.te_factor;
        let actual_length = wav_duration_secs / te;
        g.add("Length", &format!("{:.6}", actual_length));
        g.add("TE", &format!("{}", te));
    } else {
        g.add("Length", &format!("{:.6}", wav_duration_secs));
    }

    g.add("Samplerate", &output_rate.to_string());

    // Preserve Make/Model from source if present (recording hardware info)
    if let Some(src) = source_guano {
        for (k, v) in &src.fields {
            match k.as_str() {
                "Make" | "Model" => { g.add(k, v); }
                _ => {}
            }
        }
    }

    g.add("Original Filename", source_filename);

    // Preserve location from source if present
    if let Some(src) = source_guano {
        for (k, v) in &src.fields {
            match k.as_str() {
                "Loc Position" | "Loc Elevation" | "Loc Accuracy" => {
                    g.add(k, v);
                }
                _ => {}
            }
        }
    }

    // ── Oversample-specific fields (after standard ones) ────────────────
    g.add("Oversample|App|Version", version);

    // Preserve Oversample mic/device fields from source
    if let Some(src) = source_guano {
        for (k, v) in &src.fields {
            if k.starts_with("Oversample|") && k != "Oversample|App|Version" {
                g.add(k, v);
            }
        }
    }

    // Export filter settings (these are actually applied to the audio)
    if params.filter_enabled {
        let hp_khz = params.filter_freq_low / 1000.0;
        let lp_khz = params.filter_freq_high / 1000.0;
        if hp_khz > 0.0 {
            g.add("Filter HP", &format!("{:.1}", hp_khz));
        }
        if lp_khz > 0.0 && lp_khz < output_rate as f64 / 2000.0 {
            g.add("Filter LP", &format!("{:.1}", lp_khz));
        }
    }

    g
}

/// Information about what will be exported, used for button text.
pub struct ExportInfo {
    pub count: usize,
    pub source_label: &'static str, // "selection" or "region" or "regions"
    pub mode_label: Option<String>,  // e.g. "TE 10x", "HF", etc.
    pub estimated_duration_secs: Option<f64>,
}

/// Determine what will be exported and return info for the button label.
pub fn get_export_info(state: &AppState) -> Option<ExportInfo> {
    let selected_ids = state.selected_annotation_ids.get();
    let selection = state.selection.get();

    // Count selected annotations that are regions/segments (have time bounds)
    let (region_count, region_duration_sum) = if !selected_ids.is_empty() {
        if let Some(idx) = state.current_file_index.get() {
            let store = state.annotation_store.get();
            if let Some(Some(set)) = store.sets.get(idx) {
                let mut count = 0usize;
                let mut dur = 0.0f64;
                for a in &set.annotations {
                    if selected_ids.contains(&a.id) {
                        if let AnnotationKind::Region(ref r) = a.kind {
                            count += 1;
                            dur += r.time_end - r.time_start;
                        }
                    }
                }
                (count, dur)
            } else { (0, 0.0) }
        } else { (0, 0.0) }
    } else { (0, 0.0) };

    let (count, source_label, source_duration) = if region_count > 0 {
        let label = if region_count == 1 { "region" } else { "regions" };
        (region_count, label, Some(region_duration_sum))
    } else if let Some(ref sel) = selection {
        (1, "selection", Some(sel.time_end - sel.time_start))
    } else if state.current_file_index.get().is_some() {
        let files = state.files.get();
        let file_dur = state.current_file_index.get()
            .and_then(|idx| files.get(idx))
            .map(|f| f.audio.duration_secs);
        (1, "file", file_dur)
    } else {
        return None;
    };

    // Build mode label
    let mode = state.playback_mode.get();
    let hfr = state.hfr_enabled.get();
    let mode_label = match mode {
        PlaybackMode::Normal if hfr => Some("HF".to_string()),
        PlaybackMode::TimeExpansion => {
            let te = state.te_factor.get();
            Some(format!("TE {te}x"))
        }
        PlaybackMode::PitchShift => Some("pitch shift".to_string()),
        PlaybackMode::PhaseVocoder => Some("PV".to_string()),
        PlaybackMode::Heterodyne => Some("heterodyne".to_string()),
        PlaybackMode::ZeroCrossing => Some("ZC".to_string()),
        _ => None,
    };

    let estimated_duration_secs = source_duration.map(|d| match mode {
        PlaybackMode::TimeExpansion => d * state.te_factor.get(),
        _ => d,
    });

    Some(ExportInfo { count, source_label, mode_label, estimated_duration_secs })
}

/// Format a duration in seconds to a human-readable string.
pub fn format_duration(secs: f64) -> String {
    if secs < 60.0 {
        format!("{:.1}s", secs)
    } else if secs < 3600.0 {
        let m = (secs / 60.0) as u32;
        let s = (secs % 60.0) as u32;
        format!("{}m {:02}s", m, s)
    } else {
        let h = (secs / 3600.0) as u32;
        let m = ((secs % 3600.0) / 60.0) as u32;
        let s = (secs % 60.0) as u32;
        format!("{}h {:02}m {:02}s", h, m, s)
    }
}

/// Get the list of selected Region annotations.
pub fn get_selected_regions(state: &AppState) -> Vec<(Annotation, Region)> {
    let selected_ids = state.selected_annotation_ids.get_untracked();
    if selected_ids.is_empty() {
        return Vec::new();
    }
    let idx = match state.current_file_index.get_untracked() {
        Some(i) => i,
        None => return Vec::new(),
    };
    let store = state.annotation_store.get_untracked();
    let set = match store.sets.get(idx).and_then(|s| s.as_ref()) {
        Some(s) => s,
        None => return Vec::new(),
    };
    set.annotations.iter()
        .filter(|a| selected_ids.contains(&a.id))
        .filter_map(|a| {
            if let AnnotationKind::Region(ref r) = a.kind {
                Some((a.clone(), r.clone()))
            } else {
                None
            }
        })
        .collect()
}

/// Export all selected regions (or current selection) as WAV files.
pub fn export_selected(state: &AppState) {
    let file = match state.current_file() {
        Some(f) => f,
        None => return,
    };
    let source = &file.audio.source;
    let sample_rate = file.audio.sample_rate;
    let use_region_focus = state.export_use_region_focus.get_untracked();
    let source_filename = &file.name;
    let source_guano = file.audio.metadata.guano.as_ref();

    // Strip extension from source filename for export naming
    let base_name = file.name.trim_end_matches(".wav")
        .trim_end_matches(".WAV")
        .trim_end_matches(".w4v")
        .trim_end_matches(".W4V")
        .trim_end_matches(".flac")
        .trim_end_matches(".FLAC")
        .trim_end_matches(".ogg")
        .trim_end_matches(".OGG")
        .trim_end_matches(".mp3")
        .trim_end_matches(".MP3");

    let regions = get_selected_regions(state);

    if !regions.is_empty() {
        // Export selected annotation regions
        for (i, (_annotation, region)) in regions.iter().enumerate() {
            let params = build_export_params(state, Some(region), use_region_focus, sample_rate);
            let label = region.label.as_deref()
                .or(if regions.len() > 1 { None } else { Some("") });
            let suffix = match label {
                Some(l) if !l.is_empty() => format!("_{}", l.replace(' ', "_")),
                _ => {
                    if regions.len() > 1 {
                        format!("_{}", i + 1)
                    } else {
                        String::new()
                    }
                }
            };
            let filename = format!("{base_name}{suffix}.wav");
            export_one_region(
                source.as_ref(), sample_rate,
                region.time_start, region.time_end,
                &params, &filename,
                source_filename, source_guano,
            );
        }
    } else if let Some(sel) = state.selection.get_untracked() {
        // Export current selection
        let params = build_export_params(state, None, false, sample_rate);
        let filename = format!("{base_name}_selection.wav");
        export_one_region(
            source.as_ref(), sample_rate,
            sel.time_start, sel.time_end,
            &params, &filename,
            source_filename, source_guano,
        );
    } else {
        // No selection — export the whole file
        let params = build_export_params(state, None, false, sample_rate);
        let duration = file.audio.source.duration_secs();
        let filename = format!("{base_name}_export.wav");
        export_one_region(
            source.as_ref(), sample_rate,
            0.0, duration,
            &params, &filename,
            source_filename, source_guano,
        );
    }
}
