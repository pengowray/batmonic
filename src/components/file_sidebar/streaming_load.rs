use leptos::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::File;
use std::sync::Arc;
use crate::audio::loader::{id3v2_tag_size, is_m4a, is_mp3, is_ogg, parse_flac_header, parse_m4a_chapters, parse_mp3_header, parse_ogg_header, parse_wav_header_with_file_size};
use crate::audio::streaming_source::{FileHandle, StreamingFlacSource, StreamingM4aSource, StreamingMp3Source, StreamingOggSource, StreamingWavSource, read_blob_range};
use crate::dsp::fft::compute_preview;
use crate::state::{AppState, FileSettings, LoadedFile};
use crate::types::{AudioData, SpectrogramData};

pub(super) enum SilenceCheck {
    Silent,
    HighGain(f64),
}

/// Decoded size threshold for streaming (512 MB of f32 samples).
const STREAMING_DECODED_THRESHOLD: u64 = 512 * 1024 * 1024;

fn should_stream_from_decoded_size(decoded_bytes: u64, force_streaming: bool) -> Result<(), String> {
    if force_streaming || decoded_bytes >= STREAMING_DECODED_THRESHOLD {
        Ok(())
    } else {
        Err(format!(
            "Decoded size {:.0} MB below streaming threshold",
            decoded_bytes as f64 / 1_048_576.0
        ))
    }
}

/// Attempt to open a large WAV file using the streaming path.
/// Returns Ok(()) if successful, Err if the file is not suitable for streaming
/// (not WAV, decoded size below threshold, unsupported format).
pub(super) async fn try_streaming_wav(file: &File, name: &str, state: AppState, force_streaming: bool, load_id: u64) -> Result<(), String> {
    // Read first 64KB for header parsing
    let header_size = 65536.0f64.min(file.size());
    let header_bytes = read_blob_range(file, 0.0, header_size).await?;

    if header_bytes.len() < 12 {
        return Err("Header too small".into());
    }
    let magic = &header_bytes[0..4];
    if magic != b"RIFF" && magic != b"RF64" {
        return Err("Not a RIFF/RF64 file".into());
    }

    let header = parse_wav_header_with_file_size(&header_bytes, Some(file.size() as u64))?;

    // Check if decoded size warrants streaming
    let decoded_bytes = header.total_frames * header.channels as u64 * 4; // f32 per sample
    should_stream_from_decoded_size(decoded_bytes, force_streaming)?;

    log::info!(
        "Streaming WAV: {} — {} frames, {} ch, {} Hz, {:.1}s, decoded {:.0} MB",
        name,
        header.total_frames,
        header.channels,
        header.sample_rate,
        header.total_frames as f64 / header.sample_rate as f64,
        decoded_bytes as f64 / 1_048_576.0,
    );

    if force_streaming && decoded_bytes < STREAMING_DECODED_THRESHOLD {
        state.show_info_toast(format!(
            "Streaming file to keep total open files under control ({:.0} MB)",
            file.size() / 1_000_000.0
        ));
    } else {
        state.show_info_toast(format!(
            "Streaming large file ({:.0} MB)",
            file.size() / 1_000_000.0
        ));
    }

    // Decode first 30s for head samples
    use crate::audio::source::DEFAULT_ANALYSIS_WINDOW_SECS;
    let head_frames = ((DEFAULT_ANALYSIS_WINDOW_SECS * header.sample_rate as f64) as u64)
        .min(header.total_frames);
    let bytes_per_frame = header.channels as u64 * (header.bits_per_sample as u64 / 8);
    let head_byte_len = head_frames * bytes_per_frame;
    let head_byte_start = header.data_offset;
    let head_byte_end = head_byte_start + head_byte_len;

    let head_pcm_bytes = read_blob_range(file, head_byte_start as f64, head_byte_end as f64).await?;

    // Decode PCM to f32
    let head_interleaved = decode_head_pcm(
        &head_pcm_bytes,
        header.bits_per_sample,
        header.is_float,
        header.channels,
    );

    let channels = header.channels as usize;
    let (head_mono, head_raw) = if channels == 1 {
        (head_interleaved, None)
    } else {
        let mono: Vec<f32> = head_interleaved
            .chunks_exact(channels)
            .map(|frame| frame.iter().sum::<f32>() / channels as f32)
            .collect();
        (mono, Some(head_interleaved))
    };

    // Try to get GUANO metadata if not already in header
    let mut guano = header.guano.clone();
    if guano.is_none() {
        // GUANO might be after the data chunk — read tail of file
        let file_size = file.size();
        let data_end = header.data_offset + header.data_size;
        if (data_end as f64) < file_size {
            let tail_start = data_end as f64;
            // Read up to 64KB from after the data chunk
            let tail_end = file_size.min(tail_start + 65536.0);
            if let Ok(tail_bytes) = read_blob_range(file, tail_start, tail_end).await {
                guano = scan_tail_for_guano(&tail_bytes);
            }
        }
    }

    // Create StreamingWavSource
    let source = Arc::new(StreamingWavSource::new(
        FileHandle::WebFile(file.clone()),
        &header,
        head_mono.clone(),
        head_raw,
    ));

    let sample_rate = header.sample_rate;
    let total_frames = header.total_frames;
    let duration_secs = total_frames as f64 / sample_rate as f64;

    // For backward compat: audio.samples = head_mono
    let samples = Arc::new(head_mono);

    let audio = AudioData {
        samples,
        source,
        sample_rate,
        channels: header.channels as u32,
        duration_secs,
        metadata: crate::types::FileMetadata {
            file_size: file.size() as usize,
            format: "WAV",
            bits_per_sample: header.bits_per_sample,
            is_float: header.is_float,
            guano,
            data_offset: Some(header.data_offset),
            data_size: Some(header.data_size),
        },
    };

    // Compute preview from head samples (fast)
    let preview = compute_preview(&audio, 256, 128);

    // Check for silence/quiet in head
    let (silence_check, cached_peak_db) = {
        use crate::audio::source::ChannelView;
        let scan = audio.source.read_region(ChannelView::MonoMix, 0, audio.source.total_samples().min(
            (DEFAULT_ANALYSIS_WINDOW_SECS * sample_rate as f64) as u64,
        ) as usize);
        let peak = scan.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        if peak < 0.002 {
            (Some(SilenceCheck::Silent), None)
        } else if peak > 1e-10 {
            let peak_db = 20.0 * (peak as f64).log10();
            let auto_db = -3.0 - peak_db;
            let sc = if auto_db > 30.0 { Some(SilenceCheck::HighGain(auto_db)) } else { None };
            (sc, Some(peak_db))
        } else {
            (None, None)
        }
    };

    // Build placeholder spectrogram metadata (tiles computed on demand)
    const HOP_SIZE: usize = 512;
    let fft_size: usize = state.spect_fft_mode.get_untracked().fft_for_lod(crate::canvas::tile_cache::LOD_BASELINE);
    let total_len = total_frames as usize;
    let total_cols = if total_len >= fft_size {
        (total_len - fft_size) / HOP_SIZE + 1
    } else {
        0
    };

    let spectrogram = SpectrogramData {
        columns: Arc::new(Vec::new()),
        total_columns: total_cols,
        freq_resolution: sample_rate as f64 / fft_size as f64,
        time_resolution: HOP_SIZE as f64 / sample_rate as f64,
        max_freq: sample_rate as f64 / 2.0,
        sample_rate,
    };

    let wav_markers = header.wav_markers.clone();
    let name_owned = name.to_string();
    let file_index;
    {
        let mut idx = 0;
        state.files.update(|files| {
            idx = files.len();
            files.push(LoadedFile {
                name: name_owned.clone(),
                audio,
                spectrogram,
                preview: Some(preview),
                overview_image: None,
                xc_metadata: None,
                xc_hashes: None,
                is_demo: false,
                is_recording: false,
                is_live_listen: false,
                settings: FileSettings::default(),
                add_order: idx,
                last_modified_ms: None,
                identity: None,
                file_handle: Some(FileHandle::WebFile(file.clone())),
                cached_peak_db,
                cached_full_peak_db: None,
                read_only: false,
                had_sidecar: false,
                verify_outcome: crate::state::VerifyOutcome::Pending,
                all_hashes_verified: false,
                wav_markers,
                loading_id: Some(load_id),
                min_display_freq: None,
                max_display_freq: None,
            });
            state.current_file_index.set(Some(idx));
        });
        file_index = idx;
    }

    // Schedule async full-file peak scan (for files > 30s)
    crate::audio::peak::start_full_peak_scan(state, file_index);

    // Notify user about silent/quiet files
    if let Some(check) = silence_check {
        match check {
            SilenceCheck::Silent => {
                state.auto_gain.set(false);
                state.gain_db.set(0.0);
                state.show_info_toast("File appears silent \u{2014} auto-gain disabled");
            }
            SilenceCheck::HighGain(db) => {
                state.show_info_toast(format!("Quiet file \u{2014} auto-gain: +{:.0} dB", db));
            }
        }
    }

    // For streaming files, tiles are computed on-demand by tile_cache.
    // Schedule visible tiles to kick off the initial view.
    use crate::canvas::{spectral_store, tile_cache};
    spectral_store::init(file_index, total_cols, fft_size);

    // Prefetch first viewport worth of audio, then schedule tiles
    let audio_ref = state.files.get_untracked().get(file_index).cloned();
    if let Some(f) = audio_ref {
        if let Some(streaming) = f.audio.source.as_any().downcast_ref::<StreamingWavSource>() {
            // Prefetch the head region — already loaded, but schedule visible tiles
            let scroll = state.scroll_offset.get_untracked();
            let zoom = state.zoom_level.get_untracked();
            let canvas_w = state.spectrogram_canvas_width.get_untracked();
            let time_res = HOP_SIZE as f64 / sample_rate as f64;
            let visible_time = if zoom > 0.0 { canvas_w / zoom * time_res } else { 1.0 };
            let start_sample = (scroll / time_res * HOP_SIZE as f64) as u64;
            let visible_samples = (visible_time * sample_rate as f64) as usize;
            streaming.prefetch_region(start_sample, visible_samples + fft_size).await;
        }

        tile_cache::schedule_visible_tiles_from_store(state, file_index, total_cols);
    }

    state.tile_ready_signal.update(|n| *n = n.wrapping_add(1));

    // Spawn low-priority background task to build the full overview spectrogram.
    // This only runs when the system is idle (not playing, not actively rendering tiles).
    {
        let name_for_overview = name.to_string();
        wasm_bindgen_futures::spawn_local(build_streaming_overview(
            state,
            file_index,
            name_for_overview,
        ));
    }

    Ok(())
}

