use leptos::prelude::*;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use js_sys;
use wasm_bindgen_futures::JsFuture;
use web_sys::{File, FileReader};
use crate::audio::loader::load_audio;
use crate::dsp::fft::{compute_overview_from_spectrogram, compute_preview, compute_spectrogram_partial};
use crate::state::{AppState, FileSettings, LoadedFile};
use crate::types::SpectrogramData;
use std::sync::Arc;

use super::streaming_load::{SilenceCheck, STREAMING_CHECK_SIZE, try_streaming_wav, try_streaming_flac, try_streaming_mp3, try_streaming_ogg, build_streaming_overview};

/// Maximum file size the browser can handle for full in-memory decode (~2 GB).
/// Files above this MUST use the streaming path; if streaming fails, they're rejected.
const MAX_FILE_SIZE: f64 = 2_000_000_000.0;

/// Once the total size of opened files reaches this threshold, prefer streaming
/// for newly opened supported formats to avoid piling up more in-memory decodes.
const TOTAL_OPEN_FILE_STREAMING_THRESHOLD: u64 = 500_000_000;

fn total_open_file_bytes(state: AppState) -> u64 {
    state.files.with_untracked(|files| {
        files.iter()
            .map(|file| file.audio.metadata.file_size as u64)
            .sum()
    })
}

pub(super) async fn read_and_load_file(file: File, state: AppState, load_id: u64) -> Result<(), String> {
    let name = file.name();
    let size = file.size();
    let last_modified_ms = Some(file.last_modified());
    let projected_total_open_bytes = total_open_file_bytes(state).saturating_add(size as u64);
    let force_streaming = projected_total_open_bytes >= TOTAL_OPEN_FILE_STREAMING_THRESHOLD;

    // Helper: set last_modified_ms and compute file identity on the most recently added file
    let name_for_identity = name.clone();
    let finalize_loaded_file = move |state: AppState, lm: Option<f64>| {
        let file_size = size as u64;
        let file_name = name_for_identity.clone();
        state.files.update(|files| {
            if let Some(f) = files.last_mut() {
                f.last_modified_ms = lm;
            }
        });
        // Compute file identity (Layer 1 + Layer 2 async)
        let file_index = state.files.get_untracked().len().saturating_sub(1);
        // Read data_offset/data_size from the loaded file's metadata
        let (data_offset, data_size) = state.files.with_untracked(|files| {
            files.get(file_index)
                .map(|f| (f.audio.metadata.data_offset, f.audio.metadata.data_size))
                .unwrap_or((None, None))
        });
        crate::file_identity::start_identity_computation(
            state, file_index, file_name, file_size, None,
            data_offset, data_size, lm,
        );
    };

    // For large files, or once the workspace has accumulated enough opened
    // files, attempt the streaming path for supported formats.
    if size > STREAMING_CHECK_SIZE || force_streaming {
        state.loading_update(load_id, crate::state::LoadingStage::Streaming);
        match try_streaming_wav(&file, &name, state, force_streaming).await {
            Ok(()) => { finalize_loaded_file(state, last_modified_ms); return Ok(()); }
            Err(e) => {
                log::info!("WAV streaming not applicable for {}: {}", name, e);
            }
        }
        match try_streaming_flac(&file, &name, state, force_streaming).await {
            Ok(()) => { finalize_loaded_file(state, last_modified_ms); return Ok(()); }
            Err(e) => {
                log::info!("FLAC streaming not applicable for {}: {}", name, e);
            }
        }
        match try_streaming_mp3(&file, &name, state, force_streaming).await {
            Ok(()) => { finalize_loaded_file(state, last_modified_ms); return Ok(()); }
            Err(e) => {
                log::info!("MP3 streaming not applicable for {}: {}", name, e);
            }
        }
        match try_streaming_ogg(&file, &name, state, force_streaming).await {
            Ok(()) => { finalize_loaded_file(state, last_modified_ms); return Ok(()); }
            Err(e) => {
                log::info!("OGG streaming not applicable for {}: {}", name, e);
            }
        }
        // Streaming didn't apply — fall through to full decode
        state.loading_update(load_id, crate::state::LoadingStage::Decoding);
    }

    if size > MAX_FILE_SIZE {
        let msg = format!(
            "File too large ({:.1} GB) — only WAV, FLAC, MP3, and OGG files can be streamed above 2 GB",
            size / 1_000_000_000.0
        );
        state.show_error_toast(&msg);
        return Err(msg);
    }
    let bytes = read_file_bytes(&file).await?;
    let result = load_named_bytes(name, &bytes, None, state, load_id).await;
    if result.is_ok() {
        finalize_loaded_file(state, last_modified_ms);
    }
    result
}

