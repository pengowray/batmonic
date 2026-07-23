use crate::audio::mic_backend::{with_live_samples, with_live_samples_mut};
use crate::audio::source::InMemorySource;
use crate::audio::wav_encoder::try_tauri_save;
use crate::dsp::fft::{compute_preview, compute_spectrogram_partial, compute_stft_columns};
use crate::dsp::resonators::{
    adaptive_fast_bw, adaptive_tonal_fft, apply_adaptive_blend, apply_clean_gate, warmup_samples,
    ResonatorAlphaMode, ResonatorHybridMode, StreamingResonators,
};
use crate::state::store_fields::*;
use crate::state::MainView;
use crate::state::{AppState, FileSettings, LoadedFile};
use crate::types::{AudioData, FileMetadata, SpectrogramData};
use leptos::prelude::*;
use std::sync::Arc;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

/// FFT and hop sizes for live waterfall rendering.
/// FFT=1024 gives 513 frequency bins for good resolution; hop=256 for smooth scrolling.
const LIVE_FFT: usize = 1024;
const LIVE_HOP: usize = 256;

/// Config key for the persistent live resonator bank: (sample_rate, fft, hop,
/// bandwidth bits, alpha-mode, Q bits, layout, viewport-range bits, density
/// bits). A change rebuilds the bank.
type ResoKey = (u32, usize, usize, u32, u8, u32, u8, Option<(u32, u32)>, u32);

/// Clean up the live recording file when finalization fails (empty samples,
/// command error, etc.).  If the file has no audio data and no preview,
/// removes it entirely and fixes `current_file_index`.  Otherwise marks it
/// as not-recording so the overview doesn't say "Recording…" forever.
pub(crate) fn cleanup_failed_recording(state: &AppState) {
    let live_idx = state.mic.live_file_idx().get_untracked();
    state.mic.live_file_idx().set(None);

    let Some(idx) = live_idx else { return };

    let is_empty = state.library.files().with_untracked(|files| {
        files
            .get(idx)
            .map_or(true, |f| f.audio.samples.is_empty() && f.preview.is_none())
    });

    if is_empty {
        // Remove the phantom live file
        state.library.files().update(|files| {
            if idx < files.len() {
                files.remove(idx);
            }
        });
        // Adjust current_file_index after removal
        let len = state.library.files().with_untracked(|f| f.len());
        match state.library.current_index().get_untracked() {
            Some(ci) if ci == idx => {
                state.library.current_index().set(if len > 0 {
                    Some(idx.min(len - 1))
                } else {
                    None
                });
            }
            Some(ci) if ci > idx => {
                state.library.current_index().set(Some(ci - 1));
            }
            _ => {}
        }
    } else {
        // File has partial data — keep it but stop the recording indicator
        state.library.files().update(|files| {
            if let Some(f) = files.get_mut(idx) {
                f.is_recording = false;
            }
        });
    }
}

/// Create a live LoadedFile at recording start for real-time visualization.
/// Returns the file index where the live file was inserted.
pub(crate) fn start_live_recording(state: &AppState, sample_rate: u32) -> usize {
    let now = js_sys::Date::new_0();
    let name = format!(
        "batcap_{:04}{:02}{:02}_{:02}{:02}{:02}.wav",
        now.get_full_year(),
        now.get_month() + 1,
        now.get_date(),
        now.get_hours(),
        now.get_minutes(),
        now.get_seconds(),
    );

    let samples: Arc<Vec<f32>> = Arc::new(Vec::new());
    let source = Arc::new(InMemorySource {
        samples: samples.clone(),
        raw_samples: None,
        sample_rate,
        channels: 1,
    });
    let audio = AudioData {
        samples,
        source,
        sample_rate,
        channels: 1,
        duration_secs: 0.0,
        metadata: FileMetadata {
            file_size: 0,
            format: "REC",
            bits_per_sample: state.mic.bits_per_sample().get_untracked(),
            is_float: false,
            guano: None,
            data_offset: None,
            data_size: None,
            zc_data: None,
        },
    };

    let placeholder_spec = SpectrogramData {
        columns: Arc::new(Vec::new()),
        total_columns: 0,
        freq_resolution: sample_rate as f64 / LIVE_FFT as f64,
        time_resolution: LIVE_HOP as f64 / sample_rate as f64,
        max_freq: sample_rate as f64 / 2.0,
        sample_rate,
    };

    let mut file_index = 0;
    state.library.files().update(|files| {
        file_index = files.len();
        files.push(LoadedFile {
            id: crate::state::next_file_id(),
            name,
            audio,
            spectrogram: placeholder_spec,
            preview: None,
            overview_image: None,
            xc_metadata: None,
            source_label: None,
            xc_hashes: None,
            is_demo: false,
            is_recording: true,
            is_live_listen: false,
            settings: FileSettings::default(),
            add_order: file_index,
            last_modified_ms: None,
            identity: None,
            file_handle: None,
            cached_peak_db: None,
            cached_full_peak_db: None,
            read_only: false,
            had_sidecar: false,
            verify_outcome: crate::state::VerifyOutcome::Pending,
            all_hashes_verified: false,
            wav_markers: Vec::new(),
            loading_id: None,
            min_display_freq: None,
            max_display_freq: None,
        });
    });

    state.library.current_index().set(Some(file_index));
    state.mic.live_file_idx().set(Some(file_index));

    // Set zoom for comfortable live recording scroll speed.
    // Use hop=256 to match the actual hop size in spawn_live_processing_loop.
    let canvas_w = state.viewmode.spectrogram_canvas_width().get_untracked();
    let live_time_res = LIVE_HOP as f64 / sample_rate as f64;
    state
        .view
        .zoom_level()
        .set(crate::viewport::recording_zoom(canvas_w, live_time_res));
    state.view.scroll_offset().set(0.0);

    file_index
}

/// Create an empty "armed" live document — mic is open and ready, but no
/// streaming has started yet. Lets the user configure HFR mode/range/bandpass
/// before they press Listen or Record. The file persists in the list until
/// closed, listened-to, or recorded-into. Returns the file index.
pub(crate) fn start_live_armed(state: &AppState, sample_rate: u32) -> usize {
    let now = js_sys::Date::new_0();
    let name = format!(
        "Live mic ({:02}:{:02}:{:02})",
        now.get_hours(),
        now.get_minutes(),
        now.get_seconds(),
    );

    let samples: Arc<Vec<f32>> = Arc::new(Vec::new());
    let source = Arc::new(InMemorySource {
        samples: samples.clone(),
        raw_samples: None,
        sample_rate,
        channels: 1,
    });
    let audio = AudioData {
        samples,
        source,
        sample_rate,
        channels: 1,
        duration_secs: 0.0,
        metadata: FileMetadata {
            file_size: 0,
            format: "MIC",
            bits_per_sample: state.mic.bits_per_sample().get_untracked(),
            is_float: false,
            guano: None,
            data_offset: None,
            data_size: None,
            zc_data: None,
        },
    };

    let placeholder_spec = SpectrogramData {
        columns: Arc::new(Vec::new()),
        total_columns: 0,
        freq_resolution: sample_rate as f64 / LIVE_FFT as f64,
        time_resolution: LIVE_HOP as f64 / sample_rate as f64,
        max_freq: sample_rate as f64 / 2.0,
        sample_rate,
    };

    let mut file_index = 0;
    state.library.files().update(|files| {
        file_index = files.len();
        files.push(LoadedFile {
            id: crate::state::next_file_id(),
            name,
            audio,
            spectrogram: placeholder_spec,
            preview: None,
            overview_image: None,
            xc_metadata: None,
            source_label: None,
            xc_hashes: None,
            is_demo: false,
            is_recording: false,
            is_live_listen: false,
            settings: FileSettings::default(),
            add_order: file_index,
            last_modified_ms: None,
            identity: None,
            file_handle: None,
            cached_peak_db: None,
            cached_full_peak_db: None,
            read_only: false,
            had_sidecar: false,
            verify_outcome: crate::state::VerifyOutcome::Pending,
            all_hashes_verified: false,
            wav_markers: Vec::new(),
            loading_id: None,
            min_display_freq: None,
            max_display_freq: None,
        });
    });

    state.library.current_index().set(Some(file_index));
    state.mic.live_file_idx().set(Some(file_index));
    // Reset display so the gutter immediately picks up the mic Nyquist.
    state.view.min_display_freq().set(None);
    state.view.max_display_freq().set(None);
    // Pre-set the live recording zoom + scroll origin so the user sees the
    // same viewport they'll get once audio actually starts streaming.
    set_live_recording_zoom(state, sample_rate);

    file_index
}

/// Apply the standard live-recording zoom (~recording_zoom) and reset
/// scroll_offset. Shared by start_live_recording, start_live_armed, and the
/// armed→record promotion path.
pub(crate) fn set_live_recording_zoom(state: &AppState, sample_rate: u32) {
    let canvas_w = state.viewmode.spectrogram_canvas_width().get_untracked();
    let live_time_res = LIVE_HOP as f64 / sample_rate as f64;
    state
        .view
        .zoom_level()
        .set(crate::viewport::recording_zoom(canvas_w, live_time_res));
    state.view.scroll_offset().set(0.0);
}

/// True when `f` is a throwaway live-mic placeholder with no recorded content
/// behind it — an armed doc OR a (possibly stale) listen entry. Such entries
/// have the synthetic "MIC" format, no real samples, and no backing file, so
/// they're safe to reuse or prune. Distinguished from a saved recording (which
/// has samples / a "REC" format) and from a loaded file (real format + samples
/// or a file handle).
pub(crate) fn is_empty_live_placeholder(f: &crate::state::LoadedFile) -> bool {
    f.audio.samples.is_empty() && f.file_handle.is_none() && f.audio.metadata.format == "MIC"
}

/// True when the file at `idx` is a reusable live-mic placeholder — an armed
/// (idle, empty) doc OR a stale listen entry (its `is_live_listen`/
/// `is_recording` flags still set but carrying no recorded audio). Used to
/// decide whether Listen/Record/+New can reuse the existing live entry rather
/// than spawning a second one.
pub(crate) fn is_reusable_live_doc(state: &AppState, idx: usize) -> bool {
    state
        .library
        .files()
        .with_untracked(|files| files.get(idx).map_or(false, is_empty_live_placeholder))
}

