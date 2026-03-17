//! Streaming audio sources — streams decoded samples from a file on demand.
//!
//! For audio files whose decoded size exceeds a threshold (~512 MB), only the
//! header is parsed and the first 30 seconds are decoded upfront. Remaining
//! samples are decoded on demand into an LRU chunk cache.
//!
//! - `StreamingWavSource`: random-access PCM via `File.slice()` byte ranges
//! - `StreamingFlacSource`: progressive sequential FLAC decode in 4 MB windows
//!
//! Consumers call `prefetch_region()` (async) before `read_samples()` (sync).
//! If a region is not cached, `read_samples()` returns silence for those frames.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use crate::audio::loader::{FlacHeader, WavHeader};

// Re-export MP3 and OGG sources from their own modules
pub use super::streaming_mp3::StreamingMp3Source;
pub use super::streaming_ogg::StreamingOggSource;
use crate::audio::source::{AudioSource, ChannelView};

/// Number of per-channel frames per cache chunk (~256K frames).
/// At 384 kHz stereo 32-bit float, one chunk ≈ 2 MB raw + 1 MB mono = 3 MB.
pub(crate) const CHUNK_FRAMES: usize = 262_144;

/// Maximum cache size in bytes (128 MB).
const CACHE_MAX_BYTES: usize = 128 * 1024 * 1024;

/// Format info extracted from the WAV header, needed for PCM decoding.
#[derive(Clone, Debug)]
struct WavFormatInfo {
    sample_rate: u32,
    channels: u32,
    bits_per_sample: u16,
    is_float: bool,
    data_offset: u64,
    bytes_per_frame: u32, // channels * (bits_per_sample / 8)
}

/// A single cached chunk of decoded audio.
pub(crate) struct CachedChunk {
    /// Mono-mixed samples for this chunk.
    pub(crate) mono: Vec<f32>,
    /// Interleaved raw samples (if multi-channel). `None` for mono files.
    pub(crate) raw: Option<Vec<f32>>,
}

impl CachedChunk {
    pub(crate) fn byte_size(&self) -> usize {
        self.mono.len() * 4
            + self.raw.as_ref().map_or(0, |r| r.len() * 4)
    }
}

/// LRU chunk cache for decoded audio regions.
pub(crate) struct ChunkCache {
    /// Decoded chunks keyed by chunk index (frame / CHUNK_FRAMES).
    chunks: HashMap<u64, CachedChunk>,
    /// LRU order: most recently used at the back.
    lru: Vec<u64>,
    /// Current total memory usage in bytes.
    total_bytes: usize,
}

impl ChunkCache {
    pub(crate) fn new() -> Self {
        Self {
            chunks: HashMap::new(),
            lru: Vec::new(),
            total_bytes: 0,
        }
    }

    /// Get a cached chunk, updating LRU order.
    pub(crate) fn get(&mut self, chunk_idx: u64) -> Option<&CachedChunk> {
        if self.chunks.contains_key(&chunk_idx) {
            // Move to back of LRU
            self.lru.retain(|&k| k != chunk_idx);
            self.lru.push(chunk_idx);
            self.chunks.get(&chunk_idx)
        } else {
            None
        }
    }

    /// Insert a chunk, evicting oldest entries if over budget.
    pub(crate) fn insert(&mut self, chunk_idx: u64, chunk: CachedChunk) {
        let size = chunk.byte_size();

        // Remove existing entry first to fix memory accounting on duplicate inserts
        if let Some(existing) = self.chunks.remove(&chunk_idx) {
            self.total_bytes -= existing.byte_size();
            self.lru.retain(|&k| k != chunk_idx);
        }

        // Evict until we have room
        while self.total_bytes + size > CACHE_MAX_BYTES && !self.lru.is_empty() {
            let oldest = self.lru.remove(0);
            if let Some(removed) = self.chunks.remove(&oldest) {
                self.total_bytes -= removed.byte_size();
            }
        }

        self.total_bytes += size;
        self.chunks.insert(chunk_idx, chunk);
        self.lru.push(chunk_idx);
    }

    /// Update LRU position for a cached chunk (mark as recently used).
    pub(crate) fn touch(&mut self, chunk_idx: u64) {
        if self.chunks.contains_key(&chunk_idx) {
            self.lru.retain(|&k| k != chunk_idx);
            self.lru.push(chunk_idx);
        }
    }

    /// Check if a chunk is cached (without updating LRU).
    pub(crate) fn contains(&self, chunk_idx: u64) -> bool {
        self.chunks.contains_key(&chunk_idx)
    }
}

/// Handle to the underlying file — either a browser `web_sys::File` or a native
/// path string (used via Tauri IPC `read_file_range` command).
#[derive(Clone)]
pub enum FileHandle {
    /// Browser / webview: uses `File.slice()` + `FileReader`.
    WebFile(web_sys::File),
    /// Tauri desktop/mobile: uses native `read_file_range` IPC command.
    TauriPath(String),
}