/// Attempt to open a large FLAC file using the streaming path.
/// Returns Ok(()) if successful, Err if the file is not suitable for streaming.
pub(super) async fn try_streaming_flac(file: &File, name: &str, state: AppState, force_streaming: bool, load_id: u64) -> Result<(), String> {
    // Read first 64KB for header parsing
    let header_size = 65536.0f64.min(file.size());
    let header_bytes = read_blob_range(file, 0.0, header_size).await?;

    if header_bytes.len() < 42 {
        return Err("Header too small".into());
    }
    if &header_bytes[0..4] != b"fLaC" {
        return Err("Not a FLAC file".into());
    }

    let header = parse_flac_header(&header_bytes)?;

    // Check if decoded size warrants streaming
    let decoded_bytes = header.total_frames * header.channels as u64 * 4; // f32 per sample
    should_stream_from_decoded_size(decoded_bytes, force_streaming)?;

    log::info!(
        "Streaming FLAC: {} — {} frames, {} ch, {} Hz, {:.1}s, decoded {:.0} MB",
        name,
        header.total_frames,
        header.channels,
        header.sample_rate,
        header.total_frames as f64 / header.sample_rate as f64,
        decoded_bytes as f64 / 1_048_576.0,
    );

    if force_streaming && decoded_bytes < STREAMING_DECODED_THRESHOLD {
        state.show_info_toast(format!(
            "Streaming FLAC to keep total open files under control ({:.0} MB)",
            file.size() / 1_000_000.0
        ));
    } else {
        state.show_info_toast(format!(
            "Streaming large FLAC ({:.0} MB)",
            file.size() / 1_000_000.0
        ));
    }

    // Decode first 30s by reading a generous initial chunk.
    // Estimate: at 1411 kbps (CD quality) FLAC compresses ~50-70%, so 30s ≈ ~3 MB.
    // At 384 kHz 24-bit stereo, 30s uncompressed ≈ 66 MB, compressed ≈ ~40 MB.
    // Read 48 MB to be safe for high sample-rate files.
    use crate::audio::source::DEFAULT_ANALYSIS_WINDOW_SECS;
    let head_target_frames = ((DEFAULT_ANALYSIS_WINDOW_SECS * header.sample_rate as f64) as u64)
        .min(header.total_frames);
    let initial_read_size = (48 * 1024 * 1024u64).min(file.size() as u64);
    let initial_bytes = read_blob_range(file, 0.0, initial_read_size as f64).await?;

    // Decode using FlacReader (which parses the STREAMINFO from the beginning)
    let cursor = std::io::Cursor::new(&initial_bytes[..]);
    let mut reader = claxon::FlacReader::new(cursor)
        .map_err(|e| format!("FLAC reader error: {}", e))?;

    let channels = header.channels as usize;
    let max_val = (1u32 << (header.bits_per_sample - 1)) as f32;
    let mut head_interleaved: Vec<f32> = Vec::new();
    let mut head_frame_count: u64 = 0;
    let mut frames_since_yield: u64 = 0;

    {
        let mut blocks = reader.blocks();
        let mut block_buf = Vec::new();
        loop {
            match blocks.read_next_or_eof(block_buf) {
                Ok(Some(block)) => {
                    let n_frames = block.duration() as usize;
                    for frame_idx in 0..n_frames {
                        for ch in 0..channels {
                            let sample = block.sample(ch as u32, frame_idx as u32);
                            head_interleaved.push(sample as f32 / max_val);
                        }
                    }
                    head_frame_count += n_frames as u64;
                    frames_since_yield += n_frames as u64;
                    block_buf = block.into_buffer();

                    if frames_since_yield >= 65_536 {
                        frames_since_yield = 0;
                        crate::canvas::tile_cache::yield_to_browser().await;
                    }

                    if head_frame_count >= head_target_frames {
                        break;
                    }
                }
                Ok(None) => break,
                Err(e) => {
                    if head_frame_count == 0 {
                        return Err(format!("FLAC decode error: {}", e));
                    }
                    // Partial decode — we have some head samples, proceed
                    break;
                }
            }
        }
    }

    if head_frame_count == 0 {
        return Err("No FLAC frames decoded".into());
    }

    // Truncate to exact head target
    let actual_head_frames = head_frame_count.min(head_target_frames) as usize;
    head_interleaved.truncate(actual_head_frames * channels);

    let (head_mono, head_raw) = if channels == 1 {
        (head_interleaved, None)
    } else {
        let mono: Vec<f32> = head_interleaved
            .chunks_exact(channels)
            .map(|frame| frame.iter().sum::<f32>() / channels as f32)
            .collect();
        (mono, Some(head_interleaved))
    };

    // Estimate byte cursor position after head decode.
    // We can't get the exact position from claxon, so estimate from compression ratio.
    // Use the ratio of frames decoded vs total, applied to file size.
    let file_size = file.size() as u64;
    let ratio = head_frame_count as f64 / header.total_frames.max(1) as f64;
    let estimated_byte_cursor = ((file_size as f64 * ratio) as u64)
        .max(header.first_frame_offset)
        .min(file_size);

    // Create StreamingFlacSource
    let source = Arc::new(StreamingFlacSource::new(
        FileHandle::WebFile(file.clone()),
        &header,
        head_mono.clone(),
        head_raw,
        estimated_byte_cursor,
        head_frame_count,
    ));

    let sample_rate = header.sample_rate;
    let total_frames = header.total_frames;
    let duration_secs = total_frames as f64 / sample_rate as f64;

    let samples = Arc::new(head_mono);

    let audio = AudioData {
        samples,
        source: source.clone(),
        sample_rate,
        channels: header.channels as u32,
        duration_secs,
        metadata: crate::types::FileMetadata {
            file_size: file.size() as usize,
            format: "FLAC",
            bits_per_sample: header.bits_per_sample,
            is_float: false,
            guano: None,
            data_offset: Some(header.first_frame_offset),
            data_size: Some((file.size() as u64).saturating_sub(header.first_frame_offset)),
        },
    };

    // Compute preview from head samples
    let preview = compute_preview(&audio, 256, 128);

    // Check for silence/quiet in head
    let (silence_check, cached_peak_db) = {
        use crate::audio::source::ChannelView;
        let scan = audio.source.read_region(ChannelView::MonoMix, 0, audio.source.total_samples().min(
            (DEFAULT_ANALYSIS_WINDOW_SECS * sample_rate as f64) as u64,
        ) as usize);
        let peak = scan.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        if peak < 0.002 {
            (Some(SilenceCheck::Silent), None)
        } else if peak > 1e-10 {
            let peak_db = 20.0 * (peak as f64).log10();
            let auto_db = -3.0 - peak_db;
            let sc = if auto_db > 30.0 { Some(SilenceCheck::HighGain(auto_db)) } else { None };
            (sc, Some(peak_db))
        } else {
            (None, None)
        }
    };

    // Build placeholder spectrogram metadata
    const HOP_SIZE: usize = 512;
    let fft_size: usize = state.spect_fft_mode.get_untracked().fft_for_lod(crate::canvas::tile_cache::LOD_BASELINE);
    let total_len = total_frames as usize;
    let total_cols = if total_len >= fft_size {
        (total_len - fft_size) / HOP_SIZE + 1
    } else {
        0
    };

    let spectrogram = SpectrogramData {
        columns: Arc::new(Vec::new()),
        total_columns: total_cols,
        freq_resolution: sample_rate as f64 / fft_size as f64,
        time_resolution: HOP_SIZE as f64 / sample_rate as f64,
        max_freq: sample_rate as f64 / 2.0,
        sample_rate,
    };

    let name_owned = name.to_string();
    let file_index;
    {
        let mut idx = 0;
        state.files.update(|files| {
            idx = files.len();
            files.push(LoadedFile {
                name: name_owned.clone(),
                audio,
                spectrogram,
                preview: Some(preview),
                overview_image: None,
                xc_metadata: None,
                xc_hashes: None,
                is_demo: false,
                is_recording: false,
                is_live_listen: false,
                settings: FileSettings::default(),
                add_order: idx,
                last_modified_ms: None,
                identity: None,
                file_handle: Some(FileHandle::WebFile(file.clone())),
                cached_peak_db,
                cached_full_peak_db: None,
                read_only: false,
                had_sidecar: false,
                verify_outcome: crate::state::VerifyOutcome::Pending,
                all_hashes_verified: false,
                wav_markers: Vec::new(),
                loading_id: Some(load_id),
                min_display_freq: None,
                max_display_freq: None,
            });
            state.current_file_index.set(Some(idx));
        });
        file_index = idx;
    }

    // Schedule async full-file peak scan (for files > 30s)
    crate::audio::peak::start_full_peak_scan(state, file_index);

    if let Some(check) = silence_check {
        match check {
            SilenceCheck::Silent => {
                state.auto_gain.set(false);
                state.gain_db.set(0.0);
                state.show_info_toast("File appears silent \u{2014} auto-gain disabled");
            }
            SilenceCheck::HighGain(db) => {
                state.show_info_toast(format!("Quiet file \u{2014} auto-gain: +{:.0} dB", db));
            }
        }
    }

    use crate::canvas::{spectral_store, tile_cache};
    spectral_store::init(file_index, total_cols, fft_size);

    // Prefetch first viewport and schedule tiles
    {
        let scroll = state.scroll_offset.get_untracked();
        let zoom = state.zoom_level.get_untracked();
        let canvas_w = state.spectrogram_canvas_width.get_untracked();
        let time_res = HOP_SIZE as f64 / sample_rate as f64;
        let visible_time = if zoom > 0.0 { canvas_w / zoom * time_res } else { 1.0 };
        let start_sample = (scroll / time_res * HOP_SIZE as f64) as u64;
        let visible_samples = (visible_time * sample_rate as f64) as usize;
        source.prefetch_region(start_sample, visible_samples + fft_size).await;
    }

    tile_cache::schedule_visible_tiles_from_store(state, file_index, total_cols);
    state.tile_ready_signal.update(|n| *n = n.wrapping_add(1));

    // Spawn background progressive decode
    {
        let source_bg = source.clone();
        let name_bg = name.to_string();
        wasm_bindgen_futures::spawn_local(background_flac_decode(
            state,
            file_index,
            name_bg.clone(),
            source_bg,
        ));
    }

    // Spawn background overview
    {
        let name_for_overview = name.to_string();
        wasm_bindgen_futures::spawn_local(build_streaming_overview(
            state,
            file_index,
            name_for_overview,
        ));
    }

    Ok(())
}

