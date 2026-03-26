use leptos::prelude::*;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use crate::state::{AppState, FileSettings, LoadedFile};
use crate::audio::source::InMemorySource;
use crate::audio::microphone::with_live_samples;
use crate::audio::wav_encoder::{encode_wav_with_guano, try_tauri_save};
use crate::types::{AudioData, FileMetadata, SpectrogramData};
use crate::dsp::fft::{compute_preview, compute_spectrogram_partial, compute_stft_columns};
use std::sync::Arc;

/// Create a live LoadedFile at recording start for real-time visualization.
/// Returns the file index where the live file was inserted.
pub(crate) fn start_live_recording(state: &AppState, sample_rate: u32) -> usize {
    let now = js_sys::Date::new_0();
    let name = format!(
        "batcap_{:04}-{:02}-{:02}_{:02}{:02}{:02}.wav",
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
            bits_per_sample: state.mic_bits_per_sample.get_untracked(),
            is_float: false,
            guano: None,
            data_offset: None,
            data_size: None,
        },
    };

    // FFT=256/hop=256 for live waterfall rendering
    let (live_fft, live_hop) = (256.0, 256.0);
    let placeholder_spec = SpectrogramData {
        columns: Arc::new(Vec::new()),
        total_columns: 0,
        freq_resolution: sample_rate as f64 / live_fft,
        time_resolution: live_hop / sample_rate as f64,
        max_freq: sample_rate as f64 / 2.0,
        sample_rate,
    };

    let mut file_index = 0;
    state.files.update(|files| {
        file_index = files.len();
        files.push(LoadedFile {
            name,
            audio,
            spectrogram: placeholder_spec,
            preview: None,
            overview_image: None,
            xc_metadata: None,
            xc_hashes: None,
            is_recording: true,
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
        });
    });

    state.current_file_index.set(Some(file_index));
    state.mic_live_file_idx.set(Some(file_index));

    // Set zoom for comfortable live recording scroll speed
    let canvas_w = state.spectrogram_canvas_width.get_untracked();
    let live_time_res = 64.0 / sample_rate as f64;
    state.zoom_level.set(crate::viewport::recording_zoom(canvas_w, live_time_res));
    state.scroll_offset.set(0.0);

    file_index
}

/// Spawns an async processing loop that incrementally computes STFT columns
/// and pushes them to the live waterfall for direct canvas rendering.
/// No tile cache or spectral store is used — the waterfall renders directly.
pub(crate) fn spawn_live_processing_loop(state: AppState, file_index: usize, sample_rate: u32) {
    use crate::canvas::live_waterfall;

    // FFT=256 for low latency, hop=256 for reasonable column rate.
    let (fft_size, hop_size): (usize, usize) = (256, 256);
    const PROCESS_INTERVAL_MS: i32 = 50;

    wasm_bindgen_futures::spawn_local(async move {
        let mut last_processed_col: usize = 0;
        let mut last_snapshot_len: usize = 0;
        let is_tauri = state.is_tauri;

        // Initialize waterfall for direct rendering
        live_waterfall::create(fft_size, hop_size, sample_rate);

        loop {
            let p = js_sys::Promise::new(&mut |resolve, _| {
                if let Some(w) = web_sys::window() {
                    let _ = w.set_timeout_with_callback_and_timeout_and_arguments_0(
                        &resolve, PROCESS_INTERVAL_MS,
                    );
                }
            });
            let _ = JsFuture::from(p).await;

            // Check if still recording/listening
            let is_recording = state.mic_recording.get_untracked();
            let is_listening = state.mic_listening.get_untracked();
            if !is_recording && !is_listening {
                break;
            }
            // Check file still valid (recording mode only)
            if is_recording && state.mic_live_file_idx.get_untracked() != Some(file_index) {
                break;
            }

            // Compute new FFT columns from the live buffer
            let any_update = with_live_samples(is_tauri, |samples| -> bool {
                if samples.len() < fft_size {
                    return false;
                }

                let total_possible_cols = (samples.len() - fft_size) / hop_size + 1;
                if total_possible_cols <= last_processed_col {
                    return false;
                }

                let new_col_count = total_possible_cols - last_processed_col;

                // Compute new STFT columns
                let new_cols = compute_stft_columns(
                    samples,
                    sample_rate,
                    fft_size,
                    hop_size,
                    last_processed_col,
                    new_col_count,
                );

                if new_cols.is_empty() {
                    return false;
                }

                // Push to waterfall for direct rendering
                live_waterfall::push_columns(&new_cols);

                // Update file metadata (recording mode)
                if is_recording {
                    let duration = samples.len() as f64 / sample_rate as f64;
                    state.files.update(|files| {
                        if let Some(f) = files.get_mut(file_index) {
                            f.spectrogram.total_columns = total_possible_cols;
                            f.audio.duration_secs = duration;
                        }
                    });
                }

                // Periodically snapshot for waveform rendering (~1s interval)
                if is_recording {
                    let snapshot_threshold = (sample_rate as usize).max(44100);
                    let do_snapshot = samples.len() - last_snapshot_len >= snapshot_threshold || last_snapshot_len == 0;
                    if do_snapshot {
                        let snapshot = Arc::new(samples.to_vec());
                        state.files.update(|files| {
                            if let Some(f) = files.get_mut(file_index) {
                                f.audio.samples = snapshot;
                            }
                        });
                        last_snapshot_len = samples.len();
                    }
                }

                last_processed_col = total_possible_cols;
                true
            });

            if any_update {
                let total_cols = live_waterfall::total_columns();
                state.mic_live_data_cols.set(total_cols);

                // Trigger spectrogram redraw
                state.tile_ready_signal.update(|n| *n = n.wrapping_add(1));

                // Set target scroll for waterfall effect
                if total_cols > 0 {
                    let time_res = hop_size as f64 / sample_rate as f64;
                    let recording_time = total_cols as f64 * time_res;
                    let canvas_w = state.spectrogram_canvas_width.get_untracked();
                    let zoom = state.zoom_level.get_untracked();
                    if zoom > 0.0 && canvas_w > 0.0 {
                        let visible_cols = canvas_w / zoom;
                        let visible_time = visible_cols * time_res;
                        let target_scroll = (recording_time - visible_time).max(0.0);
                        state.mic_recording_target_scroll.set(target_scroll);
                    }
                }
            }
        }

        // Processing loop exited — clean up
        if !state.mic_recording.get_untracked() {
            // Only clear waterfall when fully done (not when switching from listen to record)
            live_waterfall::clear();
        }
        state.mic_live_file_idx.set(None);
        state.mic_live_data_cols.set(0);
        state.mic_recording_target_scroll.set(0.0);
    });
}

