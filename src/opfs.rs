use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{FileSystemDirectoryHandle, FileSystemFileHandle, FileSystemWritableFileStream, WritableStream};

const OPFS_DIR: &str = "batmonic-annotations";

/// Get the OPFS batmonic-annotations directory, creating it if needed.
async fn get_opfs_dir() -> Result<FileSystemDirectoryHandle, String> {
    let window = web_sys::window().ok_or("no window")?;
    let navigator = window.navigator();
    let storage = navigator.storage();
    let root: FileSystemDirectoryHandle = JsFuture::from(storage.get_directory())
        .await
        .map_err(|e| format!("OPFS root: {e:?}"))?
        .unchecked_into();

    let opts = web_sys::FileSystemGetDirectoryOptions::new();
    opts.set_create(true);
    let dir: FileSystemDirectoryHandle =
        JsFuture::from(root.get_directory_handle_with_options(OPFS_DIR, &opts))
            .await
            .map_err(|e| format!("OPFS dir: {e:?}"))?
            .unchecked_into();
    Ok(dir)
}

/// Build a storage key for a file. Uses spot_hash_b3 if available, else legacy_spot_hash, else filename+size.
pub fn opfs_key(identity: &crate::annotations::FileIdentity) -> String {
    if let Some(ref hash) = identity.spot_hash_b3 {
        format!("b3_{}.batm", hash)
    } else if let Some(ref hash) = identity.legacy_spot_hash {
        format!("{}.batm", hash)
    } else {
        opfs_fallback_key(&identity.filename, identity.file_size)
    }
}

/// Build the filename+size fallback key.
fn opfs_fallback_key(filename: &str, file_size: u64) -> String {
    let safe_name: String = filename.chars()
        .map(|c| if c.is_alphanumeric() || c == '.' || c == '-' || c == '_' { c } else { '_' })
        .collect();
    format!("{}_{}.batm", safe_name, file_size)
}

/// Save annotation YAML to OPFS.
pub async fn opfs_save(key: &str, yaml: &str) -> Result<(), String> {
    let dir = get_opfs_dir().await?;

    let opts = web_sys::FileSystemGetFileOptions::new();
    opts.set_create(true);
    let file_handle: FileSystemFileHandle =
        JsFuture::from(dir.get_file_handle_with_options(key, &opts))
            .await
            .map_err(|e| format!("OPFS get file: {e:?}"))?
            .unchecked_into();

    let writable: FileSystemWritableFileStream =
        JsFuture::from(file_handle.create_writable())
            .await
            .map_err(|e| format!("OPFS create writable: {e:?}"))?
            .unchecked_into();

    JsFuture::from(
        writable.write_with_str(yaml).map_err(|e| format!("OPFS write: {e:?}"))?,
    )
    .await
    .map_err(|e| format!("OPFS write await: {e:?}"))?;

    let ws: &WritableStream = writable.unchecked_ref();
    JsFuture::from(ws.close())
        .await
        .map_err(|e| format!("OPFS close: {e:?}"))?;

    Ok(())
}

/// Load annotation YAML from OPFS. Returns None if file doesn't exist.
pub async fn opfs_load(key: &str) -> Result<Option<String>, String> {
    let dir = get_opfs_dir().await?;

    // Try to get file handle without create — returns error if not found
    let file_handle_result = JsFuture::from(dir.get_file_handle(key)).await;
    let file_handle: FileSystemFileHandle = match file_handle_result {
        Ok(h) => h.unchecked_into(),
        Err(_) => return Ok(None), // file doesn't exist
    };

    let file: web_sys::File = JsFuture::from(file_handle.get_file())
        .await
        .map_err(|e| format!("OPFS get file: {e:?}"))?
        .unchecked_into();

    let text = JsFuture::from(file.text())
        .await
        .map_err(|e| format!("OPFS read text: {e:?}"))?;

    Ok(text.as_string())
}