/// Remove stale, empty live-mic placeholders from the file list, keeping at
/// most the one at `keep_idx` (the active live entry / reuse candidate). This
/// stops the list from accumulating more than one empty "Live mic" / stopped
/// "Listening" entry. Real recordings and loaded files are never touched
/// (they have samples or a backing file). `current_file_index` and
/// `mic_live_file_idx` are fixed up for the removals.
pub(crate) fn prune_empty_live_placeholders(state: &AppState, keep_idx: Option<usize>) {
    let victims: Vec<usize> = state.library.files().with_untracked(|files| {
        files
            .iter()
            .enumerate()
            .filter(|&(i, f)| Some(i) != keep_idx && is_empty_live_placeholder(f))
            .map(|(i, _)| i)
            .collect()
    });
    if victims.is_empty() {
        return;
    }

    // Remove from the back so earlier indices stay valid mid-loop.
    state.library.files().update(|files| {
        for &i in victims.iter().rev() {
            if i < files.len() {
                files.remove(i);
            }
        }
    });

    // How many removed entries sat strictly below `idx` (so it shifts down).
    let below = |idx: usize| victims.iter().filter(|&&v| v < idx).count();
    let new_len = state.library.files().with_untracked(|f| f.len());

    state.library.current_index().update(|ci| {
        if let Some(c) = *ci {
            *ci = if victims.contains(&c) {
                // The viewed file itself was pruned — clamp into range.
                if new_len == 0 {
                    None
                } else {
                    Some(c.saturating_sub(below(c)).min(new_len - 1))
                }
            } else {
                Some(c - below(c))
            };
        }
    });
    state.mic.live_file_idx().update(|mi| {
        if let Some(m) = *mi {
            if victims.contains(&m) {
                *mi = None;
            } else {
                *mi = Some(m - below(m));
            }
        }
    });
}

/// Promote an armed live doc to a listening file. Sets the flags the live
/// processing loop and waveform overview key off without altering the file's
/// position or name. Use only on a file that satisfies `is_reusable_live_doc`.
pub(crate) fn promote_armed_to_listening(state: &AppState, idx: usize) {
    state.library.files().update(|files| {
        if let Some(f) = files.get_mut(idx) {
            f.is_live_listen = true;
            // Reuse the recording display path for the live waveform/overview,
            // matching what start_live_listening sets.
            f.is_recording = true;
        }
    });
    state.library.current_index().set(Some(idx));
    let sr = state.library.files().with_untracked(|files| {
        files
            .get(idx)
            .map(|f| f.audio.sample_rate)
            .unwrap_or(48_000)
    });
    set_live_recording_zoom(state, sr);
}

/// Promote an armed live doc to a recording file. Renames it to the standard
/// recording filename and resets metadata so the recovery sidecar/wav-part
/// uses the right filename. Use only on a file that satisfies `is_reusable_live_doc`.
///
/// MUST be called BEFORE `mic_backend::start_recording` so the recovery
/// sidecar/.wav.part filename (built by `build_start_recording_args` from the
/// file at `mic_live_file_idx`) picks up the new name.
pub(crate) fn promote_armed_to_recording(state: &AppState, idx: usize) {
    let now = js_sys::Date::new_0();
    let new_name = format!(
        "batcap_{:04}{:02}{:02}_{:02}{:02}{:02}.wav",
        now.get_full_year(),
        now.get_month() + 1,
        now.get_date(),
        now.get_hours(),
        now.get_minutes(),
        now.get_seconds(),
    );
    state.library.files().update(|files| {
        if let Some(f) = files.get_mut(idx) {
            f.name = new_name;
            f.is_recording = true;
            f.is_live_listen = false;
            f.audio.metadata.format = "REC";
        }
    });
    state.library.current_index().set(Some(idx));
}

/// Roll back a recording-promoted file to its armed state. Used when
/// `start_recording` fails after we've already renamed the armed file.
pub(crate) fn revert_recording_to_armed(state: &AppState, idx: usize) {
    let now = js_sys::Date::new_0();
    let armed_name = format!(
        "Live mic ({:02}:{:02}:{:02})",
        now.get_hours(),
        now.get_minutes(),
        now.get_seconds(),
    );
    state.library.files().update(|files| {
        if let Some(f) = files.get_mut(idx) {
            f.name = armed_name;
            f.is_recording = false;
            f.is_live_listen = false;
            f.audio.metadata.format = "MIC";
        }
    });
}

/// Create a transient listening file — appears in the file list, shows the
/// waterfall / waveform, and is auto-removed when listening stops.
/// Returns the file index.
pub(crate) fn start_live_listening(state: &AppState, sample_rate: u32) -> usize {
    let samples: Arc<Vec<f32>> = Arc::new(Vec::new());
    let source = Arc::new(InMemorySource {
        samples: samples.clone(),
        raw_samples: None,
        sample_rate,
        channels: 1,
    });
    let audio = AudioData {
        samples,
        source,
        sample_rate,
        channels: 1,
        duration_secs: 0.0,
        metadata: FileMetadata {
            file_size: 0,
            format: "MIC",
            bits_per_sample: 16,
            is_float: false,
            guano: None,
            data_offset: None,
            data_size: None,
            zc_data: None,
        },
    };

    let placeholder_spec = SpectrogramData {
        columns: Arc::new(Vec::new()),
        total_columns: 0,
        freq_resolution: sample_rate as f64 / LIVE_FFT as f64,
        time_resolution: LIVE_HOP as f64 / sample_rate as f64,
        max_freq: sample_rate as f64 / 2.0,
        sample_rate,
    };

    let mut file_index = 0;
    state.library.files().update(|files| {
        file_index = files.len();
        files.push(LoadedFile {
            id: crate::state::next_file_id(),
            name: "Listening".to_string(),
            audio,
            spectrogram: placeholder_spec,
            preview: None,
            overview_image: None,
            xc_metadata: None,
            source_label: None,
            xc_hashes: None,
            is_demo: false,
            is_recording: true, // reuse recording display path for waveform/overview
            is_live_listen: true,
            settings: FileSettings::default(),
            add_order: file_index,
            last_modified_ms: None,
            identity: None,
            file_handle: None,
            cached_peak_db: None,
            cached_full_peak_db: None,
            read_only: false,
            had_sidecar: false,
            verify_outcome: crate::state::VerifyOutcome::Pending,
            all_hashes_verified: false,
            wav_markers: Vec::new(),
            loading_id: None,
            min_display_freq: None,
            max_display_freq: None,
        });
    });

    state.library.current_index().set(Some(file_index));
    state.mic.live_file_idx().set(Some(file_index));

    // Set zoom for comfortable waterfall viewing.
    // Use LIVE_HOP to match the actual hop size in spawn_live_processing_loop.
    let canvas_w = state.viewmode.spectrogram_canvas_width().get_untracked();
    let live_time_res = LIVE_HOP as f64 / sample_rate as f64;
    state
        .view
        .zoom_level()
        .set(crate::viewport::recording_zoom(canvas_w, live_time_res));
    state.view.scroll_offset().set(0.0);

    file_index
}

/// Convert the transient listening file into an empty "armed" live doc.
/// Keeps the entry in the file list (so the user isn't dropped back into "no
/// file") and resets it to the same shape `start_live_armed` produces, so a
/// subsequent Listen/Record can reuse it via the armed-doc promotion path.
/// No-op if there's no live file or the file isn't a listen entry.
pub(crate) fn convert_listen_to_armed(state: &AppState) {
    let Some(idx) = state.mic.live_file_idx().get_untracked() else {
        return;
    };
    let is_listen = state
        .library
        .files()
        .with_untracked(|files| files.get(idx).map_or(false, |f| f.is_live_listen));
    if !is_listen {
        return;
    }

    let now = js_sys::Date::new_0();
    let armed_name = format!(
        "Live mic ({:02}:{:02}:{:02})",
        now.get_hours(),
        now.get_minutes(),
        now.get_seconds(),
    );

    state.library.files().update(|files| {
        if let Some(f) = files.get_mut(idx) {
            let sr = f.audio.sample_rate;
            let empty: Arc<Vec<f32>> = Arc::new(Vec::new());
            let source = Arc::new(InMemorySource {
                samples: empty.clone(),
                raw_samples: None,
                sample_rate: sr,
                channels: 1,
            });
            f.name = armed_name;
            f.is_live_listen = false;
            f.is_recording = false;
            f.audio.samples = empty;
            f.audio.source = source;
            f.audio.duration_secs = 0.0;
            f.audio.metadata.format = "MIC";
            f.spectrogram = SpectrogramData {
                columns: Arc::new(Vec::new()),
                total_columns: 0,
                freq_resolution: sr as f64 / LIVE_FFT as f64,
                time_resolution: LIVE_HOP as f64 / sr as f64,
                max_freq: sr as f64 / 2.0,
                sample_rate: sr,
            };
            f.preview = None;
            f.overview_image = None;
            f.wav_markers.clear();
        }
    });

    state.library.current_index().set(Some(idx));
    state.view.min_display_freq().set(None);
    state.view.max_display_freq().set(None);
}

/// Remove the transient listening file and fix indices.
pub(crate) fn cleanup_listen_file(state: &AppState) {
    let live_idx = state.mic.live_file_idx().get_untracked();
    state.mic.live_file_idx().set(None);

    let Some(idx) = live_idx else { return };

    let is_listen = state
        .library
        .files()
        .with_untracked(|files| files.get(idx).map_or(false, |f| f.is_live_listen));
    if !is_listen {
        return;
    }

    state.library.files().update(|files| {
        if idx < files.len() {
            files.remove(idx);
        }
    });

    // A file may have been loaded while listening, putting the transient listen
    // doc mid-list; reconcile the positional caches + multi-select + timeline
    // the same way the close-button path does.
    crate::components::file_sidebar::files_panel::reconcile_after_file_removed(state, idx);

    // Fix current_file_index after removal
    let len = state.library.files().with_untracked(|f| f.len());
    match state.library.current_index().get_untracked() {
        Some(ci) if ci == idx => {
            state.library.current_index().set(if len > 0 {
                Some(idx.min(len - 1))
            } else {
                None
            });
        }
        Some(ci) if ci > idx => {
            state.library.current_index().set(Some(ci - 1));
        }
        _ => {}
    }
}

