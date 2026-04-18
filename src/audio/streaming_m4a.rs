//! Streaming M4A source — on-demand AAC/ALAC decode via symphonia.
//!
//! Unlike MP3 (where each frame is self-contained), M4A needs the `moov`
//! sample table before any frame can be decoded. We pragmatically hold the
//! full compressed bytes in RAM and keep a persistent `FormatReader` alive,
//! seeking + decoding into an LRU chunk cache on demand. Compressed AAC/ALAC
//! is typically 10–20× smaller than decoded f32 PCM, so this still avoids
//! the upfront full-decode cost for long audiobooks.

use std::cell::{Cell, RefCell};
use std::sync::Arc;

use symphonia::core::codecs::Decoder;
use symphonia::core::formats::{FormatReader, SeekMode, SeekTo};
use symphonia::core::units::Time;

use crate::audio::source::{AudioSource, ChannelView};
use super::streaming_source::{FileHandle, ChunkCache, CachedChunk, CHUNK_FRAMES, mix_to_mono};

/// Clears `decoding_in_progress` on scope exit, so an early return or error
/// path can't leave the source locked forever.
struct DecodeGuard<'a>(&'a Cell<bool>);
impl Drop for DecodeGuard<'_> {
    fn drop(&mut self) { self.0.set(false); }
}

/// Reader state held across prefetch calls. Not thread-safe — WASM is
/// single-threaded. `RefCell` gives us interior mutability.
struct ReaderState {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn Decoder>,
    track_id: u32,
    /// Frame index of the next packet the reader will return. Tracks symphonia's
    /// internal position so we can decide when to seek vs. decode sequentially.
    next_frame: u64,
}

pub struct StreamingM4aSource {
    #[allow(dead_code)] // kept for parity with other streaming sources
    handle: FileHandle,
    sample_rate: u32,
    channels: u32,
    file_size: u64,
    total_frames: u64,
    head_mono: Arc<Vec<f32>>,
    head_raw: Option<Arc<Vec<f32>>>,
    head_frames: usize,
    cache: RefCell<ChunkCache>,
    reader: RefCell<ReaderState>,
    /// Frame up to which all chunks are guaranteed cached (background decode cursor).
    decode_frame_cursor: Cell<u64>,
    fully_decoded: Cell<bool>,
    /// True while a decode_chunk call is in progress — prevents the background
    /// decoder and a viewport prefetch from concurrently mutating the shared
    /// symphonia reader / decoder state across `.await` points.
    decoding_in_progress: Cell<bool>,
}

// SAFETY: WASM is single-threaded.
unsafe impl Send for StreamingM4aSource {}
unsafe impl Sync for StreamingM4aSource {}

impl std::fmt::Debug for StreamingM4aSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamingM4aSource")
            .field("total_frames", &self.total_frames)
            .field("head_frames", &self.head_frames)
            .field("sample_rate", &self.sample_rate)
            .field("channels", &self.channels)
            .finish()
    }
}

impl StreamingM4aSource {
    pub fn new(
        handle: FileHandle,
        format: Box<dyn FormatReader>,
        decoder: Box<dyn Decoder>,
        track_id: u32,
        sample_rate: u32,
        channels: u32,
        total_frames: u64,
        file_size: u64,
        head_mono: Vec<f32>,
        head_raw: Option<Vec<f32>>,
        initial_next_frame: u64,
    ) -> Self {
        let head_frames = head_mono.len();
        Self {
            handle,
            sample_rate,
            channels,
            file_size,
            total_frames,
            head_mono: Arc::new(head_mono),
            head_raw: head_raw.map(Arc::new),
            head_frames,
            cache: RefCell::new(ChunkCache::new()),
            reader: RefCell::new(ReaderState {
                format,
                decoder,
                track_id,
                next_frame: initial_next_frame,
            }),
            decode_frame_cursor: Cell::new(initial_next_frame),
            fully_decoded: Cell::new(false),
            decoding_in_progress: Cell::new(false),
        }
    }

    pub fn file_size(&self) -> u64 { self.file_size }
    pub fn is_fully_decoded(&self) -> bool { self.fully_decoded.get() }
    pub fn decode_frame_cursor_value(&self) -> u64 { self.decode_frame_cursor.get() }

    /// Ensure all chunks covering `[start_frame, start_frame + len)` are cached.
    /// For M4A we can seek directly to the target region via the sample table —
    /// no need to decode everything in between.
    pub async fn prefetch_region(&self, start_frame: u64, len: usize) {
        let end_frame = (start_frame + len as u64).min(self.total_frames);
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
        if all_cached { return; }

        for chunk_idx in first_chunk..=last_chunk {
            if self.cache.borrow().contains(chunk_idx) { continue; }
            if self.decode_chunk(chunk_idx).await.is_err() {
                break;
            }
            crate::canvas::tile_cache::yield_to_browser().await;
        }
    }