/// Spawns a requestAnimationFrame loop that smoothly interpolates
/// `scroll_offset` toward `mic_recording_target_scroll` for waterfall scrolling.
/// Automatically stops when recording and listening both end.
pub(crate) fn spawn_smooth_scroll_animation(state: AppState) {
    use std::rc::Rc;
    use std::cell::RefCell;

    let cb: Rc<RefCell<Option<Closure<dyn FnMut()>>>> = Rc::new(RefCell::new(None));
    let cb_clone = cb.clone();

    *cb.borrow_mut() = Some(Closure::new(move || {
        if !state.mic_recording.get_untracked() && !state.mic_listening.get_untracked() {
            // Neither recording nor listening — exit the animation loop
            return;
        }
        let target = state.mic_recording_target_scroll.get_untracked();
        let current = state.scroll_offset.get_untracked();
        let diff = target - current;
        if diff.abs() > 0.0001 {
            // Exponential ease: move 30% of remaining distance each frame (~60fps)
            let new_scroll = current + diff * 0.3;
            state.scroll_offset.set(new_scroll);
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

/// Finalize a live recording by updating the existing live file in-place.
/// Clears the progressive tiles and re-runs full spectrogram computation for
/// accurate normalization. Works for both web and Tauri modes.
pub(crate) fn finalize_live_recording(samples: Vec<f32>, sample_rate: u32, state: AppState) {
    use crate::canvas::{spectral_store, tile_cache, live_waterfall};

    let live_idx = state.mic_live_file_idx.get_untracked();
    state.mic_live_file_idx.set(None);

    // If no live file exists, fall back to the old path
    let file_index = match live_idx {
        Some(idx) => idx,
        None => {
            finalize_recording(samples, sample_rate, state);
            return;
        }
    };

    if samples.is_empty() {
        log::warn!("Empty recording, removing live file");
        state.files.update(|files| {
            if file_index < files.len() {
                files.remove(file_index);
            }
        });
        return;
    }

    let duration_secs = samples.len() as f64 / sample_rate as f64;

    let name_check = state.files.with_untracked(|files| {
        files.get(file_index).map(|f| f.name.clone()).unwrap_or_default()
    });

    let mic_name = state.mic_device_name.get_untracked();
    let guano = crate::audio::guano::build_recording_guano(
        sample_rate, duration_secs, &name_check, state.is_tauri, mic_name.as_deref(),
    );

    let samples: Arc<Vec<f32>> = samples.into();
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
        duration_secs,
        metadata: FileMetadata {
            file_size: 0,
            format: "REC",
            bits_per_sample: state.mic_bits_per_sample.get_untracked(),
            is_float: false,
            guano: Some(guano),
            data_offset: None,
            data_size: None,
        },
    };

    let preview = compute_preview(&audio, 256, 128);
    let audio_for_stft = audio.clone();

    let is_tauri = state.is_tauri;
    let name_for_save = name_check.clone();

    // Update the existing file with final audio data and preview
    state.files.update(|files| {
        if let Some(f) = files.get_mut(file_index) {
            f.audio = audio;
            f.preview = Some(preview);
        }
    });

    // Clear live waterfall, progressive tiles, and spectral store — will be re-rendered with final normalization
    live_waterfall::clear();
    tile_cache::clear_file(file_index);
    spectral_store::clear_file(file_index);

    // Set Layer 1 identity (estimated WAV size since file may not be on disk yet)
    let bits_per_sample = state.mic_bits_per_sample.get_untracked();
    let num_samples = (duration_secs * sample_rate as f64).ceil() as u64;
    let estimated_size = 44 + num_samples * (bits_per_sample as u64 / 8);
    crate::file_identity::start_identity_computation(
        state, file_index, name_check.clone(), estimated_size, None,
        None, None, None,
    );

    // Try Tauri auto-save in background
    if is_tauri {
        let mic_name = state.mic_device_name.get_untracked();
        let samples_ref = state.files.get_untracked();
        if let Some(file) = samples_ref.get(file_index) {
            let wav_data = encode_wav_with_guano(&file.audio.samples, file.audio.sample_rate, &name_for_save, true, mic_name.as_deref());
            let filename = name_for_save;
            wasm_bindgen_futures::spawn_local(async move {
                if try_tauri_save(&wav_data, &filename).await {
                    state.files.update(|files| {
                        if let Some(f) = files.get_mut(file_index) {
                            f.is_recording = false;
                        }
                    });
                }
            });
        }
    }

    // Zoom to fit the entire recording
    let canvas_w = state.spectrogram_canvas_width.get_untracked();
    let final_time_res = 512.0 / sample_rate as f64;
    state.zoom_level.set(crate::viewport::fit_zoom(canvas_w, final_time_res, duration_secs));
    state.scroll_offset.set(0.0);

    // Re-compute full spectrogram with accurate final normalization
    spawn_spectrogram_computation(audio_for_stft, name_check, file_index, state);
}

/// Convert recorded samples into a LoadedFile and add to state (web mode).
/// Used as a fallback when no live file exists.
fn finalize_recording(samples: Vec<f32>, sample_rate: u32, state: AppState) {
    let duration_secs = samples.len() as f64 / sample_rate as f64;
    let now = js_sys::Date::new_0();
    let name = format!(
        "batcap_{:04}-{:02}-{:02}_{:02}{:02}{:02}.wav",
        now.get_full_year(),
        now.get_month() + 1,
        now.get_date(),
        now.get_hours(),
        now.get_minutes(),
        now.get_seconds(),
    );

    let mic_name = state.mic_device_name.get_untracked();
    let guano = crate::audio::guano::build_recording_guano(
        sample_rate, duration_secs, &name, state.is_tauri, mic_name.as_deref(),
    );

    let samples: Arc<Vec<f32>> = samples.into();
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
        duration_secs,
        metadata: FileMetadata {
            file_size: 0,
            format: "REC",
            bits_per_sample: 16,
            is_float: false,
            guano: Some(guano),
            data_offset: None,
            data_size: None,
        },
    };

    let preview = compute_preview(&audio, 256, 128);
    let audio_for_stft = audio.clone();
    let name_check = name.clone();
    let name_for_save = name.clone();
    let is_tauri = state.is_tauri;

    let total_cols = if audio.samples.len() >= 2048 {
        (audio.samples.len() - 2048) / 512 + 1
    } else {
        0
    };
    let placeholder_spec = SpectrogramData {
        columns: Vec::new().into(),
        total_columns: total_cols,
        freq_resolution: sample_rate as f64 / 2048.0,
        time_resolution: 512.0 / sample_rate as f64,
        max_freq: sample_rate as f64 / 2.0,
        sample_rate,
    };

    let file_index;
    {
        let mut idx = 0;
        state.files.update(|files| {
            idx = files.len();
            files.push(LoadedFile {
                name,
                audio,
                spectrogram: placeholder_spec,
                preview: Some(preview),
                overview_image: None,
                xc_metadata: None,
                xc_hashes: None,
                is_recording: true,
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
            });
        });
        file_index = idx;
    }
    state.current_file_index.set(Some(file_index));

    // Set Layer 1 identity (estimated WAV size)
    let num_samples_est = (duration_secs * sample_rate as f64).ceil() as u64;
    let estimated_size = 44 + num_samples_est * (16 / 8); // bits_per_sample=16 for this path
    crate::file_identity::start_identity_computation(
        state, file_index, name_check.clone(), estimated_size, None,
        None, None, None,
    );

    // Try Tauri auto-save in background (web mode path for old save_recording command)
    if is_tauri {
        let samples_ref = state.files.get_untracked();
        if let Some(file) = samples_ref.get(file_index) {
            let wav_data = encode_wav_with_guano(&file.audio.samples, file.audio.sample_rate, &name_for_save, true, mic_name.as_deref());
            let filename = name_for_save;
            wasm_bindgen_futures::spawn_local(async move {
                if try_tauri_save(&wav_data, &filename).await {
                    state.files.update(|files| {
                        if let Some(f) = files.get_mut(file_index) {
                            f.is_recording = false;
                        }
                    });
                }
            });
        }
    }

    // Zoom to fit the entire recording
    let canvas_w = state.spectrogram_canvas_width.get_untracked();
    let final_time_res = 512.0 / sample_rate as f64;
    state.zoom_level.set(crate::viewport::fit_zoom(canvas_w, final_time_res, duration_secs));
    state.scroll_offset.set(0.0);

    spawn_spectrogram_computation(audio_for_stft, name_check, file_index, state);
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

        const FFT_SIZE: usize = 2048;
        const HOP_SIZE: usize = 512;
        const CHUNK_COLS: usize = 32;

        let total_cols = if audio.samples.len() >= FFT_SIZE {
            (audio.samples.len() - FFT_SIZE) / HOP_SIZE + 1
        } else {
            0
        };

        use crate::canvas::spectral_store;
        use crate::canvas::tile_cache::{self, TILE_COLS};

        // Initialise spectral store for progressive tile generation
        spectral_store::init(file_index, total_cols, FFT_SIZE);

        let n_tiles = total_cols.div_ceil(TILE_COLS);
        let mut tile_scheduled = vec![false; n_tiles];
        let mut chunk_start = 0;

        while chunk_start < total_cols {
            let still_present = state.files.get_untracked()
                .get(file_index)
                .map(|f| f.name == name_check)
                .unwrap_or(false);
            if !still_present {
                spectral_store::clear_file(file_index);
                return;
            }

            let chunk = compute_spectrogram_partial(
                &audio,
                FFT_SIZE,
                HOP_SIZE,
                chunk_start,
                CHUNK_COLS,
            );

            // Insert into spectral store for progressive tile generation
            spectral_store::insert_columns(file_index, chunk_start, &chunk);

            // Check if any tile is now complete and render it synchronously
            // (must be sync — async schedule_tile_from_store races with drain_columns below)
            let first_tile = chunk_start / TILE_COLS;
            let last_tile = ((chunk_start + chunk.len()).saturating_sub(1)) / TILE_COLS;
            let mut any_tile_rendered = false;
            let tile_end_idx = last_tile.min(n_tiles.saturating_sub(1));
            for (tile_idx, scheduled) in tile_scheduled.iter_mut().enumerate().take(tile_end_idx + 1).skip(first_tile) {
                if *scheduled { continue; }
                let tile_start = tile_idx * TILE_COLS;
                let tile_end = (tile_start + TILE_COLS).min(total_cols);
                if spectral_store::tile_complete(file_index, tile_start, tile_end) {
                    if tile_cache::render_tile_from_store_sync(file_index, tile_idx, FFT_SIZE) {
                        any_tile_rendered = true;
                    }
                    *scheduled = true;
                }
            }
            if any_tile_rendered {
                state.tile_ready_signal.update(|n| *n = n.wrapping_add(1));
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
        let final_columns = spectral_store::drain_columns(file_index)
            .unwrap_or_default();

        let freq_resolution = audio.sample_rate as f64 / FFT_SIZE as f64;
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

        state.files.update(|files| {
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
        let file_for_tiles = state.files.get_untracked().get(file_index).cloned();
        if let Some(file) = file_for_tiles {
            tile_cache::schedule_all_tiles(state, file, file_index);
        }

        state.tile_ready_signal.update(|n| *n += 1);
    });
}