/// Rename the listening file to its final `batcap_*.wav` name, keeping the
/// rest of the listening state intact (still `is_live_listen=true`, etc.).
///
/// MUST be called BEFORE `mic_backend::start_recording` so the recovery
/// sidecar/.wav.part filename and any shared-storage MediaStore entry (built
/// by `build_start_recording_args` / `try_create_shared_fd` from the file at
/// `mic_live_file_idx`) pick up the new name. If we wait until after the
/// IPC, the MediaStore entry ends up with `DISPLAY_NAME="Listening"` (no
/// `.wav`) which Android either rejects outright or hides from file managers
/// — surfacing as "record succeeded but no file appeared".
///
/// Mirrors `promote_armed_to_recording` for the armed-doc path.
pub(crate) fn rename_listen_to_recording(state: &AppState, sample_rate: u32) {
    let Some(file_index) = state.mic.live_file_idx().get_untracked() else {
        return;
    };

    // When pre-roll is active, backdate the filename to reflect the actual
    // start of audio data (i.e. the beginning of the pre-roll buffer).
    let preroll = state.mic.preroll_samples().get_untracked();
    let preroll_ms = if preroll > 0 && sample_rate > 0 {
        (preroll as f64 / sample_rate as f64) * 1000.0
    } else {
        0.0
    };
    let ts = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(
        js_sys::Date::now() - preroll_ms,
    ));
    let name = format!(
        "batcap_{:04}{:02}{:02}_{:02}{:02}{:02}.wav",
        ts.get_full_year(),
        ts.get_month() + 1,
        ts.get_date(),
        ts.get_hours(),
        ts.get_minutes(),
        ts.get_seconds(),
    );

    state.library.files().update(|files| {
        if let Some(f) = files.get_mut(file_index) {
            f.name = name;
        }
    });
}

/// Convert the listening file into a recording file (listen → record transition).
/// Returns the existing file index.  Does NOT clear the audio buffer so the
/// last ≤10 s of listened audio becomes pre-roll in the recording.
///
/// Assumes `rename_listen_to_recording` has already run — this only flips the
/// listening-state flags and updates metadata.
pub(crate) fn convert_listen_to_recording(state: &AppState, _sample_rate: u32) -> usize {
    let file_index = state
        .mic
        .live_file_idx()
        .get_untracked()
        .expect("convert_listen_to_recording: no live file");

    state.library.files().update(|files| {
        if let Some(f) = files.get_mut(file_index) {
            f.is_live_listen = false;
            f.audio.metadata.format = "REC";
            f.audio.metadata.bits_per_sample = state.mic.bits_per_sample().get_untracked();
        }
    });

    state.library.current_index().set(Some(file_index));

    // Don't reset scroll_offset — the smooth scroll animation is already
    // running from the listen phase and will keep the view at the right edge.
    // Resetting to 0 causes a visible jump to the beginning.

    file_index
}