impl std::fmt::Debug for FileHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FileHandle::WebFile(file) => write!(f, "WebFile(\"{}\")", file.name()),
            FileHandle::TauriPath(path) => write!(f, "TauriPath(\"{}\")", path),
        }
    }
}

// SAFETY: WASM is single-threaded; Send+Sync required for storage in RwSignal<Vec<LoadedFile>>.
unsafe impl Send for FileHandle {}
unsafe impl Sync for FileHandle {}

/// Streaming audio source backed by a file handle (browser File or native path).
///
/// The first ~30 seconds are decoded during construction and kept in memory
/// permanently. Beyond that, samples are decoded on demand via file range reads
/// and cached in an LRU chunk cache.
pub struct StreamingWavSource {
    /// The file handle — browser File or native path.
    handle: FileHandle,
    /// WAV format info for PCM decoding.
    info: WavFormatInfo,
    /// Total per-channel frames in the file.
    total_frames: u64,
    /// Pre-decoded first 30s (mono-mixed). Always available.
    head_mono: Arc<Vec<f32>>,
    /// Pre-decoded first 30s (raw interleaved, if multi-channel).
    head_raw: Option<Arc<Vec<f32>>>,
    /// Number of head frames (per-channel).
    head_frames: usize,
    /// LRU cache for chunks beyond the head region.
    cache: RefCell<ChunkCache>,
}

// SAFETY: WASM is single-threaded; these are required by AudioSource: Send + Sync.
unsafe impl Send for StreamingWavSource {}
unsafe impl Sync for StreamingWavSource {}

impl std::fmt::Debug for StreamingWavSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamingWavSource")
            .field("total_frames", &self.total_frames)
            .field("head_frames", &self.head_frames)
            .field("sample_rate", &self.info.sample_rate)
            .field("channels", &self.info.channels)
            .finish()
    }
}

impl StreamingWavSource {
    /// Create a new streaming source from a parsed header and pre-decoded head samples.
    ///
    /// - `handle`: file handle (browser File or native path)
    /// - `header`: parsed WAV header with format info and data offset
    /// - `head_mono`: mono-mixed samples for the first ~30s
    /// - `head_raw`: raw interleaved samples for the first ~30s (None for mono)
    pub fn new(
        handle: FileHandle,
        header: &WavHeader,
        head_mono: Vec<f32>,
        head_raw: Option<Vec<f32>>,
    ) -> Self {
        let head_frames = head_mono.len();
        Self {
            handle,
            info: WavFormatInfo {
                sample_rate: header.sample_rate,
                channels: header.channels as u32,
                bits_per_sample: header.bits_per_sample,
                is_float: header.is_float,
                data_offset: header.data_offset,
                bytes_per_frame: header.channels as u32 * (header.bits_per_sample as u32 / 8),
            },
            total_frames: header.total_frames,
            head_mono: Arc::new(head_mono),
            head_raw: head_raw.map(Arc::new),
            head_frames,
            cache: RefCell::new(ChunkCache::new()),
        }
    }

    /// Async: ensure all chunks covering `[start_frame, start_frame + len)` are cached.
    ///
    /// Must be called before `read_samples()` for regions beyond the head.
    /// Safe to call for head-region frames (no-op).
    pub async fn prefetch_region(&self, start_frame: u64, len: usize) {
        let end_frame = (start_frame + len as u64).min(self.total_frames);
        if end_frame <= self.head_frames as u64 {
            return; // Entirely within head — no fetch needed
        }

        // Determine which chunks we need
        let fetch_start = start_frame.max(self.head_frames as u64);
        let first_chunk = fetch_start / CHUNK_FRAMES as u64;
        let last_chunk = end_frame.saturating_sub(1) / CHUNK_FRAMES as u64;

        for chunk_idx in first_chunk..=last_chunk {
            // Skip chunks already cached, but touch LRU to protect from eviction
            // while we await fetching subsequent chunks
            {
                let mut cache = self.cache.borrow_mut();
                if cache.contains(chunk_idx) {
                    cache.touch(chunk_idx);
                    continue;
                }
            }
            // Compute byte range for this chunk
            let chunk_start_frame = chunk_idx * CHUNK_FRAMES as u64;
            let chunk_end_frame = (chunk_start_frame + CHUNK_FRAMES as u64).min(self.total_frames);
            let frames_in_chunk = (chunk_end_frame - chunk_start_frame) as usize;

            let byte_start = self.info.data_offset
                + chunk_start_frame * self.info.bytes_per_frame as u64;
            let byte_len = frames_in_chunk as u64 * self.info.bytes_per_frame as u64;

            // Read raw bytes from file via the appropriate method
            let bytes = match &self.handle {
                FileHandle::WebFile(file) => {
                    read_blob_range(file, byte_start as f64, (byte_start + byte_len) as f64).await
                }
                FileHandle::TauriPath(path) => {
                    crate::tauri_bridge::read_file_range(path, byte_start, byte_len).await
                }
            };
            let bytes = match bytes {
                Ok(b) => b,
                Err(e) => {
                    log::warn!("StreamingWavSource: prefetch chunk {} failed: {}", chunk_idx, e);
                    continue;
                }
            };

            // Decode PCM bytes to f32 samples
            let interleaved = decode_pcm_bytes(&bytes, &self.info);
            let channels = self.info.channels as usize;

            let (mono, raw) = if channels == 1 {
                (interleaved, None)
            } else {
                let mono = mix_to_mono(&interleaved, channels);
                (mono, Some(interleaved))
            };

            self.cache.borrow_mut().insert(chunk_idx, CachedChunk { mono, raw });
        }
    }