    /// Decode one CHUNK_FRAMES-aligned region into the cache.
    async fn decode_chunk(&self, chunk_idx: u64) -> Result<(), String> {
        use symphonia::core::audio::SampleBuffer;
        use symphonia::core::errors::Error as SymphoniaError;

        let chunk_start = chunk_idx * CHUNK_FRAMES as u64;
        let chunk_end = (chunk_start + CHUNK_FRAMES as u64).min(self.total_frames);
        if chunk_start >= self.total_frames { return Err("Past EOF".into()); }

        // Cooperative mutex: the FormatReader + Decoder are shared mutable
        // state, so two decode_chunk calls cannot interleave across .await
        // points without corrupting the decoder position. Wait for any
        // in-flight decode to finish before starting our own.
        let mut attempts = 0u32;
        while self.decoding_in_progress.get() {
            attempts += 1;
            if attempts > 600 {
                return Err("decode_chunk waited >6s for lock".into());
            }
            let p = js_sys::Promise::new(&mut |resolve, _| {
                web_sys::window().unwrap()
                    .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 10).unwrap();
            });
            wasm_bindgen_futures::JsFuture::from(p).await.ok();
        }
        self.decoding_in_progress.set(true);
        let _guard = DecodeGuard(&self.decoding_in_progress);

        let channels = self.channels as usize;

        // Seek unless we're already *exactly* at chunk_start. The chunk cache
        // is keyed by chunk_idx = frame / CHUNK_FRAMES, so the samples we
        // flush MUST start at chunk_start — otherwise offsets within the
        // chunk point at the wrong frames. Short sequential runs (chunk N
        // followed by chunk N+1) already leave next_frame == chunk_start,
        // so they skip the seek naturally.
        {
            let mut state = self.reader.borrow_mut();
            let need_seek = state.next_frame != chunk_start;
            if need_seek {
                let secs = chunk_start as f64 / self.sample_rate as f64;
                let time = Time::new(secs.trunc() as u64, secs.fract());
                let track_id = state.track_id;
                match state.format.seek(
                    SeekMode::Accurate,
                    SeekTo::Time { time, track_id: Some(track_id) },
                ) {
                    Ok(_seeked) => {
                        // seeked.actual_ts is in the track's TimeBase (container
                        // timescale) which may not match our self.sample_rate
                        // (the decoder's actual output rate) when SBR halves
                        // things. Treat the requested chunk_start as truth so
                        // next_frame stays in the same units we use for
                        // cache/chunk math; any small seek rounding error
                        // shows up as a few ms of silence, which is harmless.
                        state.next_frame = chunk_start;
                        state.decoder.reset();
                    }
                    Err(_) => {
                        // Seek failed — keep sequential position.
                    }
                }
            }
        }

        // Decode packets until we cover at least [chunk_start, chunk_end).
        let mut pending_interleaved: Vec<f32> = Vec::new();
        let mut pending_start_frame = {
            let state = self.reader.borrow();
            state.next_frame
        };
        let mut frames_decoded = 0u64;
        let mut frames_since_yield = 0u64;
        const YIELD_EVERY_FRAMES: u64 = 65_536;

