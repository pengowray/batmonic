use crate::types::{XcGroupTaxonomy, XcRecording};
use std::fs;
use std::path::{Path, PathBuf};

/// File hashes and size computed from audio bytes.
#[derive(Clone, Debug)]
pub struct FileHashes {
    pub size_bytes: u64,
    pub sha256: String,
    pub blake3: String,
    /// Multi-point spot hash: BLAKE3 of 16×1MB chunks across the audio data region.
    /// Matches the main app's Layer 2 spot_hash_b3.
    pub spot_hash_b3: String,
    /// Content hash: BLAKE3 of entire file with header bytes zeroed.
    /// Survives metadata/header edits while preserving audio content identity.
    pub content_hash: String,
    /// Audio data region byte offset (WAV: start of data chunk payload).
    pub data_offset: Option<u64>,
    /// Audio data region byte length.
    pub data_size: Option<u64>,
}

/// Hash data extracted from an XC sidecar JSON (stored under "_app" key,
/// with fallback to legacy top-level keys).
///
/// Canonical definition lives in the dependency-light `oversample-ipc` crate so
/// it can be shared with the WASM frontend (which can't depend on `xc-lib`).
pub use oversample_ipc::SidecarHashes;

/// Extract hash data from an XC sidecar JSON.
/// Tries `json["_app"]` first (new format), then falls back to top-level keys (legacy).
pub fn extract_sidecar_hashes(json: &serde_json::Value) -> SidecarHashes {
    let src = if json["_app"].is_object() {
        &json["_app"]
    } else {
        json
    };
    SidecarHashes {
        blake3: src["blake3"].as_str().map(|s| s.to_string()),
        sha256: src["sha256"].as_str().map(|s| s.to_string()),
        file_size: src["file_size"].as_u64(),
        spot_hash_b3: src["spot_hash_b3"].as_str().map(|s| s.to_string()),
        content_hash: src["content_hash"].as_str().map(|s| s.to_string()),
        data_offset: src["data_offset"].as_u64(),
        data_size: src["data_size"].as_u64(),
    }
}

/// Migrate a sidecar JSON from legacy format (hashes at top level) to new format
/// (hashes nested under "_app"). Returns true if migration was performed.
pub fn migrate_sidecar_json(json: &mut serde_json::Value) -> bool {
    // Already migrated?
    if json["_app"].is_object() {
        return false;
    }
    let obj = match json.as_object_mut() {
        Some(o) => o,
        None => return false,
    };

    let hash_keys = [
        "blake3",
        "sha256",
        "file_size",
        "spot_hash",
        "spot_hash_b3",
        "content_hash",
        "data_offset",
        "data_size",
    ];
    let mut app_meta = serde_json::Map::new();
    let mut found_any = false;

    for key in &hash_keys {
        if let Some(val) = obj.remove(*key) {
            app_meta.insert((*key).to_string(), val);
            found_any = true;
        }
    }

    // Also move "retrieved" into the app sub-object (it's our metadata, not XC's)
    if let Some(val) = obj.remove("retrieved") {
        app_meta.insert("retrieved".to_string(), val);
    }

    if found_any || !app_meta.is_empty() {
        obj.insert("_app".to_string(), serde_json::Value::Object(app_meta));
        true
    } else {
        false
    }
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
    sanitize_filename(&format!(
        "XC{} - {} - {} {}",
        rec.id, rec.en, rec.genus, rec.sp
    ))
}

