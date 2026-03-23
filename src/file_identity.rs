use sha2::{Sha256, Digest};
use leptos::prelude::{Update, WithUntracked};
use crate::annotations::FileIdentity;
use crate::state::AppState;

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
    (0..num_chunks).map(|i| {
        let chunk_start = audio_start + i * (audio_len / num_chunks);
        let remaining = audio_len - (chunk_start - audio_start);
        let chunk_len = SPOT_CHUNK_SIZE.min(remaining);
        (chunk_start, chunk_len)
    }).collect()
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

    let chunk_hashes: Vec<blake3::Hash> = positions.iter().map(|&(start, len)| {
        let s = start as usize;
        let e = (start + len).min(file_bytes.len() as u64) as usize;
        blake3::hash(&file_bytes[s..e])
    }).collect();

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

/// Compute Layers 3 + 4 together: content hash (header-zeroed BLAKE3) and full BLAKE3.
/// Reads the entire file in 1 MB chunks. For the content hash, bytes in [0, data_offset)
/// are zeroed before hashing.
///
/// Returns (content_hash, full_blake3) as hex strings.
pub async fn compute_full_hashes(
    reader: &(impl AsyncRangeReader + ?Sized),
    file_size: u64,
    data_offset: Option<u64>,
    generation: u32,
    check_cancelled: impl Fn(u32) -> bool,
) -> Result<(String, String), String> {
    let header_end = data_offset.unwrap_or(0);
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

        // Content hash: zero the header region
        if offset < header_end {
            // This chunk overlaps the header
            let header_bytes_in_chunk = ((header_end - offset) as usize).min(bytes.len());
            let mut modified = bytes.clone();
            modified[..header_bytes_in_chunk].fill(0);
            content_hasher.update(&modified);
        } else {
            content_hasher.update(&bytes);
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

    Ok(format!("{:x}", hasher.finalize()))
}

/// Trait for async range reading (implemented for web File blobs and Tauri paths).
pub trait AsyncRangeReader {
    fn read(&self, offset: u64, length: u64) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<u8>, String>> + '_>>;
}

/// Web File blob range reader.
pub struct BlobRangeReader {
    pub file: web_sys::File,
}

impl AsyncRangeReader for BlobRangeReader {
    fn read(&self, offset: u64, length: u64) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<u8>, String>> + '_>> {
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
    fn read(&self, offset: u64, length: u64) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<u8>, String>> + '_>> {
        Box::pin(async move {
            crate::tauri_bridge::read_file_range(&self.path, offset, length).await
        })
    }
}

/// Construct an AsyncRangeReader from a FileHandle.
pub fn reader_from_handle(handle: &crate::audio::streaming_source::FileHandle) -> Box<dyn AsyncRangeReader + '_> {
    match handle {
        crate::audio::streaming_source::FileHandle::WebFile(file) => {
            Box::new(BlobRangeReader { file: file.clone() })
        }
        crate::audio::streaming_source::FileHandle::TauriPath(path) => {
            Box::new(TauriRangeReader { path: path.clone() })
        }
    }
}

/// Yield to the browser event loop (setTimeout(0)).
async fn yield_now() {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        web_sys::window()
            .unwrap()
            .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 0)
            .unwrap();
    });
    wasm_bindgen_futures::JsFuture::from(promise).await.ok();
}