/// Delete a file from OPFS by key.
pub async fn opfs_delete(key: &str) -> Result<(), String> {
    let dir = get_opfs_dir().await?;
    JsFuture::from(dir.remove_entry(key))
        .await
        .map_err(|e| format!("OPFS delete: {e:?}"))?;
    Ok(())
}

/// Load and parse a .batm sidecar by its OPFS key.
pub async fn load_batm_by_key(key: &str) -> Result<Option<crate::annotations::AnnotationSet>, String> {
    match opfs_load(key).await? {
        Some(yaml) => {
            let set: crate::annotations::AnnotationSet = yaml_serde::from_str(&yaml)
                .map_err(|e| format!("YAML parse: {e}"))?;
            Ok(Some(set))
        }
        None => Ok(None),
    }
}

/// Build a NoiseProfile from current app state, or None if there's nothing to save.
fn sync_noise_profile_from_state(state: crate::state::AppState) -> Option<crate::dsp::notch::NoiseProfile> {
    use leptos::prelude::GetUntracked;

    let bands = state.notch_bands.get_untracked();
    let noise_floor = state.noise_reduce_floor.get_untracked();
    if bands.is_empty() && noise_floor.is_none() {
        return None;
    }

    let files = state.files.get_untracked();
    let idx = state.current_file_index.get_untracked();
    let sample_rate = idx
        .and_then(|i| files.get(i))
        .map(|f| f.audio.sample_rate)
        .unwrap_or(0);

    let name = state.notch_profile_name.get_untracked();
    let profile_name = if name.is_empty() {
        idx.and_then(|i| files.get(i))
            .map(|f| {
                let base = f.name.rsplit('/').next().unwrap_or(&f.name);
                let base = base.rsplit('\\').next().unwrap_or(base);
                base.rsplit_once('.').map(|(n, _)| n).unwrap_or(base).to_string()
            })
            .unwrap_or_else(|| "Noise Profile".to_string())
    } else {
        name
    };

    Some(crate::dsp::notch::NoiseProfile {
        name: profile_name,
        bands,
        source_sample_rate: sample_rate,
        created: crate::annotations::now_iso8601(),
        noise_floor,
        harmonic_suppression: state.notch_harmonic_suppression.get_untracked(),
    })
}

/// Save annotations for a specific file index (OPFS on browser, central store on Tauri).
/// Respects read_only (skips all saves) and had_sidecar (only writes file-adjacent if it existed).
pub fn save_annotations(state: crate::state::AppState, file_idx: usize) {
    use leptos::prelude::{GetUntracked, Update, WithUntracked};

    // Check read_only flag — skip all saves
    let (read_only, had_sidecar) = state.files.with_untracked(|files: &Vec<crate::state::LoadedFile>| {
        files.get(file_idx)
            .map(|f| (f.read_only, f.had_sidecar))
            .unwrap_or((false, false))
    });
    if read_only {
        return;
    }

    // Sync noise profile and touch modified_at before saving
    state.annotation_store.update(|store| {
        if let Some(Some(ref mut set)) = store.sets.get_mut(file_idx) {
            // Capture current NR state into the sidecar
            set.noise_profile = sync_noise_profile_from_state(state);
            set.touch();
        }
    });

    let store = state.annotation_store.get_untracked();
    let set = match store.sets.get(file_idx).and_then(|s| s.as_ref()) {
        Some(s) => s.clone(),
        None => return,
    };

    let key = opfs_key(&set.file_identity);
    let yaml = match yaml_serde::to_string(&set) {
        Ok(y) => y,
        Err(e) => {
            log::warn!("Annotation serialize error: {e}");
            return;
        }
    };

    if state.is_tauri {
        // Tauri: always save to central annotations directory
        wasm_bindgen_futures::spawn_local(async move {
            if let Err(e) = tauri_save_central(&key, &yaml).await {
                log::warn!("Tauri central save error: {e}");
            } else {
                log::debug!("Tauri saved annotations: {key}");
            }
            // Only auto-save file-adjacent sidecar if one already existed on load
            if had_sidecar {
                if let Some(ref path) = set.file_identity.file_path {
                    if let Err(e) = tauri_save_sidecar(path, &yaml).await {
                        log::debug!("Tauri sidecar save skipped for {path}: {e}");
                    } else {
                        log::debug!("Tauri saved sidecar: {path}.batm");
                    }
                }
            }
        });
    } else {
        // Browser: save to OPFS
        wasm_bindgen_futures::spawn_local(async move {
            if let Err(e) = opfs_save(&key, &yaml).await {
                log::warn!("OPFS save error: {e}");
            } else {
                log::debug!("OPFS saved annotations: {key}");
            }
        });
    }
}