/// Progressively decode the rest of a streaming FLAC file in the background.
async fn background_flac_decode(
    state: AppState,
    file_index: usize,
    expected_name: String,
    source: Arc<StreamingFlacSource>,
) {
    // Initial delay — let the UI settle
    let p = js_sys::Promise::new(&mut |resolve, _| {
        web_sys::window().unwrap()
            .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 200).unwrap();
    });
    JsFuture::from(p).await.ok();

    while !source.is_fully_decoded() {
        // Check file still loaded
        let still_valid = state.files.get_untracked()
            .get(file_index)
            .map(|f| f.name == expected_name)
            .unwrap_or(false);
        if !still_valid { return; }

        // Defer while playing or loading
        let is_busy = state.is_playing.get_untracked()
            || state.loading_files.with_untracked(|v| !v.is_empty());
        if is_busy {
            let p = js_sys::Promise::new(&mut |resolve, _| {
                web_sys::window().unwrap()
                    .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 500).unwrap();
            });
            JsFuture::from(p).await.ok();
            continue;
        }

        // Decode one window worth of frames
        let cursor = source.decode_frame_cursor_value();
        source.prefetch_region(cursor, 262_144).await;

        // Yield to browser
        let p = js_sys::Promise::new(&mut |resolve, _| {
            web_sys::window().unwrap()
                .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 20).unwrap();
        });
        JsFuture::from(p).await.ok();
    }

    log::info!("Background FLAC decode complete for {}", expected_name);
}