/// Determine audio file extension from the recording's original filename.
pub fn audio_extension(rec: &XcRecording) -> &str {
    rec.file_name.rsplit('.').next().unwrap_or("wav")
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
pub fn load_taxonomy(
    root: &Path,
    group: &str,
    country: Option<&str>,
) -> Result<Option<XcGroupTaxonomy>, String> {
    let path = taxonomy_path(root, group, country);
    if !path.exists() {
        return Ok(None);
    }
    let content =
        fs::read_to_string(&path).map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
    let taxonomy: XcGroupTaxonomy = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse {}: {e}", path.display()))?;
    Ok(Some(taxonomy))
}

/// Save taxonomy cache for a group.
pub fn save_taxonomy(
    root: &Path,
    group: &str,
    country: Option<&str>,
    data: &XcGroupTaxonomy,
) -> Result<(), String> {
    let path = taxonomy_path(root, group, country);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create dir {}: {e}", parent.display()))?;
    }
    let json = serde_json::to_string_pretty(data).map_err(|e| format!("Serialize error: {e}"))?;
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
        Some(format!(
            "{hours} hour{} ago",
            if hours == 1 { "" } else { "s" }
        ))
    } else {
        let days = duration.num_days();
        Some(format!(
            "{days} day{} ago",
            if days == 1 { "" } else { "s" }
        ))
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

/// Detect the audio-sample region of a file by format. Returns
/// `(data_offset, data_size)` such that `data[data_offset..data_offset+data_size]`
/// spans only the audio samples (WAV PCM, MP3 frames, or Ogg pages), excluding
/// the container header and any trailing metadata (e.g. ID3v1/APE/Lyrics3 for
/// MP3, RIFF metadata or GUANO chunks after the `data` chunk for WAV, or
/// garbage after the final Ogg page).
///
/// Returns `(None, None)` for unknown/unsupported formats — callers should then
/// treat the whole file as audio.
pub fn detect_audio_region(data: &[u8]) -> (Option<u64>, Option<u64>) {
    if data.len() >= 12 && &data[0..4] == b"RIFF" && &data[8..12] == b"WAVE" {
        return detect_wav_data_region(data);
    }
    if data.len() >= 4 && &data[0..4] == b"OggS" {
        return detect_ogg_data_region(data);
    }
    if is_mp3(data) {
        return detect_mp3_data_region(data);
    }
    (None, None)
}

/// Magic-byte MP3 detector: ID3v2 tag at start, or a plausible MPEG audio sync.
pub fn is_mp3(data: &[u8]) -> bool {
    if data.len() >= 3 && &data[0..3] == b"ID3" {
        return true;
    }
    // MPEG frame sync: 11 ones in the top bits, and a valid (non-reserved) layer/version.
    data.len() >= 2
        && data[0] == 0xFF
        && (data[1] & 0xE0) == 0xE0
        && (data[1] & 0x18) != 0x08   // not reserved MPEG version
        && (data[1] & 0x06) != 0x00 // not reserved layer
}

/// Size of the ID3v2 tag at `data[0..]`, or 0 if none.
fn id3v2_tag_size(data: &[u8]) -> u64 {
    if data.len() >= 10 && &data[0..3] == b"ID3" {
        let size = ((data[6] as u64 & 0x7F) << 21)
            | ((data[7] as u64 & 0x7F) << 14)
            | ((data[8] as u64 & 0x7F) << 7)
            | (data[9] as u64 & 0x7F);
        10 + size
    } else {
        0
    }
}

/// Detect MP3 audio region, skipping ID3v2 at the start and ID3v1 / APEv2 /
/// Lyrics3v2 at the end.
pub fn detect_mp3_data_region(data: &[u8]) -> (Option<u64>, Option<u64>) {
    let len = data.len();
    let start = id3v2_tag_size(data);
    let mut end = len;

    // ID3v1: trailing 128 bytes starting with "TAG"
    if end >= 128 && &data[end - 128..end - 125] == b"TAG" {
        end -= 128;
    }

    // Lyrics3v2: ends with "LYRICS200" preceded by 6-digit ASCII size
    if end >= 15 && &data[end - 9..end] == b"LYRICS200" {
        if let Ok(s) = std::str::from_utf8(&data[end - 15..end - 9]) {
            if let Ok(sz) = s.parse::<u64>() {
                end = end.saturating_sub((sz as usize) + 15);
            }
        }
    }

    // APEv2/v1 footer: 32-byte footer at the end starting with "APETAGEX".
    // Footer layout: "APETAGEX"(8) | version(4 LE) | tag_size(4 LE: items+footer)
    //              | item_count(4) | flags(4 LE) | reserved(8). Bit 31 of flags
    // indicates a 32-byte header precedes the tag body.
    if end >= 32 && &data[end - 32..end - 24] == b"APETAGEX" {
        let tag_size = u32::from_le_bytes([
            data[end - 20],
            data[end - 19],
            data[end - 18],
            data[end - 17],
        ]) as u64;
        let flags = u32::from_le_bytes([
            data[end - 12],
            data[end - 11],
            data[end - 10],
            data[end - 9],
        ]);
        let has_header = (flags & 0x8000_0000) != 0;
        let total = if has_header { tag_size + 32 } else { tag_size };
        end = end.saturating_sub(total as usize);
    }

    let start = (start as usize).min(end);
    (Some(start as u64), Some((end - start) as u64))
}

/// Detect the region spanned by complete Ogg pages, trimming any trailing
/// garbage after the last fully-valid page.
pub fn detect_ogg_data_region(data: &[u8]) -> (Option<u64>, Option<u64>) {
    if data.len() < 27 || &data[0..4] != b"OggS" {
        return (None, None);
    }
    let mut pos = 0usize;
    let mut last_end = 0usize;
    while pos + 27 <= data.len() && &data[pos..pos + 4] == b"OggS" {
        let n_segs = data[pos + 26] as usize;
        let table_end = pos + 27 + n_segs;
        if table_end > data.len() {
            break;
        }
        let segs_sum: usize = data[pos + 27..table_end].iter().map(|&b| b as usize).sum();
        let page_end = table_end + segs_sum;
        if page_end > data.len() {
            break;
        }
        last_end = page_end;
        pos = page_end;
    }
    if last_end == 0 {
        return (None, None);
    }
    (Some(0), Some(last_end as u64))
}

/// Detect WAV audio data region by scanning for the "data" chunk in a RIFF file.
/// Returns (data_offset, data_size) or (None, None) for non-WAV files.
pub fn detect_wav_data_region(data: &[u8]) -> (Option<u64>, Option<u64>) {
    // Check RIFF/WAVE header
    if data.len() < 44 || &data[0..4] != b"RIFF" || &data[8..12] != b"WAVE" {
        return (None, None);
    }
    // Scan chunks after the 12-byte RIFF header
    let mut pos = 12usize;
    while pos + 8 <= data.len() {
        let chunk_id = &data[pos..pos + 4];
        let chunk_size =
            u32::from_le_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]]) as u64;
        if chunk_id == b"data" {
            let offset = (pos + 8) as u64;
            return (Some(offset), Some(chunk_size));
        }
        // Advance to next chunk (chunks are word-aligned)
        let advance = 8 + chunk_size as usize;
        let advance = (advance + 1) & !1; // word-align
        pos += advance;
    }
    (None, None)
}

