// SPDX-License-Identifier: GPL-3.0-only OR MIT OR Apache-2.0
use crate::annotations::FileIdentity;
use crate::state::store_fields::*;
use crate::state::AppState;
use crate::web_util::yield_now;
use leptos::prelude::{Update, WithUntracked};
use sha2::{Digest, Sha256};

/// Size of each chunk for the multi-point spot hash (1 MB).
const SPOT_CHUNK_SIZE: u64 = 1_048_576;
/// Maximum number of chunks for the spot hash.
const NUM_SPOT_CHUNKS: u64 = 16;

/// Create a Layer 1 identity (filename + size). Instant.
pub fn identity_layer1(filename: &str, file_size: u64) -> FileIdentity {
    FileIdentity {
        filename: filename.to_string(),
        file_size,
        spot_hash_b3: None,
        content_hash: None,
        full_blake3: None,
        full_sha256: None,
        legacy_spot_hash: None,
        data_offset: None,
        data_size: None,
        last_modified: None,
        file_path: None,
    }
}

/// Determine audio region boundaries for hashing.
/// Returns (audio_start, audio_len) in bytes.
fn audio_region(file_size: u64, data_offset: Option<u64>, data_size: Option<u64>) -> (u64, u64) {
    let audio_start = data_offset.unwrap_or(0);
    let audio_len = data_size.unwrap_or(file_size.saturating_sub(audio_start));
    (audio_start, audio_len)
}

/// Compute the chunk positions for the multi-point spot hash.
/// Returns Vec of (chunk_start, chunk_len) pairs.
fn spot_chunk_positions(audio_start: u64, audio_len: u64) -> Vec<(u64, u64)> {
    if audio_len == 0 {
        return Vec::new();
    }
    let num_chunks = NUM_SPOT_CHUNKS.min((audio_len / SPOT_CHUNK_SIZE).max(1));
    (0..num_chunks)
        .map(|i| {
            let chunk_start = audio_start + i * (audio_len / num_chunks);
            let remaining = audio_len - (chunk_start - audio_start);
            let chunk_len = SPOT_CHUNK_SIZE.min(remaining);
            (chunk_start, chunk_len)
        })
        .collect()
}

/// Combine individual chunk hashes into the final spot hash.
fn finalize_spot_hash(chunk_hashes: &[blake3::Hash]) -> String {
    let mut combined = Vec::with_capacity(chunk_hashes.len() * 32);
    for h in chunk_hashes {
        combined.extend_from_slice(h.as_bytes());
    }
    blake3::hash(&combined).to_hex().to_string()
}

/// Compute Layer 2 BLAKE3 multi-point spot hash from in-memory file bytes (sync).
pub fn compute_spot_hash_b3_sync(
    file_bytes: &[u8],
    data_offset: Option<u64>,
    data_size: Option<u64>,
) -> String {
    let (audio_start, audio_len) = audio_region(file_bytes.len() as u64, data_offset, data_size);
    let positions = spot_chunk_positions(audio_start, audio_len);

    let chunk_hashes: Vec<blake3::Hash> = positions
        .iter()
        .map(|&(start, len)| {
            let s = start as usize;
            let e = (start + len).min(file_bytes.len() as u64) as usize;
            blake3::hash(&file_bytes[s..e])
        })
        .collect();

    finalize_spot_hash(&chunk_hashes)
}

/// Compute Layer 2 BLAKE3 multi-point spot hash via async range reader.
pub async fn compute_spot_hash_b3(
    reader: &(impl AsyncRangeReader + ?Sized),
    file_size: u64,
    data_offset: Option<u64>,
    data_size: Option<u64>,
) -> Result<String, String> {
    let (audio_start, audio_len) = audio_region(file_size, data_offset, data_size);
    let positions = spot_chunk_positions(audio_start, audio_len);

    let mut chunk_hashes = Vec::with_capacity(positions.len());
    for (i, &(start, len)) in positions.iter().enumerate() {
        let bytes = reader.read(start, len).await?;
        chunk_hashes.push(blake3::hash(&bytes));

        // Yield to browser every 4 chunks
        if (i + 1) % 4 == 0 {
            yield_now().await;
        }
    }

    Ok(finalize_spot_hash(&chunk_hashes))
}

