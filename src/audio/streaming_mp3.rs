//! Streaming MP3 source — progressive decode via symphonia.

use std::cell::{Cell, RefCell};
use std::sync::Arc;

use super::streaming_source::{
    mix_to_mono, read_blob_range, CachedChunk, ChunkCache, FileHandle, CHUNK_FRAMES,
};
use crate::audio::source::{AudioSource, ChannelView};

/// Size of each compressed read window for MP3 streaming (4 MB).
const MP3_WINDOW_BYTES: u64 = 4 * 1024 * 1024;

/// Overlap bytes for MP3 window reads.  Must be large enough to contain
/// several complete MP3 frames so the decoder can fill its bit reservoir
/// (main_data_begin can reference up to 511 bytes from prior frames).
/// 32 KB covers ~20+ frames at typical bitrates, plenty for warm-up.
const MP3_OVERLAP_BYTES: u64 = 32 * 1024;

///
/// Like FLAC, MP3 is compressed and frame-based, so we decode sequentially in
/// 4 MB windows, storing decoded chunks in the same LRU cache.
pub struct StreamingMp3Source {
    handle: FileHandle,
    sample_rate: u32,
    channels: u32,
    file_size: u64,
    /// Byte offset where audio frames begin (after ID3v2 tags).
    data_offset: u64,
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
    /// Set when a seek skip happened; cleared after the caller reads it.
    pub(crate) did_seek_skip: Cell<bool>,
    /// Size (bytes) of the first decoded MP3 packet; used to detect VBR.
    first_packet_size: Cell<u32>,
    /// True once we've seen packets of different sizes (variable bitrate).
    pub(crate) is_vbr: Cell<bool>,
    /// Set once decode has reached the real end of the file (byte EOF / empty
    /// read). Decoupled from `total_frames` (which only *estimates* the length
    /// until this is set), so `is_fully_decoded()` reflects true completion
    /// instead of the cursor merely catching up to an under-estimate — the bug
    /// that truncated long header-less MP3s (audit lows #8/#12).
    reached_eof: Cell<bool>,
}

// SAFETY: required by AudioSource: Send + Sync, but this holds RefCell
// caches/cursors so it is NOT inherently Sync. Sound ONLY because the app is
// single-threaded (WASM) and the source is touched from that one thread; unsound
// from a real worker thread. Invariant: never call a read_* method while an
// internal cache.borrow_mut() is alive (BorrowMutError).
unsafe impl Send for StreamingMp3Source {}
unsafe impl Sync for StreamingMp3Source {}

impl std::fmt::Debug for StreamingMp3Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamingMp3Source")
            .field("total_frames", &*self.total_frames.borrow())
            .field("head_frames", &self.head_frames)
            .field("sample_rate", &self.sample_rate)
            .field("channels", &self.channels)
            .field("decode_frame_cursor", &*self.decode_frame_cursor.borrow())
            .finish()
    }
}