/// Size of each chunk for the multi-point spot hash (1 MB).
const SPOT_CHUNK_SIZE: u64 = 1_048_576;
/// Maximum number of chunks for the spot hash.
const NUM_SPOT_CHUNKS: u64 = 16;

/// Compute BLAKE3 multi-point spot hash (16×1MB chunks across audio data region).
/// Matches the algorithm in the main app's file_identity.rs.
pub fn compute_spot_hash_b3(
    data: &[u8],
    data_offset: Option<u64>,
    data_size: Option<u64>,
) -> String {
    let file_size = data.len() as u64;
    let audio_start = data_offset.unwrap_or(0);
    let audio_len = data_size.unwrap_or(file_size.saturating_sub(audio_start));

    if audio_len == 0 {
        return blake3::hash(&[]).to_hex().to_string();
    }

    let num_chunks = NUM_SPOT_CHUNKS.min((audio_len / SPOT_CHUNK_SIZE).max(1));
    let mut chunk_hashes = Vec::with_capacity(num_chunks as usize);

    for i in 0..num_chunks {
        let chunk_start = audio_start + i * (audio_len / num_chunks);
        let remaining = audio_len - (chunk_start - audio_start);
        let chunk_len = SPOT_CHUNK_SIZE.min(remaining);
        let s = chunk_start as usize;
        let e = (chunk_start + chunk_len).min(file_size) as usize;
        chunk_hashes.push(blake3::hash(&data[s..e]));
    }

    // Combine chunk hashes
    let mut combined = Vec::with_capacity(chunk_hashes.len() * 32);
    for h in &chunk_hashes {
        combined.extend_from_slice(h.as_bytes());
    }
    blake3::hash(&combined).to_hex().to_string()
}