pub(crate) async fn load_named_bytes(name: String, bytes: &[u8], xc_metadata: Option<Vec<(String, String)>>, state: AppState, load_id: u64) -> Result<(), String> {
    let audio = load_audio(bytes)?;
    log::info!(
        "Loaded {}: {} samples, {} Hz, {:.2}s",
        name,
        audio.source.total_samples(),
        audio.sample_rate,
        audio.duration_secs
    );

    // Phase 1: fast preview
    state.loading_update(load_id, crate::state::LoadingStage::Preview);
    let preview = compute_preview(&audio, 256, 128);
    let audio_for_stft = audio.clone();
    let name_check = name.clone();

    const HOP_SIZE: usize = 512; // LOD1 hop
    let fft_size: usize = state.spect_fft_mode.get_untracked().fft_for_lod(HOP_SIZE);

    // Check for silent/quiet files — scan first 30s only
    let silence_check = {
        use crate::audio::source::{ChannelView, DEFAULT_ANALYSIS_WINDOW_SECS};
        let total_len = audio.source.total_samples() as usize;
        let scan_end = total_len.min(
            (DEFAULT_ANALYSIS_WINDOW_SECS * audio.sample_rate as f64) as usize,
        );
        let scan_samples = audio.source.read_region(ChannelView::MonoMix, 0, scan_end);
        let peak = scan_samples.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        if peak < 0.002 {
            Some(SilenceCheck::Silent)
        } else if peak > 1e-10 {
            let peak_db = 20.0 * (peak as f64).log10();
            let auto_db = -3.0 - peak_db;
            if auto_db > 30.0 { Some(SilenceCheck::HighGain(auto_db)) } else { None }
        } else {
            None
        }
    };

    let total_len = audio.source.total_samples() as usize;
    let total_cols = if total_len >= fft_size {
        (total_len - fft_size) / HOP_SIZE + 1
    } else {
        0
    };

    let placeholder_spec = SpectrogramData {
        columns: Arc::new(Vec::new()),
        total_columns: total_cols,
        freq_resolution: audio.sample_rate as f64 / fft_size as f64,
        time_resolution: HOP_SIZE as f64 / audio.sample_rate as f64,
        max_freq: audio.sample_rate as f64 / 2.0,
        sample_rate: audio.sample_rate,
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
                xc_metadata,
                is_recording: false,
                settings: FileSettings::default(),
                add_order: idx,
                last_modified_ms: None,
                identity: None,
                file_handle: None,
            });
            if files.len() == 1 {
                state.current_file_index.set(Some(0));
            }
        });
        file_index = idx;
    }

    // Compute file identity (Layer 1 + Layer 2 with bytes available)
    let (data_offset, data_size) = state.files.with_untracked(|files| {
        files.get(file_index)
            .map(|f| (f.audio.metadata.data_offset, f.audio.metadata.data_size))
            .unwrap_or((None, None))
    });
    crate::file_identity::start_identity_computation(
        state, file_index, name_check.clone(), bytes.len() as u64, Some(bytes.to_vec()),
        data_offset, data_size, None,
    );

    // Notify user about silent/quiet files
    if let Some(check) = silence_check {
        match check {
            SilenceCheck::Silent => {
                state.auto_gain.set(false);
                state.gain_db.set(0.0);
                state.show_info_toast("File appears silent \u{2014} auto-gain disabled");
            }
            SilenceCheck::HighGain(db) => {
                state.show_info_toast(format!("Quiet file \u{2014} auto-gain: +{:.0} dB", db));
            }
        }
    }

    // Yield to let the UI render the preview
    let yield_promise = js_sys::Promise::new(&mut |resolve, _| {
        web_sys::window()
            .unwrap()
            .set_timeout_with_callback(&resolve)
            .unwrap();
    });
    JsFuture::from(yield_promise).await.ok();

    // Phase 2: full spectrogram — computed in small chunks so the browser
    // stays responsive.  Chunks are computed viewport-first (expanding
    // outward from the current scroll position) so the visible region
    // appears quickly even for very long files.
    //
    // Columns are inserted into the spectral store as they are computed,
    // and completed TILE_COLS-wide tiles are scheduled for rendering
    // immediately — so the user sees tiles appearing progressively.
    const CHUNK_COLS: usize = 32; // ~50 ms of work per chunk on typical hardware

    // total_cols already computed above for placeholder_spec

    // Initialise the spectral column store for incremental tile generation
    use crate::canvas::spectral_store;
    use crate::canvas::tile_cache::{self, TILE_COLS};
    spectral_store::init(file_index, total_cols, fft_size);

    // Build chunk schedule: viewport-first expanding order
    let time_resolution = HOP_SIZE as f64 / audio_for_stft.sample_rate as f64;
    let scroll = state.scroll_offset.get_untracked();
    let zoom = state.zoom_level.get_untracked();
    let canvas_w = state.spectrogram_canvas_width.get_untracked();
    let visible_time = if zoom > 0.0 { canvas_w / zoom * time_resolution } else { 1.0 };
    let center_col = ((scroll + visible_time / 2.0) / time_resolution) as usize;
    let center_col = center_col.min(total_cols.saturating_sub(1));

    // Generate chunk start indices in expanding-ring order from center
    let total_chunks = (total_cols + CHUNK_COLS - 1) / CHUNK_COLS;
    let center_chunk = center_col / CHUNK_COLS;
    let chunk_order = expanding_chunk_order(center_chunk, total_chunks);

    // Track which tile-width ranges have been fully computed
    let n_tiles = (total_cols + TILE_COLS - 1) / TILE_COLS;
    let mut tile_scheduled = vec![false; n_tiles];

    state.loading_update(load_id, crate::state::LoadingStage::Spectrogram(0));
    let mut chunks_done = 0usize;
    let mut last_reported_pct = 0u16;

    for chunk_idx in chunk_order {
        let chunk_start = chunk_idx * CHUNK_COLS;
        if chunk_start >= total_cols {
            continue;
        }

        // Check the file is still loaded (user may have removed it)
        let still_present = state.files.get_untracked()
            .get(file_index)
            .map(|f| f.name == name_check)
            .unwrap_or(false);
        if !still_present {
            spectral_store::clear_file(file_index);
            return Ok(());
        }

        let chunk = compute_spectrogram_partial(
            &audio_for_stft,
            fft_size,
            HOP_SIZE,
            chunk_start,
            CHUNK_COLS,
        );

        // Insert into spectral store (updates running max magnitude)
        spectral_store::insert_columns(file_index, chunk_start, &chunk);

        // Check if any tile-width ranges are now complete and render them
        // synchronously — before more insertions can evict the columns.
        let first_affected_tile = chunk_start / TILE_COLS;
        let last_affected_tile = ((chunk_start + chunk.len()).saturating_sub(1)) / TILE_COLS;
        let mut any_tile_rendered = false;
        for tile_idx in first_affected_tile..=last_affected_tile.min(n_tiles.saturating_sub(1)) {
            if tile_scheduled[tile_idx] { continue; }
            let tile_start = tile_idx * TILE_COLS;
            let tile_end = (tile_start + TILE_COLS).min(total_cols);
            if spectral_store::tile_complete(file_index, tile_start, tile_end) {
                if tile_cache::render_tile_from_store_sync(file_index, tile_idx, fft_size) {
                    any_tile_rendered = true;
                }
                tile_scheduled[tile_idx] = true;
            }
        }
        if any_tile_rendered {
            state.tile_ready_signal.update(|n| *n = n.wrapping_add(1));
        }

        // Update loading progress (every ~5%)
        chunks_done += 1;
        let pct = ((chunks_done as f64 / total_chunks as f64) * 100.0) as u16;
        if pct >= last_reported_pct + 5 || chunks_done == total_chunks {
            state.loading_update(load_id, crate::state::LoadingStage::Spectrogram(pct.min(100)));
            last_reported_pct = pct;
        }

        // Yield so the browser can process events / paint between chunks
        let p = js_sys::Promise::new(&mut |resolve, _| {
            web_sys::window().unwrap().set_timeout_with_callback(&resolve).unwrap();
        });
        JsFuture::from(p).await.ok();
    }

    state.loading_update(load_id, crate::state::LoadingStage::Finalizing);

    // Large-file threshold: above this, we keep the spectral store alive and
    // don't assemble a monolithic SpectrogramData (saves hundreds of MB).
    // ~50 000 columns ≈ 5 min @ 44.1 kHz or 2.7 min @ 96 kHz ≈ 200 MB of column data.
    const LARGE_FILE_COLS: usize = 50_000;
    let is_large = total_cols > LARGE_FILE_COLS;

    let freq_resolution = audio_for_stft.sample_rate as f64 / fft_size as f64;
    let max_freq = audio_for_stft.sample_rate as f64 / 2.0;

    if is_large {
        // Large file: keep spectral store alive, don't assemble full column data.
        // Tiles will be computed on-demand from the store (or recomputed from audio).
        log::info!(
            "Large file ({} columns) — keeping spectral store, skipping full assembly",
            total_cols
        );

        // Update metadata without draining columns
        let spectrogram = SpectrogramData {
            columns: Arc::new(Vec::new()),
            total_columns: total_cols,
            freq_resolution,
            time_resolution,
            max_freq,
            sample_rate: audio_for_stft.sample_rate,
        };
        state.files.update(|files| {
            if let Some(f) = files.get_mut(file_index) {
                if f.name == name_check {
                    f.spectrogram = spectrogram;
                }
            }
        });

        // Large non-streaming files also lack an overview — build one in the background
        let name_for_overview = name_check.clone();
        wasm_bindgen_futures::spawn_local(build_streaming_overview(
            state,
            file_index,
            name_for_overview,
        ));
    } else {
        // Small file: drain store and assemble full SpectrogramData.
        // Flow mode and harmonics analysis need full column data.
        let final_columns = spectral_store::drain_columns(file_index)
            .unwrap_or_default();

        let spectrogram = SpectrogramData {
            columns: Arc::new(final_columns),
            total_columns: total_cols,
            freq_resolution,
            time_resolution,
            max_freq,
            sample_rate: audio_for_stft.sample_rate,
        };

        log::info!(
            "Spectrogram: {} columns, freq_res={:.1} Hz, time_res={:.4}s",
            spectrogram.columns.len(),
            spectrogram.freq_resolution,
            spectrogram.time_resolution
        );

        // Compute higher-resolution overview image from the full spectrogram
        let overview_img = compute_overview_from_spectrogram(&spectrogram);

        state.files.update(|files| {
            if let Some(f) = files.get_mut(file_index) {
                if f.name == name_check {
                    f.spectrogram = spectrogram;
                    f.overview_image = overview_img;
                }
            }
        });
    }

    // Re-schedule all tiles with the final (accurate) max magnitude.
    // During progressive loading, early tiles may have used a provisional max;
    // if the final max differs significantly, re-render for consistent brightness.
    // For large files, tiles are computed from the spectral store on-demand.
    if !is_large {
        // Clear stale tiles rendered during progressive loading — they used
        // the spectral store's running max_magnitude at the time of rendering,
        // which grows as louder columns are discovered.  Without clearing,
        // schedule_all_tiles() skips already-cached tiles and they keep their
        // inconsistent normalization (visible as a stepped brightness gradient).
        tile_cache::clear_file(file_index);
        let file_for_tiles = state.files.get_untracked().get(file_index).cloned();
        if let Some(file) = file_for_tiles {
            tile_cache::schedule_all_tiles(state, file, file_index);
        }
    } else {
        // Large files: clear tile cache and re-render with final normalization.
        // During loading, the running max_magnitude grew as louder columns were found,
        // so early tiles used a lower max than late tiles — creating visible brightness
        // discontinuities.  Clearing forces re-rendering with the correct final max.
        // The colormapped preview base layer fills the gaps until new tiles arrive.
        tile_cache::clear_file(file_index);
        tile_cache::schedule_visible_tiles_from_store(state, file_index, total_cols);
    }

    // Signal the spectrogram canvas to repaint with the new data
    state.tile_ready_signal.update(|n| *n = n.wrapping_add(1));

    Ok(())
}

