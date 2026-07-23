//! MP4 video export: render spectrogram frames + DSP-processed audio into an MP4 file
//! using the WebCodecs API and mp4-muxer JS library.

use crate::state::store_fields::*;
use std::cell::RefCell;
use std::rc::Rc;

use leptos::prelude::*;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement};

use crate::audio::export::{
    build_export_params, get_selected_regions, process_region, save_export_bytes,
};
use crate::audio::webcodecs_bindings as wc;
use crate::canvas::spectrogram_renderer::{self, ColormapMode, SpectDisplaySettings, TileSource};
use crate::state::{AppState, AudioCodecOption, PlaybackMode, VideoCodec, VideoViewMode};
use crate::web_util::{sleep_ms, yield_now};

/// Frames per second for exported video.
const FPS: f64 = 30.0;

/// Default video bitrate (bits per second).
const DEFAULT_VIDEO_BITRATE: u32 = 2_000_000;

/// Default audio bitrate (bits per second).
const DEFAULT_AUDIO_BITRATE: u32 = 128_000;

/// Keyframe interval in frames.
const KEYFRAME_INTERVAL: u32 = 60;

/// Snapshot of all rendering parameters captured at export start.
struct RenderParams {
    file_idx: usize,
    total_cols: usize,
    time_res: f64,
    duration: f64,
    start_time: f64,
    end_time: f64,
    file_max_freq: f64,
    freq_crop_lo: f64,
    freq_crop_hi: f64,
    colormap: ColormapMode,
    display_settings: SpectDisplaySettings,
    min_freq: f64,
    max_freq: f64,
    canvas_w: u32,
    canvas_h: u32,
    shield_style: crate::state::ShieldStyle,
}

/// Check if video export is available in this browser.
pub fn is_available() -> bool {
    wc::has_video_encoder() && wc::has_mp4_muxer()
}

/// Launch the async video export. Call from a button click handler.
pub fn start_export(state: &AppState) {
    let state = *state;
    leptos::task::spawn_local(async move {
        state.export.video_cancel().set(false);
        state.export.video_progress().set(Some(0.0));
        state
            .export
            .video_status()
            .set(Some("Preparing...".to_string()));

        match export_video_impl(&state).await {
            Ok(()) => {
                state.export.video_progress().set(None);
                state.export.video_status().set(None);
                if state.export.video_cancel().get_untracked() {
                    log::info!("Video export cancelled");
                } else {
                    log::info!("Video export complete");
                }
            }
            Err(e) => {
                let msg = format!("{:?}", e);
                log::error!("Video export failed: {msg}");
                state.export.video_progress().set(None);
                state
                    .export
                    .video_status()
                    .set(Some(format!("Export failed: {msg}")));
                // Clear error after 10 seconds
                let state2 = state;
                leptos::task::spawn_local(async move {
                    sleep_ms(10_000).await;
                    if state2
                        .export
                        .video_status()
                        .get_untracked()
                        .as_ref()
                        .map(|s| s.starts_with("Export failed"))
                        .unwrap_or(false)
                    {
                        state2.export.video_status().set(None);
                    }
                });
            }
        }
    });
}

