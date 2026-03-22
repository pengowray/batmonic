use std::fs;
use std::path::{Path, PathBuf};
use crate::types::{XcGroupTaxonomy, XcRecording};

/// File hashes and size computed from audio bytes.
#[derive(Clone, Debug)]
pub struct FileHashes {
    pub size_bytes: u64,
    pub sha256: String,
    pub blake3: String,
    /// Quick identity hash: BLAKE3 of first 8KB + last 8KB.
    /// For files under 16KB, same as full BLAKE3.
    pub spot_hash: String,
}

/// Sanitize a string for use in filenames.
pub fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            _ => c,
        })
        .collect()
}

/// Build the base filename stem for a recording (no extension).
/// e.g. "XC928094 - Pond Myotis - Myotis dasycneme"
pub fn recording_stem(rec: &XcRecording) -> String {
    sanitize_filename(&format!("XC{} - {} - {} {}", rec.id, rec.en, rec.genus, rec.sp))
}

/// Determine audio file extension from the recording's original filename.
pub fn audio_extension(rec: &XcRecording) -> &str {
    rec.file_name
        .rsplit('.')
        .next()
        .unwrap_or("wav")
}

/// Path to the taxonomy cache file for a group.
pub fn taxonomy_path(root: &Path, group: &str, country: Option<&str>) -> PathBuf {
    let dir = root.join("taxonomy");
    let name = match country {
        Some(cnt) => format!("{}_{}.json", group, sanitize_filename(cnt)),
        None => format!("{}.json", group),
    };
    dir.join(name)
}

/// Load cached taxonomy for a group, if it exists.
pub fn load_taxonomy(root: &Path, group: &str, country: Option<&str>) -> Result<Option<XcGroupTaxonomy>, String> {
    let path = taxonomy_path(root, group, country);
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
    let taxonomy: XcGroupTaxonomy = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse {}: {e}", path.display()))?;
    Ok(Some(taxonomy))
}

/// Save taxonomy cache for a group.
pub fn save_taxonomy(root: &Path, group: &str, country: Option<&str>, data: &XcGroupTaxonomy) -> Result<(), String> {
    let path = taxonomy_path(root, group, country);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create dir {}: {e}", parent.display()))?;
    }
    let json = serde_json::to_string_pretty(data)
        .map_err(|e| format!("Serialize error: {e}"))?;
    fs::write(&path, format!("{json}\n"))
        .map_err(|e| format!("Failed to write {}: {e}", path.display()))?;
    Ok(())
}

/// Check how old the taxonomy cache is (returns human-readable string).
pub fn taxonomy_age_string(root: &Path, group: &str, country: Option<&str>) -> Option<String> {
    let taxonomy = load_taxonomy(root, group, country).ok()??;
    let updated = chrono::DateTime::parse_from_rfc3339(&taxonomy.last_updated).ok()?;
    let now = chrono::Utc::now();
    let duration = now.signed_duration_since(updated);

    let hours = duration.num_hours();
    if hours < 1 {
        Some("just now".to_string())
    } else if hours < 24 {
        Some(format!("{hours} hour{} ago", if hours == 1 { "" } else { "s" }))
    } else {
        let days = duration.num_days();
        Some(format!("{days} day{} ago", if days == 1 { "" } else { "s" }))
    }
}

/// Check if a recording's audio is already cached.
pub fn is_recording_cached(root: &Path, id: u64) -> bool {
    let sounds_dir = root.join("sounds");
    if !sounds_dir.exists() {
        return false;
    }
    // Look for any file starting with "XC{id} -"
    let prefix = format!("XC{id} -");
    if let Ok(entries) = fs::read_dir(&sounds_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with(&prefix) && !name.ends_with(".xc.json") {
                return true;
            }
        }
    }
    false
}

/// Find the cached audio file path for a recording.
pub fn cached_audio_path(root: &Path, id: u64) -> Option<PathBuf> {
    let sounds_dir = root.join("sounds");
    let prefix = format!("XC{id} -");
    if let Ok(entries) = fs::read_dir(&sounds_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with(&prefix) && !name.ends_with(".xc.json") {
                return Some(entry.path());
            }
        }
    }
    None
}