const DEMO_SOUNDS_BASE: &str =
    "https://raw.githubusercontent.com/pengowray/batmonic-demo-sounds/main";

pub(super) async fn fetch_bytes(url: &str) -> Result<Vec<u8>, String> {
    let window = web_sys::window().ok_or("No window")?;
    let resp_value = JsFuture::from(window.fetch_with_str(url))
        .await
        .map_err(|e| format!("fetch error: {e:?}"))?;
    let resp: web_sys::Response = resp_value
        .dyn_into()
        .map_err(|_| "Response cast failed".to_string())?;
    if !resp.ok() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let buf = JsFuture::from(resp.array_buffer().map_err(|e| format!("{e:?}"))?)
        .await
        .map_err(|e| format!("array_buffer: {e:?}"))?;
    let uint8 = js_sys::Uint8Array::new(&buf);
    Ok(uint8.to_vec())
}

async fn fetch_text(url: &str) -> Result<String, String> {
    let window = web_sys::window().ok_or("No window")?;
    let resp_value = JsFuture::from(window.fetch_with_str(url))
        .await
        .map_err(|e| format!("fetch error: {e:?}"))?;
    let resp: web_sys::Response = resp_value
        .dyn_into()
        .map_err(|_| "Response cast failed".to_string())?;
    if !resp.ok() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let text = JsFuture::from(resp.text().map_err(|e| format!("{e:?}"))?)
        .await
        .map_err(|e| format!("text: {e:?}"))?;
    text.as_string().ok_or("Not a string".to_string())
}