    /// Read mono samples from the head buffer.
    fn read_head_mono(&self, start: u64, buf: &mut [f32]) -> usize {
        let start = start as usize;
        let avail = self.head_frames.saturating_sub(start);
        let n = buf.len().min(avail);
        buf[..n].copy_from_slice(&self.head_mono[start..start + n]);
        n
    }

    /// Read mono samples from the cache (or return 0s for uncached regions).
    fn read_cached_mono(&self, start: u64, buf: &mut [f32]) -> usize {
        let end = (start + buf.len() as u64).min(self.total_frames);
        let total_to_read = (end - start) as usize;
        let mut written = 0usize;
        let mut cache = self.cache.borrow_mut();

        while written < total_to_read {
            let frame = start + written as u64;
            let chunk_idx = frame / CHUNK_FRAMES as u64;
            let offset_in_chunk = (frame % CHUNK_FRAMES as u64) as usize;

            let chunk_end_frame = ((chunk_idx + 1) * CHUNK_FRAMES as u64).min(self.total_frames);
            let avail_in_chunk = (chunk_end_frame - frame) as usize;
            let to_read = avail_in_chunk.min(total_to_read - written);

            if let Some(chunk) = cache.get(chunk_idx) {
                let src = &chunk.mono[offset_in_chunk..offset_in_chunk + to_read];
                buf[written..written + to_read].copy_from_slice(src);
            } else {
                // Not cached — fill with silence
                for s in &mut buf[written..written + to_read] {
                    *s = 0.0;
                }
            }

            written += to_read;
        }

        total_to_read
    }

    /// Read raw interleaved samples from cache for a specific channel.
    fn read_cached_channel(&self, ch: u32, start: u64, buf: &mut [f32]) -> usize {
        let end = (start + buf.len() as u64).min(self.total_frames);
        let total_to_read = (end - start) as usize;
        let channels = self.info.channels as usize;
        let mut written = 0usize;
        let mut cache = self.cache.borrow_mut();

        while written < total_to_read {
            let frame = start + written as u64;
            let chunk_idx = frame / CHUNK_FRAMES as u64;
            let offset_in_chunk = (frame % CHUNK_FRAMES as u64) as usize;

            let chunk_end_frame = ((chunk_idx + 1) * CHUNK_FRAMES as u64).min(self.total_frames);
            let avail_in_chunk = (chunk_end_frame - frame) as usize;
            let to_read = avail_in_chunk.min(total_to_read - written);

            if let Some(chunk) = cache.get(chunk_idx) {
                if let Some(raw) = &chunk.raw {
                    for i in 0..to_read {
                        buf[written + i] = raw[(offset_in_chunk + i) * channels + ch as usize];
                    }
                } else {
                    // Mono file — raw not stored, use mono
                    let src = &chunk.mono[offset_in_chunk..offset_in_chunk + to_read];
                    buf[written..written + to_read].copy_from_slice(src);
                }
            } else {
                for s in &mut buf[written..written + to_read] {
                    *s = 0.0;
                }
            }

            written += to_read;
        }

        total_to_read
    }

    /// Read head raw interleaved samples for a specific channel.
    fn read_head_channel(&self, ch: u32, start: u64, buf: &mut [f32]) -> usize {
        let start = start as usize;
        let avail = self.head_frames.saturating_sub(start);
        let n = buf.len().min(avail);

        if let Some(raw) = &self.head_raw {
            let channels = self.info.channels as usize;
            for i in 0..n {
                buf[i] = raw[(start + i) * channels + ch as usize];
            }
        } else {
            // Mono — same as mono mix
            buf[..n].copy_from_slice(&self.head_mono[start..start + n]);
        }
        n
    }
}