/// Explicitly save a file-adjacent .batm sidecar (Tauri only).
/// Called when user clicks "Save .batm sidecar". Sets had_sidecar so future auto-saves update it.
pub fn save_sidecar_explicit(state: crate::state::AppState, file_idx: usize) {
    use leptos::prelude::{GetUntracked, Update};

    // Sync noise profile and touch modified_at
    state.annotation_store.update(|store| {
        if let Some(Some(ref mut set)) = store.sets.get_mut(file_idx) {
            set.noise_profile = sync_noise_profile_from_state(state);
            set.touch();
        }
    });

    let store = state.annotation_store.get_untracked();
    let set = match store.sets.get(file_idx).and_then(|s| s.as_ref()) {
        Some(s) => s.clone(),
        None => { state.show_error_toast("No annotations to save"); return; }
    };

    let path = match set.file_identity.file_path {
        Some(ref p) => p.clone(),
        None => { state.show_error_toast("No file path — file was not opened from disk"); return; }
    };

    let yaml = match yaml_serde::to_string(&set) {
        Ok(y) => y,
        Err(e) => { state.show_error_toast(format!("Serialize error: {e}")); return; }
    };

    // Mark had_sidecar so future auto-saves keep updating it
    state.files.update(|files| {
        if let Some(f) = files.get_mut(file_idx) {
            f.had_sidecar = true;
        }
    });

    wasm_bindgen_futures::spawn_local(async move {
        match tauri_save_sidecar(&path, &yaml).await {
            Ok(()) => state.show_info_toast(format!("Saved {path}.batm")),
            Err(e) => state.show_error_toast(format!("Sidecar save failed: {e}")),
        }
    });
}

/// Legacy alias — use `save_annotations` instead.
pub fn save_annotations_to_opfs(state: crate::state::AppState, file_idx: usize) {
    save_annotations(state, file_idx);
}

/// Apply a loaded sidecar to the annotation store and restore NR profile to file settings.
fn apply_loaded_sidecar(state: crate::state::AppState, file_idx: usize, loaded: crate::annotations::AnnotationSet) {
    use leptos::prelude::Update;

    // If the sidecar has a noise profile, store it in the file's per-file settings
    if let Some(ref profile) = loaded.noise_profile {
        state.files.update(|files| {
            if let Some(f) = files.get_mut(file_idx) {
                f.settings.notch_bands = profile.bands.clone();
                f.settings.notch_profile_name = profile.name.clone();
                f.settings.notch_harmonic_suppression = profile.harmonic_suppression;
                if !profile.bands.is_empty() {
                    f.settings.notch_enabled = true;
                }
                if let Some(ref floor) = profile.noise_floor {
                    f.settings.noise_reduce_floor = Some(floor.clone());
                    f.settings.noise_reduce_enabled = true;
                }
            }
        });
    }

    state.annotation_store.update(|store| {
        store.ensure_len(file_idx + 1);
        store.sets[file_idx] = Some(loaded);
    });
}

/// Try to load annotations for a file (OPFS on browser, central store + sidecar on Tauri).
/// If found, merges into the annotation store at the given index.
pub fn load_annotations(state: crate::state::AppState, file_idx: usize, identity: crate::annotations::FileIdentity) {
    if state.is_tauri {
        load_annotations_tauri(state, file_idx, identity);
    } else {
        load_annotations_opfs(state, file_idx, identity);
    }
}