/// Spawns an async processing loop that incrementally computes STFT columns
/// and pushes them to the live waterfall for direct canvas rendering.
/// No tile cache or spectral store is used — the waterfall renders directly.
pub(crate) fn spawn_live_processing_loop(state: AppState, file_index: usize, sample_rate: u32) {
    use crate::canvas::live_waterfall;

    let (fft_size, hop_size) = (LIVE_FFT, LIVE_HOP);
    const PROCESS_INTERVAL_MS: i32 = 50;
    // fps governor (auto-quality) thresholds, on the smoothed per-tick resonator
    // compute time. Step density down above DOWN, back up below UP (the gap is
    // the hysteresis band); COOLDOWN ticks between changes (12 × 50 ms ≈ 600 ms).
    const GOV_DOWN_MS: f64 = 22.0;
    const GOV_UP_MS: f64 = 8.0;
    const GOV_COOLDOWN_TICKS: i32 = 12;

    // Bump the generation counter so any previous processing loop will exit.
    let gen = state.mic.processing_gen().get_untracked().wrapping_add(1);
    state.mic.processing_gen().set(gen);

    // Initialize waterfall synchronously so the renderer sees it immediately
    // (before any async yield that could allow a spectrogram draw)
    live_waterfall::create(fft_size, hop_size, sample_rate);

    // Pin to the live file's STABLE id, not its current index. Closing another
    // file shifts this file's index, so the loop re-resolves the index each
    // tick from the id (see the guard below).
    let live_id = state
        .library
        .files()
        .with_untracked(|f| f.get(file_index).map(|x| x.id));

    wasm_bindgen_futures::spawn_local(async move {
        let mut last_processed_col: usize = 0;
        let mut last_snapshot_len: usize = 0;
        let is_tauri = state.is_tauri;
        // Persistent streaming resonator bank for the Resonators view, kept
        // across ticks so each sample is processed once. Rebuilt when the key
        // (rate/fft/hop/bandwidth/layout/viewport) changes. Lives for this
        // session only — a new spawn starts fresh (None).
        let mut reso: Option<(ResoKey, StreamingResonators)> = None;
        // Absolute column index of buffer column 0 (sum of all trimmed columns).
        // `last_processed_col` is buffer-relative; adding this gives the absolute
        // column so emitted `time_offset`s stay monotonic across buffer trims.
        let mut col_base: usize = 0;
        // Fresh session: reset the published ring base (see set_live_ring_base).
        crate::audio::mic_backend::set_live_ring_base(0);
        // fps-governor state (live Resonators auto-quality), kept across ticks:
        // `comp_ema` = smoothed per-tick resonator compute ms; `gov_density` =
        // the governor's current density; `gov_cooldown` = ticks until the next
        // allowed change. `perf` gives wall time for the measurement.
        let perf = web_sys::window().and_then(|w| w.performance());
        let mut comp_ema = 0.0f64;
        let mut gov_density = 1.0f32;
        let mut gov_cooldown = 0i32;
        // Second persistent bank for the live Adaptive hybrid (fast bank).
        // `fast_fed_last` tracks whether it was fed on the previous tick so a
        // gap (mode toggled off then on) forces a rebuild+warm.
        let mut reso_fast: Option<(ResoKey, StreamingResonators)> = None;
        let mut fast_fed_last = false;

        loop {
            let p = js_sys::Promise::new(&mut |resolve, _| {
                if let Some(w) = web_sys::window() {
                    let _ = w.set_timeout_with_callback_and_timeout_and_arguments_0(
                        &resolve,
                        PROCESS_INTERVAL_MS,
                    );
                }
            });
            let _ = JsFuture::from(p).await;

            // A newer processing loop has started — this one is stale.
            if state.mic.processing_gen().get_untracked() != gen {
                log::info!(
                    "Processing loop superseded (gen {} vs {}), exiting",
                    gen,
                    state.mic.processing_gen().get_untracked()
                );
                break;
            }

            // Check if still recording/listening
            let is_recording = state.mic.recording().get_untracked();
            let is_listening = state.mic.listening().get_untracked();
            if !is_recording && !is_listening {
                break;
            }
            // Re-resolve our file by its stable id every tick. Closing an
            // earlier file shifts the live doc's index down (remove_file_at
            // decrements mic_live_file_idx in lock-step), so a captured index
            // would start writing recording data into the wrong file. Returns
            // None once our file itself is closed -> stop the loop.
            let Some(file_index) = live_id.and_then(|id| state.file_idx_for_id(id)) else {
                break;
            };
            // It must still be the live-capture slot.
            if state.mic.live_file_idx().get_untracked() != Some(file_index) {
                break;
            }

            // Compute new FFT columns from the live buffer
            let (any_update, peak_normalized) =
                with_live_samples(is_tauri, |samples| -> (bool, f32) {
                    if samples.len() < fft_size {
                        return (false, 0.0);
                    }

                    let total_possible_cols = (samples.len() - fft_size) / hop_size + 1;
                    if total_possible_cols <= last_processed_col {
                        return (false, 0.0);
                    }

                    let new_col_count = total_possible_cols - last_processed_col;

                    // Compute new spectral columns using the currently selected view.
                    // Resonators are stateful: a PERSISTENT streaming bank processes
                    // each incoming sample exactly once (no per-tick re-create or
                    // re-warm — the dominant cost at high sample rates). It's rebuilt
                    // only when the config (bandwidth / layout / viewport / rate)
                    // changes, warming once from recent buffer history so the first
                    // columns are converged. The hop-aligned buffer trim below keeps
                    // the column↔sample mapping exact across trims.
                    let new_cols = if state.viewmode.main_view().get_untracked()
                        == MainView::Resonators
                    {
                        let bandwidth_hz = state.resonator.bandwidth_hz().get_untracked().max(1.0);
                        let alpha_mode = state.resonator.alpha_mode().get_untracked();
                        let hybrid_mode = state.resonator.hybrid_mode().get_untracked();
                        // Adaptive defines its own tradeoff via the sharp+fast banks,
                        // so the sharp bank uses ConstBandwidth regardless of alpha_mode.
                        let sharp_alpha = if hybrid_mode == ResonatorHybridMode::Adaptive {
                            ResonatorAlphaMode::ConstBandwidth
                        } else {
                            alpha_mode
                        };
                        let q = state.resonator.q().get_untracked();
                        let layout = state.resonator.layout().get_untracked();
                        let freq_range = state
                            .resonator
                            .viewport_range()
                            .get_untracked()
                            .map(|(lo, hi)| (lo as f32, hi as f32));
                        // Density lever (100/50/25%): fewer resonators, still 513
                        // rows — what makes Resonators usable at high sample rates.
                        // The fps governor (auto-quality) may dial density BELOW the
                        // user's chosen density (treated as a ceiling) under load.
                        let ceiling = state.resonator.fft_mode().get_untracked().bank_density();
                        let auto = state.resonator.auto_quality().get_untracked();
                        let density = if auto {
                            gov_density.min(ceiling)
                        } else {
                            ceiling
                        };
                        let key: ResoKey = (
                            sample_rate,
                            fft_size,
                            hop_size,
                            bandwidth_hz.to_bits(),
                            sharp_alpha as u8,
                            q.to_bits(),
                            layout as u8,
                            freq_range.map(|(a, b)| (a.to_bits(), b.to_bits())),
                            density.to_bits(),
                        );
                        let need_rebuild = reso.as_ref().map_or(true, |(k, _)| *k != key);
                        if need_rebuild {
                            let mut s = StreamingResonators::new(
                                sample_rate,
                                fft_size,
                                hop_size,
                                bandwidth_hz,
                                sharp_alpha,
                                q,
                                layout,
                                freq_range,
                                density,
                            );
                            // Warm from the most-recent ~5τ of already-consumed audio
                            // so the rebuilt bank's first emitted columns are settled.
                            let warm_end = (last_processed_col * hop_size).min(samples.len());
                            let warm_start =
                                warm_end.saturating_sub(warmup_samples(sample_rate, bandwidth_hz));
                            s.warm(&samples[warm_start..warm_end]);
                            reso = Some((key, s));
                        }
                        let s = &mut reso.as_mut().unwrap().1;
                        let feed_start = last_processed_col * hop_size;
                        let feed_end = total_possible_cols * hop_size;
                        let mut cols = if feed_end > feed_start && feed_end <= samples.len() {
                            // first_col is ABSOLUTE (buffer-relative + col_base) so the
                            // emitted time_offsets are monotonic across trims. Time the
                            // compute to feed the fps governor.
                            let t0 = perf.as_ref().map(|p| p.now());
                            let c = s.push_hops(
                                &samples[feed_start..feed_end],
                                last_processed_col + col_base,
                            );
                            if let (Some(p), Some(t0)) = (perf.as_ref(), t0) {
                                let dt = p.now() - t0;
                                comp_ema = if comp_ema <= 0.0 {
                                    dt
                                } else {
                                    comp_ema * 0.8 + dt * 0.2
                                };
                            }
                            c
                        } else {
                            Vec::new()
                        };
                        // fps governor (auto mode): hysteresis + cooldown. Step density
                        // down when the smoothed per-tick compute is heavy, back up
                        // toward the ceiling when there's headroom. Density is part of
                        // ResoKey, so a change rebuilds the bank next tick (cheap warm,
                        // no flash).
                        if auto {
                            if gov_cooldown > 0 {
                                gov_cooldown -= 1;
                            } else if comp_ema > GOV_DOWN_MS && gov_density > 0.25 {
                                gov_density = if gov_density > 0.5 { 0.5 } else { 0.25 };
                                gov_cooldown = GOV_COOLDOWN_TICKS;
                            } else if comp_ema < GOV_UP_MS && gov_density < ceiling {
                                gov_density = if gov_density < 0.5 { 0.5 } else { 1.0 };
                                gov_cooldown = GOV_COOLDOWN_TICKS;
                            }
                        }
                        // FFT-steered hybrid (live): same per-pixel math as the tile
                        // path, applied to the persistent bank's new columns.
                        let n_new = total_possible_cols.saturating_sub(last_processed_col);
                        let valid = n_new > 0 && feed_end > feed_start && feed_end <= samples.len();
                        match hybrid_mode {
                            ResonatorHybridMode::Off => fast_fed_last = false,
                            ResonatorHybridMode::Clean => {
                                fast_fed_last = false;
                                if valid {
                                    let fft = compute_stft_columns(
                                        samples,
                                        sample_rate,
                                        fft_size,
                                        hop_size,
                                        last_processed_col,
                                        n_new,
                                    );
                                    apply_clean_gate(&mut cols, &fft, fft_size, hop_size);
                                }
                            }
                            ResonatorHybridMode::Adaptive if valid => {
                                let fast_bw = adaptive_fast_bw(bandwidth_hz);
                                let fast_key: ResoKey = (
                                    sample_rate,
                                    fft_size,
                                    hop_size,
                                    fast_bw.to_bits(),
                                    ResonatorAlphaMode::ConstBandwidth as u8,
                                    q.to_bits(),
                                    layout as u8,
                                    freq_range.map(|(a, b)| (a.to_bits(), b.to_bits())),
                                    density.to_bits(),
                                );
                                // Rebuild on key change OR after a gap (mode toggled).
                                if !fast_fed_last
                                    || reso_fast.as_ref().map_or(true, |(k, _)| *k != fast_key)
                                {
                                    let mut s = StreamingResonators::new(
                                        sample_rate,
                                        fft_size,
                                        hop_size,
                                        fast_bw,
                                        ResonatorAlphaMode::ConstBandwidth,
                                        q,
                                        layout,
                                        freq_range,
                                        density,
                                    );
                                    let warm_end =
                                        (last_processed_col * hop_size).min(samples.len());
                                    let warm_start = warm_end.saturating_sub(warmup_samples(
                                        sample_rate,
                                        fast_bw.max(1.0),
                                    ));
                                    s.warm(&samples[warm_start..warm_end]);
                                    reso_fast = Some((fast_key, s));
                                }
                                let fast_cols = reso_fast.as_mut().unwrap().1.push_hops(
                                    &samples[feed_start..feed_end],
                                    last_processed_col + col_base,
                                );
                                let tonal_fft = adaptive_tonal_fft(fft_size);
                                let tonal = compute_stft_columns(
                                    samples,
                                    sample_rate,
                                    tonal_fft,
                                    hop_size,
                                    last_processed_col,
                                    n_new,
                                );
                                apply_adaptive_blend(
                                    &mut cols,
                                    &fast_cols,
                                    &tonal,
                                    fft_size,
                                    tonal_fft,
                                    hop_size,
                                    sample_rate,
                                );
                                fast_fed_last = true;
                            }
                            ResonatorHybridMode::Adaptive => fast_fed_last = false,
                        }
                        cols
                    } else {
                        compute_stft_columns(
                            samples,
                            sample_rate,
                            fft_size,
                            hop_size,
                            last_processed_col,
                            new_col_count,
                        )
                    };

                    if new_cols.is_empty() {
                        return (false, 0.0);
                    }

                    // Push to waterfall for direct rendering
                    live_waterfall::push_columns(&new_cols);

                    // Update file metadata (recording OR listening with a live file)
                    let has_live_file =
                        state.mic.live_file_idx().get_untracked() == Some(file_index);
                    if has_live_file {
                        let duration = samples.len() as f64 / sample_rate as f64;
                        state.library.files().update(|files| {
                            if let Some(f) = files.get_mut(file_index) {
                                f.spectrogram.total_columns = total_possible_cols;
                                f.audio.duration_secs = duration;
                            }
                        });
                    }

                    // Periodically snapshot for waveform rendering.
                    //
                    // A fixed ~1s interval is O(N²) in total copy cost and blew the
                    // Android heap for multi-minute high-SR recordings. Instead we
                    // scale the interval with the current buffer length so each
                    // snapshot does at most ~25% new work — amortized O(N) total,
                    // while still giving frequent updates at the start.
                    if has_live_file {
                        let base = (sample_rate as usize).max(44100);
                        let adaptive = (samples.len() / 4).max(base);
                        let do_snapshot = last_snapshot_len == 0
                            || samples.len().saturating_sub(last_snapshot_len) >= adaptive;
                        if do_snapshot {
                            let snapshot = Arc::new(samples.to_vec());
                            // Keep `source` in lock-step with `samples`: during
                            // capture the source was left as the empty placeholder
                            // from arm time while `samples` grew, so they diverged.
                            // Nothing reads the live source today, but the mismatch
                            // is a latent wrong-read hazard — close it cheaply by
                            // reusing the snapshot Arc we just allocated.
                            let new_source = Arc::new(InMemorySource {
                                samples: snapshot.clone(),
                                raw_samples: None,
                                sample_rate,
                                channels: 1,
                            });
                            state.library.files().update(|files| {
                                if let Some(f) = files.get_mut(file_index) {
                                    f.audio.samples = snapshot;
                                    f.audio.source = new_source;
                                }
                            });
                            last_snapshot_len = samples.len();
                        }
                    }

                    // Compute peak of recent samples for VU meter
                    let vu_start = samples.len().saturating_sub(2048);
                    let peak = samples[vu_start..]
                        .iter()
                        .fold(0.0f32, |max, &s| max.max(s.abs()));
                    let peak_db = if peak > 0.0 {
                        20.0 * peak.log10()
                    } else {
                        -96.0
                    };
                    let normalized = ((peak_db + 60.0) / 60.0).clamp(0.0, 1.0);

                    last_processed_col = total_possible_cols;
                    (true, normalized)
                });

            // Trim the WASM-side circular buffer.
            //
            // We do this during listen AND during recording (whenever the
            // Tauri side is streaming to disk, which is the default). During
            // recording the WASM buffer is purely for the live waterfall + a
            // recent-samples waveform strip — the full recording lives on
            // disk via the .wav.part file. Keeping the full recording in
            // WASM would blow the heap for long / high-SR captures.
            //
            // The two exceptions that DO need the full buffer in WASM RAM:
            //   - Pre-roll recording: WASM re-encodes the WAV with the
            //     pre-roll samples + cue marker, so everything must survive
            //     until stop.
            //   - To-memory mode: user explicitly opted out of disk writes.
            let to_memory =
                state.playback.record_mode().get_untracked() == crate::state::RecordMode::ToMemory;
            // Pre-roll now streams to disk natively (seeded from the native
            // listening ring), so on Tauri the WASM buffer is just the live
            // display for pre-roll captures and can be trimmed like a normal
            // streaming recording. Web has no native side, so it stays
            // authoritative there (the WASM buffer IS the recording).
            let wasm_is_authoritative = to_memory || !is_tauri;
            let should_trim =
                any_update && (is_listening || (is_recording && !wasm_is_authoritative));
            if should_trim {
                with_live_samples_mut(is_tauri, |samples| {
                    // Keep an extra 2 s of headroom beyond the user-requested
                    // pre-roll duration. `toggle_record_with_preroll` subtracts
                    // the long-press gesture time (typically 400–700 ms, but
                    // up to a few seconds if the user holds) from the buffer
                    // length to work out where the "press moment" was — if the
                    // buffer were exactly the requested length, the subtracted
                    // gesture-time would eat into the actual pre-roll the
                    // user asked for. With headroom the cap below ends up at
                    // exactly the user's setting.
                    const GESTURE_HEADROOM_SECS: u32 = 2;
                    let buf_secs = state
                        .mic
                        .preroll_buffer_secs()
                        .get_untracked()
                        .max(2)
                        .saturating_add(GESTURE_HEADROOM_SECS)
                        as usize;
                    let max_samples = (sample_rate as usize) * buf_secs;
                    if samples.len() > max_samples {
                        // Hop-align the trim so the column index ↔ sample index
                        // mapping stays exact across trims — required by the
                        // persistent streaming resonator bank, which feeds new
                        // samples addressed by column. (A non-aligned trim would
                        // skip up to hop-1 unconsumed samples each tick.)
                        let trim = ((samples.len() - max_samples) / hop_size) * hop_size;
                        if trim > 0 {
                            samples.drain(..trim);
                            let trimmed_cols = trim / hop_size;
                            last_processed_col = last_processed_col.saturating_sub(trimmed_cols);
                            col_base += trimmed_cols; // keep absolute column index intact
                            last_snapshot_len = last_snapshot_len.saturating_sub(trim);
                        }
                    }
                });
            }
            // Publish the ring's absolute base (= total trimmed samples) so the
            // live waveform maps ring samples to absolute positions WITHOUT the
            // lagging waterfall clock. col_base advances only on a hop-aligned
            // trim, so this is exact across trims.
            crate::audio::mic_backend::set_live_ring_base(col_base as u64 * hop_size as u64);

            if any_update {
                state.mic.peak_level().set(peak_normalized);
                let total_cols = live_waterfall::total_columns();
                state.mic.live_data_cols().set(total_cols);

                // Trigger spectrogram redraw
                state
                    .viewmode
                    .tile_ready_signal()
                    .update(|n| *n = n.wrapping_add(1));

                // Set target scroll for waterfall effect
                if total_cols > 0 {
                    let time_res = hop_size as f64 / sample_rate as f64;
                    let recording_time = total_cols as f64 * time_res;
                    let canvas_w = state.viewmode.spectrogram_canvas_width().get_untracked();
                    let zoom = state.view.zoom_level().get_untracked();
                    if zoom > 0.0 && canvas_w > 0.0 {
                        let visible_cols = canvas_w / zoom;
                        let visible_time = visible_cols * time_res;
                        let target_scroll = (recording_time - visible_time).max(0.0);
                        state.mic.recording_target_scroll().set(target_scroll);
                    }
                }
            } else if is_recording && last_processed_col == 0 {
                // No audio chunks have arrived yet — update file duration from
                // wall-clock time so the overview can show elapsed recording time
                // instead of static "Recording…" text.
                if let Some(start) = state.mic.recording_start_time().get_untracked() {
                    let elapsed = (js_sys::Date::now() - start) / 1000.0;
                    state.library.files().update(|files| {
                        if let Some(f) = files.get_mut(file_index) {
                            f.audio.duration_secs = elapsed;
                        }
                    });
                }
            }
        }

        // Processing loop exited — clean up. But if a NEWER loop has already
        // taken over (gen bumped — e.g. a rapid stop→start / listen→record, or
        // a synth-test restart), it now owns the waterfall + live signals it
        // just created. This (stale) loop must NOT clear them, or the new loop
        // would push columns into a destroyed waterfall and render nothing.
        let superseded = state.mic.processing_gen().get_untracked() != gen;
        state.mic.peak_level().set(0.0);
        if !superseded {
            if !state.mic.recording().get_untracked() {
                // Only clear waterfall when fully done (not when switching from listen to record)
                live_waterfall::clear();
            }
            // Note: do NOT clear mic_live_file_idx here — finalize_recording() is
            // responsible for that. Clearing it here causes a race: this loop exits
            // as soon as mic_recording is false, but the async stop command hasn't
            // returned yet, so finalize_recording sees mic_live_file_idx=None and
            // creates a duplicate file.
            state.mic.live_data_cols().set(0);
            state.mic.recording_target_scroll().set(0.0);
            state.mic.scroll_user_pan_until().set(0.0);
        }
    });
}