/// Attempt to open a large MP3 file using the streaming path.
/// Returns Ok(()) if successful, Err if the file is not suitable for streaming.
pub(super) async fn try_streaming_mp3(file: &File, name: &str, state: AppState, force_streaming: bool, load_id: u64) -> Result<(), String> {
    // Read first 64KB for initial detection
    let initial_size = 65536.0f64.min(file.size());
    let initial_bytes = read_blob_range(file, 0.0, initial_size).await?;

    if !is_mp3(&initial_bytes) {
        return Err("Not an MP3 file".into());
    }

    // MP3 files can have large ID3v2 tags (artwork, etc.) that push the first
    // audio frame far into the file.  Read enough to cover the tag + some audio.
    let id3_size = id3v2_tag_size(&initial_bytes);
    let header_size = ((id3_size + 65536).min(file.size() as u64)) as f64;
    let header_bytes = if header_size > initial_size {
        read_blob_range(file, 0.0, header_size).await?
    } else {
        initial_bytes
    };

    let file_size = file.size() as u64;
    let header = parse_mp3_header(&header_bytes, file_size)?;

    // Check if decoded size warrants streaming
    let decoded_bytes = header.estimated_total_frames * header.channels as u64 * 4;
    should_stream_from_decoded_size(decoded_bytes, force_streaming)?;

    log::info!(
        "Streaming MP3: {} — ~{} frames, {} ch, {} Hz, ~{:.1}s, decoded ~{:.0} MB",
        name,
        header.estimated_total_frames,
        header.channels,
        header.sample_rate,
        header.estimated_total_frames as f64 / header.sample_rate as f64,
        decoded_bytes as f64 / 1_048_576.0,
    );

    if force_streaming && decoded_bytes < STREAMING_DECODED_THRESHOLD {
        state.show_info_toast(format!(
            "Streaming MP3 to keep total open files under control ({:.0} MB)",
            file.size() / 1_000_000.0
        ));
    } else {
        state.show_info_toast(format!(
            "Streaming large MP3 ({:.0} MB)",
            file.size() / 1_000_000.0
        ));
    }

    // Decode first 30s by reading a generous initial chunk.
    // MP3 at 320 kbps: 30s ≈ 1.2 MB. At lower bitrates even less.
    // Read 48 MB to be safe for any bitrate.
    use crate::audio::source::DEFAULT_ANALYSIS_WINDOW_SECS;
    let head_target_frames = ((DEFAULT_ANALYSIS_WINDOW_SECS * header.sample_rate as f64) as u64)
        .min(header.estimated_total_frames);
    let initial_read_size = (48 * 1024 * 1024u64).min(file_size);
    let initial_bytes = read_blob_range(file, 0.0, initial_read_size as f64).await?;

    // Decode head using symphonia
    use symphonia::core::audio::SampleBuffer;
    use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
    use symphonia::core::errors::Error as SymphoniaError;
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    use symphonia::core::probe::Hint;

    let cursor = std::io::Cursor::new(initial_bytes);
    let mss = MediaSourceStream::new(Box::new(cursor), Default::default());

    let mut hint = Hint::new();
    hint.with_extension("mp3");

    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| format!("MP3 probe error: {e}"))?;

    let mut format = probed.format;

    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or("No audio track found in MP3")?;

    let sample_rate = track.codec_params.sample_rate.ok_or("MP3 missing sample rate")?;
    let channels = track.codec_params.channels.ok_or("MP3 missing channel info")?.count();
    let track_id = track.id;

    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| format!("MP3 decoder error: {e}"))?;

    let mut head_interleaved: Vec<f32> = Vec::new();
    let mut head_frame_count: u64 = 0;
    let mut frames_since_yield: u64 = 0;

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

        match decoder.decode(&packet) {
            Ok(decoded) => {
                let spec = *decoded.spec();
                let mut buf = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
                buf.copy_interleaved_ref(decoded);
                let samples = buf.samples();
                let n_frames = samples.len() / channels;
                head_interleaved.extend_from_slice(samples);
                head_frame_count += n_frames as u64;
                frames_since_yield += n_frames as u64;

                // Yield periodically so the browser stays responsive during head decode
                if frames_since_yield >= 65_536 {
                    frames_since_yield = 0;
                    crate::canvas::tile_cache::yield_to_browser().await;
                }

                if head_frame_count >= head_target_frames {
                    break;
                }
            }
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(_) => break,
        }
    }

    if head_frame_count == 0 {
        return Err("No MP3 frames decoded".into());
    }

    // Truncate to exact head target
    let actual_head_frames = head_frame_count.min(head_target_frames) as usize;
    head_interleaved.truncate(actual_head_frames * channels);

    let (head_mono, head_raw) = if channels == 1 {
        (head_interleaved, None)
    } else {
        let mono: Vec<f32> = head_interleaved
            .chunks_exact(channels)
            .map(|frame| frame.iter().sum::<f32>() / channels as f32)
            .collect();
        (mono, Some(head_interleaved))
    };

    // Estimate byte cursor position after head decode
    let ratio = head_frame_count as f64 / header.estimated_total_frames.max(1) as f64;
    let estimated_byte_cursor = ((file_size as f64 * ratio) as u64).min(file_size);

    // Create StreamingMp3Source
    let source = Arc::new(StreamingMp3Source::new(
        FileHandle::WebFile(file.clone()),
        &header,
        head_mono.clone(),
        head_raw,
        file_size,
        estimated_byte_cursor,
        head_frame_count,
    ));

    let total_frames = header.estimated_total_frames;
    let duration_secs = total_frames as f64 / sample_rate as f64;

    let samples = Arc::new(head_mono);

    let audio = AudioData {
        samples,
        source: source.clone(),
        sample_rate,
        channels: channels as u32,
        duration_secs,
        metadata: crate::types::FileMetadata {
            file_size: file.size() as usize,
            format: "MP3",
            bits_per_sample: 16,
            is_float: false,
            guano: None,
            data_offset: Some(header.data_offset),
            data_size: Some((file.size() as u64).saturating_sub(header.data_offset)),
        },
    };

    // Compute preview from head samples
    let preview = compute_preview(&audio, 256, 128);

    // Check for silence/quiet in head
    let (silence_check, cached_peak_db) = {
        use crate::audio::source::ChannelView;
        let scan = audio.source.read_region(ChannelView::MonoMix, 0, audio.source.total_samples().min(
            (DEFAULT_ANALYSIS_WINDOW_SECS * sample_rate as f64) as u64,
        ) as usize);
        let peak = scan.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        if peak < 0.002 {
            (Some(SilenceCheck::Silent), None)
        } else if peak > 1e-10 {
            let peak_db = 20.0 * (peak as f64).log10();
            let auto_db = -3.0 - peak_db;
            let sc = if auto_db > 30.0 { Some(SilenceCheck::HighGain(auto_db)) } else { None };
            (sc, Some(peak_db))
        } else {
            (None, None)
        }
    };

    // Build placeholder spectrogram metadata
    const HOP_SIZE: usize = 512;
    let fft_size: usize = state.spect_fft_mode.get_untracked().fft_for_lod(crate::canvas::tile_cache::LOD_BASELINE);
    let total_len = total_frames as usize;
    let total_cols = if total_len >= fft_size {
        (total_len - fft_size) / HOP_SIZE + 1
    } else {
        0
    };

    let spectrogram = SpectrogramData {
        columns: Arc::new(Vec::new()),
        total_columns: total_cols,
        freq_resolution: sample_rate as f64 / fft_size as f64,
        time_resolution: HOP_SIZE as f64 / sample_rate as f64,
        max_freq: sample_rate as f64 / 2.0,
        sample_rate,
    };

    let name_owned = name.to_string();
    let file_index;
    {
        let mut idx = 0;
        state.files.update(|files| {
            idx = files.len();
            files.push(LoadedFile {
                name: name_owned.clone(),
                audio,
                spectrogram,
                preview: Some(preview),
                overview_image: None,
                xc_metadata: None,
                xc_hashes: None,
                is_demo: false,
                is_recording: false,
                is_live_listen: false,
                settings: FileSettings::default(),
                add_order: idx,
                last_modified_ms: None,
                identity: None,
                file_handle: Some(FileHandle::WebFile(file.clone())),
                cached_peak_db,
                cached_full_peak_db: None,
                read_only: false,
                had_sidecar: false,
                verify_outcome: crate::state::VerifyOutcome::Pending,
                all_hashes_verified: false,
                wav_markers: Vec::new(),
                loading_id: Some(load_id),
                min_display_freq: None,
                max_display_freq: None,
            });
            state.current_file_index.set(Some(idx));
        });
        file_index = idx;
    }

    // Schedule async full-file peak scan (for files > 30s)
    crate::audio::peak::start_full_peak_scan(state, file_index);

    if let Some(check) = silence_check {
        match check {
            SilenceCheck::Silent => {
                state.auto_gain.set(false);
                state.gain_db.set(0.0);
                state.show_info_toast("File appears silent \u{2014} auto-gain disabled");
            }
            SilenceCheck::HighGain(db) => {
                state.show_info_toast(format!("Quiet file \u{2014} auto-gain: +{:.0} dB", db));
            }
        }
    }

    use crate::canvas::{spectral_store, tile_cache};
    spectral_store::init(file_index, total_cols, fft_size);

    // Prefetch first viewport and schedule tiles
    {
        let scroll = state.scroll_offset.get_untracked();
        let zoom = state.zoom_level.get_untracked();
        let canvas_w = state.spectrogram_canvas_width.get_untracked();
        let time_res = HOP_SIZE as f64 / sample_rate as f64;
        let visible_time = if zoom > 0.0 { canvas_w / zoom * time_res } else { 1.0 };
        let start_sample = (scroll / time_res * HOP_SIZE as f64) as u64;
        let visible_samples = (visible_time * sample_rate as f64) as usize;
        source.prefetch_region(start_sample, visible_samples + fft_size).await;
    }

    tile_cache::schedule_visible_tiles_from_store(state, file_index, total_cols);
    state.tile_ready_signal.update(|n| *n = n.wrapping_add(1));

    // Spawn background progressive decode
    {
        let source_bg = source.clone();
        let name_bg = name.to_string();
        wasm_bindgen_futures::spawn_local(background_mp3_decode(
            state,
            file_index,
            name_bg,
            source_bg,
        ));
    }

    // Spawn background overview
    {
        let name_for_overview = name.to_string();
        wasm_bindgen_futures::spawn_local(build_streaming_overview(
            state,
            file_index,
            name_for_overview,
        ));
    }

    Ok(())
}

/// Progressively decode the rest of a streaming MP3 file in the background.
async fn background_mp3_decode(
    state: AppState,
    file_index: usize,
    expected_name: String,
    source: Arc<StreamingMp3Source>,
) {
    use crate::canvas::tile_cache::{self, TILE_COLS};

    // Initial delay — let the UI settle
    let p = js_sys::Promise::new(&mut |resolve, _| {
        web_sys::window().unwrap()
            .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 200).unwrap();
    });
    JsFuture::from(p).await.ok();

    let hop_size = 512usize;
    let tile_samples = TILE_COLS * hop_size;
    let mut last_tile_scheduled: Option<usize> = None;

    while !source.is_fully_decoded() {
        // Check file still loaded
        let still_valid = state.files.get_untracked()
            .get(file_index)
            .map(|f| f.name == expected_name)
            .unwrap_or(false);
        if !still_valid { return; }

        // Defer while playing or loading
        let is_busy = state.is_playing.get_untracked()
            || state.loading_files.with_untracked(|v| !v.is_empty());
        if is_busy {
            let p = js_sys::Promise::new(&mut |resolve, _| {
                web_sys::window().unwrap()
                    .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 500).unwrap();
            });
            JsFuture::from(p).await.ok();
            continue;
        }

        // Decode one window worth of frames
        let cursor_before = source.decode_frame_cursor_value();
        source.prefetch_region(cursor_before, 262_144).await;
        let cursor_after = source.decode_frame_cursor_value();

        // Schedule tiles left-to-right for newly decoded regions
        if cursor_after > cursor_before && tile_samples > 0 {
            let first_tile = cursor_before as usize / tile_samples;
            let last_tile = cursor_after as usize / tile_samples;
            let start = last_tile_scheduled.map(|t| t + 1).unwrap_or(first_tile);
            for t in start..=last_tile {
                tile_cache::schedule_tile_on_demand(state, file_index, t);
            }
            last_tile_scheduled = Some(last_tile);
            state.tile_ready_signal.update(|n| *n = n.wrapping_add(1));
        }

        // Yield to browser
        let p = js_sys::Promise::new(&mut |resolve, _| {
            web_sys::window().unwrap()
                .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 20).unwrap();
        });
        JsFuture::from(p).await.ok();
    }

    log::info!("Background MP3 decode complete for {}", expected_name);
}