/// Compute content hash: BLAKE3 over just the audio samples
/// (`file[data_offset..data_offset+data_size]`). Header and any trailing
/// metadata (e.g. GUANO) are excluded, so metadata edits don't change it.
pub fn compute_content_hash(
    data: &[u8],
    data_offset: Option<u64>,
    data_size: Option<u64>,
) -> String {
    let start = data_offset.unwrap_or(0) as usize;
    let end = match data_size {
        Some(sz) => (start + sz as usize).min(data.len()),
        None => data.len(),
    };
    let start = start.min(end);
    blake3::hash(&data[start..end]).to_hex().to_string()
}

/// Compute hashes and size from audio bytes.
pub fn compute_file_hashes(data: &[u8]) -> FileHashes {
    use sha2::Digest;

    let size_bytes = data.len() as u64;

    // Detect audio-sample region (WAV / MP3 / OGG) for audio-aware hashing
    let (data_offset, data_size) = detect_audio_region(data);

    // SHA-256
    let sha256 = {
        let mut hasher = sha2::Sha256::new();
        hasher.update(data);
        hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    };

    // BLAKE3 (full file)
    let blake3 = blake3::hash(data).to_hex().to_string();

    // Multi-point spot hash (16×1MB chunks, matches main app Layer 2)
    let spot_hash_b3 = compute_spot_hash_b3(data, data_offset, data_size);

    // Content hash (BLAKE3 of audio samples only)
    let content_hash = compute_content_hash(data, data_offset, data_size);

    FileHashes {
        size_bytes,
        sha256,
        blake3,
        spot_hash_b3,
        content_hash,
        data_offset,
        data_size,
    }
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
        "xc_id": rec.id_num(),
        "url": &rec.url,
        "file": &rec.file_url,
        "file-name": &rec.file_name,
        "gen": &rec.genus,
        "sp": &rec.sp,
        "ssp": &rec.ssp,
        "en": &rec.en,
        "grp": &rec.grp,
        "rec": &rec.rec,
        "cnt": &rec.cnt,
        "loc": &rec.loc,
        "lat": &rec.lat,
        "lon": &rec.lon,
        "alt": &rec.alt,
        "type": &rec.sound_type,
        "sex": &rec.sex,
        "stage": &rec.stage,
        "method": &rec.method,
        "date": &rec.date,
        "time": &rec.time,
        "uploaded": &rec.uploaded,
        "also": &rec.also,
        "rmk": &rec.rmk,
        "animal-seen": &rec.animal_seen,
        "playback-used": &rec.playback_used,
        "temp": &rec.temp,
        "regnr": &rec.regnr,
        "auto": &rec.auto_rec,
        "dvc": &rec.dvc,
        "mic": &rec.mic,
        "q": &rec.q,
        "length": &rec.length,
        "smp": rec.smp.parse::<u64>().ok(),
        "lic": &rec.lic,
        "attribution": attribution,
        "retrieved": now,
    })
}

/// Build the XC metadata sidecar JSON for a recording, including file hashes.
/// Hashes are nested under a `"_app"` key to distinguish from XC API fields.
pub fn build_metadata_json_with_hashes(
    rec: &XcRecording,
    hashes: &FileHashes,
) -> serde_json::Value {
    let mut json = build_metadata_json(rec);
    if let Some(obj) = json.as_object_mut() {
        // Remove top-level "retrieved" — it goes under _app
        let retrieved = obj.remove("retrieved");
        let mut bm = serde_json::Map::new();
        bm.insert("file_size".into(), serde_json::json!(hashes.size_bytes));
        bm.insert("sha256".into(), serde_json::json!(hashes.sha256));
        bm.insert("blake3".into(), serde_json::json!(hashes.blake3));
        bm.insert(
            "spot_hash_b3".into(),
            serde_json::json!(hashes.spot_hash_b3),
        );
        bm.insert(
            "content_hash".into(),
            serde_json::json!(hashes.content_hash),
        );
        if let Some(offset) = hashes.data_offset {
            bm.insert("data_offset".into(), serde_json::json!(offset));
        }
        if let Some(size) = hashes.data_size {
            bm.insert("data_size".into(), serde_json::json!(size));
        }
        if let Some(r) = retrieved {
            bm.insert("retrieved".into(), r);
        }
        obj.insert("_app".into(), serde_json::Value::Object(bm));
    }
    json
}