async fn export_video_impl(state: &AppState) -> Result<(), JsValue> {
    log::info!("Video export: starting...");

    // Gather file info
    let file = state
        .current_file()
        .ok_or_else(|| JsValue::from_str("No file loaded"))?;
    let file_idx = state.library.current_index().get_untracked().unwrap();
    let sample_rate = file.audio.sample_rate;
    let source = &file.audio.source;

    // Determine time range (same logic as WAV export)
    let regions = get_selected_regions(state);
    let use_region_focus = state.annotations.export_use_region_focus().get_untracked();

    let (start_time, end_time) = if !regions.is_empty() {
        let r = &regions[0].1;
        (r.time_start, r.time_end)
    } else if let Some(sel) = state.interaction.selection().get_untracked() {
        (sel.time_start, sel.time_end)
    } else {
        (0.0, file.audio.source.duration_secs())
    };

    log::info!("Video export: time range {start_time:.2}s - {end_time:.2}s");

    if end_time <= start_time {
        return Err(JsValue::from_str("Invalid time range"));
    }

    // Video resolution
    let resolution = state.export.video_resolution().get_untracked();
    let canvas_w_hint = state
        .viewmode
        .spectrogram_canvas_width()
        .get_untracked()
        .max(320.0) as u32;
    let canvas_h_hint = 400u32; // reasonable default for spectrogram height
    let (vid_w, vid_h) = resolution.dimensions(canvas_w_hint, canvas_h_hint);
    // Ensure even dimensions (required by most codecs)
    let vid_w = ((vid_w + 1) & !1).max(2);
    let vid_h = ((vid_h + 1) & !1).max(2);

    // Codec
    let codec_str = match state.export.video_codec().get_untracked() {
        VideoCodec::H264 => wc::H264_CODEC,
        VideoCodec::Av1 => wc::AV1_CODEC,
    };

    log::info!("Video export: {vid_w}x{vid_h} codec={codec_str}");
    log::info!(
        "Video export: has_video_encoder={}, has_audio_encoder={}, has_mp4_muxer={}",
        wc::has_video_encoder(),
        wc::has_audio_encoder(),
        wc::has_mp4_muxer()
    );

    // Check codec support
    if !wc::is_video_config_supported(codec_str, vid_w, vid_h).await {
        return Err(JsValue::from_str(&format!(
            "Video codec {} not supported at {}x{}",
            codec_str, vid_w, vid_h
        )));
    }
    log::info!("Video export: codec supported");

    // Snapshot rendering parameters from current state
    let time_res = file.spectrogram.time_resolution;
    let file_max_freq = file.spectrogram.max_freq;
    let max_display_freq = state.view.max_display_freq().get_untracked();
    let min_display_freq = state.view.min_display_freq().get_untracked();
    let max_freq = max_display_freq.unwrap_or(file_max_freq).min(file_max_freq);
    let min_freq = min_display_freq.unwrap_or(0.0);
    let freq_crop_lo = min_freq / file_max_freq;
    let freq_crop_hi = (max_freq / file_max_freq).min(1.0);

    let hfr_enabled = state.viewmode.hfr_enabled().get_untracked();
    let colormap_pref = state.spect.colormap_preference().get_untracked();
    let hfr_colormap_pref = state.spect.hfr_colormap_preference().get_untracked();
    let band_ff_lo = state.filter.band_ff_freq_lo().get_untracked();
    let band_ff_hi = state.filter.band_ff_freq_hi().get_untracked();

    let colormap = if hfr_enabled && band_ff_hi > band_ff_lo {
        ColormapMode::HfrFocus {
            colormap: hfr_colormap_pref,
            band_ff_lo_frac: band_ff_lo / file_max_freq,
            band_ff_hi_frac: band_ff_hi / file_max_freq,
        }
    } else if hfr_enabled {
        ColormapMode::Uniform(hfr_colormap_pref)
    } else {
        ColormapMode::Uniform(colormap_pref)
    };

    let spect_floor = state.spect.floor_db().get_untracked();
    let spect_range = state.spect.range_db().get_untracked();
    let spect_gamma = state.spect.gamma().get_untracked();
    let spect_gain = state.spect.gain_db().get_untracked();

    // Compute ref_db the same way the spectrogram component does
    let fft_size = state.spect.fft_mode().get_untracked().max_fft_size() as f32;
    let fixed_ref_db = 20.0 * (fft_size / 4.0).log10();
    let display_auto_gain = state.display.auto_gain().get_untracked();
    let total_cols = {
        let tc = file.spectrogram.total_columns;
        if tc > 0 {
            tc
        } else {
            file.spectrogram.columns.len()
        }
    };
    let ref_db = if display_auto_gain && total_cols > 0 {
        let max_mag = crate::canvas::spectral_store::get_max_magnitude(file_idx);
        if max_mag > 0.0 {
            20.0 * max_mag.log10()
        } else {
            fixed_ref_db
        }
    } else {
        fixed_ref_db
    };
    let display_boost = state.display.gain_boost().get_untracked();

    let display_settings = SpectDisplaySettings {
        floor_db: spect_floor,
        range_db: spect_range,
        gamma: spect_gamma,
        gain_db: spect_gain - ref_db + display_boost,
    };

    let render = RenderParams {
        file_idx,
        total_cols,
        time_res,
        duration: file.audio.source.duration_secs(),
        start_time,
        end_time,
        file_max_freq,
        freq_crop_lo,
        freq_crop_hi,
        colormap,
        display_settings,
        min_freq,
        max_freq,
        canvas_w: vid_w,
        canvas_h: vid_h,
        shield_style: state.viewmode.shield_style().get_untracked(),
    };

    // ── Process audio ────────────────────────────────────────────────────────
    state
        .export
        .video_status()
        .set(Some("Processing audio...".to_string()));
    yield_now().await;

    let region = if !regions.is_empty() {
        Some(&regions[0].1)
    } else {
        None
    };
    let audio_params = build_export_params(state, region, use_region_focus, sample_rate);
    let audio_samples = process_region(
        source.as_ref(),
        sample_rate,
        start_time,
        end_time,
        &audio_params,
    );

    // Output sample rate (TE mode changes it)
    let output_rate = match audio_params.mode {
        PlaybackMode::TimeExpansion => {
            let te = audio_params.te_factor;
            (sample_rate as f64 / te) as u32
        }
        _ => sample_rate,
    };

    // Resample to a standard rate if needed (many players struggle with high rates)
    let (final_samples, final_rate) = normalize_audio_rate(&audio_samples, output_rate);
    let audio_duration = final_samples.len() as f64 / final_rate as f64;

    // ── Create offscreen canvas ──────────────────────────────────────────────
    let document = web_sys::window().unwrap().document().unwrap();
    let canvas: HtmlCanvasElement = document.create_element("canvas")?.dyn_into()?;
    canvas.set_width(vid_w);
    canvas.set_height(vid_h);
    let ctx: CanvasRenderingContext2d = canvas.get_context("2d")?.unwrap().dyn_into()?;

    // ── Set up encoders and muxer ────────────────────────────────────────────
    state
        .export
        .video_status()
        .set(Some("Setting up encoders...".to_string()));
    yield_now().await;

    // Resolve audio codec: (webcodecs_codec_str, muxer_codec_str) or None
    let audio_codec_choice = state.export.video_audio_codec().get_untracked();
    let resolved_audio: Option<(&str, &str)> = match audio_codec_choice {
        AudioCodecOption::NoAudio => {
            log::info!("Video export: audio disabled by user");
            None
        }
        AudioCodecOption::Aac => {
            if !wc::has_audio_encoder() {
                return Err(JsValue::from_str(
                    "AudioEncoder API not available in this browser",
                ));
            }
            log::info!("Video export: checking AAC support at {}Hz...", final_rate);
            if !wc::is_audio_config_supported(wc::AAC_WEBCODECS_CODEC, final_rate, 1).await {
                return Err(JsValue::from_str(&format!(
                    "AAC audio not supported at {}Hz in this browser",
                    final_rate
                )));
            }
            Some((wc::AAC_WEBCODECS_CODEC, wc::AAC_MUXER_CODEC))
        }
        AudioCodecOption::Opus => {
            if !wc::has_audio_encoder() {
                return Err(JsValue::from_str(
                    "AudioEncoder API not available in this browser",
                ));
            }
            log::info!("Video export: checking Opus support at {}Hz...", final_rate);
            if !wc::is_audio_config_supported(wc::OPUS_WEBCODECS_CODEC, final_rate, 1).await {
                return Err(JsValue::from_str(&format!(
                    "Opus audio not supported at {}Hz in this browser",
                    final_rate
                )));
            }
            Some((wc::OPUS_WEBCODECS_CODEC, wc::OPUS_MUXER_CODEC))
        }
        AudioCodecOption::Auto => {
            if !wc::has_audio_encoder() {
                return Err(JsValue::from_str(
                    "AudioEncoder API not available. Select 'No audio' to export video without audio."
                ));
            }
            // Try AAC first, then Opus
            log::info!(
                "Video export: auto-detecting audio codec at {}Hz...",
                final_rate
            );
            if wc::is_audio_config_supported(wc::AAC_WEBCODECS_CODEC, final_rate, 1).await {
                log::info!("Video export: using AAC");
                Some((wc::AAC_WEBCODECS_CODEC, wc::AAC_MUXER_CODEC))
            } else if wc::is_audio_config_supported(wc::OPUS_WEBCODECS_CODEC, final_rate, 1).await {
                log::info!("Video export: AAC not supported, using Opus");
                Some((wc::OPUS_WEBCODECS_CODEC, wc::OPUS_MUXER_CODEC))
            } else {
                return Err(JsValue::from_str(
                    "No supported audio codec found (tried AAC and Opus). Select 'No audio' to export video without audio."
                ));
            }
        }
    };
    log::info!("Video export: creating muxer target...");
    let target = wc::create_array_buffer_target()?;
    log::info!(
        "Video export: creating muxer (audio={:?})...",
        resolved_audio.map(|a| a.1)
    );
    let muxer = wc::create_muxer(
        &target,
        codec_str,
        vid_w,
        vid_h,
        resolved_audio.map(|(_, muxer_codec)| (muxer_codec, final_rate)),
    )?;

    // Shared muxer reference for closures
    let muxer_rc = Rc::new(RefCell::new(muxer));
    let video_error: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));

    // Video encoder
    let muxer_v = muxer_rc.clone();
    let on_video_chunk = Closure::new(move |chunk: JsValue, meta: JsValue| {
        let m = muxer_v.borrow();
        let _ = wc::muxer_add_video_chunk(&m, &chunk, &meta);
    });
    let ve = video_error.clone();
    let on_video_error = Closure::new(move |err: JsValue| {
        let msg = format!("{:?}", err);
        log::error!("VideoEncoder error: {msg}");
        *ve.borrow_mut() = Some(msg);
    });
    log::info!("Video export: creating video encoder...");
    let video_encoder = wc::create_video_encoder(&on_video_chunk, &on_video_error)?;
    log::info!("Video export: configuring video encoder...");
    wc::configure_video_encoder(
        &video_encoder,
        codec_str,
        vid_w,
        vid_h,
        DEFAULT_VIDEO_BITRATE,
        FPS,
    )?;
    log::info!("Video export: video encoder configured");

    // Audio encoder (optional — AAC support varies by browser/platform)
    let audio_encoder;
    let _on_audio_chunk;
    let _on_audio_error;

    if let Some((webcodecs_codec, _)) = resolved_audio {
        log::info!("Video export: creating audio encoder ({webcodecs_codec})...");
        let muxer_a = muxer_rc.clone();
        let cb = Closure::new(move |chunk: JsValue, meta: JsValue| {
            let m = muxer_a.borrow();
            let _ = wc::muxer_add_audio_chunk(&m, &chunk, &meta);
        });
        let eb = Closure::new(move |err: JsValue| {
            let msg = format!("{:?}", err);
            log::error!("AudioEncoder error: {msg}");
        });
        let enc = wc::create_audio_encoder(&cb, &eb)?;
        wc::configure_audio_encoder(&enc, webcodecs_codec, final_rate, 1, DEFAULT_AUDIO_BITRATE)?;
        audio_encoder = Some(enc);
        _on_audio_chunk = Some(cb);
        _on_audio_error = Some(eb);
    } else {
        audio_encoder = None;
        _on_audio_chunk = None;
        _on_audio_error = None;
    }

    // ── Encode audio ─────────────────────────────────────────────────────────
    if let Some(ref enc) = audio_encoder {
        state
            .export
            .video_status()
            .set(Some("Encoding audio...".to_string()));
        yield_now().await;

        log::info!(
            "Video export: encoding {} audio samples at {}Hz...",
            final_samples.len(),
            final_rate
        );

        // Feed audio in chunks of 1024 samples (AAC frame size)
        let chunk_size = 1024usize;
        let mut offset = 0usize;
        while offset < final_samples.len() {
            let end = (offset + chunk_size).min(final_samples.len());
            let chunk = &final_samples[offset..end];
            let timestamp_us = (offset as f64 / final_rate as f64 * 1_000_000.0) as i64;
            let audio_data = wc::create_audio_data(chunk, final_rate, timestamp_us)?;

            let encode_fn = js_sys::Reflect::get(enc, &"encode".into())?;
            let encode_fn: js_sys::Function = encode_fn.dyn_into()?;
            encode_fn.call1(enc, &audio_data)?;

            wc::close_audio_data(&audio_data)?;
            offset = end;
        }

        log::info!("Video export: flushing audio encoder...");
        wc::flush_encoder(enc).await?;
        log::info!("Video export: audio encoding complete");
    } else {
        log::info!("Video export: skipping audio (no AudioEncoder available)");
    }

    // ── Encode video frames ──────────────────────────────────────────────────
    let total_frames = (audio_duration * FPS).ceil() as u32;
    log::info!(
        "Video export: encoding {total_frames} video frames ({audio_duration:.2}s at {FPS}fps)..."
    );
    state
        .export
        .video_status()
        .set(Some("Encoding video...".to_string()));

    let view_mode = state.export.video_view_mode().get_untracked();
    let export_duration = render.end_time - render.start_time;

    // In time expansion mode, the audio is longer than the original time range.
    // Map video (wall-clock) time back to spectrogram (original) time.
    let playback_speed = if audio_duration > 0.0 {
        export_duration / audio_duration
    } else {
        1.0
    };

    match view_mode {
        VideoViewMode::StaticPlayhead => {
            // Zoom to fit the entire export range in the viewport
            let visible_time = export_duration;
            let zoom = vid_w as f64 / (visible_time / render.time_res);
            let scroll = render.start_time;
            let scroll_col = scroll / render.time_res;

            // Render background spectrogram once onto a separate canvas
            let document = web_sys::window().unwrap().document().unwrap();
            let bg_canvas: HtmlCanvasElement = document.create_element("canvas")?.dyn_into()?;
            bg_canvas.set_width(vid_w);
            bg_canvas.set_height(vid_h);
            let bg_ctx: CanvasRenderingContext2d =
                bg_canvas.get_context("2d")?.unwrap().dyn_into()?;
            render_frame(&bg_ctx, &render, scroll_col, zoom, visible_time, scroll);

            log::info!("Video export: static playhead mode, {total_frames} frames");

            for frame_idx in 0..total_frames {
                if state.export.video_cancel().get_untracked() {
                    break;
                }
                if let Some(ref e) = *video_error.borrow() {
                    return Err(JsValue::from_str(e));
                }

                // Blit cached background
                ctx.draw_image_with_html_canvas_element(&bg_canvas, 0.0, 0.0)
                    .map_err(|e| JsValue::from_str(&format!("draw_image failed: {:?}", e)))?;

                // Draw playhead line
                let t = frame_idx as f64 / FPS;
                let playhead_x = (t * playback_speed / export_duration) * vid_w as f64;
                ctx.set_stroke_style_str("#ffffff");
                ctx.set_line_width(2.0);
                ctx.begin_path();
                ctx.move_to(playhead_x, 0.0);
                ctx.line_to(playhead_x, vid_h as f64);
                ctx.stroke();

                // Encode frame (every frame is nearly identical → P-frames compress well)
                let timestamp_us = (t * 1_000_000.0) as i64;
                let frame = wc::create_video_frame(&canvas, timestamp_us)?;
                let is_key = frame_idx % KEYFRAME_INTERVAL == 0;
                wc::encode_video_frame(&video_encoder, &frame, is_key)?;
                wc::close_video_frame(&frame)?;

                let progress = (frame_idx + 1) as f64 / total_frames as f64;
                state.export.video_progress().set(Some(progress));
                if frame_idx % 5 == 0 {
                    state.export.video_status().set(Some(format!(
                        "Encoding video... {}%",
                        (progress * 100.0) as u32
                    )));
                    yield_now().await;
                }
            }
        }
        VideoViewMode::ScrollingView => {
            // Compute zoom proportional to current app zoom
            let current_zoom = state.view.zoom_level().get_untracked();
            let app_canvas_w = state.viewmode.spectrogram_canvas_width().get_untracked();
            let zoom = if app_canvas_w > 0.0 {
                current_zoom * (vid_w as f64 / app_canvas_w)
            } else {
                current_zoom
            };
            let visible_time = (vid_w as f64 / zoom) * render.time_res;
            // More frequent keyframes for scrolling (every pixel changes)
            let scrolling_keyframe_interval = 15u32;

            for frame_idx in 0..total_frames {
                if state.export.video_cancel().get_untracked() {
                    break;
                }
                if let Some(ref e) = *video_error.borrow() {
                    return Err(JsValue::from_str(e));
                }

                let t = frame_idx as f64 / FPS;
                let spect_t = t * playback_speed; // position in original timeline
                let scroll = (render.start_time + spect_t - visible_time * 0.25)
                    .max(0.0)
                    .min((render.duration - visible_time).max(0.0));
                let scroll_col = scroll / render.time_res;

                render_frame(&ctx, &render, scroll_col, zoom, visible_time, scroll);

                // Draw playhead line
                let playhead_x =
                    ((render.start_time + spect_t - scroll) / visible_time) * vid_w as f64;
                if playhead_x >= 0.0 && playhead_x <= vid_w as f64 {
                    ctx.set_stroke_style_str("#ffffff");
                    ctx.set_line_width(2.0);
                    ctx.begin_path();
                    ctx.move_to(playhead_x, 0.0);
                    ctx.line_to(playhead_x, vid_h as f64);
                    ctx.stroke();
                }

                let timestamp_us = (t * 1_000_000.0) as i64;
                let frame = wc::create_video_frame(&canvas, timestamp_us)?;
                let is_key = frame_idx % scrolling_keyframe_interval == 0;
                wc::encode_video_frame(&video_encoder, &frame, is_key)?;
                wc::close_video_frame(&frame)?;

                let progress = (frame_idx + 1) as f64 / total_frames as f64;
                state.export.video_progress().set(Some(progress));
                if frame_idx % 5 == 0 {
                    state.export.video_status().set(Some(format!(
                        "Encoding video... {}%",
                        (progress * 100.0) as u32
                    )));
                    yield_now().await;
                }
            }
        }
    }

    // If cancelled, close encoders and return early (skip finalize/download)
    if state.export.video_cancel().get_untracked() {
        let _ = wc::close_encoder(&video_encoder);
        if let Some(ref enc) = audio_encoder {
            let _ = wc::close_encoder(enc);
        }
        return Ok(());
    }

    // Flush video encoder
    state
        .export
        .video_status()
        .set(Some("Finalizing...".to_string()));
    yield_now().await;
    wc::flush_encoder(&video_encoder).await?;
    wc::close_encoder(&video_encoder)?;
    if let Some(ref enc) = audio_encoder {
        wc::close_encoder(enc)?;
    }

    // Finalize muxer and download
    log::info!("Video export: finalizing muxer...");
    let muxer = muxer_rc.borrow();
    let mp4_bytes = wc::muxer_finalize(&muxer, &target)?;
    log::info!("Video export: MP4 size = {} bytes", mp4_bytes.len());

    // Build filename
    let file = state.current_file().unwrap();
    let base_name = file
        .name
        .trim_end_matches(".wav")
        .trim_end_matches(".WAV")
        .trim_end_matches(".w4v")
        .trim_end_matches(".W4V")
        .trim_end_matches(".flac")
        .trim_end_matches(".FLAC")
        .trim_end_matches(".ogg")
        .trim_end_matches(".OGG")
        .trim_end_matches(".mp3")
        .trim_end_matches(".MP3")
        .trim_end_matches(".m4a")
        .trim_end_matches(".M4A")
        .trim_end_matches(".m4b")
        .trim_end_matches(".M4B");
    let filename = format!("{base_name}.mp4");

    save_export_bytes(state, mp4_bytes, filename, true);
    Ok(())
}