/// Compute Layers 3 + 4 together: content hash (audio-samples-only BLAKE3) and
/// full BLAKE3 over the whole file. Reads the entire file in 1 MB chunks.
///
/// The content hash covers only `file[data_offset .. data_offset+data_size]`,
/// skipping both the header and any trailing metadata (e.g. GUANO). This means
/// metadata edits don't change the content hash.
///
/// Returns (content_hash, full_blake3) as hex strings.
pub async fn compute_full_hashes(
    reader: &(impl AsyncRangeReader + ?Sized),
    file_size: u64,
    data_offset: Option<u64>,
    data_size: Option<u64>,
    generation: u32,
    check_cancelled: impl Fn(u32) -> bool,
) -> Result<(String, String), String> {
    let audio_start = data_offset.unwrap_or(0);
    let audio_end = match data_size {
        Some(sz) => (audio_start + sz).min(file_size),
        None => file_size,
    };
    let audio_start = audio_start.min(audio_end);

    let mut content_hasher = blake3::Hasher::new();
    let mut full_hasher = blake3::Hasher::new();

    let chunk_size: u64 = SPOT_CHUNK_SIZE; // 1 MB
    let mut offset: u64 = 0;

    while offset < file_size {
        if check_cancelled(generation) {
            return Err("Cancelled".into());
        }

        let len = chunk_size.min(file_size - offset);
        let bytes = reader.read(offset, len).await?;

        // Full hash: always update with original bytes
        full_hasher.update(&bytes);

        // Content hash: only feed the intersection with [audio_start, audio_end)
        let chunk_start = offset;
        let chunk_end = offset + len;
        let slice_start = audio_start.max(chunk_start);
        let slice_end = audio_end.min(chunk_end);
        if slice_start < slice_end {
            let lo = (slice_start - chunk_start) as usize;
            let hi = (slice_end - chunk_start) as usize;
            content_hasher.update(&bytes[lo..hi]);
        }

        offset += len;

        // Yield every 4 MB
        if (offset / chunk_size).is_multiple_of(4) {
            yield_now().await;
        }
    }

    let content_hash = content_hasher.finalize().to_hex().to_string();
    let full_hash = full_hasher.finalize().to_hex().to_string();
    Ok((content_hash, full_hash))
}