/// Save a recording's audio and metadata to the cache.
/// Returns the path to the saved audio file.
pub fn save_recording(
    root: &Path,
    rec: &XcRecording,
    audio_bytes: &[u8],
    precomputed_hashes: Option<&FileHashes>,
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
    fs::create_dir_all(&sounds_dir).map_err(|e| format!("Failed to create sounds dir: {e}"))?;

    let stem = recording_stem(rec);
    let ext = audio_extension(rec);
    let audio_filename = format!("{stem}.{ext}");
    let meta_filename = format!("{stem}.xc.json");

    // Write audio
    let audio_path = sounds_dir.join(&audio_filename);
    fs::write(&audio_path, audio_bytes).map_err(|e| format!("Failed to write audio: {e}"))?;

    // Write metadata sidecar (with file hashes). Reuse the caller's hashes when
    // provided — hashing a large recording (sha256 + blake3 + spot + content) is
    // expensive and `xc_download` already computed them for its return value.
    let meta_path = sounds_dir.join(&meta_filename);
    let computed;
    let hashes = match precomputed_hashes {
        Some(h) => h,
        None => {
            computed = compute_file_hashes(audio_bytes);
            &computed
        }
    };
    let metadata = build_metadata_json_with_hashes(rec, hashes);
    let json_str =
        serde_json::to_string_pretty(&metadata).map_err(|e| format!("Serialize error: {e}"))?;
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

/// Remove a recording entry from index.json by XC ID (under the index lock).
fn remove_from_index(root: &Path, id: u64) -> Result<(), String> {
    with_index_lock(root, || remove_from_index_inner(root, id))
}

fn remove_from_index_inner(root: &Path, id: u64) -> Result<(), String> {
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
    let json_str =
        serde_json::to_string_pretty(&index).map_err(|e| format!("Serialize error: {e}"))?;
    fs::write(&tmp_path, format!("{json_str}\n"))
        .map_err(|e| format!("Failed to write index.json.tmp: {e}"))?;
    fs::rename(&tmp_path, &index_path)
        .map_err(|e| format!("Failed to finalize index.json: {e}"))?;

    Ok(())
}

/// Run `f` while holding an exclusive advisory lock on the cache index, so the
/// read-modify-write of `index.json` is serialized across processes — the Tauri
/// backend and `xc-cli` share this cache dir. Without it, two concurrent writers
/// both read the old index and the later atomic rename clobbers the earlier's
/// new entry (the rename only prevents torn writes, not lost updates). The lock
/// is released when the handle drops, including on process exit/crash.
fn with_index_lock<T>(root: &Path, f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    use fs4::fs_std::FileExt;
    let lock_path = root.join("index.json.lock");
    let lock = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(|e| format!("Failed to open index lock {}: {e}", lock_path.display()))?;
    lock.lock_exclusive()
        .map_err(|e| format!("Failed to acquire index lock: {e}"))?;
    let result = f();
    let _ = FileExt::unlock(&lock);
    result
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
    with_index_lock(root, || {
        update_index_inner(root, rec, audio_filename, meta_filename)
    })
}

fn update_index_inner(
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
    let id_num = rec.id_num();
    if sounds.iter().any(|s| s["xc_id"].as_u64() == Some(id_num)) {
        return Ok(());
    }

    sounds.push(serde_json::json!({
        "filename": audio_filename,
        "metadata": meta_filename,
        "xc_id": id_num,
        "en": rec.en,
        "species": format!("{} {}", rec.genus, rec.sp),
        "source": "xeno-canto"
    }));

    let json_str =
        serde_json::to_string_pretty(&index).map_err(|e| format!("Serialize error: {e}"))?;

    // Write to temp file first, then rename for atomic update
    fs::write(&tmp_path, format!("{json_str}\n"))
        .map_err(|e| format!("Failed to write index.json.tmp: {e}"))?;
    fs::rename(&tmp_path, &index_path)
        .map_err(|e| format!("Failed to finalize index.json: {e}"))?;

    Ok(())
}