/// Attempt to open a large OGG file using the streaming path.
/// Returns Ok(()) if successful, Err if the file is not suitable for streaming.
pub(super) async fn try_streaming_ogg(file: &File, name: &str, state: AppState, force_streaming: bool, load_id: u64) -> Result<(), String> {
    // Read first 64KB for header probing
    let header_size = 65536.0f64.min(file.size());
    let header_bytes = read_blob_range(file, 0.0, header_size).await?;

    if !is_ogg(&header_bytes) {
        return Err("Not an OGG file".into());
    }

    let file_size = file.size() as u64;
    let header = parse_ogg_header(&header_bytes, file_size)?;

    // Check if decoded size warrants streaming
    let decoded_bytes = header.estimated_total_frames * header.channels as u64 * 4;
    should_stream_from_decoded_size(decoded_bytes, force_streaming)?;

    log::info!(
        "Streaming OGG: {} — ~{} frames, {} ch, {} Hz, ~{:.1}s, decoded ~{:.0} MB",
        name,
        header.estimated_total_frames,
        header.channels,
        header.sample_rate,
        header.estimated_total_frames as f64 / header.sample_rate as f64,
        decoded_bytes as f64 / 1_048_576.0,
    );

    if force_streaming && decoded_bytes < STREAMING_DECODED_THRESHOLD {
        state.show_info_toast(format!(
            "Streaming OGG to keep total open files under control ({:.0} MB)",
            file.size() / 1_000_000.0
        ));
    } else {
        state.show_info_toast(format!(
            "Streaming large OGG ({:.0} MB)",
            file.size() / 1_000_000.0
        ));
    }

    // Decode first 30s by reading a generous initial chunk.
    // Vorbis at ~192 kbps: 30s ≈ 720 KB. Read 48 MB to be safe.
    use crate::audio::source::DEFAULT_ANALYSIS_WINDOW_SECS;
    let head_target_frames = ((DEFAULT_ANALYSIS_WINDOW_SECS * header.sample_rate as f64) as u64)
        .min(header.estimated_total_frames);
    let initial_read_size = (48 * 1024 * 1024u64).min(file_size);
    let initial_bytes = read_blob_range(file, 0.0, initial_read_size as f64).await?;

    // Decode head using symphonia
    use symphonia::core::audio::SampleBuffer;
    use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
    use symphonia::core::errors::Error as SymphoniaError;
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    use symphonia::core::probe::Hint;

    let cursor = std::io::Cursor::new(initial_bytes);
    let mss = MediaSourceStream::new(Box::new(cursor), Default::default());

    let mut hint = Hint::new();
    hint.with_extension("ogg");

    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| format!("OGG probe error: {e}"))?;

    let mut format = probed.format;

    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or("No audio track found in OGG")?;

    let sample_rate = track.codec_params.sample_rate.ok_or("OGG missing sample rate")?;
    let channels = track.codec_params.channels.ok_or("OGG missing channel info")?.count();
    let track_id = track.id;

    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| format!("OGG decoder error: {e}"))?;

    let mut head_interleaved: Vec<f32> = Vec::new();
    let mut head_frame_count: u64 = 0;
    let mut frames_since_yield: u64 = 0;

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

        match decoder.decode(&packet) {
            Ok(decoded) => {
                let spec = *decoded.spec();
                let mut buf = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
                buf.copy_interleaved_ref(decoded);
                let samples = buf.samples();
                let n_frames = samples.len() / channels;
                head_interleaved.extend_from_slice(samples);
                head_frame_count += n_frames as u64;
                frames_since_yield += n_frames as u64;

                if frames_since_yield >= 65_536 {
                    frames_since_yield = 0;
                    crate::canvas::tile_cache::yield_to_browser().await;
                }

                if head_frame_count >= head_target_frames {
                    break;
                }
            }
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(_) => break,
        }
    }

    if head_frame_count == 0 {
        return Err("No OGG frames decoded".into());
    }

    // Truncate to exact head target
    let actual_head_frames = head_frame_count.min(head_target_frames) as usize;
    head_interleaved.truncate(actual_head_frames * channels);

    let (head_mono, head_raw) = if channels == 1 {
        (head_interleaved, None)
    } else {
        let mono: Vec<f32> = head_interleaved
            .chunks_exact(channels)
            .map(|frame| frame.iter().sum::<f32>() / channels as f32)
            .collect();
        (mono, Some(head_interleaved))
    };

    // Estimate byte cursor position after head decode
    let ratio = head_frame_count as f64 / header.estimated_total_frames.max(1) as f64;
    let estimated_byte_cursor = ((file_size as f64 * ratio) as u64).min(file_size);

    // Create StreamingOggSource
    let source = Arc::new(StreamingOggSource::new(
        FileHandle::WebFile(file.clone()),
        &header,
        head_mono.clone(),
        head_raw,
        file_size,
        estimated_byte_cursor,
        head_frame_count,
    ));

    let total_frames = header.estimated_total_frames;
    let duration_secs = total_frames as f64 / sample_rate as f64;

    let samples = Arc::new(head_mono);

    let audio = AudioData {
        samples,
        source: source.clone(),
        sample_rate,
        channels: channels as u32,
        duration_secs,
        metadata: crate::types::FileMetadata {
            file_size: file.size() as usize,
            format: "OGG",
            bits_per_sample: 16,
            is_float: false,
            guano: None,
            data_offset: None,
            data_size: None,
        },
    };

    // Compute preview from head samples
    let preview = compute_preview(&audio, 256, 128);

    // Check for silence/quiet in head
    let (silence_check, cached_peak_db) = {
        use crate::audio::source::ChannelView;
        let scan = audio.source.read_region(ChannelView::MonoMix, 0, audio.source.total_samples().min(
            (DEFAULT_ANALYSIS_WINDOW_SECS * sample_rate as f64) as u64,
        ) as usize);
        let peak = scan.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        if peak < 0.002 {
            (Some(SilenceCheck::Silent), None)
        } else if peak > 1e-10 {
            let peak_db = 20.0 * (peak as f64).log10();
            let auto_db = -3.0 - peak_db;
            let sc = if auto_db > 30.0 { Some(SilenceCheck::HighGain(auto_db)) } else { None };
            (sc, Some(peak_db))
        } else {
            (None, None)
        }
    };

    // Build placeholder spectrogram metadata
    const HOP_SIZE: usize = 512;
    let fft_size: usize = state.spect_fft_mode.get_untracked().fft_for_lod(crate::canvas::tile_cache::LOD_BASELINE);
    let total_len = total_frames as usize;
    let total_cols = if total_len >= fft_size {
        (total_len - fft_size) / HOP_SIZE + 1
    } else {
        0
    };

    let spectrogram = SpectrogramData {
        columns: Arc::new(Vec::new()),
        total_columns: total_cols,
        freq_resolution: sample_rate as f64 / fft_size as f64,
        time_resolution: HOP_SIZE as f64 / sample_rate as f64,
        max_freq: sample_rate as f64 / 2.0,
        sample_rate,
    };

    let name_owned = name.to_string();
    let file_index;
    {
        let mut idx = 0;
        state.files.update(|files| {
            idx = files.len();
            files.push(LoadedFile {
                name: name_owned.clone(),
                audio,
                spectrogram,
                preview: Some(preview),
                overview_image: None,
                xc_metadata: None,
                xc_hashes: None,
                is_demo: false,
                is_recording: false,
                is_live_listen: false,
                settings: FileSettings::default(),
                add_order: idx,
                last_modified_ms: None,
                identity: None,
                file_handle: Some(FileHandle::WebFile(file.clone())),
                cached_peak_db,
                cached_full_peak_db: None,
                read_only: false,
                had_sidecar: false,
                verify_outcome: crate::state::VerifyOutcome::Pending,
                all_hashes_verified: false,
                wav_markers: Vec::new(),
                loading_id: Some(load_id),
                min_display_freq: None,
                max_display_freq: None,
            });
            state.current_file_index.set(Some(idx));
        });
        file_index = idx;
    }

    // Schedule async full-file peak scan (for files > 30s)
    crate::audio::peak::start_full_peak_scan(state, file_index);

    if let Some(check) = silence_check {
        match check {
            SilenceCheck::Silent => {
                state.auto_gain.set(false);
                state.gain_db.set(0.0);
                state.show_info_toast("File appears silent \u{2014} auto-gain disabled");
            }
            SilenceCheck::HighGain(db) => {
                state.show_info_toast(format!("Quiet file \u{2014} auto-gain: +{:.0} dB", db));
            }
        }
    }

    use crate::canvas::{spectral_store, tile_cache};
    spectral_store::init(file_index, total_cols, fft_size);

    // Prefetch first viewport and schedule tiles
    {
        let scroll = state.scroll_offset.get_untracked();
        let zoom = state.zoom_level.get_untracked();
        let canvas_w = state.spectrogram_canvas_width.get_untracked();
        let time_res = HOP_SIZE as f64 / sample_rate as f64;
        let visible_time = if zoom > 0.0 { canvas_w / zoom * time_res } else { 1.0 };
        let start_sample = (scroll / time_res * HOP_SIZE as f64) as u64;
        let visible_samples = (visible_time * sample_rate as f64) as usize;
        source.prefetch_region(start_sample, visible_samples + fft_size).await;
    }

    tile_cache::schedule_visible_tiles_from_store(state, file_index, total_cols);
    state.tile_ready_signal.update(|n| *n = n.wrapping_add(1));

    // Spawn background progressive decode
    {
        let source_bg = source.clone();
        let name_bg = name.to_string();
        wasm_bindgen_futures::spawn_local(background_ogg_decode(
            state,
            file_index,
            name_bg,
            source_bg,
        ));
    }

    // Spawn background overview
    {
        let name_for_overview = name.to_string();
        wasm_bindgen_futures::spawn_local(build_streaming_overview(
            state,
            file_index,
            name_for_overview,
        ));
    }

    Ok(())
}