/// Spawns a requestAnimationFrame loop that smoothly interpolates
/// `scroll_offset` toward `mic_recording_target_scroll` for waterfall scrolling.
/// Automatically stops when recording and listening both end.
pub(crate) fn spawn_smooth_scroll_animation(state: AppState) {
    use std::cell::RefCell;
    use std::rc::Rc;

    let cb: Rc<RefCell<Option<Closure<dyn FnMut()>>>> = Rc::new(RefCell::new(None));
    let cb_clone = cb.clone();

    *cb.borrow_mut() = Some(Closure::new(move || {
        if !state.mic.recording().get_untracked() && !state.mic.listening().get_untracked() {
            // Neither recording nor listening — exit the animation loop
            return;
        }
        // If the user is panning (or just released a pan within the grace
        // window), leave scroll_offset alone so they can look at earlier
        // material without fighting the auto-scroll.
        let pan_until = state.mic.scroll_user_pan_until().get_untracked();
        let suspended = pan_until > 0.0 && js_sys::Date::now() < pan_until;
        if !suspended {
            let target = state.mic.recording_target_scroll().get_untracked();
            let current = state.view.scroll_offset().get_untracked();
            let diff = target - current;
            if diff.abs() > 0.0001 {
                // Exponential ease: move 30% of remaining distance each frame (~60fps)
                let new_scroll = current + diff * 0.3;
                state.view.scroll_offset().set(new_scroll);
            }
        }
        // Re-register for next frame
        if let Some(w) = web_sys::window() {
            if let Some(ref c) = *cb_clone.borrow() {
                let _ = w.request_animation_frame(c.as_ref().unchecked_ref());
            }
        }
    }));

    // Start the animation loop
    if let Some(w) = web_sys::window() {
        if let Some(ref c) = *cb.borrow() {
            let _ = w.request_animation_frame(c.as_ref().unchecked_ref());
        }
    }

    // Prevent the closure from being dropped by leaking it.
    // It will self-terminate when recording stops (the callback checks mic_recording).
    std::mem::forget(cb);
}

/// Parameters for the unified recording finalization.
pub(crate) struct FinalizeParams {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    pub bits_per_sample: u16,
    pub is_float: bool,
    /// Path where native backend already saved the file (empty = needs WASM-side save).
    pub saved_path: String,
    /// Actual file size from native backend (overrides WASM-side estimate).
    pub file_size: Option<usize>,
}

/// Collected recording metadata (GUANO, markers, naming) built from AppState.
struct RecordingMeta {
    guano: crate::audio::guano::GuanoMetadata,
    wav_markers: Vec<crate::types::WavMarker>,
    preroll_samples: usize,
}

/// Read mic info, location, device model, and preroll from state to build
/// GUANO metadata and WAV cue markers. Pure state-read, no mutations.
fn build_recording_meta(
    state: &AppState,
    sample_rate: u32,
    duration_secs: f64,
    filename: &str,
) -> RecordingMeta {
    let mic_name = state.mic.device_name().get_untracked();
    let mic_manufacturer = state.mic.manufacturer().get_untracked();
    let conn_type = state.mic.connection_type().get_untracked();
    let loc = state.recording_meta.location().get_untracked();
    let is_mobile = state.status.is_mobile().get_untracked();
    let (dev_make, dev_model) =
        if state.recording_meta.device_model_enabled().get_untracked() && is_mobile {
            (
                state.recording_meta.cached_make().get_untracked(),
                state.recording_meta.cached_model().get_untracked(),
            )
        } else {
            (None, None)
        };

    // Determine mic_name for GUANO: USB gets the device name, internal gets "Internal".
    // Web Audio API uses a separate "Audio Device" field instead of "Name".
    let is_usb = conn_type
        .as_deref()
        .map(|c| c.contains("USB"))
        .unwrap_or(false);
    let is_web_audio = conn_type.as_deref() == Some("Web Audio API");
    let (guano_mic_name, guano_mic_audio_device) = if is_web_audio {
        (None, mic_name.clone())
    } else if is_usb {
        (mic_name.clone(), None)
    } else if conn_type.is_some() {
        (Some("Internal".to_string()), None)
    } else {
        (mic_name.clone(), None)
    };

    let preroll = state.mic.preroll_samples().get_untracked();
    let preroll_secs = if preroll > 0 && sample_rate > 0 {
        Some(preroll as f64 / sample_rate as f64)
    } else {
        None
    };

    let guano_extra = crate::audio::guano::RecordingGuanoExtra {
        mic_interface: conn_type,
        mic_name: guano_mic_name,
        mic_audio_device: guano_mic_audio_device,
        mic_make: mic_manufacturer,
        loc_position: loc.as_ref().map(|l| (l.latitude, l.longitude)),
        loc_elevation: loc.as_ref().and_then(|l| l.elevation),
        loc_accuracy: loc.as_ref().and_then(|l| l.accuracy),
        device_make: dev_make,
        device_model: dev_model,
        preroll_secs,
    };
    let guano = crate::audio::guano::build_recording_guano(
        sample_rate,
        duration_secs,
        filename,
        state.is_tauri,
        is_mobile,
        &guano_extra,
        &crate::format_time::recording_timestamp(duration_secs),
        env!("CARGO_PKG_VERSION"),
    );

    let wav_markers = if preroll > 0 {
        vec![crate::types::WavMarker {
            id: 1,
            position: preroll as u64,
            label: Some("Recording start".to_string()),
            note: None,
        }]
    } else {
        Vec::new()
    };

    RecordingMeta {
        guano,
        wav_markers,
        preroll_samples: preroll,
    }
}

