use leptos::prelude::*;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use js_sys;
use wasm_bindgen_futures::JsFuture;
use web_sys::{File, FileReader};
use crate::audio::loader::{is_mp3, load_audio, parse_flac_header, parse_mp3_header, parse_wav_header_with_file_size};
use crate::audio::streaming_source::{FileHandle, StreamingFlacSource, StreamingMp3Source, StreamingWavSource, read_blob_range};
use crate::dsp::fft::{compute_overview_from_spectrogram, compute_preview, compute_spectrogram_partial};
use crate::state::{AppState, FileSettings, LoadedFile};
use crate::types::{AudioData, SpectrogramData};
use std::sync::Arc;

enum SilenceCheck {
    Silent,
    HighGain(f64),
}

/// Maximum file size the browser can handle for full in-memory decode (~2 GB).
/// Files above this MUST use the streaming path; if streaming fails, they're rejected.
const MAX_FILE_SIZE: f64 = 2_000_000_000.0;

/// Raw file size above which we attempt the streaming WAV path.
const STREAMING_CHECK_SIZE: f64 = 128.0 * 1024.0 * 1024.0; // 128 MB

/// Decoded size threshold for streaming (512 MB of f32 samples).
const STREAMING_DECODED_THRESHOLD: u64 = 512 * 1024 * 1024;

pub(super) async fn read_and_load_file(file: File, state: AppState, load_id: u64) -> Result<(), String> {
    let name = file.name();
    let size = file.size();
    let last_modified_ms = Some(file.last_modified());

    // Helper: set last_modified_ms and compute file identity on the most recently added file
    let name_for_identity = name.clone();
    let finalize_loaded_file = move |state: AppState, lm: Option<f64>| {
        let file_size = size as u64;
        let file_name = name_for_identity.clone();
        state.files.update(|files| {
            if let Some(f) = files.last_mut() {
                f.last_modified_ms = lm;
            }
        });
        // Compute file identity (Layer 1 + Layer 2 async)
        let file_index = state.files.get_untracked().len().saturating_sub(1);
        // Read data_offset/data_size from the loaded file's metadata
        let (data_offset, data_size) = state.files.with_untracked(|files| {
            files.get(file_index)
                .map(|f| (f.audio.metadata.data_offset, f.audio.metadata.data_size))
                .unwrap_or((None, None))
        });
        crate::file_identity::start_identity_computation(
            state, file_index, file_name, file_size, None,
            data_offset, data_size, lm,
        );
    };

    // For large files, attempt streaming path (WAV or FLAC)
    if size > STREAMING_CHECK_SIZE {
        state.loading_update(load_id, crate::state::LoadingStage::Streaming);
        match try_streaming_wav(&file, &name, state).await {
            Ok(()) => { finalize_loaded_file(state, last_modified_ms); return Ok(()); }
            Err(e) => {
                log::info!("WAV streaming not applicable for {}: {}", name, e);
            }
        }
        match try_streaming_flac(&file, &name, state).await {
            Ok(()) => { finalize_loaded_file(state, last_modified_ms); return Ok(()); }
            Err(e) => {
                log::info!("FLAC streaming not applicable for {}: {}", name, e);
            }
        }
        match try_streaming_mp3(&file, &name, state).await {
            Ok(()) => { finalize_loaded_file(state, last_modified_ms); return Ok(()); }
            Err(e) => {
                log::info!("MP3 streaming not applicable for {}: {}", name, e);
            }
        }
        // Streaming didn't apply — fall through to full decode
        state.loading_update(load_id, crate::state::LoadingStage::Decoding);
    }

    if size > MAX_FILE_SIZE {
        let msg = format!(
            "File too large ({:.1} GB) — only WAV, FLAC, and MP3 files can be streamed above 2 GB",
            size / 1_000_000_000.0
        );
        state.show_error_toast(&msg);
        return Err(msg);
    }
    let bytes = read_file_bytes(&file).await?;
    let result = load_named_bytes(name, &bytes, None, state, load_id).await;
    if result.is_ok() {
        finalize_loaded_file(state, last_modified_ms);
    }
    result
}