/// Compute full file SHA-256 via async range reader.
pub async fn compute_full_sha256(
    reader: &(impl AsyncRangeReader + ?Sized),
    file_size: u64,
    generation: u32,
    check_cancelled: impl Fn(u32) -> bool,
) -> Result<String, String> {
    let mut hasher = Sha256::new();
    let chunk_size: u64 = SPOT_CHUNK_SIZE;
    let mut offset: u64 = 0;

    while offset < file_size {
        if check_cancelled(generation) {
            return Err("Cancelled".into());
        }

        let len = chunk_size.min(file_size - offset);
        let bytes = reader.read(offset, len).await?;
        hasher.update(&bytes);
        offset += len;

        if (offset / chunk_size).is_multiple_of(4) {
            yield_now().await;
        }
    }

    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// Trait for async range reading (implemented for web File blobs and Tauri paths).
pub trait AsyncRangeReader {
    fn read(
        &self,
        offset: u64,
        length: u64,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<u8>, String>> + '_>>;
}

/// Web File blob range reader.
pub struct BlobRangeReader {
    pub file: web_sys::File,
}

impl AsyncRangeReader for BlobRangeReader {
    fn read(
        &self,
        offset: u64,
        length: u64,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<u8>, String>> + '_>> {
        let start = offset as f64;
        let end = (offset + length) as f64;
        Box::pin(async move {
            crate::audio::streaming_source::read_blob_range(&self.file, start, end).await
        })
    }
}

/// Tauri file path range reader.
pub struct TauriRangeReader {
    pub path: String,
}

impl AsyncRangeReader for TauriRangeReader {
    fn read(
        &self,
        offset: u64,
        length: u64,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<u8>, String>> + '_>> {
        Box::pin(
            async move { crate::tauri_bridge::read_file_range(&self.path, offset, length).await },
        )
    }
}

/// Android MediaStore `content://` range reader (reads back via the media-store
/// plugin). Used to hash a recording that lives only in public shared storage.
pub struct MediaStoreRangeReader {
    pub uri: String,
}

impl AsyncRangeReader for MediaStoreRangeReader {
    fn read(
        &self,
        offset: u64,
        length: u64,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<u8>, String>> + '_>> {
        Box::pin(async move {
            crate::audio::streaming_source::read_media_store_range(&self.uri, offset, length).await
        })
    }
}

/// Construct an AsyncRangeReader from a FileHandle.
pub fn reader_from_handle(
    handle: &crate::audio::streaming_source::FileHandle,
) -> Box<dyn AsyncRangeReader + '_> {
    match handle {
        crate::audio::streaming_source::FileHandle::WebFile(file) => {
            Box::new(BlobRangeReader { file: file.clone() })
        }
        crate::audio::streaming_source::FileHandle::TauriPath(path) => {
            Box::new(TauriRangeReader { path: path.clone() })
        }
        crate::audio::streaming_source::FileHandle::MediaStoreUri(uri) => {
            Box::new(MediaStoreRangeReader { uri: uri.clone() })
        }
        crate::audio::streaming_source::FileHandle::Bytes(b) => {
            Box::new(BytesRangeReader { bytes: b.clone() })
        }
    }
}

/// In-memory range reader (host tests; mirrors `FileHandle::Bytes`).
pub struct BytesRangeReader {
    pub bytes: std::sync::Arc<Vec<u8>>,
}

impl AsyncRangeReader for BytesRangeReader {
    fn read(
        &self,
        offset: u64,
        length: u64,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<u8>, String>> + '_>> {
        Box::pin(async move {
            let len = self.bytes.len() as u64;
            let s = offset.min(len) as usize;
            let e = (offset + length).min(len) as usize;
            Ok(self.bytes[s..e].to_vec())
        })
    }
}

/// Threshold: files below this size verify via full blake3; above via spot_hash_b3.
/// The spot hash reads up to 16 x 1MB = 16MB, so below ~24MB the full blake3
/// costs about the same and is more definitive.
pub const SMALL_FILE_THRESHOLD: u64 = 24_000_000;

/// Merge reference hashes from XC sidecar and annotation-store sidecar identity.
/// XC hashes take priority when both sources have a value.
fn merge_references(
    xc_hashes: &Option<crate::state::SidecarHashes>,
    sidecar_id: &Option<FileIdentity>,
) -> crate::state::SidecarHashes {
    let mut merged = crate::state::SidecarHashes::default();

    // Start with sidecar identity (from .batm file)
    if let Some(sid) = sidecar_id {
        merged.blake3 = sid.full_blake3.clone();
        merged.sha256 = sid.full_sha256.clone();
        merged.spot_hash_b3 = sid.spot_hash_b3.clone();
        merged.content_hash = sid.content_hash.clone();
        merged.file_size = Some(sid.file_size);
        merged.data_offset = sid.data_offset;
        merged.data_size = sid.data_size;
    }

    // Override with XC hashes (more authoritative for downloaded files)
    if let Some(xc) = xc_hashes {
        if xc.blake3.is_some() {
            merged.blake3 = xc.blake3.clone();
        }
        if xc.sha256.is_some() {
            merged.sha256 = xc.sha256.clone();
        }
        if xc.spot_hash_b3.is_some() {
            merged.spot_hash_b3 = xc.spot_hash_b3.clone();
        }
        if xc.content_hash.is_some() {
            merged.content_hash = xc.content_hash.clone();
        }
        if xc.file_size.is_some() {
            merged.file_size = xc.file_size;
        }
        if xc.data_offset.is_some() {
            merged.data_offset = xc.data_offset;
        }
        if xc.data_size.is_some() {
            merged.data_size = xc.data_size;
        }
    }

    merged
}

/// Compare computed identity against reference hashes. Returns the verification outcome.
///
/// Logic:
/// 1. File size check (>4KB difference = significant)
/// 2. Primary hash: blake3 for small files (<10MB), spot_hash for large files
/// 3. Content hash fallback if primary mismatches
fn run_verification(
    identity: &FileIdentity,
    reference: &crate::state::SidecarHashes,
) -> crate::state::VerifyOutcome {
    use crate::state::VerifyOutcome;

    if reference.is_empty() {
        return VerifyOutcome::Pending;
    }

    // 1. File size check
    if let Some(ref_size) = reference.file_size {
        let diff = (identity.file_size as i64 - ref_size as i64).unsigned_abs();
        if diff > 4096 {
            // Sizes differ significantly — hash verification still proceeds
            // but is very likely to fail (unless file was completely rewritten)
        }
    }

    // 2. Primary hash check based on file size
    let is_small = identity.file_size < SMALL_FILE_THRESHOLD;

    let primary_matched = if is_small {
        match (&identity.full_blake3, &reference.blake3) {
            (Some(computed), Some(expected)) => Some(computed == expected),
            _ => None,
        }
    } else {
        match (&identity.spot_hash_b3, &reference.spot_hash_b3) {
            (Some(computed), Some(expected)) => Some(computed == expected),
            _ => None,
        }
    };

    match primary_matched {
        Some(true) => return VerifyOutcome::Match,
        None => return VerifyOutcome::Pending,
        Some(false) => {} // Mismatch — try content_hash fallback
    }

    // 3. Content hash fallback
    if let (Some(computed_ch), Some(expected_ch)) =
        (&identity.content_hash, &reference.content_hash)
    {
        if computed_ch == expected_ch {
            return VerifyOutcome::ContentMatch;
        }
    }

    VerifyOutcome::Mismatch
}

/// Read the current reference hashes for a file from state.
fn get_merged_reference(state: AppState, file_index: usize) -> crate::state::SidecarHashes {
    let xc_hashes = state
        .library
        .files()
        .with_untracked(|files| files.get(file_index).and_then(|f| f.xc_hashes.clone()));
    let sidecar_id = state.file_id_at(file_index).and_then(|id| {
        state
            .annotations
            .store()
            .with_untracked(|store| store.get(id).map(|set| set.file_identity.clone()))
    });
    merge_references(&xc_hashes, &sidecar_id)
}

/// Store verification outcome on the LoadedFile.
fn set_verify_outcome(state: AppState, file_index: usize, outcome: crate::state::VerifyOutcome) {
    state.library.files().update(|files| {
        if let Some(f) = files.get_mut(file_index) {
            f.verify_outcome = outcome;
        }
    });
}

/// Compute Layer 1 identity and kick off async Layer 2 (spot-check) computation.
/// Call this after a file is added to state.library.files().
pub fn start_identity_computation(
    state: AppState,
    file_index: usize,
    filename: String,
    file_size: u64,
    file_bytes: Option<Vec<u8>>,
    data_offset: Option<u64>,
    data_size: Option<u64>,
    last_modified_ms: Option<f64>,
) {
    // Skip if identity already computed (e.g. by load_named_bytes)
    let already_has_identity = state
        .library
        .files()
        .with_untracked(|files| files.get(file_index).is_some_and(|f| f.identity.is_some()));
    if already_has_identity {
        // Still try loading saved annotations even if identity was already set
        let identity = state
            .library
            .files()
            .with_untracked(|files| files.get(file_index).and_then(|f| f.identity.clone()));
        if let Some(id) = identity {
            crate::opfs::load_annotations(state, file_index, id);
        }
        return;
    }

    // Layer 1: set immediately
    let mut identity = identity_layer1(&filename, file_size);
    identity.data_offset = data_offset;
    identity.data_size = data_size;
    identity.last_modified = last_modified_ms.map(|ms| ms.to_string());

    state.library.files().update(|files| {
        if let Some(f) = files.get_mut(file_index) {
            f.identity = Some(identity.clone());
        }
    });

    // Try loading annotations with Layer 1 key
    crate::opfs::load_annotations(state, file_index, identity);

    // Layer 2: compute BLAKE3 spot hash async (+ Layers 3+4 when bytes available)
    wasm_bindgen_futures::spawn_local(async move {
        let spot_hash = if let Some(ref bytes) = file_bytes {
            // In-memory: compute synchronously
            Some(compute_spot_hash_b3_sync(bytes, data_offset, data_size))
        } else {
            // No bytes available (streaming load) — need file handle for range reads
            let handle = state
                .library
                .files()
                .with_untracked(|files| files.get(file_index).and_then(|f| f.file_handle.clone()));
            if let Some(handle) = handle {
                let reader = reader_from_handle(&handle);
                compute_spot_hash_b3(reader.as_ref(), file_size, data_offset, data_size)
                    .await
                    .ok()
            } else {
                None
            }
        };

        if let Some(hash) = spot_hash {
            state.library.files().update(|files| {
                if let Some(f) = files.get_mut(file_index) {
                    if let Some(ref mut id) = f.identity {
                        id.spot_hash_b3 = Some(hash);
                    }
                }
            });

            // Try loading annotations again with the better spot_hash_b3 key
            let identity = state
                .library
                .files()
                .with_untracked(|files| files.get(file_index).and_then(|f| f.identity.clone()));
            if let Some(id) = identity {
                crate::opfs::load_annotations(state, file_index, id);
            }
        }

        let is_small = file_size < SMALL_FILE_THRESHOLD;

        // For small files, auto-compute blake3 + content_hash (not SHA-256)
        if is_small {
            if let Some(bytes) = file_bytes {
                yield_now().await;

                // Layer 3: content hash (audio samples only) + Layer 4: full BLAKE3
                let audio_start = data_offset.unwrap_or(0) as usize;
                let audio_end = match data_size {
                    Some(sz) => (audio_start + sz as usize).min(bytes.len()),
                    None => bytes.len(),
                };
                let audio_start = audio_start.min(audio_end);
                let content_hash = blake3::hash(&bytes[audio_start..audio_end])
                    .to_hex()
                    .to_string();
                let full_blake3 = blake3::hash(&bytes).to_hex().to_string();

                state.library.files().update(|files| {
                    if let Some(f) = files.get_mut(file_index) {
                        if let Some(ref mut id) = f.identity {
                            id.content_hash = Some(content_hash);
                            id.full_blake3 = Some(full_blake3);
                        }
                    }
                });

                crate::opfs::save_annotations_to_opfs(state, file_index);
            } else {
                // Small file without in-memory bytes: compute via file handle
                let has_handle = state.library.files().with_untracked(|files| {
                    files
                        .get(file_index)
                        .is_some_and(|f| f.file_handle.is_some())
                });
                if has_handle {
                    start_full_hash_computation(state, file_index, false);
                }
            }
        }

        // Run verification against reference hashes
        let reference = get_merged_reference(state, file_index);
        let identity = state
            .library
            .files()
            .with_untracked(|files| files.get(file_index).and_then(|f| f.identity.clone()));

        if let Some(ref id) = identity {
            let mut outcome = run_verification(id, &reference);

            // On mismatch: for large files, content_hash isn't auto-computed.
            // Compute it now as a fallback, then re-verify.
            if outcome == crate::state::VerifyOutcome::Mismatch && id.content_hash.is_none() {
                let handle = state.library.files().with_untracked(|files| {
                    files.get(file_index).and_then(|f| f.file_handle.clone())
                });
                if let Some(handle) = handle {
                    let reader = reader_from_handle(&handle);
                    let check = |_: u32| false; // no cancellation for fallback
                    if let Ok((content_hash, full_blake3)) = compute_full_hashes(
                        reader.as_ref(),
                        file_size,
                        data_offset,
                        data_size,
                        0,
                        &check,
                    )
                    .await
                    {
                        state.library.files().update(|files| {
                            if let Some(f) = files.get_mut(file_index) {
                                if let Some(ref mut fid) = f.identity {
                                    fid.content_hash = Some(content_hash);
                                    fid.full_blake3 = Some(full_blake3);
                                }
                            }
                        });
                        crate::opfs::save_annotations_to_opfs(state, file_index);

                        // Re-verify with content_hash now available
                        let updated_id = state.library.files().with_untracked(|files| {
                            files.get(file_index).and_then(|f| f.identity.clone())
                        });
                        if let Some(ref uid) = updated_id {
                            outcome = run_verification(uid, &reference);
                        }
                    }
                }
            }

            set_verify_outcome(state, file_index, outcome);
        }
    });
}

/// Compute Layers 3+4 on demand. Called when user clicks [Calculate hash].
/// Optionally also computes SHA-256 (Layer 4-alt).
pub fn start_full_hash_computation(state: AppState, file_index: usize, include_sha256: bool) {
    use leptos::prelude::{Get, Set};

    // Increment generation to cancel any in-progress computation
    let gen = state.status.hash_generation().get() + 1;
    state.status.hash_generation().set(gen);
    state.status.hash_computing().set(true);

    let file_size = state
        .library
        .files()
        .with_untracked(|files| {
            files
                .get(file_index)
                .map(|f| f.identity.as_ref().map(|id| id.file_size).unwrap_or(0))
        })
        .unwrap_or(0);

    let data_offset = state.library.files().with_untracked(|files| {
        files
            .get(file_index)
            .and_then(|f| f.identity.as_ref().and_then(|id| id.data_offset))
    });

    let data_size = state.library.files().with_untracked(|files| {
        files
            .get(file_index)
            .and_then(|f| f.identity.as_ref().and_then(|id| id.data_size))
    });

    let handle = state
        .library
        .files()
        .with_untracked(|files| files.get(file_index).and_then(|f| f.file_handle.clone()));

    let Some(handle) = handle else {
        log::warn!("No file handle available for hash computation (file_index={file_index})");
        state.status.hash_computing().set(false);
        return;
    };

    wasm_bindgen_futures::spawn_local(async move {
        let reader = reader_from_handle(&handle);
        let check = |g: u32| state.status.hash_generation().get() != g;

        // Compute BLAKE3 layers 3+4
        match compute_full_hashes(
            reader.as_ref(),
            file_size,
            data_offset,
            data_size,
            gen,
            &check,
        )
        .await
        {
            Ok((content_hash, full_blake3)) => {
                state.library.files().update(|files| {
                    if let Some(f) = files.get_mut(file_index) {
                        if let Some(ref mut id) = f.identity {
                            id.content_hash = Some(content_hash);
                            id.full_blake3 = Some(full_blake3);
                        }
                    }
                });
            }
            Err(e) => {
                if e != "Cancelled" {
                    log::warn!("Full hash computation failed: {e}");
                }
            }
        }

        // Optionally compute SHA-256
        if include_sha256 && !check(gen) {
            let reader2 = reader_from_handle(&handle);
            match compute_full_sha256(reader2.as_ref(), file_size, gen, &check).await {
                Ok(sha256) => {
                    state.library.files().update(|files| {
                        if let Some(f) = files.get_mut(file_index) {
                            if let Some(ref mut id) = f.identity {
                                id.full_sha256 = Some(sha256);
                            }
                        }
                    });
                }
                Err(e) => {
                    if e != "Cancelled" {
                        log::warn!("SHA-256 computation failed: {e}");
                    }
                }
            }
        }

        // Only clear computing flag if this is still the current generation
        if !check(gen) {
            state.status.hash_computing().set(false);

            // Mark all hashes as verified and run verification
            state.library.files().update(|files| {
                if let Some(f) = files.get_mut(file_index) {
                    f.all_hashes_verified = true;
                }
            });

            let reference = get_merged_reference(state, file_index);
            let identity = state
                .library
                .files()
                .with_untracked(|files| files.get(file_index).and_then(|f| f.identity.clone()));
            if let Some(ref id) = identity {
                let outcome = run_verification(id, &reference);
                set_verify_outcome(state, file_index, outcome);
            }

            // Save updated identity to sidecar
            crate::opfs::save_annotations_to_opfs(state, file_index);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_layer1_only_carries_filename_and_size() {
        let id = identity_layer1("rec.wav", 12345);
        assert_eq!(id.filename, "rec.wav");
        assert_eq!(id.file_size, 12345);
        assert!(id.spot_hash_b3.is_none());
        assert!(id.content_hash.is_none());
        assert!(id.full_blake3.is_none());
        assert!(id.full_sha256.is_none());
        assert!(id.data_offset.is_none());
        assert!(id.data_size.is_none());
        assert!(id.file_path.is_none());
    }

    #[test]
    fn audio_region_defaults_to_whole_file() {
        assert_eq!(audio_region(1000, None, None), (0, 1000));
    }

    #[test]
    fn audio_region_respects_explicit_offset_and_size() {
        assert_eq!(audio_region(1000, Some(44), Some(800)), (44, 800));
    }

    #[test]
    fn audio_region_derives_size_from_offset_when_size_unset() {
        // file=1000, offset=44 → audio_len = 1000 - 44 = 956
        assert_eq!(audio_region(1000, Some(44), None), (44, 956));
    }

    #[test]
    fn audio_region_saturates_when_offset_exceeds_file() {
        // Pathological case: offset larger than file_size should not underflow.
        assert_eq!(audio_region(100, Some(200), None), (200, 0));
    }

    #[test]
    fn spot_chunk_positions_empty_audio() {
        assert!(spot_chunk_positions(0, 0).is_empty());
    }

    #[test]
    fn spot_chunk_positions_small_audio_emits_one_chunk() {
        // audio_len smaller than SPOT_CHUNK_SIZE → exactly one chunk covering it all.
        let pos = spot_chunk_positions(0, 100_000);
        assert_eq!(pos.len(), 1);
        assert_eq!(pos[0], (0, 100_000));
    }

    #[test]
    fn spot_chunk_positions_evenly_spaces_chunks() {
        // 32 MB of audio → 16 chunks of 1 MB, evenly spaced.
        let audio_len = 32 * SPOT_CHUNK_SIZE;
        let pos = spot_chunk_positions(0, audio_len);
        assert_eq!(pos.len(), NUM_SPOT_CHUNKS as usize);
        let stride = audio_len / NUM_SPOT_CHUNKS;
        for (i, &(start, len)) in pos.iter().enumerate() {
            assert_eq!(start, i as u64 * stride);
            assert_eq!(len, SPOT_CHUNK_SIZE);
        }
    }

    #[test]
    fn finalize_spot_hash_is_deterministic_and_order_sensitive() {
        let h1 = blake3::hash(b"a");
        let h2 = blake3::hash(b"b");

        let ab = finalize_spot_hash(&[h1, h2]);
        let ab_again = finalize_spot_hash(&[h1, h2]);
        let ba = finalize_spot_hash(&[h2, h1]);

        assert_eq!(ab, ab_again);
        assert_ne!(ab, ba, "swapping chunk order must change the hash");
        // BLAKE3 hex digest is 64 characters.
        assert_eq!(ab.len(), 64);
    }

    #[test]
    fn compute_spot_hash_b3_sync_matches_for_identical_bytes() {
        // Two byte arrays with the same content should hash identically.
        let mut bytes = vec![0u8; 4 * SPOT_CHUNK_SIZE as usize];
        for (i, b) in bytes.iter_mut().enumerate() {
            *b = (i % 251) as u8;
        }
        let h1 = compute_spot_hash_b3_sync(&bytes, None, None);
        let h2 = compute_spot_hash_b3_sync(&bytes, None, None);
        assert_eq!(h1, h2);

        // Changing a byte inside the audio region must change the hash.
        let mut tampered = bytes.clone();
        tampered[100] ^= 0xff;
        assert_ne!(h1, compute_spot_hash_b3_sync(&tampered, None, None));
    }

    #[test]
    fn compute_spot_hash_b3_sync_skips_header_when_data_offset_provided() {
        // Same audio region, different "header" bytes — hash should be unchanged.
        let mut a = vec![0u8; 1000];
        let mut b = vec![0u8; 1000];
        for (i, byte) in a.iter_mut().enumerate() {
            *byte = (i % 251) as u8;
        }
        b.copy_from_slice(&a);
        // Vary the first 44 bytes (the "header"); audio starts at offset 44.
        a[..44].fill(0xAA);
        b[..44].fill(0x55);
        let ha = compute_spot_hash_b3_sync(&a, Some(44), Some(956));
        let hb = compute_spot_hash_b3_sync(&b, Some(44), Some(956));
        assert_eq!(ha, hb, "header bytes must not affect spot hash");
    }
}