/// Create or update the LoadedFile in state. Returns (file_index, filename).
fn update_or_create_file(
    state: AppState,
    live_idx: Option<usize>,
    audio: AudioData,
    preview: crate::types::PreviewImage,
    wav_markers: Vec<crate::types::WavMarker>,
    sample_rate: u32,
) -> (usize, String) {
    use crate::canvas::{spectral_store, tile_cache};

    // Baseline FFT/hop the progressive spectrogram + tile cache will use, so
    // the placeholder metadata set here agrees with the tiles that render into
    // it. (A stale live 1024/256 placeholder under baseline-hop tiles is part
    // of the mixed-resolution look at finalize.)
    let baseline_fft = state
        .spect
        .fft_mode()
        .get_untracked()
        .fft_for_lod(tile_cache::LOD_BASELINE);
    let placeholder_total_cols = if audio.samples.len() >= baseline_fft {
        (audio.samples.len() - baseline_fft) / 512 + 1
    } else {
        0
    };
    let placeholder_spec = SpectrogramData {
        columns: Vec::new().into(),
        total_columns: placeholder_total_cols,
        freq_resolution: sample_rate as f64 / baseline_fft as f64,
        time_resolution: 512.0 / sample_rate as f64,
        max_freq: sample_rate as f64 / 2.0,
        sample_rate,
    };

    let (file_index, name) = if let Some(idx) = live_idx {
        let name = state
            .library
            .files()
            .with_untracked(|files| files.get(idx).map(|f| f.name.clone()).unwrap_or_default());

        tile_cache::clear_file(idx);
        spectral_store::clear_file(idx);

        state.library.files().update(|files| {
            if let Some(f) = files.get_mut(idx) {
                f.audio = audio;
                f.preview = Some(preview);
                // Replace the live (1024/256) placeholder with one at the final
                // baseline resolution so progressive tiles render under matching
                // metadata instead of the stale live-waterfall resolution.
                f.spectrogram = placeholder_spec;
            }
        });

        (idx, name)
    } else {
        let name = generate_recording_name();

        let mut idx = 0;
        let name_clone = name.clone();
        state.library.files().update(|files| {
            idx = files.len();
            files.push(LoadedFile {
                id: crate::state::next_file_id(),
                name: name_clone,
                audio,
                spectrogram: placeholder_spec,
                preview: Some(preview),
                overview_image: None,
                xc_metadata: None,
                source_label: None,
                xc_hashes: None,
                is_demo: false,
                is_recording: true,
                is_live_listen: false,
                settings: FileSettings::default(),
                add_order: idx,
                last_modified_ms: None,
                identity: None,
                file_handle: None,
                cached_peak_db: None,
                cached_full_peak_db: None,
                read_only: false,
                had_sidecar: false,
                verify_outcome: crate::state::VerifyOutcome::Pending,
                all_hashes_verified: false,
                wav_markers: Vec::new(),
                loading_id: None,
                min_display_freq: None,
                max_display_freq: None,
            });
        });
        state.library.current_index().set(Some(idx));
        (idx, name)
    };

    // Store WAV markers (preroll cue point) on the file
    if !wav_markers.is_empty() {
        state.library.files().update(|files| {
            if let Some(f) = files.get_mut(file_index) {
                f.wav_markers = wav_markers;
            }
        });
    }

    (file_index, name)
}

/// Compute identity hashes and optionally save WAV bytes to disk.
/// `wav_bytes` are built once and reused for both hashing and saving.
fn persist_and_identify(
    state: AppState,
    file_index: usize,
    filename: String,
    wav_bytes: Option<Vec<u8>>,
    exact_file_size: u64,
    audio_data_size: u64,
    needs_save: bool,
    is_mobile: bool,
) {
    // If we also need to save, clone the bytes before identity computation consumes them.
    let wav_bytes_for_save = if needs_save { wav_bytes.clone() } else { None };

    // `wav_bytes = None` → hash the file from disk via its file_handle (the
    // native side already wrote it, so there's no need to re-encode the whole
    // WAV again on the WASM side). `Some` → hash the in-RAM bytes (browser path,
    // where there is no on-disk file).
    crate::file_identity::start_identity_computation(
        state,
        file_index,
        filename.clone(),
        exact_file_size,
        wav_bytes,
        Some(44),
        Some(audio_data_size),
        None,
    );

    if let Some(wav_data) = wav_bytes_for_save {
        wasm_bindgen_futures::spawn_local(async move {
            if is_mobile {
                crate::audio::wav_encoder::save_wav_to_shared(&wav_data, &filename).await;
            } else if try_tauri_save(&wav_data, &filename).await.is_some() {
                // Desktop WASM save succeeded
            }
            state.library.files().update(|files| {
                if let Some(f) = files.get_mut(file_index) {
                    f.is_recording = false;
                }
            });
        });
    }
}

/// Unified recording finalization. Handles both browser (WASM-side save) and
/// native/Tauri (already-saved) recordings. Updates the live file in-place when
/// one exists, otherwise creates a new LoadedFile as fallback.
pub(crate) fn finalize_recording(params: FinalizeParams, state: AppState) {
    use crate::canvas::live_waterfall;

    let FinalizeParams {
        samples,
        sample_rate,
        bits_per_sample,
        is_float,
        saved_path,
        file_size,
    } = params;

    let live_idx = state.mic.live_file_idx().get_untracked();

    // Streaming-to-disk mode: Tauri wrote the file during recording and
    // returned only metadata (no samples in RAM). Hand off to the streaming
    // loader so we only decode the head ~30 s for display and keep the rest
    // on disk. Avoids the multi-GB memory spike that the old "load all
    // samples into f.audio.samples" path would cause for long/high-SR
    // recordings.
    let has_native_path = !saved_path.is_empty() && !saved_path.starts_with("shared://");
    if samples.is_empty() && has_native_path {
        state.mic.live_file_idx().set(None);
        live_waterfall::clear();
        let path = saved_path.clone();
        let name = path.rsplit(['/', '\\']).next().unwrap_or(&path).to_string();
        let handle = crate::audio::streaming_source::FileHandle::TauriPath(path);
        let live_idx_for_async = live_idx;
        let fsize = file_size.unwrap_or(0) as u64;
        wasm_bindgen_futures::spawn_local(async move {
            if let Err(e) = finalize_streaming_tauri_recording(
                handle,
                name,
                fsize,
                sample_rate,
                bits_per_sample,
                is_float,
                state,
                live_idx_for_async,
            )
            .await
            {
                log::error!("Streaming finalize failed: {}", e);
                if let Some(idx) = live_idx_for_async {
                    state.library.files().update(|files| {
                        if idx < files.len() {
                            files.remove(idx);
                        }
                    });
                }
                state.show_error_toast(format!("Recording save succeeded but load failed: {}", e));
            }
        });
        return;
    }
    // Shared storage (Android MediaStore): the file lives only under a content://
    // URI in public storage. Read it back via the media-store plugin and display
    // the full recording (streamed on demand) — no on-device duplication. A
    // pre-Q "shared" path is a real filesystem path, so it uses TauriPath.
    if samples.is_empty() && saved_path.starts_with("shared://") {
        state.mic.live_file_idx().set(None);
        live_waterfall::clear();
        let uri = state.mic.pending_shared_uri().get_untracked();
        state.mic.pending_shared_uri().set(None);
        let fsize = file_size.unwrap_or(0) as u64;
        let live_idx_for_async = live_idx;
        match uri {
            Some(uri) if fsize > 44 => {
                let handle = if uri.starts_with("content://") {
                    crate::audio::streaming_source::FileHandle::MediaStoreUri(uri)
                } else {
                    crate::audio::streaming_source::FileHandle::TauriPath(uri)
                };
                let name = live_idx
                    .and_then(|i| {
                        state
                            .library
                            .files()
                            .with_untracked(|f| f.get(i).map(|f| f.name.clone()))
                    })
                    .unwrap_or_else(generate_recording_name);
                wasm_bindgen_futures::spawn_local(async move {
                    if let Err(e) = finalize_streaming_tauri_recording(
                        handle,
                        name,
                        fsize,
                        sample_rate,
                        bits_per_sample,
                        is_float,
                        state,
                        live_idx_for_async,
                    )
                    .await
                    {
                        log::error!("Shared-storage finalize failed: {}", e);
                        if let Some(idx) = live_idx_for_async {
                            state.library.files().update(|files| {
                                if idx < files.len() {
                                    files.remove(idx);
                                }
                            });
                        }
                        state.show_error_toast(format!("Recording saved but load failed: {}", e));
                    }
                });
            }
            _ => {
                // No URI to read back (unexpected) — drop the placeholder.
                if let Some(idx) = live_idx {
                    state.library.files().update(|files| {
                        if idx < files.len() {
                            files.remove(idx);
                        }
                    });
                }
                state.show_info_toast("Recording saved to device storage");
            }
        }
        return;
    }

    state.mic.live_file_idx().set(None);

    if samples.is_empty() {
        log::warn!("Empty recording");
        if let Some(idx) = live_idx {
            state.library.files().update(|files| {
                if idx < files.len() {
                    files.remove(idx);
                }
            });
        }
        return;
    }

    // The WAV encode below is O(N) over the whole recording and, for a long
    // pre-roll capture, blocked the UI thread for seconds at Stop. Defer the
    // encode + preview + file update + identity + spectrogram into a yielding
    // async task so the UI stays responsive. Capture the live file's stable id
    // so the task can re-resolve its index even if the list shifts (e.g. a
    // follow-on listen session prunes placeholders) before it runs.
    let live_id = live_idx.and_then(|i| {
        state
            .library
            .files()
            .with_untracked(|f| f.get(i).map(|f| f.id))
    });
    state.show_info_toast("Saving recording\u{2026}");
    wasm_bindgen_futures::spawn_local(async move {
        finalize_in_memory_recording(
            samples,
            sample_rate,
            bits_per_sample,
            is_float,
            saved_path,
            file_size,
            live_idx,
            live_id,
            state,
        )
        .await;
    });
}