        loop {
            let packet_result = {
                let mut state = self.reader.borrow_mut();
                let track_id = state.track_id;
                loop {
                    match state.format.next_packet() {
                        Ok(p) => {
                            if p.track_id() == track_id {
                                break Some(Ok(p));
                            }
                            continue;
                        }
                        Err(SymphoniaError::ResetRequired) => {
                            state.decoder.reset();
                            continue;
                        }
                        Err(SymphoniaError::IoError(e))
                            if e.kind() == std::io::ErrorKind::UnexpectedEof =>
                        {
                            break None;
                        }
                        Err(e) => break Some(Err(format!("M4A packet error: {e}"))),
                    }
                }
            };

            let packet = match packet_result {
                Some(Ok(p)) => p,
                Some(Err(e)) => return Err(e),
                None => {
                    // EOF — flush what we have and mark done.
                    self.fully_decoded.set(true);
                    break;
                }
            };

            let decoded_samples: Result<Option<Vec<f32>>, String> = {
                let mut state = self.reader.borrow_mut();
                match state.decoder.decode(&packet) {
                    Ok(decoded) => {
                        let spec = *decoded.spec();
                        let mut buf = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
                        buf.copy_interleaved_ref(decoded);
                        Ok(Some(buf.samples().to_vec()))
                    }
                    Err(SymphoniaError::DecodeError(_)) => Ok(None),
                    Err(e) => Err(format!("M4A decode error: {e}")),
                }
            };

            match decoded_samples {
                Ok(Some(samples)) => {
                    let n_frames = samples.len() / channels;
                    pending_interleaved.extend_from_slice(&samples);
                    frames_decoded += n_frames as u64;
                    frames_since_yield += n_frames as u64;
                    {
                        let mut state = self.reader.borrow_mut();
                        state.next_frame = state.next_frame.saturating_add(n_frames as u64);
                    }

                    // Flush complete CHUNK_FRAMES-sized chunks to cache.
                    loop {
                        let pending_frames = pending_interleaved.len() / channels;
                        if pending_frames < CHUNK_FRAMES { break; }
                        let take_samples = CHUNK_FRAMES * channels;
                        let chunk_interleaved: Vec<f32> =
                            pending_interleaved.drain(..take_samples).collect();
                        let (mono, raw) = if channels == 1 {
                            (chunk_interleaved, None)
                        } else {
                            let mono = mix_to_mono(&chunk_interleaved, channels);
                            (mono, Some(chunk_interleaved))
                        };
                        let ci = pending_start_frame / CHUNK_FRAMES as u64;
                        self.cache.borrow_mut().insert(ci, CachedChunk { mono, raw });
                        pending_start_frame += CHUNK_FRAMES as u64;
                        if pending_start_frame > self.decode_frame_cursor.get() {
                            self.decode_frame_cursor.set(pending_start_frame);
                        }
                    }

                    if frames_since_yield >= YIELD_EVERY_FRAMES {
                        frames_since_yield = 0;
                        crate::canvas::tile_cache::yield_to_browser().await;
                    }

                    let now_frame = pending_start_frame + (pending_interleaved.len() / channels) as u64;
                    if now_frame >= chunk_end {
                        break;
                    }
                }
                Ok(None) => continue,
                Err(e) => return Err(e),
            }
        }

        // Flush any remaining partial chunk.
        if !pending_interleaved.is_empty() {
            let (mono, raw) = if channels == 1 {
                (pending_interleaved, None)
            } else {
                let mono = mix_to_mono(&pending_interleaved, channels);
                (mono, Some(pending_interleaved))
            };
            let mono_len = mono.len() as u64;
            let ci = pending_start_frame / CHUNK_FRAMES as u64;
            self.cache.borrow_mut().insert(ci, CachedChunk { mono, raw });
            let end_frame = pending_start_frame + mono_len;
            if end_frame > self.decode_frame_cursor.get() {
                self.decode_frame_cursor.set(end_frame);
            }
        }

        if frames_decoded == 0 && self.fully_decoded.get() {
            return Err("EOF".into());
        }
        Ok(())
    }

    fn read_head_mono(&self, start: u64, buf: &mut [f32]) -> usize {
        let start = start as usize;
        let avail = self.head_frames.saturating_sub(start);
        let n = buf.len().min(avail);
        buf[..n].copy_from_slice(&self.head_mono[start..start + n]);
        n
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

    fn read_cached_mono(&self, start: u64, buf: &mut [f32]) -> usize {
        let end = (start + buf.len() as u64).min(self.total_frames);
        if end <= start { return 0; }
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
                let end_idx = (offset_in_chunk + to_read).min(chunk.mono.len());
                let start_idx = offset_in_chunk.min(end_idx);
                let n = end_idx - start_idx;
                buf[written..written + n].copy_from_slice(&chunk.mono[start_idx..end_idx]);
                for s in &mut buf[written + n..written + to_read] { *s = 0.0; }
            } else {
                for s in &mut buf[written..written + to_read] { *s = 0.0; }
            }
            written += to_read;
        }
        total_to_read
    }

    fn read_cached_channel(&self, ch: u32, start: u64, buf: &mut [f32]) -> usize {
        let end = (start + buf.len() as u64).min(self.total_frames);
        if end <= start { return 0; }
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
                    let end_idx = (offset_in_chunk + to_read).min(chunk.mono.len());
                    let start_idx = offset_in_chunk.min(end_idx);
                    let n = end_idx - start_idx;
                    buf[written..written + n].copy_from_slice(&chunk.mono[start_idx..end_idx]);
                    for s in &mut buf[written + n..written + to_read] { *s = 0.0; }
                }
            } else {
                for s in &mut buf[written..written + to_read] { *s = 0.0; }
            }
            written += to_read;
        }
        total_to_read
    }
}

impl AudioSource for StreamingM4aSource {
    fn total_samples(&self) -> u64 { self.total_frames }
    fn sample_rate(&self) -> u32 { self.sample_rate }
    fn channel_count(&self) -> u32 { self.channels }
    fn is_fully_loaded(&self) -> bool { false }

    fn read_samples(&self, channel: ChannelView, start: u64, buf: &mut [f32]) -> usize {
        let end = (start + buf.len() as u64).min(self.total_frames);
        if end <= start { return 0; }
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

    fn as_contiguous(&self) -> Option<&[f32]> { None }
    fn as_any(&self) -> &dyn std::any::Any { self }
}