fn parse_xc_metadata(json: &serde_json::Value) -> Vec<(String, String)> {
    let mut fields = Vec::new();
    let s = |key: &str| json[key].as_str().unwrap_or("").to_string();

    let en = s("en");
    if !en.is_empty() {
        fields.push(("Species".into(), en));
    }
    let genus = s("gen");
    let sp = s("sp");
    if !genus.is_empty() && !sp.is_empty() {
        fields.push(("Scientific name".into(), format!("{} {}", genus, sp)));
    }
    for (key, label) in [
        ("rec", "Recordist"),
        ("lic", "License"),
        ("attribution", "Attribution"),
        ("cnt", "Country"),
        ("loc", "Location"),
    ] {
        let v = s(key);
        if !v.is_empty() {
            fields.push((label.into(), v));
        }
    }
    let lat = s("lat");
    let lon = s("lon");
    if !lat.is_empty() && !lon.is_empty() {
        fields.push(("Coordinates".into(), format!("{}, {}", lat, lon)));
    }
    for (key, label) in [
        ("date", "Date"),
        ("type", "Sound type"),
        ("q", "Quality"),
        ("url", "URL"),
    ] {
        let v = s(key);
        if !v.is_empty() {
            fields.push((label.into(), v));
        }
    }
    fields
}