/// Render a single spectrogram frame to the offscreen canvas.
fn render_frame(
    ctx: &CanvasRenderingContext2d,
    r: &RenderParams,
    scroll_col: f64,
    zoom: f64,
    visible_time: f64,
    scroll_offset: f64,
) {
    // Clear canvas
    ctx.set_fill_style_str("#000");
    ctx.fill_rect(0.0, 0.0, r.canvas_w as f64, r.canvas_h as f64);

    // Blit spectrogram tiles
    spectrogram_renderer::blit_tiles_viewport(
        ctx,
        r.canvas_w as f64,
        r.canvas_h as f64,
        r.file_idx,
        r.total_cols,
        scroll_col,
        zoom,
        r.freq_crop_lo,
        r.freq_crop_hi,
        spectrogram_renderer::TileRenderMode::Spectrogram(r.colormap),
        &r.display_settings,
        None, // freq_adjustments — skip for simplicity in video export
        None, // preview fallback — not needed, tiles should be cached
        scroll_offset,
        visible_time,
        r.duration,
        TileSource::Normal,
    );

    // Draw time markers
    crate::canvas::time_markers::draw_time_markers(
        ctx,
        scroll_offset,
        visible_time,
        r.canvas_w as f64,
        r.canvas_h as f64,
        r.duration,
        None,  // no clock time config
        false, // don't show clock time
        1.0,   // time_scale = 1.0 (normal)
    );

    // Draw frequency markers
    use crate::canvas::overlays::{FreqMarkerState, FreqShiftMode};
    let ms = FreqMarkerState {
        mouse_freq: None,
        mouse_in_label_area: false,
        label_hover_opacity: 1.0,
        file_max_freq: r.file_max_freq,
        axis_drag_lo: None,
        axis_drag_hi: None,
        band_ff_drag_active: false,
        band_ff_lo: r.min_freq,
        band_ff_hi: r.max_freq,
        band_ff_handles_active: false,
        shield_style: r.shield_style,
    };
    crate::canvas::overlays::draw_freq_markers(
        ctx,
        r.min_freq,
        r.max_freq,
        r.canvas_h as f64,
        r.canvas_w as f64,
        FreqShiftMode::None,
        &ms,
        0.0,   // het_cutoff (not relevant for video)
        false, // labels on left
    );
}

/// Resample audio to a standard rate if the output rate is above 48 kHz
/// (high sample rates often cause playback issues in video players).
fn normalize_audio_rate(samples: &[f32], rate: u32) -> (Vec<f32>, u32) {
    if rate <= 48000 {
        return (samples.to_vec(), rate);
    }

    // Simple linear interpolation downsampling to 48 kHz
    let target_rate = 48000u32;
    let ratio = rate as f64 / target_rate as f64;
    let new_len = (samples.len() as f64 / ratio).ceil() as usize;
    let mut out = Vec::with_capacity(new_len);

    for i in 0..new_len {
        let src_pos = i as f64 * ratio;
        let idx = src_pos as usize;
        let frac = (src_pos - idx as f64) as f32;
        let s0 = samples.get(idx).copied().unwrap_or(0.0);
        let s1 = samples.get(idx + 1).copied().unwrap_or(s0);
        out.push(s0 + frac * (s1 - s0));
    }

    (out, target_rate)
}