/// Compute hashes and size from audio bytes.
pub fn compute_file_hashes(data: &[u8]) -> FileHashes {
    use sha2::Digest;

    let size_bytes = data.len() as u64;

    // SHA-256
    let sha256 = {
        let mut hasher = sha2::Sha256::new();
        hasher.update(data);
        format!("{:x}", hasher.finalize())
    };

    // BLAKE3 (full file)
    let blake3 = blake3::hash(data).to_hex().to_string();

    // Spot hash: BLAKE3 of first 8KB + last 8KB (quick identity check)
    let spot_hash = if data.len() <= 16384 {
        blake3.clone()
    } else {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&data[..8192]);
        hasher.update(&data[data.len() - 8192..]);
        hasher.finalize().to_hex().to_string()
    };

    FileHashes { size_bytes, sha256, blake3, spot_hash }
}

/// Build the XC metadata sidecar JSON for a recording.
pub fn build_metadata_json(rec: &XcRecording) -> serde_json::Value {
    let attribution = format!(
        "{}, XC{}. Accessible at www.xeno-canto.org/{}",
        rec.rec, rec.id, rec.id
    );
    let now = chrono::Utc::now().format("%Y-%m-%d").to_string();

    serde_json::json!({
        "source": "xeno-canto",
        "xc_id": rec.id,
        "url": format!("https://www.xeno-canto.org/{}", rec.id),
        "file_url": rec.file_url,
        "gen": rec.genus,
        "sp": rec.sp,
        "en": rec.en,
        "rec": rec.rec,
        "cnt": rec.cnt,
        "loc": rec.loc,
        "lat": rec.lat,
        "lon": rec.lon,
        "date": rec.date,
        "time": rec.time,
        "type": rec.sound_type,
        "q": rec.q,
        "length": rec.length,
        "smp": rec.smp.parse::<u64>().ok(),
        "lic": rec.lic,
        "attribution": attribution,
        "retrieved": now,
    })
}

/// Build the XC metadata sidecar JSON for a recording, including file hashes.
pub fn build_metadata_json_with_hashes(rec: &XcRecording, hashes: &FileHashes) -> serde_json::Value {
    let mut json = build_metadata_json(rec);
    if let Some(obj) = json.as_object_mut() {
        obj.insert("file_size".into(), serde_json::json!(hashes.size_bytes));
        obj.insert("sha256".into(), serde_json::json!(hashes.sha256));
        obj.insert("blake3".into(), serde_json::json!(hashes.blake3));
        obj.insert("spot_hash".into(), serde_json::json!(hashes.spot_hash));
    }
    json
}

/// Save a recording's audio and metadata to the cache.
/// Returns the path to the saved audio file.
pub fn save_recording(
    root: &Path,
    rec: &XcRecording,
    audio_bytes: &[u8],
) -> Result<PathBuf, String> {
    // Validate audio bytes before writing anything
    if audio_bytes.is_empty() {
        return Err("Downloaded file is empty".into());
    }
    if audio_bytes.len() < 44 {
        return Err(format!(
            "Downloaded file is too small ({} bytes) — probably not a valid audio file",
            audio_bytes.len()
        ));
    }

    let sounds_dir = root.join("sounds");
    fs::create_dir_all(&sounds_dir)
        .map_err(|e| format!("Failed to create sounds dir: {e}"))?;

    let stem = recording_stem(rec);
    let ext = audio_extension(rec);
    let audio_filename = format!("{stem}.{ext}");
    let meta_filename = format!("{stem}.xc.json");

    // Write audio
    let audio_path = sounds_dir.join(&audio_filename);
    fs::write(&audio_path, audio_bytes)
        .map_err(|e| format!("Failed to write audio: {e}"))?;

    // Write metadata sidecar (with file hashes)
    let meta_path = sounds_dir.join(&meta_filename);
    let hashes = compute_file_hashes(audio_bytes);
    let metadata = build_metadata_json_with_hashes(rec, &hashes);
    let json_str = serde_json::to_string_pretty(&metadata)
        .map_err(|e| format!("Serialize error: {e}"))?;
    fs::write(&meta_path, format!("{json_str}\n"))
        .map_err(|e| format!("Failed to write metadata: {e}"))?;

    // Update index.json (only after audio + metadata written successfully)
    update_index(root, rec, &audio_filename, &meta_filename)?;

    Ok(audio_path)
}