impl AudioSource for StreamingWavSource {
    fn total_samples(&self) -> u64 {
        self.total_frames
    }

    fn sample_rate(&self) -> u32 {
        self.info.sample_rate
    }

    fn channel_count(&self) -> u32 {
        self.info.channels
    }

    fn is_fully_loaded(&self) -> bool {
        false
    }

    fn read_samples(
        &self,
        channel: ChannelView,
        start: u64,
        buf: &mut [f32],
    ) -> usize {
        let end = (start + buf.len() as u64).min(self.total_frames);
        if end <= start {
            return 0;
        }
        let total_len = (end - start) as usize;
        let buf = &mut buf[..total_len];

        let head_end = self.head_frames as u64;

        match channel {
            ChannelView::Stereo | ChannelView::MonoMix => {
                if end <= head_end {
                    // Entirely within head
                    self.read_head_mono(start, buf)
                } else if start >= head_end {
                    // Entirely in cache region
                    self.read_cached_mono(start, buf)
                } else {
                    // Spans head and cache
                    let head_part = (head_end - start) as usize;
                    self.read_head_mono(start, &mut buf[..head_part]);
                    self.read_cached_mono(head_end, &mut buf[head_part..]);
                    total_len
                }
            }
            ChannelView::Channel(ch) => {
                if ch >= self.info.channels {
                    for s in buf.iter_mut() { *s = 0.0; }
                    return total_len;
                }
                if end <= head_end {
                    self.read_head_channel(ch, start, buf)
                } else if start >= head_end {
                    self.read_cached_channel(ch, start, buf)
                } else {
                    let head_part = (head_end - start) as usize;
                    self.read_head_channel(ch, start, &mut buf[..head_part]);
                    self.read_cached_channel(ch, head_end, &mut buf[head_part..]);
                    total_len
                }
            }
            ChannelView::Difference => {
                if self.info.channels < 2 {
                    for s in buf.iter_mut() { *s = 0.0; }
                    return total_len;
                }
                // Read L and R separately, compute difference
                let mut left = vec![0.0f32; total_len];
                let mut right = vec![0.0f32; total_len];
                self.read_samples(ChannelView::Channel(0), start, &mut left);
                self.read_samples(ChannelView::Channel(1), start, &mut right);
                for i in 0..total_len {
                    buf[i] = left[i] - right[i];
                }
                total_len
            }
        }
    }

    fn as_contiguous(&self) -> Option<&[f32]> {
        None
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// ─── PCM decoding ───────────────────────────────────────────────────────────

/// Decode raw PCM bytes into interleaved f32 samples.
fn decode_pcm_bytes(bytes: &[u8], info: &WavFormatInfo) -> Vec<f32> {
    match (info.is_float, info.bits_per_sample) {
        (true, 32) => {
            bytes
                .chunks_exact(4)
                .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                .collect()
        }
        (false, 16) => {
            let max = 32768.0f32;
            bytes
                .chunks_exact(2)
                .map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / max)
                .collect()
        }
        (false, 24) => {
            let max = 8388608.0f32; // 2^23
            bytes
                .chunks_exact(3)
                .map(|b| {
                    // Sign-extend 24-bit to 32-bit
                    let val = (b[0] as i32) | ((b[1] as i32) << 8) | ((b[2] as i32) << 16);
                    let val = if val & 0x800000 != 0 { val | !0xFFFFFF } else { val };
                    val as f32 / max
                })
                .collect()
        }
        (false, 32) => {
            let max = 2147483648.0f32; // 2^31
            bytes
                .chunks_exact(4)
                .map(|b| i32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f32 / max)
                .collect()
        }
        _ => {
            log::warn!(
                "Unsupported PCM format: {}bit {}",
                info.bits_per_sample,
                if info.is_float { "float" } else { "int" }
            );
            vec![0.0; bytes.len() / (info.bits_per_sample as usize / 8)]
        }
    }
}

/// Mix interleaved multi-channel samples to mono.
pub(crate) fn mix_to_mono(samples: &[f32], channels: usize) -> Vec<f32> {
    let inv = 1.0 / channels as f32;
    samples
        .chunks_exact(channels)
        .map(|frame| frame.iter().sum::<f32>() * inv)
        .collect()
}

// ─── File.slice() async helper ──────────────────────────────────────────────

// ─── Streaming FLAC source ──────────────────────────────────────────────────

/// Streaming audio source for FLAC files.
///
/// FLAC is compressed and frame-based, so we can't do random byte-offset access.
/// Instead we decode sequentially in 4 MB windows, storing decoded chunks in the
/// same LRU cache used by WAV streaming. A background task progressively decodes
/// the entire file; on-demand prefetch fast-forwards the decode cursor if needed.
pub struct StreamingFlacSource {
    handle: FileHandle,
    sample_rate: u32,
    channels: u32,
    bits_per_sample: u16,
    total_frames: u64,
    #[allow(dead_code)]
    first_frame_offset: u64,
    max_frame_size: u32,
    head_mono: Arc<Vec<f32>>,
    head_raw: Option<Arc<Vec<f32>>>,
    head_frames: usize,
    cache: RefCell<ChunkCache>,
    /// File byte offset for the next compressed read window.
    decode_byte_cursor: RefCell<u64>,
    /// Per-channel frames decoded so far (beyond the head).
    decode_frame_cursor: RefCell<u64>,
}

// SAFETY: WASM is single-threaded; these are required by AudioSource: Send + Sync.
unsafe impl Send for StreamingFlacSource {}
unsafe impl Sync for StreamingFlacSource {}

impl std::fmt::Debug for StreamingFlacSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamingFlacSource")
            .field("total_frames", &self.total_frames)
            .field("head_frames", &self.head_frames)
            .field("sample_rate", &self.sample_rate)
            .field("channels", &self.channels)
            .field("decode_frame_cursor", &*self.decode_frame_cursor.borrow())
            .finish()
    }
}