async fn background_ogg_decode(
    state: AppState,
    file_index: usize,
    expected_name: String,
    source: Arc<StreamingOggSource>,
) {
    // Initial delay — let the UI settle
    let p = js_sys::Promise::new(&mut |resolve, _| {
        web_sys::window().unwrap()
            .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 200).unwrap();
    });
    JsFuture::from(p).await.ok();

    while !source.is_fully_decoded() {
        // Check file still loaded
        let still_valid = state.files.get_untracked()
            .get(file_index)
            .map(|f| f.name == expected_name)
            .unwrap_or(false);
        if !still_valid { return; }

        // Defer while playing or loading
        let is_busy = state.is_playing.get_untracked()
            || state.loading_files.with_untracked(|v| !v.is_empty());
        if is_busy {
            let p = js_sys::Promise::new(&mut |resolve, _| {
                web_sys::window().unwrap()
                    .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 500).unwrap();
            });
            JsFuture::from(p).await.ok();
            continue;
        }

        // Decode one window worth of frames
        let cursor = source.decode_frame_cursor_value();
        source.prefetch_region(cursor, 262_144).await;

        // Yield to browser
        let p = js_sys::Promise::new(&mut |resolve, _| {
            web_sys::window().unwrap()
                .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 20).unwrap();
        });
        JsFuture::from(p).await.ok();
    }

    log::info!("Background OGG decode complete for {}", expected_name);
}

/// Build a high-res overview spectrogram image for a streaming file in the background.
///
/// Reads samples progressively from the streaming source with a large hop to produce
/// ~1024 FFT columns. Yields frequently and defers when the system is busy (playing
/// audio or computing main-view tiles).
pub(super) async fn build_streaming_overview(
    state: AppState,
    file_index: usize,
    expected_name: String,
) {
    use crate::audio::source::ChannelView;
    use crate::canvas::colors::magnitude_to_greyscale;
    use crate::types::PreviewImage;

    // Initial delay — let the UI settle after loading
    let p = js_sys::Promise::new(&mut |resolve, _| {
        web_sys::window().unwrap()
            .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 500).unwrap();
    });
    JsFuture::from(p).await.ok();

    let file = match state.files.get_untracked().get(file_index).cloned() {
        Some(f) if f.name == expected_name => f,
        _ => return,
    };

    // Skip if an overview already exists (non-streaming path already built one)
    if file.overview_image.is_some() { return; }

    let source = &file.audio.source;
    let sample_rate = file.audio.sample_rate;
    let total_samples = source.total_samples() as usize;

    const FFT_SIZE: usize = 512;
    const TARGET_COLS: usize = 1024;
    const TARGET_HEIGHT: u32 = 256;
    const COLS_PER_BATCH: usize = 8;

    let hop = (total_samples / TARGET_COLS).max(FFT_SIZE);
    let n_cols = if total_samples >= FFT_SIZE { (total_samples - FFT_SIZE) / hop + 1 } else { 0 };
    if n_cols == 0 { return; }

    let out_w = (n_cols as u32).min(TARGET_COLS as u32);
    let freq_bins = FFT_SIZE / 2 + 1;
    let out_h = (freq_bins as u32).min(TARGET_HEIGHT);

    // Accumulate magnitudes column by column
    let mut all_mags: Vec<Vec<f32>> = Vec::with_capacity(n_cols);
    let mut global_max: f32 = 0.0;

    let is_streaming_source = crate::audio::streaming_source::is_streaming(source.as_ref());

    let mut col = 0;
    while col < n_cols {
        // Check file still loaded and is still the current file
        let still_valid = state.files.get_untracked()
            .get(file_index)
            .map(|f| f.name == expected_name)
            .unwrap_or(false);
        if !still_valid { return; }

        // Defer while playing, loading new files, or if this isn't the current file
        let is_busy = state.is_playing.get_untracked()
            || state.loading_files.with_untracked(|v| !v.is_empty())
            || state.current_file_index.get_untracked() != Some(file_index);
        if is_busy {
            // Sleep 500ms and retry
            let p = js_sys::Promise::new(&mut |resolve, _| {
                web_sys::window().unwrap()
                    .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 500).unwrap();
            });
            JsFuture::from(p).await.ok();
            continue;
        }

        // Process a batch of columns
        let batch_end = (col + COLS_PER_BATCH).min(n_cols);
        for c in col..batch_end {
            let sample_start = c * hop;
            let sample_end = (sample_start + FFT_SIZE).min(total_samples);
            let read_len = sample_end - sample_start;
            if read_len < FFT_SIZE { break; }

            // Prefetch for streaming source (WAV, FLAC, or MP3)
            if is_streaming_source {
                crate::audio::streaming_source::prefetch_streaming(
                    source.as_ref(), sample_start as u64, read_len,
                ).await;
            }

            let samples = source.read_region(ChannelView::MonoMix, sample_start as u64, read_len);
            let cols = crate::dsp::fft::compute_stft_columns(
                &samples, sample_rate, FFT_SIZE, FFT_SIZE, 0, 1,
            );
            if let Some(column) = cols.into_iter().next() {
                let col_max = column.magnitudes.iter().copied().fold(0.0f32, f32::max);
                if col_max > global_max { global_max = col_max; }
                all_mags.push(column.magnitudes);
            }
        }

        col = batch_end;

        // Yield to browser between batches
        let p = js_sys::Promise::new(&mut |resolve, _| {
            web_sys::window().unwrap()
                .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 20).unwrap();
        });
        JsFuture::from(p).await.ok();
    }

    if all_mags.is_empty() || global_max <= 0.0 { return; }

    // Build the overview image
    let src_w = all_mags.len();
    let src_h = all_mags[0].len();
    let mut pixels = vec![0u8; (out_w * out_h * 4) as usize];

    for x in 0..out_w {
        let src_col = (x as usize * src_w) / out_w as usize;
        let mags = &all_mags[src_col.min(src_w - 1)];
        for y in 0..out_h {
            let src_bin = src_h - 1 - ((y as usize * src_h) / out_h as usize).min(src_h - 1);
            let mag = if src_bin < mags.len() { mags[src_bin] } else { 0.0 };
            let grey = magnitude_to_greyscale(mag, global_max);
            let idx = (y * out_w + x) as usize * 4;
            pixels[idx] = grey;
            pixels[idx + 1] = grey;
            pixels[idx + 2] = grey;
            pixels[idx + 3] = 255;
        }
    }

    let overview = PreviewImage {
        width: out_w,
        height: out_h,
        pixels: Arc::new(pixels),
    };

    // Update the file's overview image
    state.files.update(|files| {
        if let Some(f) = files.get_mut(file_index) {
            if f.name == expected_name {
                f.overview_image = Some(overview);
            }
        }
    });

    // Signal redraw so the overview panel picks up the new image
    state.tile_ready_signal.update(|n| *n = n.wrapping_add(1));
    log::info!("Background overview complete for {} ({} columns)", expected_name, src_w);
}

/// Decode raw PCM bytes to f32 samples (used for head region during streaming load).
fn decode_head_pcm(bytes: &[u8], bits_per_sample: u16, is_float: bool, _channels: u16) -> Vec<f32> {
    match (is_float, bits_per_sample) {
        (true, 32) => {
            bytes.chunks_exact(4)
                .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                .collect()
        }
        (false, 16) => {
            let max = 32768.0f32;
            bytes.chunks_exact(2)
                .map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / max)
                .collect()
        }
        (false, 24) => {
            let max = 8388608.0f32;
            bytes.chunks_exact(3)
                .map(|b| {
                    let val = (b[0] as i32) | ((b[1] as i32) << 8) | ((b[2] as i32) << 16);
                    let val = if val & 0x800000 != 0 { val | !0xFFFFFF } else { val };
                    val as f32 / max
                })
                .collect()
        }
        (false, 32) => {
            let max = 2147483648.0f32;
            bytes.chunks_exact(4)
                .map(|b| i32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f32 / max)
                .collect()
        }
        _ => {
            log::warn!("Unsupported PCM format for streaming: {}-bit {}", bits_per_sample, if is_float { "float" } else { "int" });
            vec![0.0; bytes.len() / (bits_per_sample as usize / 8)]
        }
    }
}