/// Compute Layer 1 identity and kick off async Layer 2 (spot-check) computation.
/// Call this after a file is added to state.files.
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
    let already_has_identity = state.files.with_untracked(|files| {
        files.get(file_index).is_some_and(|f| f.identity.is_some())
    });
    if already_has_identity {
        // Still try loading saved annotations even if identity was already set
        let identity = state.files.with_untracked(|files| {
            files.get(file_index).and_then(|f| f.identity.clone())
        });
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

    state.files.update(|files| {
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
            let handle = state.files.with_untracked(|files| {
                files.get(file_index).and_then(|f| f.file_handle.clone())
            });
            if let Some(handle) = handle {
                let reader = reader_from_handle(&handle);
                compute_spot_hash_b3(reader.as_ref(), file_size, data_offset, data_size).await.ok()
            } else {
                None
            }
        };

        if let Some(hash) = spot_hash {
            state.files.update(|files| {
                if let Some(f) = files.get_mut(file_index) {
                    if let Some(ref mut id) = f.identity {
                        id.spot_hash_b3 = Some(hash);
                    }
                }
            });

            // Try loading annotations again with the better spot_hash_b3 key
            let identity = state.files.with_untracked(|files| {
                files.get(file_index).and_then(|f| f.identity.clone())
            });
            if let Some(id) = identity {
                crate::opfs::load_annotations(state, file_index, id);
            }
        }

        // When we have in-memory bytes, also compute full BLAKE3 + SHA-256 (Layers 3+4)
        if let Some(bytes) = file_bytes {
            yield_now().await; // yield before heavy computation

            // Layer 3: content hash (header zeroed) + Layer 4: full BLAKE3
            let header_end = data_offset.unwrap_or(0) as usize;
            let content_hash = {
                let mut hasher = blake3::Hasher::new();
                if header_end > 0 && header_end < bytes.len() {
                    let zeroed = vec![0u8; header_end];
                    hasher.update(&zeroed);
                    hasher.update(&bytes[header_end..]);
                } else {
                    hasher.update(&bytes);
                }
                hasher.finalize().to_hex().to_string()
            };
            let full_blake3 = blake3::hash(&bytes).to_hex().to_string();

            yield_now().await;

            // Layer 4-alt: full SHA-256
            let full_sha256 = {
                use sha2::{Sha256, Digest};
                let mut hasher = Sha256::new();
                hasher.update(&bytes);
                format!("{:x}", hasher.finalize())
            };

            state.files.update(|files| {
                if let Some(f) = files.get_mut(file_index) {
                    if let Some(ref mut id) = f.identity {
                        id.content_hash = Some(content_hash);
                        id.full_blake3 = Some(full_blake3);
                        id.full_sha256 = Some(full_sha256);
                    }
                }
            });

            // Save updated identity to sidecar
            crate::opfs::save_annotations_to_opfs(state, file_index);
        }
    });
}

/// Compute Layers 3+4 on demand. Called when user clicks [Calculate hash].
/// Optionally also computes SHA-256 (Layer 4-alt).
pub fn start_full_hash_computation(state: AppState, file_index: usize, include_sha256: bool) {
    use leptos::prelude::{Get, Set};

    // Increment generation to cancel any in-progress computation
    let gen = state.hash_generation.get() + 1;
    state.hash_generation.set(gen);
    state.hash_computing.set(true);

    let file_size = state.files.with_untracked(|files| {
        files.get(file_index).map(|f| f.identity.as_ref().map(|id| id.file_size).unwrap_or(0))
    }).unwrap_or(0);

    let data_offset = state.files.with_untracked(|files| {
        files.get(file_index).and_then(|f| f.identity.as_ref().and_then(|id| id.data_offset))
    });

    let handle = state.files.with_untracked(|files| {
        files.get(file_index).and_then(|f| f.file_handle.clone())
    });

    let Some(handle) = handle else {
        log::warn!("No file handle available for hash computation (file_index={file_index})");
        state.hash_computing.set(false);
        return;
    };

    wasm_bindgen_futures::spawn_local(async move {
        let reader = reader_from_handle(&handle);
        let check = |g: u32| state.hash_generation.get() != g;

        // Compute BLAKE3 layers 3+4
        match compute_full_hashes(reader.as_ref(), file_size, data_offset, gen, &check).await {
            Ok((content_hash, full_blake3)) => {
                state.files.update(|files| {
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
                    state.files.update(|files| {
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
            state.hash_computing.set(false);

            // Save updated identity to sidecar
            crate::opfs::save_annotations_to_opfs(state, file_index);
        }
    });
}
