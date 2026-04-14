use leptos::prelude::*;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use js_sys;
use wasm_bindgen_futures::JsFuture;
use web_sys::{File, FileReader};
use crate::audio::loader::load_audio;
use crate::dsp::fft::compute_preview;
use crate::canvas::spectral_store;
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
        match try_streaming_wav(&file, &name, state, force_streaming, load_id).await {
            Ok(()) => { finalize_loaded_file(state, last_modified_ms); return Ok(()); }
            Err(e) => {
                log::info!("WAV streaming not applicable for {}: {}", name, e);
            }
        }
        match try_streaming_flac(&file, &name, state, force_streaming, load_id).await {
            Ok(()) => { finalize_loaded_file(state, last_modified_ms); return Ok(()); }
            Err(e) => {
                log::info!("FLAC streaming not applicable for {}: {}", name, e);
            }
        }
        match try_streaming_mp3(&file, &name, state, force_streaming, load_id).await {
            Ok(()) => { finalize_loaded_file(state, last_modified_ms); return Ok(()); }
            Err(e) => {
                log::info!("MP3 streaming not applicable for {}: {}", name, e);
            }
        }
        match try_streaming_ogg(&file, &name, state, force_streaming, load_id).await {
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
    let result = load_named_bytes(name, &bytes, None, None, state, load_id).await;
    if result.is_ok() {
        finalize_loaded_file(state, last_modified_ms);
    }
    result
}

pub(crate) async fn load_named_bytes(name: String, bytes: &[u8], xc_metadata: Option<Vec<(String, String)>>, xc_hashes: Option<crate::state::SidecarHashes>, state: AppState, load_id: u64) -> Result<(), String> {
    let wav_markers = crate::audio::loader::parse_wav_markers(bytes);
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

    const HOP_SIZE: usize = 512; // baseline LOD hop
    let fft_size: usize = state.spect_fft_mode.get_untracked().fft_for_lod(crate::canvas::tile_cache::LOD_BASELINE);

    // Check for silent/quiet files — scan first 30s only
    let (silence_check, cached_peak_db) = {
        use crate::audio::source::{ChannelView, DEFAULT_ANALYSIS_WINDOW_SECS};
        let total_len = audio.source.total_samples() as usize;
        let scan_end = total_len.min(
            (DEFAULT_ANALYSIS_WINDOW_SECS * audio.sample_rate as f64) as usize,
        );
        let scan_samples = audio.source.read_region(ChannelView::MonoMix, 0, scan_end);
        let peak = scan_samples.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        if peak < 0.002 {
            (Some(SilenceCheck::Silent), None)
        } else if peak > 1e-10 {
            let peak_db = 20.0 * (peak as f64).log10();
            let auto_db = -3.0 - peak_db;
            let sc = if auto_db > 30.0 { Some(SilenceCheck::HighGain(auto_db)) } else { None };
            (sc, Some(peak_db))
        } else {
            (None, None)
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
                xc_hashes,
                is_recording: false,
                is_live_listen: false,
                settings: FileSettings::default(),
                add_order: idx,
                last_modified_ms: None,
                identity: None,
                file_handle: None,
                cached_peak_db,
                cached_full_peak_db: None,
                read_only: false,
                had_sidecar: false,
                verify_outcome: crate::state::VerifyOutcome::Pending,
                all_hashes_verified: false,
                wav_markers,
                loading_id: Some(load_id),
            });
            state.current_file_index.set(Some(idx));
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

    // Schedule async full-file peak scan (for files > 30s)
    crate::audio::peak::start_full_peak_scan(state, file_index);

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

    state.loading_update(load_id, crate::state::LoadingStage::Finalizing);

    // Set spectrogram metadata — tiles are computed on-demand by the tile
    // cache (schedule_tile_lod / schedule_tile_on_demand) as the user
    // scrolls, so no upfront full-file STFT is needed.
    let time_resolution = HOP_SIZE as f64 / audio_for_stft.sample_rate as f64;
    let freq_resolution = audio_for_stft.sample_rate as f64 / fft_size as f64;
    let max_freq = audio_for_stft.sample_rate as f64 / 2.0;

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

    // Initialise spectral store so on-demand tile computation can cache
    // STFT columns for chromagram and other consumers.
    spectral_store::init(file_index, total_cols, fft_size);

    // Build waveform overview in the background
    let name_for_overview = name_check.clone();
    wasm_bindgen_futures::spawn_local(build_streaming_overview(
        state,
        file_index,
        name_for_overview,
    ));

    // Signal the spectrogram canvas to start rendering visible tiles on-demand
    state.tile_ready_signal.update(|n| *n = n.wrapping_add(1));

    Ok(())
}