/// Legacy alias — use `load_annotations` instead.
pub fn load_annotations_from_opfs(state: crate::state::AppState, file_idx: usize, identity: crate::annotations::FileIdentity) {
    load_annotations(state, file_idx, identity);
}

/// Browser: try OPFS with fallback key chain.
fn load_annotations_opfs(state: crate::state::AppState, file_idx: usize, identity: crate::annotations::FileIdentity) {
    use leptos::prelude::GetUntracked;

    let key = opfs_key(&identity);

    wasm_bindgen_futures::spawn_local(async move {
        // Build fallback key chain: try primary key, then legacy spot_hash, then filename+size
        let mut keys_to_try = vec![key.clone()];
        // If we searched by b3 key, also try old SHA-256 spot_hash key
        if identity.spot_hash_b3.is_some() {
            if let Some(ref legacy) = identity.legacy_spot_hash {
                let legacy_key = format!("{}.batm", legacy);
                if legacy_key != key {
                    keys_to_try.push(legacy_key);
                }
            }
        }
        // Always try filename+size fallback
        let fallback = opfs_fallback_key(&identity.filename, identity.file_size);
        if !keys_to_try.contains(&fallback) {
            keys_to_try.push(fallback);
        }

        for (i, try_key) in keys_to_try.iter().enumerate() {
            match opfs_load(try_key).await {
                Ok(Some(yaml)) => {
                    match yaml_serde::from_str::<crate::annotations::AnnotationSet>(&yaml) {
                        Ok(loaded) => {
                            let already_has = state.annotation_store.get_untracked()
                                .sets.get(file_idx)
                                .and_then(|s| s.as_ref())
                                .is_some();
                            if !already_has {
                                apply_loaded_sidecar(state, file_idx, loaded);
                                log::debug!("OPFS loaded annotations for file {file_idx}: {try_key}");
                                // If found via fallback key, re-save under primary key
                                if i > 0 {
                                    save_annotations(state, file_idx);
                                }
                            }
                        }
                        Err(e) => log::warn!("OPFS deserialize error for {try_key}: {e}"),
                    }
                    return; // Found it, stop trying
                }
                Ok(None) => {} // Not found, try next key
                Err(e) => log::warn!("OPFS load error for {try_key}: {e}"),
            }
        }
    });
}

/// Tauri: try central annotations store, then file-adjacent sidecar.
/// Also probes for file-adjacent sidecar existence and sets `had_sidecar` flag.
fn load_annotations_tauri(state: crate::state::AppState, file_idx: usize, identity: crate::annotations::FileIdentity) {
    use leptos::prelude::{GetUntracked, Update};

    let key = opfs_key(&identity);
    let file_path = identity.file_path.clone();

    wasm_bindgen_futures::spawn_local(async move {
        // Read file-adjacent sidecar once (if path known) for both existence check and fallback
        let sidecar_yaml = if let Some(ref path) = file_path {
            match tauri_load_sidecar(path).await {
                Ok(yaml) => yaml,
                Err(e) => { log::debug!("Tauri sidecar probe for {path}: {e}"); None }
            }
        } else {
            None
        };
        if sidecar_yaml.is_some() {
            state.files.update(|files| {
                if let Some(f) = files.get_mut(file_idx) {
                    f.had_sidecar = true;
                }
            });
        }

        // Try central annotations store first
        let mut keys_to_try = vec![key.clone()];
        let fallback = opfs_fallback_key(&identity.filename, identity.file_size);
        if fallback != key {
            keys_to_try.push(fallback);
        }

        for (i, try_key) in keys_to_try.iter().enumerate() {
            match tauri_load_central(try_key).await {
                Ok(Some(yaml)) => {
                    match yaml_serde::from_str::<crate::annotations::AnnotationSet>(&yaml) {
                        Ok(loaded) => {
                            let already_has = state.annotation_store.get_untracked()
                                .sets.get(file_idx)
                                .and_then(|s| s.as_ref())
                                .is_some();
                            if !already_has {
                                apply_loaded_sidecar(state, file_idx, loaded);
                                log::debug!("Tauri loaded central annotations for file {file_idx}: {try_key}");
                                if i > 0 {
                                    save_annotations(state, file_idx);
                                }
                            }
                        }
                        Err(e) => log::warn!("Tauri central deserialize error for {try_key}: {e}"),
                    }
                    return;
                }
                Ok(None) => {}
                Err(e) => log::warn!("Tauri central load error for {try_key}: {e}"),
            }
        }

        // Fall back to cached sidecar content
        if let Some(yaml) = sidecar_yaml {
            let path = file_path.as_ref().unwrap();
            match yaml_serde::from_str::<crate::annotations::AnnotationSet>(&yaml) {
                Ok(loaded) => {
                    let already_has = state.annotation_store.get_untracked()
                        .sets.get(file_idx)
                        .and_then(|s| s.as_ref())
                        .is_some();
                    if !already_has {
                        apply_loaded_sidecar(state, file_idx, loaded);
                        log::debug!("Tauri loaded sidecar for file {file_idx}: {path}.batm");
                        // Re-save to central store so it's found faster next time
                        save_annotations(state, file_idx);
                    }
                }
                Err(e) => log::warn!("Tauri sidecar deserialize error: {e}"),
            }
        }
    });
}