#[derive(Clone, Debug)]
pub(crate) struct DemoEntry {
    pub filename: String,
    pub metadata_file: Option<String>,
}

pub(crate) async fn fetch_demo_index() -> Result<Vec<DemoEntry>, String> {
    let index_url = format!("{}/index.json", DEMO_SOUNDS_BASE);
    let index_text = fetch_text(&index_url).await?;
    let index: serde_json::Value =
        serde_json::from_str(&index_text).map_err(|e| format!("index parse: {e}"))?;

    let sounds = index["sounds"]
        .as_array()
        .ok_or("No sounds array in index")?;

    let entries = sounds
        .iter()
        .filter_map(|sound| {
            let filename = sound["filename"].as_str()?.to_string();
            let metadata_file = sound["metadata"].as_str().map(|s| s.to_string());
            Some(DemoEntry { filename, metadata_file })
        })
        .collect();

    Ok(entries)
}

pub(crate) async fn load_single_demo(entry: &DemoEntry, state: AppState, load_id: u64) -> Result<(), String> {
    // Fetch XC metadata sidecar if available
    let xc_metadata = if let Some(meta_file) = &entry.metadata_file {
        let encoded = js_sys::encode_uri_component(meta_file);
        let meta_url = format!(
            "{}/sounds/{}",
            DEMO_SOUNDS_BASE,
            encoded.as_string().unwrap_or_default()
        );
        match fetch_text(&meta_url).await {
            Ok(text) => {
                match serde_json::from_str::<serde_json::Value>(&text) {
                    Ok(json) => Some(parse_xc_metadata(&json)),
                    Err(e) => {
                        log::warn!("Failed to parse XC metadata for {}: {}", entry.filename, e);
                        None
                    }
                }
            }
            Err(e) => {
                log::warn!("Failed to fetch XC metadata for {}: {}", entry.filename, e);
                None
            }
        }
    } else {
        None
    };

    let encoded = js_sys::encode_uri_component(&entry.filename);
    let audio_url = format!(
        "{}/sounds/{}",
        DEMO_SOUNDS_BASE,
        encoded.as_string().unwrap_or_default()
    );
    log::info!("Fetching demo: {}", entry.filename);
    let bytes = fetch_bytes(&audio_url).await?;
    load_named_bytes(entry.filename.clone(), &bytes, xc_metadata, state, load_id).await
}