/// Size of each compressed read window for FLAC streaming (4 MB).
const FLAC_WINDOW_BYTES: u64 = 4 * 1024 * 1024;

impl StreamingFlacSource {
    /// Create a new streaming FLAC source.
    ///
    /// - `handle`: browser File or native path
    /// - `header`: parsed FLAC header
    /// - `head_mono`: mono-mixed samples for the first ~30s
    /// - `head_raw`: raw interleaved samples (None for mono)
    /// - `initial_byte_cursor`: byte offset where head decoding stopped
    /// - `initial_frame_cursor`: frames decoded in the head region
    pub fn new(
        handle: FileHandle,
        header: &FlacHeader,
        head_mono: Vec<f32>,
        head_raw: Option<Vec<f32>>,
        initial_byte_cursor: u64,
        initial_frame_cursor: u64,
    ) -> Self {
        let head_frames = head_mono.len();
        Self {
            handle,
            sample_rate: header.sample_rate,
            channels: header.channels as u32,
            bits_per_sample: header.bits_per_sample,
            total_frames: header.total_frames,
            first_frame_offset: header.first_frame_offset,
            max_frame_size: header.max_frame_size,
            head_mono: Arc::new(head_mono),
            head_raw: head_raw.map(Arc::new),
            head_frames,
            cache: RefCell::new(ChunkCache::new()),
            decode_byte_cursor: RefCell::new(initial_byte_cursor),
            decode_frame_cursor: RefCell::new(initial_frame_cursor),
        }
    }

    /// Async: ensure all chunks covering `[start_frame, start_frame + len)` are cached.
    ///
    /// For FLAC this may need to decode forward from the current cursor position.
    pub async fn prefetch_region(&self, start_frame: u64, len: usize) {
        let end_frame = (start_frame + len as u64).min(self.total_frames);
        if end_frame <= self.head_frames as u64 {
            return; // Entirely within head
        }

        // Check if everything requested is already cached
        let fetch_start = start_frame.max(self.head_frames as u64);
        let first_chunk = fetch_start / CHUNK_FRAMES as u64;
        let last_chunk = end_frame.saturating_sub(1) / CHUNK_FRAMES as u64;

        let all_cached = {
            let mut cache = self.cache.borrow_mut();
            let mut ok = true;
            for ci in first_chunk..=last_chunk {
                if cache.contains(ci) {
                    cache.touch(ci);
                } else {
                    ok = false;
                    break;
                }
            }
            ok
        };
        if all_cached {
            return;
        }

        // Need to decode forward until we've covered end_frame
        while *self.decode_frame_cursor.borrow() < end_frame {
            if self.decode_one_window().await.is_err() {
                break;
            }
        }
    }