const DEMO_SOUNDS_BASE: &str =
    "https://raw.githubusercontent.com/pengowray/bat-demo-sounds/main";

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

/// Extract hash data from an XC sidecar JSON.
/// Tries `json["_app"]` first (new format), then falls back to top-level keys (legacy).
fn extract_sidecar_hashes(json: &serde_json::Value) -> crate::state::SidecarHashes {
    let src = if json["_app"].is_object() { &json["_app"] } else { json };
    crate::state::SidecarHashes {
        blake3: src["blake3"].as_str().map(|s| s.to_string()),
        sha256: src["sha256"].as_str().map(|s| s.to_string()),
        file_size: src["file_size"].as_u64(),
        spot_hash_b3: src["spot_hash_b3"].as_str().map(|s| s.to_string()),
        content_hash: src["content_hash"].as_str().map(|s| s.to_string()),
        data_offset: src["data_offset"].as_u64(),
        data_size: src["data_size"].as_u64(),
    }
}

#[derive(Clone, Debug)]
pub(crate) struct DemoEntry {
    pub filename: String,
    pub metadata_file: Option<String>,
    /// English common name (e.g. "Pond Myotis")
    pub en: Option<String>,
    /// Scientific name (e.g. "Myotis dasycneme")
    pub species: Option<String>,
    /// XC group if known (e.g. "bats")
    pub group: Option<String>,
}

impl DemoEntry {
    /// Heuristic: is this entry a bat recording?
    /// Checks group field, then falls back to known bat family name patterns in the species.
    pub fn is_bat(&self) -> bool {
        if let Some(grp) = &self.group {
            return grp == "bats";
        }
        // Check filename prefix pattern — bat recordings from XC batch download
        // will have bat species names. We also check known bat families by genus.
        // For demo sounds, we check the metadata species or filename.
        if let Some(sp) = &self.species {
            return is_bat_species(sp);
        }
        false
    }
}

/// Known bat family genera patterns (non-exhaustive but covers XC bat listings).
fn is_bat_species(species: &str) -> bool {
    // If the filename/species contains known bat genera or family indicators
    let lower = species.to_lowercase();
    // Common bat genera/family fragments
    const BAT_HINTS: &[&str] = &[
        "myotis", "eptesicus", "pipistrellus", "nyctalus", "vespertilio",
        "plecotus", "rhinolophus", "hipposideros", "miniopterus", "barbastella",
        "tadarida", "molossus", "pteropus", "rousettus", "nyctimene",
        "austronomus", "chalinolobus", "vespadelus", "scotophilus", "lasiurus",
        "artibeus", "carollia", "desmodus", "glossophaga", "phyllostomus",
        "noctilio", "mormoops", "pteronotus", "emballonura", "taphozous",
        "saccolaimus", "coleura", "nycteris", "megaderma", "rhinopoma",
        "craseonycteris", "thyroptera", "furipterus", "natalus",
    ];
    BAT_HINTS.iter().any(|hint| lower.contains(hint))
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
            let en = sound["en"].as_str().map(|s| s.to_string());
            let species = sound["species"].as_str().map(|s| s.to_string());
            let group = sound["group"].as_str().map(|s| s.to_string());
            Some(DemoEntry { filename, metadata_file, en, species, group })
        })
        .collect();

    Ok(entries)
}