async fn read_file_bytes(file: &File) -> Result<Vec<u8>, String> {
    let reader = FileReader::new().map_err(|e| format!("FileReader: {e:?}"))?;
    let reader_clone = reader.clone();

    let promise = js_sys::Promise::new(&mut |resolve, reject| {
        let resolve_clone = resolve.clone();
        let reject_clone = reject.clone();

        let onload = Closure::once(move |_: web_sys::Event| {
            resolve_clone.call0(&JsValue::NULL).unwrap();
        });
        let onerror = Closure::once(move |_: web_sys::Event| {
            reject_clone.call0(&JsValue::NULL).unwrap();
        });

        reader_clone.set_onloadend(Some(onload.as_ref().unchecked_ref()));
        reader_clone.set_onerror(Some(onerror.as_ref().unchecked_ref()));

        onload.forget();
        onerror.forget();
    });

    reader
        .read_as_array_buffer(file)
        .map_err(|e| format!("read_as_array_buffer: {e:?}"))?;

    JsFuture::from(promise)
        .await
        .map_err(|e| format!("FileReader await: {e:?}"))?;

    let result = reader.result().map_err(|e| format!("result: {e:?}"))?;
    let array_buffer = result
        .dyn_into::<js_sys::ArrayBuffer>()
        .map_err(|_| "Expected ArrayBuffer".to_string())?;
    let uint8_array = js_sys::Uint8Array::new(&array_buffer);
    Ok(uint8_array.to_vec())
}

/// Generate chunk indices in expanding-ring order from a center chunk.
/// Returns indices: center, center-1, center+1, center-2, center+2, ...
fn expanding_chunk_order(center: usize, total: usize) -> Vec<usize> {
    let mut order = Vec::with_capacity(total);
    if total == 0 {
        return order;
    }
    let center = center.min(total - 1);
    order.push(center);
    let mut dist = 1usize;
    while order.len() < total {
        let left = center.checked_sub(dist);
        let right = center + dist;
        if let Some(l) = left {
            if l < total {
                order.push(l);
            }
        }
        if right < total {
            order.push(right);
        }
        // If both are out of bounds, we're done
        if left.is_none() && right >= total {
            break;
        }
        dist += 1;
    }
    order
}
