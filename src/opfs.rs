use crate::state::store_fields::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    FileSystemDirectoryHandle, FileSystemFileHandle, FileSystemWritableFileStream, WritableStream,
};

const OPFS_DIR: &str = "oversample-annotations";

/// Get the OPFS oversample-annotations directory, creating it if needed.
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
    let safe_name: String = filename
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '.' || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
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

    let writable: FileSystemWritableFileStream = JsFuture::from(file_handle.create_writable())
        .await
        .map_err(|e| format!("OPFS create writable: {e:?}"))?
        .unchecked_into();

    JsFuture::from(
        writable
            .write_with_str(yaml)
            .map_err(|e| format!("OPFS write: {e:?}"))?,
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
pub async fn load_batm_by_key(
    key: &str,
) -> Result<Option<crate::annotations::AnnotationSet>, String> {
    match opfs_load(key).await? {
        Some(yaml) => {
            let set: crate::annotations::AnnotationSet =
                yaml_serde::from_str(&yaml).map_err(|e| format!("YAML parse: {e}"))?;
            set.warn_if_future_version();
            Ok(Some(set))
        }
        None => Ok(None),
    }
}

/// Build a NoiseProfile from current app state, or None if there's nothing to save.
fn sync_noise_profile_from_state(
    state: crate::state::AppState,
) -> Option<crate::dsp::notch::NoiseProfile> {
    use leptos::prelude::GetUntracked;

    let bands = state.notch.bands().get_untracked();
    let noise_floor = state.noise_reduce.floor().get_untracked();
    if bands.is_empty() && noise_floor.is_none() {
        return None;
    }

    let files = state.library.files().get_untracked();
    let idx = state.library.current_index().get_untracked();
    let sample_rate = idx
        .and_then(|i| files.get(i))
        .map(|f| f.audio.sample_rate)
        .unwrap_or(0);

    let name = state.notch.profile_name().get_untracked();
    let profile_name = if name.is_empty() {
        idx.and_then(|i| files.get(i))
            .map(|f| {
                let base = f.name.rsplit('/').next().unwrap_or(&f.name);
                let base = base.rsplit('\\').next().unwrap_or(base);
                base.rsplit_once('.')
                    .map(|(n, _)| n)
                    .unwrap_or(base)
                    .to_string()
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
        harmonic_suppression: state.notch.harmonic_suppression().get_untracked(),
    })
}

/// Produce YAML for a file-adjacent sidecar with `file_path` stripped from the identity.
/// The sidecar sits next to the audio file, so the full path is redundant (and `filename` already
/// stores the basename). Central annotations keep the full path for re-finding.
fn sidecar_yaml_without_full_path(set: &crate::annotations::AnnotationSet) -> String {
    let mut sidecar_set = set.clone();
    sidecar_set.file_identity.file_path = None;
    yaml_serde::to_string(&sidecar_set).unwrap_or_default()
}

/// Save annotations for a specific file index (OPFS on browser, central store on Tauri).
/// Respects read_only (skips all saves) and had_sidecar (only writes file-adjacent if it existed).
pub fn save_annotations(state: crate::state::AppState, file_idx: usize) {
    use leptos::prelude::{GetUntracked, Update, WithUntracked};

    // Check read_only flag — skip all saves
    let (read_only, had_sidecar) =
        state
            .library
            .files()
            .with_untracked(|files: &Vec<crate::state::LoadedFile>| {
                files
                    .get(file_idx)
                    .map(|f| (f.read_only, f.had_sidecar))
                    .unwrap_or((false, false))
            });
    if read_only {
        return;
    }

    // Resolve the in-memory list index to the file's stable id (untracked).
    let file_id = match state.file_id_at(file_idx) {
        Some(id) => id,
        None => return,
    };

    // Sync file identity, noise profile, and touch modified_at before saving
    state.annotations.store().update(|store| {
        if let Some(set) = store.get_mut(file_id) {
            // Sync file identity from the LoadedFile (may have been updated after AnnotationSet creation)
            if let Some(id) = state
                .library
                .files()
                .with_untracked(|files| files.get(file_idx).and_then(|f| f.identity.clone()))
            {
                set.file_identity = id;
            }
            // Capture current NR state into the sidecar
            set.noise_profile = sync_noise_profile_from_state(state);
            set.touch();
        }
    });

    let store = state.annotations.store().get_untracked();
    let set = match store.get(file_id) {
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
                    let sidecar_yaml = sidecar_yaml_without_full_path(&set);
                    if let Err(e) = tauri_save_sidecar(path, &sidecar_yaml).await {
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
    use leptos::prelude::{GetUntracked, Update, WithUntracked};

    // Get the file path from LoadedFile.identity (authoritative source)
    let file_path = state.library.files().with_untracked(|files| {
        files
            .get(file_idx)
            .and_then(|f| f.identity.as_ref().and_then(|id| id.file_path.clone()))
    });
    let path = match file_path {
        Some(p) => p,
        None => {
            state.show_error_toast("No file path — file was not opened from disk");
            return;
        }
    };

    // Resolve the in-memory list index to the file's stable id (untracked).
    let file_id = match state.file_id_at(file_idx) {
        Some(id) => id,
        None => {
            state.show_error_toast("Failed to create annotation set");
            return;
        }
    };

    // Ensure an AnnotationSet exists, creating one if needed
    state.annotations.store().update(|store| {
        // Build a fresh set from the LoadedFile only if one isn't already stored.
        let new_set = if store.contains(file_id) {
            None
        } else {
            state.library.files().with_untracked(|files| {
                files.get(file_idx).map(|f| {
                    let id = f.identity.clone().unwrap_or_else(|| {
                        crate::file_identity::identity_layer1(
                            &f.name,
                            f.audio.metadata.file_size as u64,
                        )
                    });
                    crate::annotations::AnnotationSet::new_with_metadata(
                        id,
                        &f.audio,
                        f.cached_peak_db,
                        f.cached_full_peak_db,
                    )
                })
            })
        };
        if let Some(set) = new_set {
            store.insert(file_id, set);
        }
        // Sync file identity and noise profile, touch modified_at
        if let Some(set) = store.get_mut(file_id) {
            if let Some(id) = state
                .library
                .files()
                .with_untracked(|files| files.get(file_idx).and_then(|f| f.identity.clone()))
            {
                set.file_identity = id;
            }
            set.noise_profile = sync_noise_profile_from_state(state);
            set.touch();
        }
    });

    let store = state.annotations.store().get_untracked();
    let set = match store.get(file_id) {
        Some(s) => s.clone(),
        None => {
            state.show_error_toast("Failed to create annotation set");
            return;
        }
    };

    let sidecar_yaml = sidecar_yaml_without_full_path(&set);

    // Mark had_sidecar so future auto-saves keep updating it
    state.library.files().update(|files| {
        if let Some(f) = files.get_mut(file_idx) {
            f.had_sidecar = true;
        }
    });

    wasm_bindgen_futures::spawn_local(async move {
        match tauri_save_sidecar(&path, &sidecar_yaml).await {
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
/// Keyed by the file's STABLE id; the list position is re-resolved here because
/// an async load may have completed after the list changed.
fn apply_loaded_sidecar(
    state: crate::state::AppState,
    file_id: u64,
    loaded: crate::annotations::AnnotationSet,
) {
    use leptos::prelude::Update;

    // Re-resolve the file's CURRENT position from its stable id. If it's gone
    // (closed during the load), drop the loaded set rather than applying it to
    // whatever file now sits at the old index.
    let Some(file_idx) = state.file_idx_for_id(file_id) else {
        return;
    };

    // If the sidecar has a noise profile, store it in the file's per-file settings.
    // Also restore cached peak values from sidecar metadata if not yet computed.
    let has_noise_profile = loaded.noise_profile.is_some();
    let peak_30s = loaded.audio_metadata.as_ref().and_then(|m| m.peak_db_30s);
    let peak_full = loaded.audio_metadata.as_ref().and_then(|m| m.peak_db_full);

    if has_noise_profile || peak_30s.is_some() || peak_full.is_some() {
        state.library.files().update(|files| {
            if let Some(f) = files.get_mut(file_idx) {
                // Restore peak values from sidecar if not yet computed
                if f.cached_peak_db.is_none() {
                    f.cached_peak_db = peak_30s;
                }
                if f.cached_full_peak_db.is_none() {
                    f.cached_full_peak_db = peak_full;
                }
                // Restore noise profile settings
                if let Some(ref profile) = loaded.noise_profile {
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
            }
        });
    }

    state.annotations.store().update(|store| {
        store.insert(file_id, loaded);
    });
}

/// Try to load annotations for a file (OPFS on browser, central store + sidecar on Tauri).
/// If found, merges into the annotation store for that file.
pub fn load_annotations(
    state: crate::state::AppState,
    file_idx: usize,
    identity: crate::annotations::FileIdentity,
) {
    // Bind to the file's STABLE id at call time. The async load below can
    // outlive list changes; resolving the id only at completion would risk
    // applying these annotations to whatever file later occupies `file_idx`.
    let Some(file_id) = state.file_id_at(file_idx) else {
        return;
    };
    if state.is_tauri {
        load_annotations_tauri(state, file_id, identity);
    } else {
        load_annotations_opfs(state, file_id, identity);
    }
}

/// Legacy alias — use `load_annotations` instead.
pub fn load_annotations_from_opfs(
    state: crate::state::AppState,
    file_idx: usize,
    identity: crate::annotations::FileIdentity,
) {
    load_annotations(state, file_idx, identity);
}

/// Browser: try OPFS with fallback key chain.
fn load_annotations_opfs(
    state: crate::state::AppState,
    file_id: u64,
    identity: crate::annotations::FileIdentity,
) {
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
                            loaded.warn_if_future_version();
                            let already_has =
                                state.annotations.store().get_untracked().contains(file_id);
                            if !already_has {
                                apply_loaded_sidecar(state, file_id, loaded);
                                log::debug!(
                                    "OPFS loaded annotations for file id {file_id}: {try_key}"
                                );
                                // If found via fallback key, re-save under primary key
                                if i > 0 {
                                    if let Some(idx) = state.file_idx_for_id(file_id) {
                                        save_annotations(state, idx);
                                    }
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
fn load_annotations_tauri(
    state: crate::state::AppState,
    file_id: u64,
    identity: crate::annotations::FileIdentity,
) {
    use leptos::prelude::{GetUntracked, Update};

    let key = opfs_key(&identity);
    let file_path = identity.file_path.clone();

    wasm_bindgen_futures::spawn_local(async move {
        // Read file-adjacent sidecar once (if path known) for both existence check and fallback
        let sidecar_yaml = if let Some(ref path) = file_path {
            match tauri_load_sidecar(path).await {
                Ok(yaml) => yaml,
                Err(e) => {
                    log::debug!("Tauri sidecar probe for {path}: {e}");
                    None
                }
            }
        } else {
            None
        };
        if sidecar_yaml.is_some() {
            if let Some(idx) = state.file_idx_for_id(file_id) {
                state.library.files().update(|files| {
                    if let Some(f) = files.get_mut(idx) {
                        f.had_sidecar = true;
                    }
                });
            }
        }

        // Don't clobber a store that already has this file (user edits in
        // progress, or a prior load already applied).
        if state.annotations.store().get_untracked().contains(file_id) {
            return;
        }

        // Gather BOTH persisted copies, then apply the NEWER one (by
        // modified_at). Previously central was always tried first and won on any
        // hit, so an externally-newer file-adjacent .batm sidecar was ignored —
        // and the next autosave then overwrote it with the stale central copy,
        // silently losing the external edit. Reconcile by timestamp instead.
        let mut keys_to_try = vec![key.clone()];
        let fallback = opfs_fallback_key(&identity.filename, identity.file_size);
        if fallback != key {
            keys_to_try.push(fallback);
        }

        // Best central copy: first hit across the primary then fallback key.
        let mut central: Option<crate::annotations::AnnotationSet> = None;
        let mut central_via_fallback = false;
        for (i, try_key) in keys_to_try.iter().enumerate() {
            match tauri_load_central(try_key).await {
                Ok(Some(yaml)) => {
                    match yaml_serde::from_str::<crate::annotations::AnnotationSet>(&yaml) {
                        Ok(loaded) => {
                            loaded.warn_if_future_version();
                            central = Some(loaded);
                            central_via_fallback = i > 0;
                        }
                        Err(e) => log::warn!("Tauri central deserialize error for {try_key}: {e}"),
                    }
                    break;
                }
                Ok(None) => {}
                Err(e) => log::warn!("Tauri central load error for {try_key}: {e}"),
            }
        }

        let sidecar = sidecar_yaml.as_ref().and_then(|yaml| {
            match yaml_serde::from_str::<crate::annotations::AnnotationSet>(yaml) {
                Ok(loaded) => {
                    loaded.warn_if_future_version();
                    Some(loaded)
                }
                Err(e) => {
                    log::warn!("Tauri sidecar deserialize error: {e}");
                    None
                }
            }
        });

        // Pick the winner. Central wins ties (equal/absent timestamps) so it
        // stays the canonical source when there's no reason to prefer otherwise.
        let (winner, needs_resave) = match (central, sidecar) {
            (Some(c), Some(s)) => {
                if annotation_is_newer(&s, &c) {
                    (Some(s), true) // sidecar newer → adopt + re-save to central
                } else {
                    (Some(c), central_via_fallback)
                }
            }
            (Some(c), None) => (Some(c), central_via_fallback),
            (None, Some(s)) => (Some(s), true),
            (None, None) => (None, false),
        };

        if let Some(loaded) = winner {
            apply_loaded_sidecar(state, file_id, loaded);
            log::debug!("Tauri loaded annotations for file id {file_id} (resave={needs_resave})");
            // Promote to the primary central key when we loaded from the sidecar
            // or a fallback key, so it's found directly next time.
            if needs_resave {
                if let Some(idx) = state.file_idx_for_id(file_id) {
                    save_annotations(state, idx);
                }
            }
        }
    });
}

/// True if `a` has a strictly newer `modified_at` than `b`. `modified_at` is an
/// ISO-8601 string (lexicographic order == chronological). A set with a
/// timestamp beats one without; absent-vs-absent is "not newer" (caller's tie
/// rule applies).
fn annotation_is_newer(
    a: &crate::annotations::AnnotationSet,
    b: &crate::annotations::AnnotationSet,
) -> bool {
    match (&a.modified_at, &b.modified_at) {
        (Some(ta), Some(tb)) => ta > tb,
        (Some(_), None) => true,
        (None, _) => false,
    }
}

// ── Tauri IPC helpers for annotation persistence ──────────────────────

/// Save annotations to the Tauri central annotations directory.
async fn tauri_save_central(file_key: &str, yaml: &str) -> Result<(), String> {
    let args = js_sys::Object::new();
    js_sys::Reflect::set(
        &args,
        &wasm_bindgen::JsValue::from_str("fileKey"),
        &wasm_bindgen::JsValue::from_str(file_key),
    )
    .map_err(|e| format!("set fileKey: {e:?}"))?;
    js_sys::Reflect::set(
        &args,
        &wasm_bindgen::JsValue::from_str("yaml"),
        &wasm_bindgen::JsValue::from_str(yaml),
    )
    .map_err(|e| format!("set yaml: {e:?}"))?;
    crate::tauri_bridge::tauri_invoke("write_central_annotations", &args.into()).await?;
    Ok(())
}

/// Load annotations from the Tauri central annotations directory.
async fn tauri_load_central(file_key: &str) -> Result<Option<String>, String> {
    let args = js_sys::Object::new();
    js_sys::Reflect::set(
        &args,
        &wasm_bindgen::JsValue::from_str("fileKey"),
        &wasm_bindgen::JsValue::from_str(file_key),
    )
    .map_err(|e| format!("set fileKey: {e:?}"))?;
    let result =
        crate::tauri_bridge::tauri_invoke("read_central_annotations", &args.into()).await?;
    if result.is_null() || result.is_undefined() {
        Ok(None)
    } else {
        Ok(result.as_string())
    }
}

/// Save a file-adjacent sidecar via Tauri IPC.
async fn tauri_save_sidecar(path: &str, yaml: &str) -> Result<(), String> {
    let args = js_sys::Object::new();
    js_sys::Reflect::set(
        &args,
        &wasm_bindgen::JsValue::from_str("path"),
        &wasm_bindgen::JsValue::from_str(path),
    )
    .map_err(|e| format!("set path: {e:?}"))?;
    js_sys::Reflect::set(
        &args,
        &wasm_bindgen::JsValue::from_str("yaml"),
        &wasm_bindgen::JsValue::from_str(yaml),
    )
    .map_err(|e| format!("set yaml: {e:?}"))?;
    crate::tauri_bridge::tauri_invoke("write_sidecar", &args.into()).await?;
    Ok(())
}

/// Load a file-adjacent sidecar via Tauri IPC.
async fn tauri_load_sidecar(path: &str) -> Result<Option<String>, String> {
    let args = js_sys::Object::new();
    js_sys::Reflect::set(
        &args,
        &wasm_bindgen::JsValue::from_str("path"),
        &wasm_bindgen::JsValue::from_str(path),
    )
    .map_err(|e| format!("set path: {e:?}"))?;
    let result = crate::tauri_bridge::tauri_invoke("read_sidecar", &args.into()).await?;
    if result.is_null() || result.is_undefined() {
        Ok(None)
    } else {
        Ok(result.as_string())
    }
}