pub(crate) async fn load_single_demo(entry: &DemoEntry, state: AppState, load_id: u64) -> Result<(), String> {
    // Fetch XC metadata sidecar if available
    let (xc_metadata, xc_hashes) = if let Some(meta_file) = &entry.metadata_file {
        let encoded = js_sys::encode_uri_component(meta_file);
        let meta_url = format!(
            "{}/sounds/{}",
            DEMO_SOUNDS_BASE,
            encoded.as_string().unwrap_or_default()
        );
        match fetch_text(&meta_url).await {
            Ok(text) => {
                match serde_json::from_str::<serde_json::Value>(&text) {
                    Ok(json) => {
                        let hashes = extract_sidecar_hashes(&json);
                        let hashes = if hashes.is_empty() { None } else { Some(hashes) };
                        (Some(parse_xc_metadata(&json)), hashes)
                    }
                    Err(e) => {
                        log::warn!("Failed to parse XC metadata for {}: {}", entry.filename, e);
                        (None, None)
                    }
                }
            }
            Err(e) => {
                log::warn!("Failed to fetch XC metadata for {}: {}", entry.filename, e);
                (None, None)
            }
        }
    } else {
        (None, None)
    };

    let encoded = js_sys::encode_uri_component(&entry.filename);
    let audio_url = format!(
        "{}/sounds/{}",
        DEMO_SOUNDS_BASE,
        encoded.as_string().unwrap_or_default()
    );
    log::info!("Fetching demo: {}", entry.filename);
    let bytes = fetch_bytes(&audio_url).await?;
    load_named_bytes(entry.filename.clone(), &bytes, xc_metadata, xc_hashes, state, load_id).await
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
/// Load a file from a native filesystem path (Tauri only).
/// Reads bytes via IPC, decodes in WASM, and stores the original path in FileIdentity.
pub(crate) async fn load_native_file(path: String, state: AppState, load_id: u64) -> Result<(), String> {
    // Extract filename from path
    let name = path.rsplit(['/', '\\']).next().unwrap_or(&path).to_string();

    // Read bytes via Tauri IPC
    state.loading_update(load_id, crate::state::LoadingStage::Decoding);
    let args = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&args, &wasm_bindgen::JsValue::from_str("path"), &wasm_bindgen::JsValue::from_str(&path));
    let result = crate::tauri_bridge::tauri_invoke("read_file_bytes", &args.into()).await?;

    // Convert ArrayBuffer to Vec<u8>
    let array_buffer = result
        .dyn_into::<js_sys::ArrayBuffer>()
        .map_err(|_| "Expected ArrayBuffer from read_file_bytes".to_string())?;
    let uint8 = js_sys::Uint8Array::new(&array_buffer);
    let bytes = uint8.to_vec();

    // Decode and add to state using existing pipeline
    load_named_bytes(name.clone(), &bytes, None, None, state, load_id).await?;

    // The file was just added — set the native path on identity
    let file_index = state.files.get_untracked().len().saturating_sub(1);

    // start_identity_computation was already called inside load_named_bytes.
    // Set the native file_path on the identity so future saves write the sidecar.
    state.files.update(|files| {
        if let Some(f) = files.get_mut(file_index) {
            if let Some(ref mut id) = f.identity {
                id.file_path = Some(path.clone());
            }
        }
    });

    // Also try loading a file-adjacent sidecar (central store was already tried
    // by start_identity_computation, but it didn't have the path at that point).
    let identity = state.files.with_untracked(|files| {
        files.get(file_index).and_then(|f| f.identity.clone())
    });
    if let Some(id) = identity {
        crate::opfs::load_annotations(state, file_index, id);
    }

    Ok(())
}