impl StreamingMp3Source {
    pub fn new(
        handle: FileHandle,
        header: &crate::audio::loader::Mp3Header,
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
            data_offset: header.data_offset,
            total_frames: RefCell::new(header.estimated_total_frames),
            head_mono: Arc::new(head_mono),
            head_raw: head_raw.map(Arc::new),
            head_frames,
            cache: RefCell::new(ChunkCache::new()),
            decode_byte_cursor: RefCell::new(initial_byte_cursor),
            decode_frame_cursor: RefCell::new(initial_frame_cursor),
            did_seek_skip: Cell::new(false),
            first_packet_size: Cell::new(0),
            is_vbr: Cell::new(false),
            reached_eof: Cell::new(false),
        }
    }

    /// Estimate the byte offset for a given frame number.
    /// Uses linear interpolation over the audio data region (after ID3v2 tag).
    fn estimate_byte_for_frame(&self, frame: u64) -> u64 {
        let total = *self.total_frames.borrow();
        if total == 0 {
            return self.data_offset;
        }
        let audio_bytes = self.file_size.saturating_sub(self.data_offset);
        let ratio = frame as f64 / total as f64;
        self.data_offset + (audio_bytes as f64 * ratio) as u64
    }

    /// Async: ensure all chunks covering `[start_frame, start_frame + len)` are cached.
    pub async fn prefetch_region(&self, start_frame: u64, len: usize) {
        // Cap the decode target at `total_frames` ONLY once the length is exact
        // (reached_eof). While it's still an estimate, do NOT cap: an
        // under-estimate would otherwise stop decoding short of the real end
        // (the cursor reaches the small estimate and the loop exits). Decoding
        // still halts at real EOF via `decode_one_window` returning Err.
        let end_frame = if self.reached_eof.get() {
            (start_frame + len as u64).min(*self.total_frames.borrow())
        } else {
            start_frame + len as u64
        };
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

        // If the target region is far ahead of the current decode cursor,
        // seek directly rather than decoding every window in between.
        // This trades accuracy (VBR byte estimates are approximate) for speed.
        let cursor_frame = *self.decode_frame_cursor.borrow();
        let skip_threshold = MP3_WINDOW_BYTES * 2; // ~8 MB worth of sequential decode
        if fetch_start > cursor_frame {
            let gap_bytes = self
                .estimate_byte_for_frame(fetch_start)
                .saturating_sub(*self.decode_byte_cursor.borrow());
            if gap_bytes > skip_threshold {
                let seek_byte = self.estimate_byte_for_frame(fetch_start);
                *self.decode_byte_cursor.borrow_mut() = seek_byte;
                *self.decode_frame_cursor.borrow_mut() = fetch_start;
                self.did_seek_skip.set(true);
            }
        }

        while *self.decode_frame_cursor.borrow() < end_frame {
            if self.decode_one_window().await.is_err() {
                break;
            }
            // Yield between windows so the UI stays responsive
            crate::canvas::tile_cache::yield_to_browser().await;
        }
    }

    /// Decode one window of compressed MP3 data (~4 MB), storing results in cache.
    async fn decode_one_window(&self) -> Result<usize, String> {
        use symphonia::core::codecs::audio::AudioDecoderOptions;
        use symphonia::core::errors::Error as SymphoniaError;
        use symphonia::core::formats::{probe::Hint, FormatOptions, TrackType};
        use symphonia::core::io::MediaSourceStream;
        use symphonia::core::meta::MetadataOptions;

        let byte_cursor = *self.decode_byte_cursor.borrow();
        let frame_cursor = *self.decode_frame_cursor.borrow();

        if byte_cursor >= self.file_size {
            // Real EOF: `total_frames` is now exactly the decoded count.
            *self.total_frames.borrow_mut() = frame_cursor;
            self.reached_eof.set(true);
            return Err("Already at EOF".into());
        }

        let read_start = byte_cursor.saturating_sub(MP3_OVERLAP_BYTES);
        let read_end = (read_start + MP3_WINDOW_BYTES + MP3_OVERLAP_BYTES).min(self.file_size);

        let bytes = match &self.handle {
            FileHandle::WebFile(file) => {
                read_blob_range(file, read_start as f64, read_end as f64).await
            }
            FileHandle::TauriPath(path) => {
                crate::tauri_bridge::read_file_range(path, read_start, read_end - read_start).await
            }
            FileHandle::Bytes(b) => super::streaming_source::slice_bytes(b, read_start, read_end),
            FileHandle::MediaStoreUri(uri) => {
                super::streaming_source::read_media_store_range(
                    uri,
                    read_start,
                    read_end - read_start,
                )
                .await
            }
        };
        let bytes = match bytes {
            Ok(b) if b.is_empty() => {
                self.reached_eof.set(true);
                return Err("EOF: no bytes read".into());
            }
            Ok(b) => b,
            Err(e) => return Err(format!("MP3 window read failed: {}", e)),
        };

        // Bytes in the overlap region that we need to skip (already decoded)
        let overlap_bytes = byte_cursor - read_start;

        let cursor = std::io::Cursor::new(bytes);
        let mss = MediaSourceStream::new(Box::new(cursor), Default::default());

        let mut hint = Hint::new();
        hint.with_extension("mp3");

        let mut format = symphonia::default::get_probe()
            .probe(
                &hint,
                mss,
                FormatOptions::default(),
                MetadataOptions::default(),
            )
            .map_err(|e| format!("MP3 window probe error: {e}"))?;

        let track = format
            .default_track(TrackType::Audio)
            .ok_or("No audio track in MP3 window")?;
        let audio_params = track
            .codec_params
            .as_ref()
            .and_then(|cp| cp.audio())
            .ok_or("MP3 window missing audio codec parameters")?;
        let track_id = track.id;

        let mut decoder = symphonia::default::get_codecs()
            .make_audio_decoder(audio_params, &AudioDecoderOptions::default())
            .map_err(|e| format!("MP3 window decoder error: {e}"))?;

        let channels = self.channels as usize;
        let mut total_new_frames = 0usize;
        let mut pending_interleaved: Vec<f32> = Vec::new();
        let mut pending_start_frame = frame_cursor;
        let mut window_byte_pos: u64 = 0;
        // Yield every ~64K decoded frames to keep the UI responsive
        let mut frames_since_yield = 0usize;
        const YIELD_EVERY_FRAMES: usize = 65_536;
        let mut scratch: Vec<f32> = Vec::new();

        loop {
            let packet = match format.next_packet() {
                Ok(Some(p)) => p,
                Ok(None) => break,
                Err(SymphoniaError::ResetRequired) => {
                    decoder.reset();
                    continue;
                }
                Err(_) => break,
            };

            if packet.track_id != track_id {
                continue;
            }

            // Track approximate byte position within the window
            window_byte_pos += packet.data.len() as u64;

            // Overlap region: decode to warm up the bit reservoir, but discard output.
            // MP3 frames reference prior frames via main_data_begin; skipping decode
            // leaves the reservoir empty, causing underflow errors on every window.
            let in_overlap = window_byte_pos <= overlap_bytes && overlap_bytes > 0;
            if in_overlap {
                // Decode to build up bit reservoir state, ignore errors and output
                let _ = decoder.decode(&packet);
                continue;
            }

            // Detect VBR by comparing packet sizes
            if !self.is_vbr.get() {
                let pkt_size = packet.data.len() as u32;
                let first = self.first_packet_size.get();
                if first == 0 {
                    self.first_packet_size.set(pkt_size);
                } else if pkt_size != first {
                    self.is_vbr.set(true);
                }
            }

            match decoder.decode(&packet) {
                Ok(decoded) => {
                    decoded.copy_to_vec_interleaved(&mut scratch);
                    let samples: &[f32] = &scratch;

                    let n_frames = samples.len() / channels;
                    pending_interleaved.extend_from_slice(samples);
                    total_new_frames += n_frames;
                    frames_since_yield += n_frames;

                    // Yield periodically so the browser can paint / handle input
                    if frames_since_yield >= YIELD_EVERY_FRAMES {
                        frames_since_yield = 0;
                        crate::canvas::tile_cache::yield_to_browser().await;
                    }

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
        let advance = MP3_WINDOW_BYTES.min(read_end - read_start);
        let new_byte_cursor = read_start + advance;
        let new_frame_cursor = frame_cursor + total_new_frames as u64;

        *self.decode_byte_cursor.borrow_mut() = new_byte_cursor;
        *self.decode_frame_cursor.borrow_mut() = new_frame_cursor;

        // Real EOF reached in this window: total_frames is now exact.
        if new_byte_cursor >= self.file_size {
            *self.total_frames.borrow_mut() = new_frame_cursor;
            self.reached_eof.set(true);
        } else if new_frame_cursor > *self.total_frames.borrow() {
            // Decoded past the (under-)estimate — grow total_frames to track
            // progress. NOT final: reached_eof stays false until real EOF, so
            // the background decode keeps going instead of stopping here.
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
            // No new frames decoded — treat as EOF.
            *self.total_frames.borrow_mut() = frame_cursor;
            self.reached_eof.set(true);
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
                    buf[written..written + n].copy_from_slice(&chunk.mono[start_idx..end_idx]);
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
        self.reached_eof.get()
    }

    pub fn decode_frame_cursor_value(&self) -> u64 {
        *self.decode_frame_cursor.borrow()
    }
}

impl AudioSource for StreamingMp3Source {
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

    fn length_is_estimated(&self) -> bool {
        !self.reached_eof.get()
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
                    for s in buf.iter_mut() {
                        *s = 0.0;
                    }
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
                    for s in buf.iter_mut() {
                        *s = 0.0;
                    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::loader::{load_audio, parse_mp3_header};
    use crate::audio::source::AudioSource;
    use std::sync::Arc;

    /// Minimal executor for the source's async methods. On host the awaited
    /// futures (Bytes range "reads" + the no-op `yield_to_browser`) never park,
    /// so a busy-poll with a no-op waker drives them straight to completion.
    fn block_on<F: std::future::Future>(fut: F) -> F::Output {
        use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
        const VTABLE: RawWakerVTable = RawWakerVTable::new(
            |_| RawWaker::new(std::ptr::null(), &VTABLE),
            |_| {},
            |_| {},
            |_| {},
        );
        let waker = unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) };
        let mut cx = Context::from_waker(&waker);
        let mut fut = std::pin::pin!(fut);
        loop {
            if let Poll::Ready(v) = fut.as_mut().poll(&mut cx) {
                return v;
            }
        }
    }

    /// Regression guard for the truncation bug behind audit lows #8/#12: a
    /// header-less MP3 large enough to need several decode windows must decode
    /// all the way to real EOF, NOT stop at the bitrate length *estimate*.
    ///
    /// Before the fix, `is_fully_decoded()` was `cursor >= total_frames` and
    /// `total_frames` was bumped up to the cursor mid-stream, so once the cursor
    /// passed the (small, 64 KB-derived) under-estimate the background decode
    /// declared itself done after ~one 4 MB window — silently cutting off long
    /// MP3s. Now `is_fully_decoded()` tracks a real `reached_eof` flag and
    /// prefetch isn't capped at the estimate, so decode runs to the true end.
    /// (The loader-level premise is pinned in oversample-core/tests/mp3_estimate.rs.)
    #[test]
    fn streaming_mp3_decodes_past_underestimate_to_real_eof() {
        // Repeat the committed 241 KB header-less fixture into a >4 MB buffer so
        // multiple windows are needed (one window = MP3_WINDOW_BYTES = 4 MB).
        let single: &[u8] =
            include_bytes!("../../oversample-core/tests/mp3_estimate/tone_48k_mono_noxing.mp3");
        let mut big = Vec::with_capacity(single.len() * 42);
        for _ in 0..42 {
            big.extend_from_slice(single);
        }
        let file_size = big.len() as u64;
        assert!(
            file_size > MP3_WINDOW_BYTES * 2,
            "fixture must span >2 windows"
        );

        // Ground truth: a full in-memory decode of the same bytes.
        let real = load_audio(&big).expect("decode").samples.len() as u64;

        // Build the source as the loader does: length parsed from only the first
        // 64 KB → a severe under-estimate; samples then decoded on demand.
        let header = parse_mp3_header(&big[..65536], file_size).expect("header");
        assert!(
            header.estimated_total_frames < real * 7 / 10,
            "test precondition: header should under-estimate; est={}, real={}",
            header.estimated_total_frames,
            real,
        );

        let src = StreamingMp3Source::new(
            FileHandle::Bytes(Arc::new(big)),
            &header,
            Vec::new(), // no in-memory head — force every read through decode
            None,
            file_size,
            header.data_offset, // initial byte cursor
            0,                  // initial frame cursor
        );
        assert!(src.length_is_estimated(), "length starts as an estimate");

        // Drive the background-decode loop to completion.
        let mut guard = 0;
        while !src.is_fully_decoded() {
            let cursor = src.decode_frame_cursor_value();
            block_on(src.prefetch_region(cursor, 262_144));
            guard += 1;
            assert!(
                guard < 100_000,
                "decode did not terminate (possible regression)"
            );
        }

        let total = src.total_samples();
        assert!(
            total as f64 >= 0.9 * real as f64,
            "streaming decode truncated: reached {total} of {real} frames ({:.0}%) — \
             the estimate-stop bug is back",
            100.0 * total as f64 / real as f64,
        );
        assert!(
            !src.length_is_estimated(),
            "length is exact after full decode"
        );
    }
}