// ── Tauri IPC helpers for annotation persistence ──────────────────────

/// Save annotations to the Tauri central annotations directory.
async fn tauri_save_central(file_key: &str, yaml: &str) -> Result<(), String> {
    let args = js_sys::Object::new();
    js_sys::Reflect::set(&args, &wasm_bindgen::JsValue::from_str("fileKey"), &wasm_bindgen::JsValue::from_str(file_key))
        .map_err(|e| format!("set fileKey: {e:?}"))?;
    js_sys::Reflect::set(&args, &wasm_bindgen::JsValue::from_str("yaml"), &wasm_bindgen::JsValue::from_str(yaml))
        .map_err(|e| format!("set yaml: {e:?}"))?;
    crate::tauri_bridge::tauri_invoke("write_central_annotations", &args.into()).await?;
    Ok(())
}

/// Load annotations from the Tauri central annotations directory.
async fn tauri_load_central(file_key: &str) -> Result<Option<String>, String> {
    let args = js_sys::Object::new();
    js_sys::Reflect::set(&args, &wasm_bindgen::JsValue::from_str("fileKey"), &wasm_bindgen::JsValue::from_str(file_key))
        .map_err(|e| format!("set fileKey: {e:?}"))?;
    let result = crate::tauri_bridge::tauri_invoke("read_central_annotations", &args.into()).await?;
    if result.is_null() || result.is_undefined() {
        Ok(None)
    } else {
        Ok(result.as_string())
    }
}

/// Save a file-adjacent sidecar via Tauri IPC.
async fn tauri_save_sidecar(path: &str, yaml: &str) -> Result<(), String> {
    let args = js_sys::Object::new();
    js_sys::Reflect::set(&args, &wasm_bindgen::JsValue::from_str("path"), &wasm_bindgen::JsValue::from_str(path))
        .map_err(|e| format!("set path: {e:?}"))?;
    js_sys::Reflect::set(&args, &wasm_bindgen::JsValue::from_str("yaml"), &wasm_bindgen::JsValue::from_str(yaml))
        .map_err(|e| format!("set yaml: {e:?}"))?;
    crate::tauri_bridge::tauri_invoke("write_sidecar", &args.into()).await?;
    Ok(())
}

/// Load a file-adjacent sidecar via Tauri IPC.
async fn tauri_load_sidecar(path: &str) -> Result<Option<String>, String> {
    let args = js_sys::Object::new();
    js_sys::Reflect::set(&args, &wasm_bindgen::JsValue::from_str("path"), &wasm_bindgen::JsValue::from_str(path))
        .map_err(|e| format!("set path: {e:?}"))?;
    let result = crate::tauri_bridge::tauri_invoke("read_sidecar", &args.into()).await?;
    if result.is_null() || result.is_undefined() {
        Ok(None)
    } else {
        Ok(result.as_string())
    }
}