    /// Decode one window of compressed FLAC data (~4 MB), storing results in cache.
    /// Returns Ok(frames_decoded) or Err if at EOF or read failed.
    async fn decode_one_window(&self) -> Result<usize, String> {
        let byte_cursor = *self.decode_byte_cursor.borrow();
        let frame_cursor = *self.decode_frame_cursor.borrow();

        if frame_cursor >= self.total_frames {
            return Err("Already fully decoded".into());
        }

        // Safety overlap: re-read some bytes before the cursor to catch frames
        // that straddled the previous window boundary.
        let overlap = if self.max_frame_size > 0 {
            self.max_frame_size as u64
        } else {
            65535 // max possible FLAC frame size
        };
        let read_start = byte_cursor.saturating_sub(overlap);
        let read_end = read_start + FLAC_WINDOW_BYTES + overlap;

        let bytes = match &self.handle {
            FileHandle::WebFile(file) => {
                read_blob_range(file, read_start as f64, read_end as f64).await
            }
            FileHandle::TauriPath(path) => {
                crate::tauri_bridge::read_file_range(path, read_start, read_end - read_start).await
            }
        };
        let bytes = match bytes {
            Ok(b) if b.is_empty() => return Err("EOF: no bytes read".into()),
            Ok(b) => b,
            Err(e) => return Err(format!("FLAC window read failed: {}", e)),
        };

        let bytes_len = bytes.len() as u64;

        // Create a FrameReader over the raw bytes.
        // FrameReader scans for FLAC sync codes (0xFFF8/0xFFF9), so it can
        // start decoding from the middle of a stream without a STREAMINFO header.
        let buf_reader = claxon::input::BufferedReader::new(std::io::Cursor::new(bytes));
        let mut frame_reader = claxon::frame::FrameReader::new(buf_reader);

        let channels = self.channels as usize;
        let max_val = (1u32 << (self.bits_per_sample - 1)) as f32;
        let mut total_new_frames = 0usize;

        // Accumulate interleaved f32 samples, then flush to cache in CHUNK_FRAMES-sized pieces
        let mut pending_interleaved: Vec<f32> = Vec::new();
        let mut pending_start_frame = frame_cursor;

        let mut block_buf = Vec::new();
        loop {
            match frame_reader.read_next_or_eof(block_buf) {
                Ok(Some(block)) => {
                    let block_time = block.time();
                    let block_duration = block.duration() as u64;

                    // Skip blocks from the overlap region that we already decoded
                    if block_time + block_duration <= frame_cursor {
                        block_buf = block.into_buffer();
                        continue;
                    }

                    // Convert block samples to interleaved f32
                    let n_frames = block.duration() as usize;
                    for frame_idx in 0..n_frames {
                        for ch in 0..channels {
                            let sample = block.sample(ch as u32, frame_idx as u32);
                            pending_interleaved.push(sample as f32 / max_val);
                        }
                    }
                    total_new_frames += n_frames;

                    // Flush complete CHUNK_FRAMES-sized chunks to cache
                    loop {
                        let pending_mono_frames = pending_interleaved.len() / channels;
                        if pending_mono_frames < CHUNK_FRAMES {
                            break;
                        }
                        let take_samples = CHUNK_FRAMES * channels;
                        let chunk_interleaved: Vec<f32> =
                            pending_interleaved.drain(..take_samples).collect();

                        let (mono, raw) = if channels == 1 {
                            (chunk_interleaved, None)
                        } else {
                            let mono = mix_to_mono(&chunk_interleaved, channels);
                            (mono, Some(chunk_interleaved))
                        };

                        let chunk_idx = pending_start_frame / CHUNK_FRAMES as u64;
                        self.cache
                            .borrow_mut()
                            .insert(chunk_idx, CachedChunk { mono, raw });
                        pending_start_frame += CHUNK_FRAMES as u64;
                    }

                    block_buf = block.into_buffer();
                }
                Ok(None) => break,  // Clean EOF
                Err(_) => break,    // Partial frame at window boundary — expected
            }
        }

        // Advance byte cursor: move forward by window size (minus overlap for next window)
        // We don't try to track exact byte position — just advance by the
        // non-overlap portion of the window and let the next window's overlap
        // re-decode any straddled frames.
        let advance = FLAC_WINDOW_BYTES.min(bytes_len);
        let new_byte_cursor = read_start + advance;
        let new_frame_cursor = frame_cursor + total_new_frames as u64;

        *self.decode_byte_cursor.borrow_mut() = new_byte_cursor;
        *self.decode_frame_cursor.borrow_mut() = new_frame_cursor;

        // Flush any remaining pending samples as a partial chunk
        if !pending_interleaved.is_empty() {
            let (mono, raw) = if channels == 1 {
                (pending_interleaved, None)
            } else {
                let mono = mix_to_mono(&pending_interleaved, channels);
                (mono, Some(pending_interleaved))
            };
            let chunk_idx = pending_start_frame / CHUNK_FRAMES as u64;
            self.cache
                .borrow_mut()
                .insert(chunk_idx, CachedChunk { mono, raw });
        }

        if total_new_frames == 0 {
            Err("No new frames decoded".into())
        } else {
            Ok(total_new_frames)
        }
    }

    fn read_head_mono(&self, start: u64, buf: &mut [f32]) -> usize {
        let start = start as usize;
        let avail = self.head_frames.saturating_sub(start);
        let n = buf.len().min(avail);
        buf[..n].copy_from_slice(&self.head_mono[start..start + n]);
        n
    }