/// Find the cached metadata sidecar path for a recording.
pub fn cached_metadata_path(root: &Path, id: u64) -> Option<PathBuf> {
    let sounds_dir = root.join("sounds");
    let prefix = format!("XC{id} -");
    if let Ok(entries) = fs::read_dir(&sounds_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with(&prefix) && name.ends_with(".xc.json") {
                return Some(entry.path());
            }
        }
    }
    None
}

/// Delete a recording's audio, metadata sidecar, and index entry.
/// Accepts either an XC ID or a filename. Returns names of deleted files.
pub fn delete_recording(root: &Path, id: u64) -> Result<Vec<String>, String> {
    let sounds_dir = root.join("sounds");
    let prefix = format!("XC{id} -");
    let mut deleted = Vec::new();

    if let Ok(entries) = fs::read_dir(&sounds_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with(&prefix) {
                fs::remove_file(entry.path())
                    .map_err(|e| format!("Failed to delete {}: {e}", name))?;
                deleted.push(name);
            }
        }
    }

    if deleted.is_empty() {
        return Err(format!("No cached files found for XC{id}"));
    }

    // Remove from index.json
    remove_from_index(root, id)?;

    Ok(deleted)
}

/// Remove a recording entry from index.json by XC ID.
fn remove_from_index(root: &Path, id: u64) -> Result<(), String> {
    let index_path = root.join("index.json");
    if !index_path.exists() {
        return Ok(());
    }

    let mut index = read_index(root);
    let sounds = index["sounds"]
        .as_array_mut()
        .ok_or("index.json 'sounds' is not an array")?;

    let before = sounds.len();
    sounds.retain(|s| s["xc_id"].as_u64() != Some(id));

    if sounds.len() == before {
        return Ok(()); // wasn't in the index
    }

    let tmp_path = root.join("index.json.tmp");
    let json_str = serde_json::to_string_pretty(&index)
        .map_err(|e| format!("Serialize error: {e}"))?;
    fs::write(&tmp_path, format!("{json_str}\n"))
        .map_err(|e| format!("Failed to write index.json.tmp: {e}"))?;
    fs::rename(&tmp_path, &index_path)
        .map_err(|e| format!("Failed to finalize index.json: {e}"))?;

    Ok(())
}

/// Read and parse the cache index, falling back gracefully on errors.
fn read_index(root: &Path) -> serde_json::Value {
    let index_path = root.join("index.json");
    let tmp_path = root.join("index.json.tmp");

    // Try main index first, then tmp fallback (in case rename didn't complete)
    for path in [&index_path, &tmp_path] {
        if path.exists() {
            if let Ok(content) = fs::read_to_string(path) {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
                    if val["sounds"].is_array() {
                        return val;
                    }
                }
            }
        }
    }

    // Both missing or corrupt — start fresh
    serde_json::json!({ "version": 1, "sounds": [] })
}

/// Update (or create) index.json with a new recording entry.
/// Writes atomically via a temp file to prevent corruption on crash.
fn update_index(
    root: &Path,
    rec: &XcRecording,
    audio_filename: &str,
    meta_filename: &str,
) -> Result<(), String> {
    let index_path = root.join("index.json");
    let tmp_path = root.join("index.json.tmp");

    let mut index = read_index(root);

    let sounds = index["sounds"]
        .as_array_mut()
        .ok_or("index.json 'sounds' is not an array")?;

    // Skip if already present
    if sounds.iter().any(|s| s["xc_id"].as_u64() == Some(rec.id)) {
        return Ok(());
    }

    sounds.push(serde_json::json!({
        "filename": audio_filename,
        "metadata": meta_filename,
        "xc_id": rec.id,
        "en": rec.en,
        "species": format!("{} {}", rec.genus, rec.sp),
        "source": "xeno-canto"
    }));

    let json_str = serde_json::to_string_pretty(&index)
        .map_err(|e| format!("Serialize error: {e}"))?;

    // Write to temp file first, then rename for atomic update
    fs::write(&tmp_path, format!("{json_str}\n"))
        .map_err(|e| format!("Failed to write index.json.tmp: {e}"))?;
    fs::rename(&tmp_path, &index_path)
        .map_err(|e| format!("Failed to finalize index.json: {e}"))?;

    Ok(())
}