/// Scan raw bytes (from after the data chunk) for a GUANO "guan" chunk.
fn scan_tail_for_guano(tail_bytes: &[u8]) -> Option<crate::audio::guano::GuanoMetadata> {
    let mut pos = 0usize;
    while pos + 8 <= tail_bytes.len() {
        let chunk_id = &tail_bytes[pos..pos + 4];
        let chunk_size = u32::from_le_bytes(
            tail_bytes[pos + 4..pos + 8].try_into().ok()?,
        ) as usize;
        let body_start = pos + 8;
        let body_end = body_start + chunk_size;

        if chunk_id == b"guan" && body_end <= tail_bytes.len() {
            return crate::audio::guano::parse_guano_chunk(&tail_bytes[body_start..body_end]);
        }

        pos = body_start + ((chunk_size + 1) & !1);
    }
    None
}

/// Attempt to open a large M4A file using the streaming path.
/// Unlike MP3/OGG, M4A streaming requires the full compressed bytes in RAM
/// (moov sample table). Refuses files larger than ~1.5 GB compressed.
pub(super) async fn try_streaming_m4a(file: &File, name: &str, state: AppState, force_streaming: bool, load_id: u64) -> Result<(), String> {
    let file_size = file.size() as u64;

    // Fast-path probe: read up to 2 MB to find moov at the start of the file.
    // If we can estimate decoded size and it's below the streaming threshold
    // (and we aren't being forced), bail out without reading the whole file.
    // For Audible/ffmpeg moov-at-end files moov won't be in this peek; we
    // fall through to the full-file read and the same check happens there.
    let peek_size = (2 * 1024 * 1024).min(file_size as usize) as f64;
    let peek_bytes = read_blob_range(file, 0.0, peek_size).await?;
    if !is_m4a(&peek_bytes) {
        return Err("Not an M4A file".into());
    }
    if let Some(est_decoded) = crate::audio::loader::estimate_m4a_decoded_bytes(&peek_bytes) {
        should_stream_from_decoded_size(est_decoded, force_streaming)?;
    }

    // Cap compressed size to stay within WASM's 4 GB address space (we keep
    // the full compressed file in RAM for the streaming reader).
    const MAX_COMPRESSED: u64 = 1_500_000_000;
    if file_size > MAX_COMPRESSED {
        return Err(format!(
            "M4A too large to stream in WASM: {:.1} GB > {:.1} GB cap",
            file_size as f64 / 1_073_741_824.0,
            MAX_COMPRESSED as f64 / 1_073_741_824.0,
        ));
    }

    // Load the whole compressed file.
    let all_bytes = read_blob_range(file, 0.0, file_size as f64).await?;

    use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    use symphonia::core::probe::Hint;

    let cursor = std::io::Cursor::new(all_bytes.clone());
    let mss = MediaSourceStream::new(Box::new(cursor), Default::default());
    let mut hint = Hint::new();
    hint.with_extension("m4a");
    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| format!("M4A probe error: {e}"))?;

    let format_ref = &probed.format;
    let track = format_ref
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or("No audio track in M4A")?;

    // Some ffmpeg/Audible-encoded files leave symphonia's codec_params.channels
    // (and sometimes sample_rate) unset. Fall back to reading the mp4a sample
    // entry directly so we can still stream them.
    let atom_entry = crate::audio::loader::parse_m4a_audio_entry(&all_bytes);
    let sample_rate = track.codec_params.sample_rate
        .or_else(|| atom_entry.map(|(_, sr)| sr).filter(|&sr| sr > 0))
        .or_else(|| crate::audio::loader::parse_m4a_sample_rate(&all_bytes))
        .ok_or("M4A missing sample rate")?;
    let channels = match track.codec_params.channels {
        Some(c) => c.count(),
        None => atom_entry
            .map(|(c, _)| c as usize)
            .filter(|&c| c >= 1 && c <= 8)
            .ok_or("M4A missing channel info (not in codec_params nor mp4a atom)")?,
    };
    let track_id = track.id;
    let total_frames = track.codec_params.n_frames
        .ok_or("M4A missing total frame count (sample table incomplete)")?;

    // Decoded size check — only stream if the decoded PCM would be large.
    let decoded_bytes = total_frames * channels as u64 * 4;
    should_stream_from_decoded_size(decoded_bytes, force_streaming)?;

    log::info!(
        "Streaming M4A: {} — {} frames, {} ch, {} Hz, {:.1}s, decoded {:.0} MB",
        name, total_frames, channels, sample_rate,
        total_frames as f64 / sample_rate as f64,
        decoded_bytes as f64 / 1_048_576.0,
    );

    if force_streaming && decoded_bytes < STREAMING_DECODED_THRESHOLD {
        state.show_info_toast(format!(
            "Streaming M4A to keep total open files under control ({:.0} MB)",
            file.size() / 1_000_000.0
        ));
    } else {
        state.show_info_toast(format!(
            "Streaming large M4A ({:.0} MB)",
            file.size() / 1_000_000.0
        ));
    }

    // Collect tags for the metadata panel — same path the in-memory decode
    // takes, so streaming and non-streaming M4A files show the same
    // iTunes-style keys (Title/Artist/Album/etc.) and any Nero chapters live
    // under the same "Audio file metadata" heading.
    let mut tags = crate::audio::guano::GuanoMetadata::new();
    for (k, v) in crate::audio::loader::parse_m4a_tags(&all_bytes) {
        tags.add(&k, &v);
    }

    // If symphonia's codec_params are missing channels/sample_rate (Audible,
    // some ffmpeg outputs), inject our mp4a-parsed values before creating the
    // decoder — many AAC decoder implementations refuse to initialize without.
    let mut codec_params = track.codec_params.clone();
    if codec_params.channels.is_none() {
        use symphonia::core::audio::Channels;
        let layout = match channels {
            1 => Channels::FRONT_LEFT,
            2 => Channels::FRONT_LEFT | Channels::FRONT_RIGHT,
            _ => Channels::from_bits_truncate((1u32 << channels).saturating_sub(1)),
        };
        codec_params.channels = Some(layout);
    }
    if codec_params.sample_rate.is_none() {
        codec_params.sample_rate = Some(sample_rate);
    }
    let mut decoder = symphonia::default::get_codecs()
        .make(&codec_params, &DecoderOptions::default())
        .map_err(|e| format!("M4A decoder error: {e}"))?;

    // Decode head (first ~30s).
    use crate::audio::source::DEFAULT_ANALYSIS_WINDOW_SECS;
    let head_target_frames = ((DEFAULT_ANALYSIS_WINDOW_SECS * sample_rate as f64) as u64)
        .min(total_frames);

    use symphonia::core::audio::SampleBuffer;
    use symphonia::core::errors::Error as SymphoniaError;

    let mut format = probed.format;
    let mut head_interleaved: Vec<f32> = Vec::new();
    let mut head_frame_count: u64 = 0;
    let mut next_frame: u64 = 0;
    let mut frames_since_yield: u64 = 0;
    // Authoritative spec filled in from the first successful packet — the
    // mp4a atom sometimes reports pre-SBR / pre-PS values that don't match
    // what symphonia's AAC decoder actually outputs.
    let mut actual_rate: Option<u32> = None;
    let mut actual_channels: Option<usize> = None;

    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymphoniaError::ResetRequired) => { decoder.reset(); continue; }
            Err(SymphoniaError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(_) => break,
        };
        if packet.track_id() != track_id { continue; }
        match decoder.decode(&packet) {
            Ok(decoded) => {
                let spec = *decoded.spec();
                if actual_rate.is_none() {
                    let dec_ch = spec.channels.count();
                    if spec.rate != sample_rate as u32 || dec_ch != channels {
                        log::info!(
                            "M4A spec mismatch — container said {} ch @ {} Hz, decoder outputs {} ch @ {} Hz",
                            channels, sample_rate, dec_ch, spec.rate,
                        );
                    }
                    actual_rate = Some(spec.rate);
                    actual_channels = Some(dec_ch);
                }
                let ch = actual_channels.unwrap();
                let mut buf = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
                buf.copy_interleaved_ref(decoded);
                let samples = buf.samples();
                let n_frames = samples.len() / ch;
                head_interleaved.extend_from_slice(samples);
                head_frame_count += n_frames as u64;
                next_frame += n_frames as u64;
                frames_since_yield += n_frames as u64;

                if frames_since_yield >= 65_536 {
                    frames_since_yield = 0;
                    crate::canvas::tile_cache::yield_to_browser().await;
                }
                if head_frame_count >= head_target_frames { break; }
            }
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(_) => break,
        }
    }

    if head_frame_count == 0 {
        return Err("No M4A frames decoded".into());
    }

    // Authoritative values from the decoder, now that we've seen real output.
    // If the container reported a different rate (e.g. mp4a sample_rate of
    // 22050 for a file the decoder outputs at 44100, or vice versa), scale
    // total_frames to match actual_rate so duration and seeking stay consistent.
    let pre_decode_rate = sample_rate;
    let sample_rate = actual_rate.unwrap_or(sample_rate);
    let channels = actual_channels.unwrap_or(channels);
    if pre_decode_rate > sample_rate && pre_decode_rate == sample_rate * 2 {
        // Container rate is twice the decoder's output rate — classic HE-AAC
        // SBR signature. symphonia doesn't implement SBR, so content above
        // ~sample_rate/2 is missing. For voice/audiobooks this is mostly OK.
        state.show_info_toast(format!(
            "HE-AAC detected: decoded at {} kHz without SBR (container reports {} kHz).",
            sample_rate / 1000, pre_decode_rate / 1000,
        ));
    }
    let total_frames = if pre_decode_rate > 0 && pre_decode_rate != sample_rate {
        ((total_frames as u128 * sample_rate as u128) / pre_decode_rate as u128) as u64
    } else {
        total_frames
    };

    let actual_head_frames = head_frame_count.min(head_target_frames) as usize;
    head_interleaved.truncate(actual_head_frames * channels);

    let (head_mono, head_raw) = if channels == 1 {
        (head_interleaved, None)
    } else {
        let mono: Vec<f32> = head_interleaved
            .chunks_exact(channels)
            .map(|frame| frame.iter().sum::<f32>() / channels as f32)
            .collect();
        (mono, Some(head_interleaved))
    };

    // Parse Nero chapters from the compressed bytes.
    let wav_markers = parse_m4a_chapters(&all_bytes, sample_rate);

    let source = Arc::new(StreamingM4aSource::new(
        FileHandle::WebFile(file.clone()),
        format,
        decoder,
        track_id,
        sample_rate,
        channels as u32,
        total_frames,
        file_size,
        head_mono.clone(),
        head_raw,
        next_frame,
    ));

    let duration_secs = total_frames as f64 / sample_rate as f64;
    let samples = Arc::new(head_mono);

    let audio = AudioData {
        samples,
        source: source.clone(),
        sample_rate,
        channels: channels as u32,
        duration_secs,
        metadata: crate::types::FileMetadata {
            file_size: file.size() as usize,
            format: "M4A",
            bits_per_sample: 16,
            is_float: false,
            guano: if tags.fields.is_empty() { None } else { Some(tags) },
            data_offset: None,
            data_size: None,
        },
    };

    let preview = compute_preview(&audio, 256, 128);

    let (silence_check, cached_peak_db) = {
        use crate::audio::source::ChannelView;
        let scan = audio.source.read_region(ChannelView::MonoMix, 0, audio.source.total_samples().min(
            (DEFAULT_ANALYSIS_WINDOW_SECS * sample_rate as f64) as u64,
        ) as usize);
        let peak = scan.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        if peak < 0.002 {
            (Some(SilenceCheck::Silent), None)
        } else if peak > 1e-10 {
            let peak_db = 20.0 * (peak as f64).log10();
            let auto_db = -3.0 - peak_db;
            let sc = if auto_db > 30.0 { Some(SilenceCheck::HighGain(auto_db)) } else { None };
            (sc, Some(peak_db))
        } else {
            (None, None)
        }
    };

    const HOP_SIZE: usize = 512;
    let fft_size: usize = state.spect_fft_mode.get_untracked().fft_for_lod(crate::canvas::tile_cache::LOD_BASELINE);
    let total_len = total_frames as usize;
    let total_cols = if total_len >= fft_size { (total_len - fft_size) / HOP_SIZE + 1 } else { 0 };

    let spectrogram = SpectrogramData {
        columns: Arc::new(Vec::new()),
        total_columns: total_cols,
        freq_resolution: sample_rate as f64 / fft_size as f64,
        time_resolution: HOP_SIZE as f64 / sample_rate as f64,
        max_freq: sample_rate as f64 / 2.0,
        sample_rate,
    };

    let name_owned = name.to_string();
    let file_index;
    {
        let mut idx = 0;
        state.files.update(|files| {
            idx = files.len();
            files.push(LoadedFile {
                name: name_owned.clone(),
                audio,
                spectrogram,
                preview: Some(preview),
                overview_image: None,
                xc_metadata: None,
                xc_hashes: None,
                is_demo: false,
                is_recording: false,
                is_live_listen: false,
                settings: FileSettings::default(),
                add_order: idx,
                last_modified_ms: None,
                identity: None,
                file_handle: Some(FileHandle::WebFile(file.clone())),
                cached_peak_db,
                cached_full_peak_db: None,
                read_only: false,
                had_sidecar: false,
                verify_outcome: crate::state::VerifyOutcome::Pending,
                all_hashes_verified: false,
                wav_markers,
                loading_id: Some(load_id),
                min_display_freq: None,
                max_display_freq: None,
            });
            state.current_file_index.set(Some(idx));
        });
        file_index = idx;
    }

    crate::audio::peak::start_full_peak_scan(state, file_index);

    if let Some(check) = silence_check {
        match check {
            SilenceCheck::Silent => {
                state.auto_gain.set(false);
                state.gain_db.set(0.0);
                state.show_info_toast("File appears silent \u{2014} auto-gain disabled");
            }
            SilenceCheck::HighGain(db) => {
                state.show_info_toast(format!("Quiet file \u{2014} auto-gain: +{:.0} dB", db));
            }
        }
    }

    use crate::canvas::{spectral_store, tile_cache};
    spectral_store::init(file_index, total_cols, fft_size);

    // Prefetch first viewport.
    {
        let scroll = state.scroll_offset.get_untracked();
        let zoom = state.zoom_level.get_untracked();
        let canvas_w = state.spectrogram_canvas_width.get_untracked();
        let time_res = HOP_SIZE as f64 / sample_rate as f64;
        let visible_time = if zoom > 0.0 { canvas_w / zoom * time_res } else { 1.0 };
        let start_sample = (scroll / time_res * HOP_SIZE as f64) as u64;
        let visible_samples = (visible_time * sample_rate as f64) as usize;
        source.prefetch_region(start_sample, visible_samples + fft_size).await;
    }

    tile_cache::schedule_visible_tiles_from_store(state, file_index, total_cols);
    state.tile_ready_signal.update(|n| *n = n.wrapping_add(1));

    // Spawn background progressive decode to fill remaining chunks.
    {
        let source_bg = source.clone();
        let name_bg = name.to_string();
        wasm_bindgen_futures::spawn_local(background_m4a_decode(state, file_index, name_bg, source_bg));
    }

    // Spawn background overview build.
    {
        let name_for_overview = name.to_string();
        wasm_bindgen_futures::spawn_local(build_streaming_overview(state, file_index, name_for_overview));
    }

    Ok(())
}