    fn read_cached_mono(&self, start: u64, buf: &mut [f32]) -> usize {
        let end = (start + buf.len() as u64).min(self.total_frames);
        let total_to_read = (end - start) as usize;
        let mut written = 0usize;
        let mut cache = self.cache.borrow_mut();

        while written < total_to_read {
            let frame = start + written as u64;
            let chunk_idx = frame / CHUNK_FRAMES as u64;
            let offset_in_chunk = (frame % CHUNK_FRAMES as u64) as usize;
            let chunk_end_frame = ((chunk_idx + 1) * CHUNK_FRAMES as u64).min(self.total_frames);
            let avail_in_chunk = (chunk_end_frame - frame) as usize;
            let to_read = avail_in_chunk.min(total_to_read - written);

            if let Some(chunk) = cache.get(chunk_idx) {
                if offset_in_chunk + to_read <= chunk.mono.len() {
                    buf[written..written + to_read]
                        .copy_from_slice(&chunk.mono[offset_in_chunk..offset_in_chunk + to_read]);
                } else {
                    // Partial chunk — read what's available, silence the rest
                    let avail = chunk.mono.len().saturating_sub(offset_in_chunk);
                    let n = avail.min(to_read);
                    if n > 0 {
                        buf[written..written + n]
                            .copy_from_slice(&chunk.mono[offset_in_chunk..offset_in_chunk + n]);
                    }
                    for s in &mut buf[written + n..written + to_read] {
                        *s = 0.0;
                    }
                }
            } else {
                for s in &mut buf[written..written + to_read] {
                    *s = 0.0;
                }
            }
            written += to_read;
        }
        total_to_read
    }

    fn read_cached_channel(&self, ch: u32, start: u64, buf: &mut [f32]) -> usize {
        let end = (start + buf.len() as u64).min(self.total_frames);
        let total_to_read = (end - start) as usize;
        let channels = self.channels as usize;
        let mut written = 0usize;
        let mut cache = self.cache.borrow_mut();

        while written < total_to_read {
            let frame = start + written as u64;
            let chunk_idx = frame / CHUNK_FRAMES as u64;
            let offset_in_chunk = (frame % CHUNK_FRAMES as u64) as usize;
            let chunk_end_frame = ((chunk_idx + 1) * CHUNK_FRAMES as u64).min(self.total_frames);
            let avail_in_chunk = (chunk_end_frame - frame) as usize;
            let to_read = avail_in_chunk.min(total_to_read - written);

            if let Some(chunk) = cache.get(chunk_idx) {
                if let Some(raw) = &chunk.raw {
                    for i in 0..to_read {
                        let idx = (offset_in_chunk + i) * channels + ch as usize;
                        buf[written + i] = if idx < raw.len() { raw[idx] } else { 0.0 };
                    }
                } else {
                    // Mono — use mono data
                    let end_idx = (offset_in_chunk + to_read).min(chunk.mono.len());
                    let start_idx = offset_in_chunk.min(end_idx);
                    let n = end_idx - start_idx;
                    buf[written..written + n]
                        .copy_from_slice(&chunk.mono[start_idx..end_idx]);
                    for s in &mut buf[written + n..written + to_read] {
                        *s = 0.0;
                    }
                }
            } else {
                for s in &mut buf[written..written + to_read] {
                    *s = 0.0;
                }
            }
            written += to_read;
        }
        total_to_read
    }

    fn read_head_channel(&self, ch: u32, start: u64, buf: &mut [f32]) -> usize {
        let start = start as usize;
        let avail = self.head_frames.saturating_sub(start);
        let n = buf.len().min(avail);
        if let Some(raw) = &self.head_raw {
            let channels = self.channels as usize;
            for i in 0..n {
                buf[i] = raw[(start + i) * channels + ch as usize];
            }
        } else {
            buf[..n].copy_from_slice(&self.head_mono[start..start + n]);
        }
        n
    }

    /// Check if background decode has finished the entire file.
    pub fn is_fully_decoded(&self) -> bool {
        *self.decode_frame_cursor.borrow() >= self.total_frames
    }

    /// Get the current decode frame cursor value (for background decode progress).
    pub fn decode_frame_cursor_value(&self) -> u64 {
        *self.decode_frame_cursor.borrow()
    }
}

