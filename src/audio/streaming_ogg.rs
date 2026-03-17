//! Streaming OGG/Vorbis source — progressive decode via symphonia.

use std::cell::RefCell;
use std::sync::Arc;

use crate::audio::loader::OggHeader;
use crate::audio::source::{AudioSource, ChannelView};
use super::streaming_source::{FileHandle, ChunkCache, CachedChunk, CHUNK_FRAMES, mix_to_mono, read_blob_range};

/// Size of each compressed read window for OGG streaming (4 MB).
const OGG_WINDOW_BYTES: u64 = 4 * 1024 * 1024;

/// Overlap bytes for OGG window reads. Vorbis frames can be larger than MP3
/// frames, so use a generous overlap.
const OGG_OVERLAP_BYTES: u64 = 8192;

/// Streaming audio source for OGG/Vorbis files.
///
/// Like FLAC and MP3, OGG/Vorbis is compressed and frame-based, so we decode
/// sequentially in 4 MB windows, storing decoded chunks in the same LRU cache.
pub struct StreamingOggSource {
    handle: FileHandle,
    sample_rate: u32,
    channels: u32,
    file_size: u64,
    /// Total per-channel frames. May be estimated (refined when decode finishes).
    total_frames: RefCell<u64>,
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
unsafe impl Send for StreamingOggSource {}
unsafe impl Sync for StreamingOggSource {}

impl std::fmt::Debug for StreamingOggSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamingOggSource")
            .field("total_frames", &*self.total_frames.borrow())
            .field("head_frames", &self.head_frames)
            .field("sample_rate", &self.sample_rate)
            .field("channels", &self.channels)
            .field("decode_frame_cursor", &*self.decode_frame_cursor.borrow())
            .finish()
    }
}

impl StreamingOggSource {
    pub fn new(
        handle: FileHandle,
        header: &OggHeader,
        head_mono: Vec<f32>,
        head_raw: Option<Vec<f32>>,
        file_size: u64,
        initial_byte_cursor: u64,
        initial_frame_cursor: u64,
    ) -> Self {
        let head_frames = head_mono.len();
        Self {
            handle,
            sample_rate: header.sample_rate,
            channels: header.channels as u32,
            file_size,
            total_frames: RefCell::new(header.estimated_total_frames),
            head_mono: Arc::new(head_mono),
            head_raw: head_raw.map(Arc::new),
            head_frames,
            cache: RefCell::new(ChunkCache::new()),
            decode_byte_cursor: RefCell::new(initial_byte_cursor),
            decode_frame_cursor: RefCell::new(initial_frame_cursor),
        }
    }

    /// Async: ensure all chunks covering `[start_frame, start_frame + len)` are cached.
    pub async fn prefetch_region(&self, start_frame: u64, len: usize) {
        let total = *self.total_frames.borrow();
        let end_frame = (start_frame + len as u64).min(total);
        if end_frame <= self.head_frames as u64 {
            return;
        }

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

        while *self.decode_frame_cursor.borrow() < end_frame {
            if self.decode_one_window().await.is_err() {
                break;
            }
        }
    }

    /// Decode one window of compressed OGG data (~4 MB), storing results in cache.
    async fn decode_one_window(&self) -> Result<usize, String> {
        use symphonia::core::audio::SampleBuffer;
        use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
        use symphonia::core::errors::Error as SymphoniaError;
        use symphonia::core::formats::FormatOptions;
        use symphonia::core::io::MediaSourceStream;
        use symphonia::core::meta::MetadataOptions;
        use symphonia::core::probe::Hint;

        let byte_cursor = *self.decode_byte_cursor.borrow();
        let frame_cursor = *self.decode_frame_cursor.borrow();

        if byte_cursor >= self.file_size {
            let mut tf = self.total_frames.borrow_mut();
            if frame_cursor < *tf {
                *tf = frame_cursor;
            }
            return Err("Already at EOF".into());
        }

        let read_start = byte_cursor.saturating_sub(OGG_OVERLAP_BYTES);
        let read_end = (read_start + OGG_WINDOW_BYTES + OGG_OVERLAP_BYTES).min(self.file_size);

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
            Err(e) => return Err(format!("OGG window read failed: {}", e)),
        };

        let overlap_bytes = byte_cursor - read_start;

        let cursor = std::io::Cursor::new(bytes);
        let mss = MediaSourceStream::new(Box::new(cursor), Default::default());

        let mut hint = Hint::new();
        hint.with_extension("ogg");

        let probed = symphonia::default::get_probe()
            .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
            .map_err(|e| format!("OGG window probe error: {e}"))?;

        let mut format = probed.format;
        let track = format
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
            .ok_or("No audio track in OGG window")?;
        let track_id = track.id;