/// Progressively decode remaining chunks of a streaming M4A in the background.
async fn background_m4a_decode(
    state: AppState,
    file_index: usize,
    expected_name: String,
    source: Arc<StreamingM4aSource>,
) {
    use crate::canvas::tile_cache::{self, TILE_COLS};

    // Initial delay.
    let p = js_sys::Promise::new(&mut |resolve, _| {
        web_sys::window().unwrap()
            .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 200).unwrap();
    });
    JsFuture::from(p).await.ok();

    let hop_size = 512usize;
    let tile_samples = TILE_COLS * hop_size;
    let mut last_tile_scheduled: Option<usize> = None;
    // Local cursor — advances regardless of whether prefetch_region actually
    // decoded (it may be a no-op if the range is already cached, or bail out
    // because another task is mid-decode). Using source.decode_frame_cursor
    // as a "still making progress" signal caused bg to give up permanently
    // the first time a viewport prefetch raced it.
    use crate::audio::source::AudioSource;
    let total_frames = source.total_samples();
    let mut bg_cursor: u64 = source.decode_frame_cursor_value();

    while bg_cursor < total_frames && !source.is_fully_decoded() {
        let still_valid = state.files.get_untracked()
            .get(file_index)
            .map(|f| f.name == expected_name)
            .unwrap_or(false);
        if !still_valid { return; }

        let is_busy = state.is_playing.get_untracked()
            || state.loading_files.with_untracked(|v| !v.is_empty());
        if is_busy {
            let p = js_sys::Promise::new(&mut |resolve, _| {
                web_sys::window().unwrap()
                    .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 500).unwrap();
            });
            JsFuture::from(p).await.ok();
            continue;
        }

        let cursor_before = bg_cursor;
        source.prefetch_region(bg_cursor, 262_144).await;
        bg_cursor = (bg_cursor + 262_144).min(total_frames);

        if tile_samples > 0 && bg_cursor > cursor_before {
            let first_tile = cursor_before as usize / tile_samples;
            let last_tile = (bg_cursor - 1) as usize / tile_samples;
            let start = last_tile_scheduled.map(|t| t + 1).unwrap_or(first_tile);
            if start <= last_tile {
                for t in start..=last_tile {
                    tile_cache::schedule_tile_on_demand(state, file_index, t);
                }
                last_tile_scheduled = Some(last_tile);
                state.tile_ready_signal.update(|n| *n = n.wrapping_add(1));
            }
        }

        let p = js_sys::Promise::new(&mut |resolve, _| {
            web_sys::window().unwrap()
                .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 20).unwrap();
        });
        JsFuture::from(p).await.ok();
    }

    log::info!("Background M4A decode complete for {}", expected_name);
}