impl AudioSource for StreamingFlacSource {
    fn total_samples(&self) -> u64 {
        self.total_frames
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn channel_count(&self) -> u32 {
        self.channels
    }

    fn is_fully_loaded(&self) -> bool {
        false
    }

    fn read_samples(&self, channel: ChannelView, start: u64, buf: &mut [f32]) -> usize {
        let end = (start + buf.len() as u64).min(self.total_frames);
        if end <= start {
            return 0;
        }
        let total_len = (end - start) as usize;
        let buf = &mut buf[..total_len];
        let head_end = self.head_frames as u64;

        match channel {
            ChannelView::Stereo | ChannelView::MonoMix => {
                if end <= head_end {
                    self.read_head_mono(start, buf)
                } else if start >= head_end {
                    self.read_cached_mono(start, buf)
                } else {
                    let head_part = (head_end - start) as usize;
                    self.read_head_mono(start, &mut buf[..head_part]);
                    self.read_cached_mono(head_end, &mut buf[head_part..]);
                    total_len
                }
            }
            ChannelView::Channel(ch) => {
                if ch >= self.channels {
                    for s in buf.iter_mut() { *s = 0.0; }
                    return total_len;
                }
                if end <= head_end {
                    self.read_head_channel(ch, start, buf)
                } else if start >= head_end {
                    self.read_cached_channel(ch, start, buf)
                } else {
                    let head_part = (head_end - start) as usize;
                    self.read_head_channel(ch, start, &mut buf[..head_part]);
                    self.read_cached_channel(ch, head_end, &mut buf[head_part..]);
                    total_len
                }
            }
            ChannelView::Difference => {
                if self.channels < 2 {
                    for s in buf.iter_mut() { *s = 0.0; }
                    return total_len;
                }
                let mut left = vec![0.0f32; total_len];
                let mut right = vec![0.0f32; total_len];
                self.read_samples(ChannelView::Channel(0), start, &mut left);
                self.read_samples(ChannelView::Channel(1), start, &mut right);
                for i in 0..total_len {
                    buf[i] = left[i] - right[i];
                }
                total_len
            }
        }
    }

    fn as_contiguous(&self) -> Option<&[f32]> {
        None
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// ─── Streaming helpers (format-agnostic) ────────────────────────────────────

/// Prefetch a sample region from a streaming source (WAV, FLAC, MP3, or OGG).
/// No-op for in-memory sources.
pub async fn prefetch_streaming(source: &dyn AudioSource, start: u64, len: usize) {
    if let Some(s) = source.as_any().downcast_ref::<StreamingWavSource>() {
        s.prefetch_region(start, len).await;
    } else if let Some(s) = source.as_any().downcast_ref::<StreamingFlacSource>() {
        s.prefetch_region(start, len).await;
    } else if let Some(s) = source.as_any().downcast_ref::<StreamingMp3Source>() {
        s.prefetch_region(start, len).await;
    } else if let Some(s) = source.as_any().downcast_ref::<StreamingOggSource>() {
        s.prefetch_region(start, len).await;
    }
}

/// Check if a source is a streaming (non-in-memory) source.
pub fn is_streaming(source: &dyn AudioSource) -> bool {
    source.as_any().downcast_ref::<StreamingWavSource>().is_some()
        || source.as_any().downcast_ref::<StreamingFlacSource>().is_some()
        || source.as_any().downcast_ref::<StreamingMp3Source>().is_some()
        || source.as_any().downcast_ref::<StreamingOggSource>().is_some()
}

/// Read a byte range from a `web_sys::File` using `File.slice()` + `FileReader`.
///
/// `start` and `end` are byte offsets (like `File.slice(start, end)`).
pub async fn read_blob_range(file: &web_sys::File, start: f64, end: f64) -> Result<Vec<u8>, String> {
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;

    let blob = file
        .slice_with_f64_and_f64(start, end)
        .map_err(|e| format!("File.slice failed: {:?}", e))?;

    let reader = web_sys::FileReader::new()
        .map_err(|e| format!("FileReader::new failed: {:?}", e))?;

    let reader_clone = reader.clone();
    let promise = js_sys::Promise::new(&mut |resolve, reject| {
        let resolve_clone = resolve.clone();
        let reject_clone = reject.clone();

        let onload = wasm_bindgen::closure::Closure::once(move |_: web_sys::Event| {
            resolve_clone.call0(&wasm_bindgen::JsValue::NULL).unwrap();
        });
        let onerror = wasm_bindgen::closure::Closure::once(move |_: web_sys::Event| {
            reject_clone.call0(&wasm_bindgen::JsValue::NULL).unwrap();
        });

        reader_clone.set_onloadend(Some(onload.as_ref().unchecked_ref()));
        reader_clone.set_onerror(Some(onerror.as_ref().unchecked_ref()));

        onload.forget();
        onerror.forget();
    });

    reader
        .read_as_array_buffer(&blob)
        .map_err(|e| format!("read_as_array_buffer failed: {:?}", e))?;

    JsFuture::from(promise)
        .await
        .map_err(|e| format!("FileReader await failed: {:?}", e))?;

    let result = reader
        .result()
        .map_err(|e| format!("FileReader.result() failed: {:?}", e))?;
    let array_buffer = result
        .dyn_into::<js_sys::ArrayBuffer>()
        .map_err(|_| "Expected ArrayBuffer from FileReader".to_string())?;
    let uint8 = js_sys::Uint8Array::new(&array_buffer);
    Ok(uint8.to_vec())
}