/// Attempt to open a large WAV file using the streaming path.
/// Returns Ok(()) if successful, Err if the file is not suitable for streaming
/// (not WAV, decoded size below threshold, unsupported format).
async fn try_streaming_wav(file: &File, name: &str, state: AppState) -> Result<(), String> {
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
    if decoded_bytes < STREAMING_DECODED_THRESHOLD {
        return Err(format!(
            "Decoded size {:.0} MB below streaming threshold",
            decoded_bytes as f64 / 1_048_576.0
        ));
    }

    log::info!(
        "Streaming WAV: {} — {} frames, {} ch, {} Hz, {:.1}s, decoded {:.0} MB",
        name,
        header.total_frames,
        header.channels,
        header.sample_rate,
        header.total_frames as f64 / header.sample_rate as f64,
        decoded_bytes as f64 / 1_048_576.0,
    );

    state.show_info_toast(format!(
        "Streaming large file ({:.0} MB)",
        file.size() / 1_000_000.0
    ));

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
    let silence_check = {
        use crate::audio::source::ChannelView;
        let scan = audio.source.read_region(ChannelView::MonoMix, 0, audio.source.total_samples().min(
            (DEFAULT_ANALYSIS_WINDOW_SECS * sample_rate as f64) as u64,
        ) as usize);
        let peak = scan.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        if peak < 0.002 {
            Some(SilenceCheck::Silent)
        } else if peak > 1e-10 {
            let peak_db = 20.0 * (peak as f64).log10();
            let auto_db = -3.0 - peak_db;
            if auto_db > 30.0 { Some(SilenceCheck::HighGain(auto_db)) } else { None }
        } else {
            None
        }
    };

    // Build placeholder spectrogram metadata (tiles computed on demand)
    const HOP_SIZE: usize = 512;
    let fft_size: usize = state.spect_fft_mode.get_untracked().fft_for_lod(HOP_SIZE);
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
                is_recording: false,
                settings: FileSettings::default(),
                add_order: idx,
                last_modified_ms: None,
                identity: None,
                file_handle: Some(FileHandle::WebFile(file.clone())),
            });
            if files.len() == 1 {
                state.current_file_index.set(Some(0));
            }
        });
        file_index = idx;
    }

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
    spectral_store::init(file_index, total_cols);

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
async fn try_streaming_flac(file: &File, name: &str, state: AppState) -> Result<(), String> {
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
    if decoded_bytes < STREAMING_DECODED_THRESHOLD {
        return Err(format!(
            "Decoded size {:.0} MB below streaming threshold",
            decoded_bytes as f64 / 1_048_576.0
        ));
    }

    log::info!(
        "Streaming FLAC: {} — {} frames, {} ch, {} Hz, {:.1}s, decoded {:.0} MB",
        name,
        header.total_frames,
        header.channels,
        header.sample_rate,
        header.total_frames as f64 / header.sample_rate as f64,
        decoded_bytes as f64 / 1_048_576.0,
    );

    state.show_info_toast(format!(
        "Streaming large FLAC ({:.0} MB)",
        file.size() / 1_000_000.0
    ));

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
                    block_buf = block.into_buffer();

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
    let silence_check = {
        use crate::audio::source::ChannelView;
        let scan = audio.source.read_region(ChannelView::MonoMix, 0, audio.source.total_samples().min(
            (DEFAULT_ANALYSIS_WINDOW_SECS * sample_rate as f64) as u64,
        ) as usize);
        let peak = scan.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        if peak < 0.002 {
            Some(SilenceCheck::Silent)
        } else if peak > 1e-10 {
            let peak_db = 20.0 * (peak as f64).log10();
            let auto_db = -3.0 - peak_db;
            if auto_db > 30.0 { Some(SilenceCheck::HighGain(auto_db)) } else { None }
        } else {
            None
        }
    };

    // Build placeholder spectrogram metadata
    const HOP_SIZE: usize = 512;
    let fft_size: usize = state.spect_fft_mode.get_untracked().fft_for_lod(HOP_SIZE);
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
                is_recording: false,
                settings: FileSettings::default(),
                add_order: idx,
                last_modified_ms: None,
                identity: None,
                file_handle: Some(FileHandle::WebFile(file.clone())),
            });
            if files.len() == 1 {
                state.current_file_index.set(Some(0));
            }
        });
        file_index = idx;
    }

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
    spectral_store::init(file_index, total_cols);

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
async fn try_streaming_mp3(file: &File, name: &str, state: AppState) -> Result<(), String> {
    // Read first 64KB for header probing
    let header_size = 65536.0f64.min(file.size());
    let header_bytes = read_blob_range(file, 0.0, header_size).await?;

    if !is_mp3(&header_bytes) {
        return Err("Not an MP3 file".into());
    }

    let file_size = file.size() as u64;
    let header = parse_mp3_header(&header_bytes, file_size)?;

    // Check if decoded size warrants streaming
    let decoded_bytes = header.estimated_total_frames * header.channels as u64 * 4;
    if decoded_bytes < STREAMING_DECODED_THRESHOLD {
        return Err(format!(
            "Decoded size {:.0} MB below streaming threshold",
            decoded_bytes as f64 / 1_048_576.0
        ));
    }

    log::info!(
        "Streaming MP3: {} — ~{} frames, {} ch, {} Hz, ~{:.1}s, decoded ~{:.0} MB",
        name,
        header.estimated_total_frames,
        header.channels,
        header.sample_rate,
        header.estimated_total_frames as f64 / header.sample_rate as f64,
        decoded_bytes as f64 / 1_048_576.0,
    );

    state.show_info_toast(format!(
        "Streaming large MP3 ({:.0} MB)",
        file.size() / 1_000_000.0
    ));

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
    let silence_check = {
        use crate::audio::source::ChannelView;
        let scan = audio.source.read_region(ChannelView::MonoMix, 0, audio.source.total_samples().min(
            (DEFAULT_ANALYSIS_WINDOW_SECS * sample_rate as f64) as u64,
        ) as usize);
        let peak = scan.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        if peak < 0.002 {
            Some(SilenceCheck::Silent)
        } else if peak > 1e-10 {
            let peak_db = 20.0 * (peak as f64).log10();
            let auto_db = -3.0 - peak_db;
            if auto_db > 30.0 { Some(SilenceCheck::HighGain(auto_db)) } else { None }
        } else {
            None
        }
    };

    // Build placeholder spectrogram metadata
    const HOP_SIZE: usize = 512;
    let fft_size: usize = state.spect_fft_mode.get_untracked().fft_for_lod(HOP_SIZE);
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
                is_recording: false,
                settings: FileSettings::default(),
                add_order: idx,
                last_modified_ms: None,
                identity: None,
                file_handle: Some(FileHandle::WebFile(file.clone())),
            });
            if files.len() == 1 {
                state.current_file_index.set(Some(0));
            }
        });
        file_index = idx;
    }

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
    spectral_store::init(file_index, total_cols);

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

    log::info!("Background MP3 decode complete for {}", expected_name);
}