/// Heavy tail of `finalize_recording`: WAV encode + preview + file update +
/// identity + spectrogram, run in a spawned task with cooperative yields so a
/// long recording's `encode_wav_complete` can't freeze the UI thread at Stop.
async fn finalize_in_memory_recording(
    samples: Vec<f32>,
    sample_rate: u32,
    bits_per_sample: u16,
    is_float: bool,
    saved_path: String,
    file_size: Option<usize>,
    live_idx: Option<usize>,
    live_id: Option<u64>,
    state: AppState,
) {
    use crate::canvas::live_waterfall;

    // Re-resolve the live file's current index by its stable id; the list may
    // have shifted since finalize_recording captured `live_idx`.
    let live_idx = live_id
        .and_then(|id| {
            state
                .library
                .files()
                .with_untracked(|f| f.iter().position(|x| x.id == id))
        })
        .or(live_idx);

    let duration_secs = samples.len() as f64 / sample_rate as f64;

    // ── Phase 1: Build metadata (GUANO + WAV markers) from state ────────
    let recording_name = live_idx
        .and_then(|idx| {
            state
                .library
                .files()
                .with_untracked(|f| f.get(idx).map(|f| f.name.clone()))
        })
        .unwrap_or_else(generate_recording_name);
    let meta = build_recording_meta(&state, sample_rate, duration_secs, &recording_name);

    // Whether the native side already wrote the file to disk (to-memory writes
    // the WAV at stop; shared storage streamed it into the content:// fd). When
    // it did, we hash that file from disk in Phase 4 instead of re-encoding the
    // whole WAV again here.
    let shared_saved = saved_path.starts_with("shared://");
    let native_saved = !saved_path.is_empty();
    let has_disk_file = native_saved || shared_saved;

    // ── Phase 2: Encode WAV bytes — only when there is no on-disk file to hash
    // (the browser path). For to-memory the native side already saved the WAV,
    // so skip this second O(N) encode and hash from disk instead. ─
    let samples: Arc<Vec<f32>> = samples.into();
    let source = Arc::new(InMemorySource {
        samples: samples.clone(),
        raw_samples: None,
        sample_rate,
        channels: 1,
    });
    let wav_bytes: Option<Vec<u8>> = if has_disk_file {
        None
    } else {
        // Yield so the "Saving…" toast paints before the heavy encode runs.
        crate::web_util::yield_now().await;
        Some(crate::audio::wav_encoder::encode_wav_complete(
            &samples,
            sample_rate,
            Some(&meta.guano),
            &meta.wav_markers,
        ))
    };
    let exact_file_size = file_size
        .or_else(|| wav_bytes.as_ref().map(|b| b.len()))
        .unwrap_or(0);
    let num_samples = samples.len() as u64;
    let audio_data_size = num_samples * (bits_per_sample as u64 / 8);

    let audio = AudioData {
        samples,
        source,
        sample_rate,
        channels: 1,
        duration_secs,
        metadata: FileMetadata {
            file_size: exact_file_size,
            format: "REC",
            bits_per_sample,
            is_float,
            guano: Some(meta.guano),
            data_offset: Some(44),
            data_size: Some(audio_data_size),
            zc_data: None,
        },
    };

    let preview = compute_preview(&audio, 256, 128);
    let audio_for_stft = audio.clone();

    // ── Phase 3: Update or create the file in state ─────────────────────
    let (file_index, name_check) = update_or_create_file(
        state,
        live_idx,
        audio,
        preview,
        meta.wav_markers,
        sample_rate,
    );

    live_waterfall::clear();

    // Wire up the file handle to the on-disk file so identity hashing (and any
    // later re-open) read from disk. A real path → TauriPath; an Android shared
    // recording lives only at its content:// URI → MediaStoreUri.
    if native_saved && !shared_saved {
        state.library.files().update(|files| {
            if let Some(f) = files.get_mut(file_index) {
                f.file_handle = Some(crate::audio::streaming_source::FileHandle::TauriPath(
                    saved_path,
                ));
            }
        });
    } else if shared_saved {
        if let Some(uri) = state.mic.pending_shared_uri().get_untracked() {
            let handle = if uri.starts_with("content://") {
                crate::audio::streaming_source::FileHandle::MediaStoreUri(uri)
            } else {
                crate::audio::streaming_source::FileHandle::TauriPath(uri)
            };
            state.library.files().update(|files| {
                if let Some(f) = files.get_mut(file_index) {
                    f.file_handle = Some(handle);
                }
            });
        }
        state.mic.pending_shared_uri().set(None);
    }

    // Mark saved if native backend already persisted the file
    let is_tauri = state.is_tauri;
    let is_mobile = state.status.is_mobile().get_untracked();
    let to_memory =
        state.playback.record_mode().get_untracked() == crate::state::RecordMode::ToMemory;

    if has_disk_file && !to_memory {
        state.library.files().update(|files| {
            if let Some(f) = files.get_mut(file_index) {
                f.is_recording = false;
            }
        });
    }

    // ── Phase 4: Hash computation + optional WAV save ───────────────────
    // `needs_save` only when there's no on-disk file the native side already
    // wrote (browser path); otherwise we hash that file from disk (wav_bytes
    // is None) and skip both the re-encode and a redundant save.
    let needs_save = !to_memory
        && !has_disk_file
        && if is_mobile {
            true
        } else {
            is_tauri && !native_saved
        };

    crate::web_util::yield_now().await;
    persist_and_identify(
        state,
        file_index,
        name_check.clone(),
        wav_bytes,
        exact_file_size as u64,
        audio_data_size,
        needs_save,
        is_mobile,
    );

    // ── Phase 5: Reset preroll + zoom + spectrogram ─────────────────────
    if meta.preroll_samples > 0 {
        state.mic.preroll_samples().set(0);
    }

    let canvas_w = state.viewmode.spectrogram_canvas_width().get_untracked();
    let final_time_res = 512.0 / sample_rate as f64;
    state.view.zoom_level().set(crate::viewport::fit_zoom(
        canvas_w,
        final_time_res,
        duration_secs,
    ));
    state.view.scroll_offset().set(0.0);

    spawn_spectrogram_computation(audio_for_stft, name_check, file_index, state);
}