        let mut decoder = symphonia::default::get_codecs()
            .make(&track.codec_params, &DecoderOptions::default())
            .map_err(|e| format!("OGG window decoder error: {e}"))?;

        let channels = self.channels as usize;
        let mut total_new_frames = 0usize;
        let mut pending_interleaved: Vec<f32> = Vec::new();
        let mut pending_start_frame = frame_cursor;
        let mut window_byte_pos: u64 = 0;

        loop {
            let packet = match format.next_packet() {
                Ok(p) => p,
                Err(SymphoniaError::ResetRequired) => {
                    decoder.reset();
                    continue;
                }
                Err(SymphoniaError::IoError(e))
                    if e.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    break;
                }
                Err(_) => break,
            };

            if packet.track_id() != track_id {
                continue;
            }

            window_byte_pos += packet.buf().len() as u64;

            // Skip packets that fall within the overlap region (already decoded)
            if window_byte_pos <= overlap_bytes && overlap_bytes > 0 {
                continue;
            }

            match decoder.decode(&packet) {
                Ok(decoded) => {
                    let spec = *decoded.spec();
                    let mut buf = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
                    buf.copy_interleaved_ref(decoded);
                    let samples = buf.samples();

                    let n_frames = samples.len() / channels;
                    pending_interleaved.extend_from_slice(samples);
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
                }
                Err(SymphoniaError::DecodeError(_)) => continue,
                Err(_) => break,
            }
        }

        // Advance cursors
        let advance = OGG_WINDOW_BYTES.min((read_end - read_start) as u64);
        let new_byte_cursor = read_start + advance;
        let new_frame_cursor = frame_cursor + total_new_frames as u64;

        *self.decode_byte_cursor.borrow_mut() = new_byte_cursor;
        *self.decode_frame_cursor.borrow_mut() = new_frame_cursor;

        // Update total_frames if we've reached EOF and decoded more than estimated
        if new_byte_cursor >= self.file_size {
            let mut tf = self.total_frames.borrow_mut();
            *tf = new_frame_cursor;
        } else if new_frame_cursor > *self.total_frames.borrow() {
            *self.total_frames.borrow_mut() = new_frame_cursor;
        }

        // Flush remaining partial chunk
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
            let mut tf = self.total_frames.borrow_mut();
            *tf = frame_cursor;
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
        let total = *self.total_frames.borrow();
        let end = (start + buf.len() as u64).min(total);
        let total_to_read = (end - start) as usize;
        let mut written = 0usize;
        let mut cache = self.cache.borrow_mut();

        while written < total_to_read {
            let frame = start + written as u64;
            let chunk_idx = frame / CHUNK_FRAMES as u64;
            let offset_in_chunk = (frame % CHUNK_FRAMES as u64) as usize;
            let chunk_end_frame = ((chunk_idx + 1) * CHUNK_FRAMES as u64).min(total);
            let avail_in_chunk = (chunk_end_frame - frame) as usize;
            let to_read = avail_in_chunk.min(total_to_read - written);

            if let Some(chunk) = cache.get(chunk_idx) {
                if offset_in_chunk + to_read <= chunk.mono.len() {
                    buf[written..written + to_read]
                        .copy_from_slice(&chunk.mono[offset_in_chunk..offset_in_chunk + to_read]);
                } else {
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
        let total = *self.total_frames.borrow();
        let end = (start + buf.len() as u64).min(total);
        let total_to_read = (end - start) as usize;
        let channels = self.channels as usize;
        let mut written = 0usize;
        let mut cache = self.cache.borrow_mut();

        while written < total_to_read {
            let frame = start + written as u64;
            let chunk_idx = frame / CHUNK_FRAMES as u64;
            let offset_in_chunk = (frame % CHUNK_FRAMES as u64) as usize;
            let chunk_end_frame = ((chunk_idx + 1) * CHUNK_FRAMES as u64).min(total);
            let avail_in_chunk = (chunk_end_frame - frame) as usize;
            let to_read = avail_in_chunk.min(total_to_read - written);

            if let Some(chunk) = cache.get(chunk_idx) {
                if let Some(raw) = &chunk.raw {
                    for i in 0..to_read {
                        let idx = (offset_in_chunk + i) * channels + ch as usize;
                        buf[written + i] = if idx < raw.len() { raw[idx] } else { 0.0 };
                    }
                } else {
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

    pub fn is_fully_decoded(&self) -> bool {
        *self.decode_frame_cursor.borrow() >= *self.total_frames.borrow()
    }

    pub fn decode_frame_cursor_value(&self) -> u64 {
        *self.decode_frame_cursor.borrow()
    }
}

impl AudioSource for StreamingOggSource {
    fn total_samples(&self) -> u64 {
        *self.total_frames.borrow()
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
        let total = *self.total_frames.borrow();
        let end = (start + buf.len() as u64).min(total);
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