/// Build a high-res overview spectrogram image for a streaming file in the background.
///
/// Reads samples progressively from the streaming source with a large hop to produce
/// ~1024 FFT columns. Yields frequently and defers when the system is busy (playing
/// audio or computing main-view tiles).
async fn build_streaming_overview(
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

pub(crate) async fn load_named_bytes(name: String, bytes: &[u8], xc_metadata: Option<Vec<(String, String)>>, state: AppState, load_id: u64) -> Result<(), String> {
    let audio = load_audio(bytes)?;
    log::info!(
        "Loaded {}: {} samples, {} Hz, {:.2}s",
        name,
        audio.source.total_samples(),
        audio.sample_rate,
        audio.duration_secs
    );

    // Phase 1: fast preview
    state.loading_update(load_id, crate::state::LoadingStage::Preview);
    let preview = compute_preview(&audio, 256, 128);
    let audio_for_stft = audio.clone();
    let name_check = name.clone();

    const HOP_SIZE: usize = 512; // LOD1 hop
    let fft_size: usize = state.spect_fft_mode.get_untracked().fft_for_lod(HOP_SIZE);

    // Check for silent/quiet files — scan first 30s only
    let silence_check = {
        use crate::audio::source::{ChannelView, DEFAULT_ANALYSIS_WINDOW_SECS};
        let total_len = audio.source.total_samples() as usize;
        let scan_end = total_len.min(
            (DEFAULT_ANALYSIS_WINDOW_SECS * audio.sample_rate as f64) as usize,
        );
        let scan_samples = audio.source.read_region(ChannelView::MonoMix, 0, scan_end);
        let peak = scan_samples.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        if peak < 0.002 {
            Some(SilenceCheck::Silent)
        } else if peak > 1e-10 {
            let peak_db = 20.0 * (peak as f64).log10();
            let auto_db = -3.0 - peak_db;
            if auto_db > 30.0 { Some(SilenceCheck::HighGain(auto_db)) } else { None }
        } else {
            None
        }
    };

    let total_len = audio.source.total_samples() as usize;
    let total_cols = if total_len >= fft_size {
        (total_len - fft_size) / HOP_SIZE + 1
    } else {
        0
    };

    let placeholder_spec = SpectrogramData {
        columns: Arc::new(Vec::new()),
        total_columns: total_cols,
        freq_resolution: audio.sample_rate as f64 / fft_size as f64,
        time_resolution: HOP_SIZE as f64 / audio.sample_rate as f64,
        max_freq: audio.sample_rate as f64 / 2.0,
        sample_rate: audio.sample_rate,
    };

    let file_index;
    {
        let mut idx = 0;
        state.files.update(|files| {
            idx = files.len();
            files.push(LoadedFile {
                name,
                audio,
                spectrogram: placeholder_spec,
                preview: Some(preview),
                overview_image: None,
                xc_metadata,
                is_recording: false,
                settings: FileSettings::default(),
                add_order: idx,
                last_modified_ms: None,
                identity: None,
                file_handle: None,
            });
            if files.len() == 1 {
                state.current_file_index.set(Some(0));
            }
        });
        file_index = idx;
    }

    // Compute file identity (Layer 1 + Layer 2 with bytes available)
    let (data_offset, data_size) = state.files.with_untracked(|files| {
        files.get(file_index)
            .map(|f| (f.audio.metadata.data_offset, f.audio.metadata.data_size))
            .unwrap_or((None, None))
    });
    crate::file_identity::start_identity_computation(
        state, file_index, name_check.clone(), bytes.len() as u64, Some(bytes.to_vec()),
        data_offset, data_size, None,
    );

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

    // Yield to let the UI render the preview
    let yield_promise = js_sys::Promise::new(&mut |resolve, _| {
        web_sys::window()
            .unwrap()
            .set_timeout_with_callback(&resolve)
            .unwrap();
    });
    JsFuture::from(yield_promise).await.ok();

    // Phase 2: full spectrogram — computed in small chunks so the browser
    // stays responsive.  Chunks are computed viewport-first (expanding
    // outward from the current scroll position) so the visible region
    // appears quickly even for very long files.
    //
    // Columns are inserted into the spectral store as they are computed,
    // and completed TILE_COLS-wide tiles are scheduled for rendering
    // immediately — so the user sees tiles appearing progressively.
    const CHUNK_COLS: usize = 32; // ~50 ms of work per chunk on typical hardware

    // total_cols already computed above for placeholder_spec

    // Initialise the spectral column store for incremental tile generation
    use crate::canvas::spectral_store;
    use crate::canvas::tile_cache::{self, TILE_COLS};
    spectral_store::init(file_index, total_cols);

    // Build chunk schedule: viewport-first expanding order
    let time_resolution = HOP_SIZE as f64 / audio_for_stft.sample_rate as f64;
    let scroll = state.scroll_offset.get_untracked();
    let zoom = state.zoom_level.get_untracked();
    let canvas_w = state.spectrogram_canvas_width.get_untracked();
    let visible_time = if zoom > 0.0 { canvas_w / zoom * time_resolution } else { 1.0 };
    let center_col = ((scroll + visible_time / 2.0) / time_resolution) as usize;
    let center_col = center_col.min(total_cols.saturating_sub(1));

    // Generate chunk start indices in expanding-ring order from center
    let total_chunks = (total_cols + CHUNK_COLS - 1) / CHUNK_COLS;
    let center_chunk = center_col / CHUNK_COLS;
    let chunk_order = expanding_chunk_order(center_chunk, total_chunks);

    // Track which tile-width ranges have been fully computed
    let n_tiles = (total_cols + TILE_COLS - 1) / TILE_COLS;
    let mut tile_scheduled = vec![false; n_tiles];

    state.loading_update(load_id, crate::state::LoadingStage::Spectrogram(0));
    let mut chunks_done = 0usize;
    let mut last_reported_pct = 0u16;

    for chunk_idx in chunk_order {
        let chunk_start = chunk_idx * CHUNK_COLS;
        if chunk_start >= total_cols {
            continue;
        }

        // Check the file is still loaded (user may have removed it)
        let still_present = state.files.get_untracked()
            .get(file_index)
            .map(|f| f.name == name_check)
            .unwrap_or(false);
        if !still_present {
            spectral_store::clear_file(file_index);
            return Ok(());
        }

        let chunk = compute_spectrogram_partial(
            &audio_for_stft,
            fft_size,
            HOP_SIZE,
            chunk_start,
            CHUNK_COLS,
        );

        // Insert into spectral store (updates running max magnitude)
        spectral_store::insert_columns(file_index, chunk_start, &chunk);

        // Check if any tile-width ranges are now complete and render them
        // synchronously — before more insertions can evict the columns.
        let first_affected_tile = chunk_start / TILE_COLS;
        let last_affected_tile = ((chunk_start + chunk.len()).saturating_sub(1)) / TILE_COLS;
        let mut any_tile_rendered = false;
        for tile_idx in first_affected_tile..=last_affected_tile.min(n_tiles.saturating_sub(1)) {
            if tile_scheduled[tile_idx] { continue; }
            let tile_start = tile_idx * TILE_COLS;
            let tile_end = (tile_start + TILE_COLS).min(total_cols);
            if spectral_store::tile_complete(file_index, tile_start, tile_end) {
                if tile_cache::render_tile_from_store_sync(file_index, tile_idx) {
                    any_tile_rendered = true;
                }
                tile_scheduled[tile_idx] = true;
            }
        }
        if any_tile_rendered {
            state.tile_ready_signal.update(|n| *n = n.wrapping_add(1));
        }

        // Update loading progress (every ~5%)
        chunks_done += 1;
        let pct = ((chunks_done as f64 / total_chunks as f64) * 100.0) as u16;
        if pct >= last_reported_pct + 5 || chunks_done == total_chunks {
            state.loading_update(load_id, crate::state::LoadingStage::Spectrogram(pct.min(100)));
            last_reported_pct = pct;
        }

        // Yield so the browser can process events / paint between chunks
        let p = js_sys::Promise::new(&mut |resolve, _| {
            web_sys::window().unwrap().set_timeout_with_callback(&resolve).unwrap();
        });
        JsFuture::from(p).await.ok();
    }

    state.loading_update(load_id, crate::state::LoadingStage::Finalizing);

    // Large-file threshold: above this, we keep the spectral store alive and
    // don't assemble a monolithic SpectrogramData (saves hundreds of MB).
    // ~50 000 columns ≈ 5 min @ 44.1 kHz or 2.7 min @ 96 kHz ≈ 200 MB of column data.
    const LARGE_FILE_COLS: usize = 50_000;
    let is_large = total_cols > LARGE_FILE_COLS;

    let freq_resolution = audio_for_stft.sample_rate as f64 / fft_size as f64;
    let max_freq = audio_for_stft.sample_rate as f64 / 2.0;

    if is_large {
        // Large file: keep spectral store alive, don't assemble full column data.
        // Tiles will be computed on-demand from the store (or recomputed from audio).
        log::info!(
            "Large file ({} columns) — keeping spectral store, skipping full assembly",
            total_cols
        );

        // Update metadata without draining columns
        let spectrogram = SpectrogramData {
            columns: Arc::new(Vec::new()),
            total_columns: total_cols,
            freq_resolution,
            time_resolution,
            max_freq,
            sample_rate: audio_for_stft.sample_rate,
        };
        state.files.update(|files| {
            if let Some(f) = files.get_mut(file_index) {
                if f.name == name_check {
                    f.spectrogram = spectrogram;
                }
            }
        });

        // Large non-streaming files also lack an overview — build one in the background
        let name_for_overview = name_check.clone();
        wasm_bindgen_futures::spawn_local(build_streaming_overview(
            state,
            file_index,
            name_for_overview,
        ));
    } else {
        // Small file: drain store and assemble full SpectrogramData.
        // Flow mode and harmonics analysis need full column data.
        let final_columns = spectral_store::drain_columns(file_index)
            .unwrap_or_default();

        let spectrogram = SpectrogramData {
            columns: Arc::new(final_columns),
            total_columns: total_cols,
            freq_resolution,
            time_resolution,
            max_freq,
            sample_rate: audio_for_stft.sample_rate,
        };

        log::info!(
            "Spectrogram: {} columns, freq_res={:.1} Hz, time_res={:.4}s",
            spectrogram.columns.len(),
            spectrogram.freq_resolution,
            spectrogram.time_resolution
        );

        // Compute higher-resolution overview image from the full spectrogram
        let overview_img = compute_overview_from_spectrogram(&spectrogram);

        state.files.update(|files| {
            if let Some(f) = files.get_mut(file_index) {
                if f.name == name_check {
                    f.spectrogram = spectrogram;
                    f.overview_image = overview_img;
                }
            }
        });
    }

    // Re-schedule all tiles with the final (accurate) max magnitude.
    // During progressive loading, early tiles may have used a provisional max;
    // if the final max differs significantly, re-render for consistent brightness.
    // For large files, tiles are computed from the spectral store on-demand.
    if !is_large {
        // Clear stale tiles rendered during progressive loading — they used
        // the spectral store's running max_magnitude at the time of rendering,
        // which grows as louder columns are discovered.  Without clearing,
        // schedule_all_tiles() skips already-cached tiles and they keep their
        // inconsistent normalization (visible as a stepped brightness gradient).
        tile_cache::clear_file(file_index);
        let file_for_tiles = state.files.get_untracked().get(file_index).cloned();
        if let Some(file) = file_for_tiles {
            tile_cache::schedule_all_tiles(state, file, file_index);
        }
    } else {
        // Large files: clear tile cache and re-render with final normalization.
        // During loading, the running max_magnitude grew as louder columns were found,
        // so early tiles used a lower max than late tiles — creating visible brightness
        // discontinuities.  Clearing forces re-rendering with the correct final max.
        // The colormapped preview base layer fills the gaps until new tiles arrive.
        tile_cache::clear_file(file_index);
        tile_cache::schedule_visible_tiles_from_store(state, file_index, total_cols);
    }

    // Signal the spectrogram canvas to repaint with the new data
    state.tile_ready_signal.update(|n| *n = n.wrapping_add(1));

    Ok(())
}

const DEMO_SOUNDS_BASE: &str =
    "https://raw.githubusercontent.com/pengowray/batmonic-demo-sounds/main";

pub(super) async fn fetch_bytes(url: &str) -> Result<Vec<u8>, String> {
    let window = web_sys::window().ok_or("No window")?;
    let resp_value = JsFuture::from(window.fetch_with_str(url))
        .await
        .map_err(|e| format!("fetch error: {e:?}"))?;
    let resp: web_sys::Response = resp_value
        .dyn_into()
        .map_err(|_| "Response cast failed".to_string())?;
    if !resp.ok() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let buf = JsFuture::from(resp.array_buffer().map_err(|e| format!("{e:?}"))?)
        .await
        .map_err(|e| format!("array_buffer: {e:?}"))?;
    let uint8 = js_sys::Uint8Array::new(&buf);
    Ok(uint8.to_vec())
}

async fn fetch_text(url: &str) -> Result<String, String> {
    let window = web_sys::window().ok_or("No window")?;
    let resp_value = JsFuture::from(window.fetch_with_str(url))
        .await
        .map_err(|e| format!("fetch error: {e:?}"))?;
    let resp: web_sys::Response = resp_value
        .dyn_into()
        .map_err(|_| "Response cast failed".to_string())?;
    if !resp.ok() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let text = JsFuture::from(resp.text().map_err(|e| format!("{e:?}"))?)
        .await
        .map_err(|e| format!("text: {e:?}"))?;
    text.as_string().ok_or("Not a string".to_string())
}

fn parse_xc_metadata(json: &serde_json::Value) -> Vec<(String, String)> {
    let mut fields = Vec::new();
    let s = |key: &str| json[key].as_str().unwrap_or("").to_string();

    let en = s("en");
    if !en.is_empty() {
        fields.push(("Species".into(), en));
    }
    let genus = s("gen");
    let sp = s("sp");
    if !genus.is_empty() && !sp.is_empty() {
        fields.push(("Scientific name".into(), format!("{} {}", genus, sp)));
    }
    for (key, label) in [
        ("rec", "Recordist"),
        ("lic", "License"),
        ("attribution", "Attribution"),
        ("cnt", "Country"),
        ("loc", "Location"),
    ] {
        let v = s(key);
        if !v.is_empty() {
            fields.push((label.into(), v));
        }
    }
    let lat = s("lat");
    let lon = s("lon");
    if !lat.is_empty() && !lon.is_empty() {
        fields.push(("Coordinates".into(), format!("{}, {}", lat, lon)));
    }
    for (key, label) in [
        ("date", "Date"),
        ("type", "Sound type"),
        ("q", "Quality"),
        ("url", "URL"),
    ] {
        let v = s(key);
        if !v.is_empty() {
            fields.push((label.into(), v));
        }
    }
    fields
}

#[derive(Clone, Debug)]
pub(crate) struct DemoEntry {
    pub filename: String,
    pub metadata_file: Option<String>,
}

pub(crate) async fn fetch_demo_index() -> Result<Vec<DemoEntry>, String> {
    let index_url = format!("{}/index.json", DEMO_SOUNDS_BASE);
    let index_text = fetch_text(&index_url).await?;
    let index: serde_json::Value =
        serde_json::from_str(&index_text).map_err(|e| format!("index parse: {e}"))?;

    let sounds = index["sounds"]
        .as_array()
        .ok_or("No sounds array in index")?;

    let entries = sounds
        .iter()
        .filter_map(|sound| {
            let filename = sound["filename"].as_str()?.to_string();
            let metadata_file = sound["metadata"].as_str().map(|s| s.to_string());
            Some(DemoEntry { filename, metadata_file })
        })
        .collect();

    Ok(entries)
}

pub(crate) async fn load_single_demo(entry: &DemoEntry, state: AppState, load_id: u64) -> Result<(), String> {
    // Fetch XC metadata sidecar if available
    let xc_metadata = if let Some(meta_file) = &entry.metadata_file {
        let encoded = js_sys::encode_uri_component(meta_file);
        let meta_url = format!(
            "{}/sounds/{}",
            DEMO_SOUNDS_BASE,
            encoded.as_string().unwrap_or_default()
        );
        match fetch_text(&meta_url).await {
            Ok(text) => {
                match serde_json::from_str::<serde_json::Value>(&text) {
                    Ok(json) => Some(parse_xc_metadata(&json)),
                    Err(e) => {
                        log::warn!("Failed to parse XC metadata for {}: {}", entry.filename, e);
                        None
                    }
                }
            }
            Err(e) => {
                log::warn!("Failed to fetch XC metadata for {}: {}", entry.filename, e);
                None
            }
        }
    } else {
        None
    };

    let encoded = js_sys::encode_uri_component(&entry.filename);
    let audio_url = format!(
        "{}/sounds/{}",
        DEMO_SOUNDS_BASE,
        encoded.as_string().unwrap_or_default()
    );
    log::info!("Fetching demo: {}", entry.filename);
    let bytes = fetch_bytes(&audio_url).await?;
    load_named_bytes(entry.filename.clone(), &bytes, xc_metadata, state, load_id).await
}

async fn read_file_bytes(file: &File) -> Result<Vec<u8>, String> {
    let reader = FileReader::new().map_err(|e| format!("FileReader: {e:?}"))?;
    let reader_clone = reader.clone();

    let promise = js_sys::Promise::new(&mut |resolve, reject| {
        let resolve_clone = resolve.clone();
        let reject_clone = reject.clone();

        let onload = Closure::once(move |_: web_sys::Event| {
            resolve_clone.call0(&JsValue::NULL).unwrap();
        });
        let onerror = Closure::once(move |_: web_sys::Event| {
            reject_clone.call0(&JsValue::NULL).unwrap();
        });

        reader_clone.set_onloadend(Some(onload.as_ref().unchecked_ref()));
        reader_clone.set_onerror(Some(onerror.as_ref().unchecked_ref()));

        onload.forget();
        onerror.forget();
    });

    reader
        .read_as_array_buffer(file)
        .map_err(|e| format!("read_as_array_buffer: {e:?}"))?;

    JsFuture::from(promise)
        .await
        .map_err(|e| format!("FileReader await: {e:?}"))?;

    let result = reader.result().map_err(|e| format!("result: {e:?}"))?;
    let array_buffer = result
        .dyn_into::<js_sys::ArrayBuffer>()
        .map_err(|_| "Expected ArrayBuffer".to_string())?;
    let uint8_array = js_sys::Uint8Array::new(&array_buffer);
    Ok(uint8_array.to_vec())
}

/// Generate chunk indices in expanding-ring order from a center chunk.
/// Returns indices: center, center-1, center+1, center-2, center+2, ...
fn expanding_chunk_order(center: usize, total: usize) -> Vec<usize> {
    let mut order = Vec::with_capacity(total);
    if total == 0 {
        return order;
    }
    let center = center.min(total - 1);
    order.push(center);
    let mut dist = 1usize;
    while order.len() < total {
        let left = center.checked_sub(dist);
        let right = center + dist;
        if let Some(l) = left {
            if l < total {
                order.push(l);
            }
        }
        if right < total {
            order.push(right);
        }
        // If both are out of bounds, we're done
        if left.is_none() && right >= total {
            break;
        }
        dist += 1;
    }
    order
}