/// Async handoff for Tauri streaming-mode recordings. The `.wav` is already
/// on disk (written incrementally during recording). We read just the header
/// + first ~30 s via range reads, build a `StreamingWavSource`, and update
/// the live file entry so the rest of the recording is streamed on demand
/// via `read_file_range`. Peak memory = head window (~30 s of f32), not the
/// full recording.
async fn finalize_streaming_tauri_recording(
    handle: crate::audio::streaming_source::FileHandle,
    name: String,
    file_size: u64,
    expected_sample_rate: u32,
    expected_bits_per_sample: u16,
    _expected_is_float: bool,
    state: AppState,
    live_idx: Option<usize>,
) -> Result<(), String> {
    use crate::audio::loader::parse_wav_header_with_file_size;
    use crate::audio::source::DEFAULT_ANALYSIS_WINDOW_SECS;
    use crate::audio::streaming_source::StreamingWavSource;
    use crate::canvas::{spectral_store, tile_cache};
    use crate::components::file_sidebar::streaming_load::{decode_head_pcm, scan_tail_for_guano};

    // Read first 64 KB for header parsing (covers fmt, optional fact, and
    // usually the data chunk start). Reads go through the handle, which is a
    // filesystem path (desktop / internal) or an Android MediaStore content://
    // URI (shared storage) read back via the media-store plugin.
    let header_size = 65536u64.min(file_size);
    let header_bytes = handle.read_range(0, header_size).await?;
    let header = parse_wav_header_with_file_size(&header_bytes, Some(file_size))?;
    if header.sample_rate != expected_sample_rate {
        log::warn!(
            "recording sample rate mismatch: file says {}, expected {}",
            header.sample_rate,
            expected_sample_rate,
        );
    }

    // Decode the first ~30 s into mono f32 for fast display + analysis.
    let head_frames = ((DEFAULT_ANALYSIS_WINDOW_SECS * header.sample_rate as f64) as u64)
        .min(header.total_frames);
    let bytes_per_frame = header.channels as u64 * (header.bits_per_sample as u64 / 8);
    let head_byte_len = head_frames * bytes_per_frame;
    let head_pcm_bytes = handle.read_range(header.data_offset, head_byte_len).await?;
    let head_interleaved = decode_head_pcm(
        &head_pcm_bytes,
        header.bits_per_sample,
        header.is_float,
        header.channels,
    );
    let channels = header.channels as usize;
    let (head_mono, head_raw) = if channels == 1 {
        (head_interleaved, None)
    } else {
        let mono: Vec<f32> = head_interleaved
            .chunks_exact(channels)
            .map(|frame| frame.iter().sum::<f32>() / channels as f32)
            .collect();
        (mono, Some(head_interleaved))
    };

    // GUANO chunk is typically after the data chunk in our writer — scan the
    // tail of the file for it.
    let mut guano = header.guano.clone();
    if guano.is_none() {
        let data_end = header.data_offset + header.data_size;
        if data_end < file_size {
            let tail_len = (file_size - data_end).min(65536);
            if let Ok(tail_bytes) = handle.read_range(data_end, tail_len).await {
                guano = scan_tail_for_guano(&tail_bytes);
            }
        }
    }

    let source = Arc::new(StreamingWavSource::new(
        handle.clone(),
        &header,
        head_mono.clone(),
        head_raw,
    ));

    let duration_secs = header.total_frames as f64 / header.sample_rate as f64;
    let samples_arc = Arc::new(head_mono);
    let audio = AudioData {
        samples: samples_arc,
        source,
        sample_rate: header.sample_rate,
        channels: header.channels as u32,
        duration_secs,
        metadata: crate::types::FileMetadata {
            file_size: file_size as usize,
            format: "REC",
            bits_per_sample: header.bits_per_sample,
            is_float: header.is_float,
            guano,
            data_offset: Some(header.data_offset),
            data_size: Some(header.data_size),
            zc_data: None,
        },
    };
    let preview = crate::dsp::fft::compute_preview(&audio, 256, 128);

    state.log_debug(
        "rec",
        format!(
        "finalize_streaming: head={} samples, preview {}x{}, dur={:.1}s, file_size={}, handle={:?}",
        audio.samples.len(), preview.width, preview.height, duration_secs, file_size, handle,
    ),
    );

    let (file_index, name_check) = update_or_create_file(
        state,
        live_idx,
        audio,
        preview,
        Vec::new(),
        header.sample_rate,
    );

    // Display the FULL recording straight from the on-disk .wav — no 30 s cap.
    // Size the spectrogram + spectral store for the whole file and let the tile
    // scheduler compute baseline tiles on demand from the StreamingWavSource
    // (reading via read_file_range), exactly like a regular large streaming-WAV
    // load. The live capture's ~30 s circular window during recording/listening
    // is a separate buffer and is unaffected — this only governs the post-Stop
    // displayed file.
    const HOP_SIZE: usize = 512;
    let fft_size = state
        .spect
        .fft_mode()
        .get_untracked()
        .fft_for_lod(tile_cache::LOD_BASELINE);
    let total_len = header.total_frames as usize;
    let total_cols = if total_len >= fft_size {
        (total_len - fft_size) / HOP_SIZE + 1
    } else {
        0
    };

    // Rename the live entry to match the on-disk filename, wire up the file
    // handle, and install the full-length (empty) spectrogram metadata so the
    // renderer maps the whole recording.
    state.library.files().update(|files| {
        if let Some(f) = files.get_mut(file_index) {
            f.name = name.clone();
            f.file_handle = Some(handle.clone());
            f.is_recording = false;
            f.audio.metadata.bits_per_sample = expected_bits_per_sample;
            f.spectrogram = SpectrogramData {
                columns: Arc::new(Vec::new()),
                total_columns: total_cols,
                freq_resolution: header.sample_rate as f64 / fft_size as f64,
                time_resolution: HOP_SIZE as f64 / header.sample_rate as f64,
                max_freq: header.sample_rate as f64 / 2.0,
                sample_rate: header.sample_rate,
            };
        }
    });

    let canvas_w = state.viewmode.spectrogram_canvas_width().get_untracked();
    let final_time_res = HOP_SIZE as f64 / header.sample_rate as f64;
    state.view.zoom_level().set(crate::viewport::fit_zoom(
        canvas_w,
        final_time_res,
        duration_secs,
    ));
    state.view.scroll_offset().set(0.0);

    // Place the "Recording start" cue marker at the pre-roll boundary. The native
    // side seeded the pre-roll from its listening ring (and recorded the duration
    // in GUANO on disk); this is the visual marker on the displayed file. Clamp
    // to the file length so a short listen can't push it past the end.
    let preroll = state.mic.preroll_samples().get_untracked();
    if preroll > 0 {
        state.mic.preroll_samples().set(0);
        let pos = (preroll as u64).min(header.total_frames);
        state.library.files().update(|files| {
            if let Some(f) = files.get_mut(file_index) {
                f.wav_markers = vec![crate::types::WavMarker {
                    id: 1,
                    position: pos,
                    label: Some("Recording start".to_string()),
                    note: None,
                }];
            }
        });
    }

    // Clear the provisional live tiles + store, init the store for the full
    // length, prefetch the initial viewport from disk, then schedule the
    // visible tiles. Remaining tiles stream in on demand as the user scrolls —
    // the same machinery as try_streaming_wav.
    tile_cache::clear_file(file_index);
    spectral_store::clear_file(file_index);
    spectral_store::init(file_index, total_cols, fft_size);

    let (start_sample, count) = crate::components::file_sidebar::streaming_load::prefetch_window(
        state,
        header.sample_rate,
        fft_size,
    );
    if let Some(f) = state
        .library
        .files()
        .get_untracked()
        .get(file_index)
        .cloned()
    {
        if let Some(streaming) = f.audio.source.as_any().downcast_ref::<StreamingWavSource>() {
            streaming.prefetch_region(start_sample, count).await;
        }
    }
    tile_cache::schedule_visible_tiles_from_store(state, file_index, total_cols);
    state
        .viewmode
        .tile_ready_signal()
        .update(|n| *n = n.wrapping_add(1));
    wasm_bindgen_futures::spawn_local(
        crate::components::file_sidebar::streaming_load::build_streaming_overview(
            state, file_index, name_check,
        ),
    );

    // Compute identity hash from the file on disk so the Name field in GUANO
    // and future sidecar resolution stay consistent.
    crate::file_identity::start_identity_computation(
        state,
        file_index,
        name,
        file_size,
        None,
        Some(44),
        Some(header.data_size),
        None,
    );

    Ok(())
}

fn generate_recording_name() -> String {
    let now = js_sys::Date::new_0();
    format!(
        "batcap_{:04}{:02}{:02}_{:02}{:02}{:02}.wav",
        now.get_full_year(),
        now.get_month() + 1,
        now.get_date(),
        now.get_hours(),
        now.get_minutes(),
        now.get_seconds(),
    )
}

/// Shared async spectrogram computation (used by both web and Tauri modes).
pub(crate) fn spawn_spectrogram_computation(
    audio: AudioData,
    name_check: String,
    file_index: usize,
    state: AppState,
) {
    wasm_bindgen_futures::spawn_local(async move {
        let yield_promise = js_sys::Promise::new(&mut |resolve, _| {
            if let Some(w) = web_sys::window() {
                let _ = w.set_timeout_with_callback(&resolve);
            }
        });
        JsFuture::from(yield_promise).await.ok();

        // Match the active baseline FFT used by the regular file-load path and
        // the tile scheduler. A hard-coded 2048 here diverged from the default
        // AdaptiveM baseline (1024): the provisional store tiles rendered at
        // 1025 bins, then `schedule_all_tiles` re-rendered them on-demand at
        // 513 bins (because spectrogram_fft != baseline), so mid-finalize the
        // view showed tiles at two different resolutions (+ a brightness step).
        // Using the baseline keeps the store tiles, f.spectrogram metadata, and
        // any later baseline render all consistent — no re-render churn.
        let fft_size = state
            .spect
            .fft_mode()
            .get_untracked()
            .fft_for_lod(crate::canvas::tile_cache::LOD_BASELINE);
        const HOP_SIZE: usize = 512;
        const CHUNK_COLS: usize = 32;

        let total_cols = if audio.samples.len() >= fft_size {
            (audio.samples.len() - fft_size) / HOP_SIZE + 1
        } else {
            0
        };

        use crate::canvas::spectral_store;
        use crate::canvas::tile_cache::{self, TILE_COLS};

        // Initialise spectral store for progressive tile generation
        spectral_store::init(file_index, total_cols, fft_size);

        let n_tiles = total_cols.div_ceil(TILE_COLS);
        let mut tile_scheduled = vec![false; n_tiles];
        let mut chunk_start = 0;

        while chunk_start < total_cols {
            let still_present = state
                .library
                .files()
                .get_untracked()
                .get(file_index)
                .map(|f| f.name == name_check)
                .unwrap_or(false);
            if !still_present {
                spectral_store::clear_file(file_index);
                return;
            }

            let chunk =
                compute_spectrogram_partial(&audio, fft_size, HOP_SIZE, chunk_start, CHUNK_COLS);

            // Insert into spectral store for progressive tile generation
            spectral_store::insert_columns(file_index, chunk_start, &chunk);

            // Check if any tile is now complete and render it synchronously
            // (must be sync — async schedule_tile_from_store races with drain_columns below)
            let first_tile = chunk_start / TILE_COLS;
            let last_tile = ((chunk_start + chunk.len()).saturating_sub(1)) / TILE_COLS;
            let mut any_tile_rendered = false;
            let tile_end_idx = last_tile.min(n_tiles.saturating_sub(1));
            for (tile_idx, scheduled) in tile_scheduled
                .iter_mut()
                .enumerate()
                .take(tile_end_idx + 1)
                .skip(first_tile)
            {
                if *scheduled {
                    continue;
                }
                let tile_start = tile_idx * TILE_COLS;
                let tile_end = (tile_start + TILE_COLS).min(total_cols);
                if spectral_store::tile_complete(file_index, tile_start, tile_end) {
                    if tile_cache::render_tile_from_store_sync(file_index, tile_idx, fft_size) {
                        any_tile_rendered = true;
                    }
                    *scheduled = true;
                }
            }
            if any_tile_rendered {
                state
                    .viewmode
                    .tile_ready_signal()
                    .update(|n| *n = n.wrapping_add(1));
            }

            chunk_start += CHUNK_COLS;

            let p = js_sys::Promise::new(&mut |resolve, _| {
                if let Some(w) = web_sys::window() {
                    let _ = w.set_timeout_with_callback(&resolve);
                }
            });
            JsFuture::from(p).await.ok();
        }

        // Drain store and assemble final SpectrogramData
        let final_columns = spectral_store::drain_columns(file_index).unwrap_or_default();

        let freq_resolution = audio.sample_rate as f64 / fft_size as f64;
        let time_resolution = HOP_SIZE as f64 / audio.sample_rate as f64;
        let max_freq = audio.sample_rate as f64 / 2.0;

        let col_count = final_columns.len();
        let spectrogram = SpectrogramData {
            columns: final_columns.into(),
            total_columns: col_count,
            freq_resolution,
            time_resolution,
            max_freq,
            sample_rate: audio.sample_rate,
        };

        log::info!(
            "Recording spectrogram: {} columns, freq_res={:.1} Hz, time_res={:.4}s",
            spectrogram.columns.len(),
            spectrogram.freq_resolution,
            spectrogram.time_resolution
        );

        // Compute overview image for the recording
        let overview_img = crate::dsp::fft::compute_overview_from_spectrogram(&spectrogram);

        state.library.files().update(|files| {
            if let Some(f) = files.get_mut(file_index) {
                if f.name == name_check {
                    f.spectrogram = spectrogram;
                    f.overview_image = overview_img;
                }
            }
        });

        // Clear stale tiles (rendered with provisional max_magnitude) and
        // re-schedule with accurate final normalization.
        tile_cache::clear_file(file_index);
        let file_for_tiles = state
            .library
            .files()
            .get_untracked()
            .get(file_index)
            .cloned();
        if let Some(file) = file_for_tiles {
            tile_cache::schedule_all_tiles(state, file, file_index);
        }

        state.viewmode.tile_ready_signal().update(|n| *n += 1);
    });
}
