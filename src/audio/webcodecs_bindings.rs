//! Thin wrappers around the WebCodecs API and mp4-muxer JS library.
//!
//! WebCodecs types are not yet in web-sys, so we use `js_sys::Reflect` / `JsValue`
//! for all interactions.  The mp4-muxer IIFE bundle exposes a global `Mp4Muxer`
//! namespace (loaded via `<script src="mp4-muxer.js">`).

use js_sys::{self, Float32Array, Object, Reflect, Uint8Array};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::HtmlCanvasElement;

// ── Feature detection ────────────────────────────────────────────────────────

/// Returns true if the browser has the WebCodecs `VideoEncoder` API.
pub fn has_video_encoder() -> bool {
    js_sys::eval("typeof VideoEncoder !== 'undefined'")
        .map(|v| v.as_bool().unwrap_or(false))
        .unwrap_or(false)
}

/// Returns true if the browser has the WebCodecs `AudioEncoder` API.
pub fn has_audio_encoder() -> bool {
    js_sys::eval("typeof AudioEncoder !== 'undefined'")
        .map(|v| v.as_bool().unwrap_or(false))
        .unwrap_or(false)
}

/// Check whether a video codec configuration is supported.
pub async fn is_video_config_supported(codec: &str, width: u32, height: u32) -> bool {
    let code = format!(
        r#"(async () => {{
            if (typeof VideoEncoder === 'undefined') return false;
            try {{
                const support = await VideoEncoder.isConfigSupported({{
                    codec: "{codec}",
                    width: {width},
                    height: {height},
                    bitrate: 1000000,
                }});
                return !!support.supported;
            }} catch(e) {{ console.warn('isConfigSupported error:', e); return false; }}
        }})()"#,
    );
    match js_sys::eval(&code) {
        Ok(promise) => {
            let promise: js_sys::Promise = match promise.dyn_into() {
                Ok(p) => p,
                Err(_) => return false,
            };
            match wasm_bindgen_futures::JsFuture::from(promise).await {
                Ok(val) => val.as_bool().unwrap_or(false),
                Err(_) => false,
            }
        }
        Err(_) => false,
    }
}

// ── Video codec strings ──────────────────────────────────────────────────────

/// H.264 Baseline Level 3.1 — maximum compatibility (Discord, most players).
pub const H264_CODEC: &str = "avc1.42001f";

/// H.264 Main Level 4.0 — better quality at same bitrate, still widely supported.
pub const H264_MAIN_CODEC: &str = "avc1.4d0028";

/// AV1 Main Profile, Level 4.0.
pub const AV1_CODEC: &str = "av01.0.08M.08";

// ── VideoEncoder wrapper ─────────────────────────────────────────────────────

/// Create a `VideoEncoder` and return its JsValue handle.
///
/// `on_chunk(chunk, metadata)` is called for each encoded chunk.
/// `on_error(error)` is called on encoder error.
pub fn create_video_encoder(
    on_chunk: &Closure<dyn FnMut(JsValue, JsValue)>,
    on_error: &Closure<dyn FnMut(JsValue)>,
) -> Result<JsValue, JsValue> {
    let init = Object::new();
    Reflect::set(&init, &"output".into(), on_chunk.as_ref())?;
    Reflect::set(&init, &"error".into(), on_error.as_ref())?;

    let ctor = Reflect::get(&js_sys::global(), &"VideoEncoder".into())?;
    let ctor: js_sys::Function = ctor.dyn_into()?;
    js_sys::Reflect::construct(&ctor, &js_sys::Array::of1(&init))
}

/// Configure the video encoder.
pub fn configure_video_encoder(
    encoder: &JsValue,
    codec: &str,
    width: u32,
    height: u32,
    bitrate: u32,
    framerate: f64,
) -> Result<(), JsValue> {
    let config = Object::new();
    Reflect::set(&config, &"codec".into(), &codec.into())?;
    Reflect::set(&config, &"width".into(), &width.into())?;
    Reflect::set(&config, &"height".into(), &height.into())?;
    Reflect::set(&config, &"bitrate".into(), &bitrate.into())?;
    Reflect::set(&config, &"framerate".into(), &framerate.into())?;

    let configure = Reflect::get(encoder, &"configure".into())?;
    let configure: js_sys::Function = configure.dyn_into()?;
    configure.call1(encoder, &config)?;
    Ok(())
}

/// Create a `VideoFrame` from an `HtmlCanvasElement`.
pub fn create_video_frame(
    canvas: &HtmlCanvasElement,
    timestamp_us: i64,
) -> Result<JsValue, JsValue> {
    let opts = Object::new();
    Reflect::set(
        &opts,
        &"timestamp".into(),
        &JsValue::from_f64(timestamp_us as f64),
    )?;

    let ctor = Reflect::get(&js_sys::global(), &"VideoFrame".into())?;
    let ctor: js_sys::Function = ctor.dyn_into()?;
    js_sys::Reflect::construct(&ctor, &js_sys::Array::of2(&canvas.into(), &opts))
}

/// Encode a `VideoFrame` with the given encoder. `key_frame` forces a keyframe.
pub fn encode_video_frame(
    encoder: &JsValue,
    frame: &JsValue,
    key_frame: bool,
) -> Result<(), JsValue> {
    let opts = Object::new();
    Reflect::set(&opts, &"keyFrame".into(), &key_frame.into())?;
    let encode_fn = Reflect::get(encoder, &"encode".into())?;
    let encode_fn: js_sys::Function = encode_fn.dyn_into()?;
    encode_fn.call2(encoder, frame, &opts)?;
    Ok(())
}

/// Close a `VideoFrame` to free GPU resources.
pub fn close_video_frame(frame: &JsValue) -> Result<(), JsValue> {
    let close_fn = Reflect::get(frame, &"close".into())?;
    let close_fn: js_sys::Function = close_fn.dyn_into()?;
    close_fn.call0(frame)?;
    Ok(())
}

/// Flush the encoder and wait for all pending output.
pub async fn flush_encoder(encoder: &JsValue) -> Result<(), JsValue> {
    let flush_fn = Reflect::get(encoder, &"flush".into())?;
    let flush_fn: js_sys::Function = flush_fn.dyn_into()?;
    let promise: js_sys::Promise = flush_fn.call0(encoder)?.dyn_into()?;
    wasm_bindgen_futures::JsFuture::from(promise).await?;
    Ok(())
}

/// Close the encoder.
pub fn close_encoder(encoder: &JsValue) -> Result<(), JsValue> {
    let close_fn = Reflect::get(encoder, &"close".into())?;
    let close_fn: js_sys::Function = close_fn.dyn_into()?;
    close_fn.call0(encoder)?;
    Ok(())
}

// ── AudioEncoder wrapper ─────────────────────────────────────────────────────

/// Create an `AudioEncoder`.
pub fn create_audio_encoder(
    on_chunk: &Closure<dyn FnMut(JsValue, JsValue)>,
    on_error: &Closure<dyn FnMut(JsValue)>,
) -> Result<JsValue, JsValue> {
    let init = Object::new();
    Reflect::set(&init, &"output".into(), on_chunk.as_ref())?;
    Reflect::set(&init, &"error".into(), on_error.as_ref())?;

    let ctor = Reflect::get(&js_sys::global(), &"AudioEncoder".into())?;
    let ctor: js_sys::Function = ctor.dyn_into()?;
    js_sys::Reflect::construct(&ctor, &js_sys::Array::of1(&init))
}

/// WebCodecs codec string for AAC-LC.
pub const AAC_WEBCODECS_CODEC: &str = "mp4a.40.2";
/// mp4-muxer short name for AAC.
pub const AAC_MUXER_CODEC: &str = "aac";
/// WebCodecs codec string for Opus.
pub const OPUS_WEBCODECS_CODEC: &str = "opus";
/// mp4-muxer short name for Opus.
pub const OPUS_MUXER_CODEC: &str = "opus";

/// Check whether an audio codec configuration is supported.
/// `codec` is the WebCodecs codec string (e.g. "mp4a.40.2" or "opus").
pub async fn is_audio_config_supported(codec: &str, sample_rate: u32, channels: u32) -> bool {
    let code = format!(
        r#"(async () => {{
            if (typeof AudioEncoder === 'undefined') return false;
            try {{
                const support = await AudioEncoder.isConfigSupported({{
                    codec: "{codec}",
                    sampleRate: {sample_rate},
                    numberOfChannels: {channels},
                    bitrate: 128000,
                }});
                return !!support.supported;
            }} catch(e) {{ console.warn('AudioEncoder.isConfigSupported error:', e); return false; }}
        }})()"#,
    );
    match js_sys::eval(&code) {
        Ok(promise) => {
            let promise: js_sys::Promise = match promise.dyn_into() {
                Ok(p) => p,
                Err(_) => return false,
            };
            match wasm_bindgen_futures::JsFuture::from(promise).await {
                Ok(val) => val.as_bool().unwrap_or(false),
                Err(_) => false,
            }
        }
        Err(_) => false,
    }
}

/// Configure the audio encoder.
/// `codec` is the WebCodecs codec string (e.g. "mp4a.40.2" or "opus").
pub fn configure_audio_encoder(
    encoder: &JsValue,
    codec: &str,
    sample_rate: u32,
    channels: u32,
    bitrate: u32,
) -> Result<(), JsValue> {
    let config = Object::new();
    Reflect::set(&config, &"codec".into(), &codec.into())?;
    Reflect::set(&config, &"sampleRate".into(), &sample_rate.into())?;
    Reflect::set(&config, &"numberOfChannels".into(), &channels.into())?;
    Reflect::set(&config, &"bitrate".into(), &bitrate.into())?;

    let configure = Reflect::get(encoder, &"configure".into())?;
    let configure: js_sys::Function = configure.dyn_into()?;
    configure.call1(encoder, &config)?;
    Ok(())
}

/// Create an `AudioData` object from f32 samples.
pub fn create_audio_data(
    samples: &[f32],
    sample_rate: u32,
    timestamp_us: i64,
) -> Result<JsValue, JsValue> {
    let data = Float32Array::new_with_length(samples.len() as u32);
    data.copy_from(samples);

    let opts = Object::new();
    Reflect::set(&opts, &"format".into(), &"f32-planar".into())?;
    Reflect::set(&opts, &"sampleRate".into(), &sample_rate.into())?;
    Reflect::set(&opts, &"numberOfChannels".into(), &1u32.into())?;
    Reflect::set(
        &opts,
        &"numberOfFrames".into(),
        &(samples.len() as u32).into(),
    )?;
    Reflect::set(
        &opts,
        &"timestamp".into(),
        &JsValue::from_f64(timestamp_us as f64),
    )?;
    Reflect::set(&opts, &"data".into(), &data)?;

    let ctor = Reflect::get(&js_sys::global(), &"AudioData".into())?;
    let ctor: js_sys::Function = ctor.dyn_into()?;
    js_sys::Reflect::construct(&ctor, &js_sys::Array::of1(&opts))
}

/// Close an `AudioData` to free resources.
pub fn close_audio_data(data: &JsValue) -> Result<(), JsValue> {
    let close_fn = Reflect::get(data, &"close".into())?;
    let close_fn: js_sys::Function = close_fn.dyn_into()?;
    close_fn.call0(data)?;
    Ok(())
}

// ── mp4-muxer wrapper ────────────────────────────────────────────────────────

/// Check if the mp4-muxer library is loaded.
pub fn has_mp4_muxer() -> bool {
    Reflect::get(&js_sys::global(), &"Mp4Muxer".into())
        .map(|v| !v.is_undefined() && !v.is_null())
        .unwrap_or(false)
}

/// Create an `ArrayBufferTarget` (the mp4-muxer output sink).
pub fn create_array_buffer_target() -> Result<JsValue, JsValue> {
    let ns = Reflect::get(&js_sys::global(), &"Mp4Muxer".into())?;
    let ctor = Reflect::get(&ns, &"ArrayBufferTarget".into())?;
    let ctor: js_sys::Function = ctor.dyn_into()?;
    js_sys::Reflect::construct(&ctor, &js_sys::Array::new())
}

/// Map a WebCodecs video codec string to the mp4-muxer short name.
/// mp4-muxer expects: "avc", "hevc", "vp9", "av1" (not the full WebCodecs codec strings).
pub fn muxer_video_codec(webcodecs_codec: &str) -> &'static str {
    if webcodecs_codec.starts_with("avc") {
        "avc"
    } else if webcodecs_codec.starts_with("av01") {
        "av1"
    } else if webcodecs_codec.starts_with("hvc") || webcodecs_codec.starts_with("hev") {
        "hevc"
    } else if webcodecs_codec.starts_with("vp09") || webcodecs_codec.starts_with("vp9") {
        "vp9"
    } else {
        "avc" // fallback
    }
}

/// Create a `Muxer` from the mp4-muxer library.
///
/// `target` is the `ArrayBufferTarget`.
/// `video_codec` should be a WebCodecs codec string (e.g. "avc1.42001f") — it will be
/// mapped to the mp4-muxer short name (e.g. "avc") automatically.
/// `audio` is optional: (muxer_codec, sample_rate) — use "aac" or "opus".
pub fn create_muxer(
    target: &JsValue,
    video_codec: &str,
    video_width: u32,
    video_height: u32,
    audio_codec: Option<(&str, u32)>, // (muxer_codec like "aac", sample_rate)
) -> Result<JsValue, JsValue> {
    let ns = Reflect::get(&js_sys::global(), &"Mp4Muxer".into())?;
    let ctor = Reflect::get(&ns, &"Muxer".into())?;
    let ctor: js_sys::Function = ctor.dyn_into()?;

    let opts = Object::new();
    Reflect::set(&opts, &"target".into(), target)?;
    Reflect::set(&opts, &"fastStart".into(), &"in-memory".into())?;

    let muxer_vcodec = muxer_video_codec(video_codec);
    let video = Object::new();
    Reflect::set(&video, &"codec".into(), &muxer_vcodec.into())?;
    Reflect::set(&video, &"width".into(), &video_width.into())?;
    Reflect::set(&video, &"height".into(), &video_height.into())?;
    Reflect::set(&opts, &"video".into(), &video)?;

    if let Some((ac, sr)) = audio_codec {
        let audio = Object::new();
        Reflect::set(&audio, &"codec".into(), &ac.into())?;
        Reflect::set(&audio, &"sampleRate".into(), &sr.into())?;
        Reflect::set(&audio, &"numberOfChannels".into(), &1u32.into())?;
        Reflect::set(&opts, &"audio".into(), &audio)?;
    }

    js_sys::Reflect::construct(&ctor, &js_sys::Array::of1(&opts))
}

/// Add an encoded video chunk to the muxer.
pub fn muxer_add_video_chunk(
    muxer: &JsValue,
    chunk: &JsValue,
    meta: &JsValue,
) -> Result<(), JsValue> {
    let add = Reflect::get(muxer, &"addVideoChunk".into())?;
    let add: js_sys::Function = add.dyn_into()?;
    add.call2(muxer, chunk, meta)?;
    Ok(())
}

/// Add an encoded audio chunk to the muxer.
pub fn muxer_add_audio_chunk(
    muxer: &JsValue,
    chunk: &JsValue,
    meta: &JsValue,
) -> Result<(), JsValue> {
    let add = Reflect::get(muxer, &"addAudioChunk".into())?;
    let add: js_sys::Function = add.dyn_into()?;
    add.call2(muxer, chunk, meta)?;
    Ok(())
}

/// Finalize the muxer and return the MP4 bytes as a `Vec<u8>`.
pub fn muxer_finalize(muxer: &JsValue, target: &JsValue) -> Result<Vec<u8>, JsValue> {
    let finalize = Reflect::get(muxer, &"finalize".into())?;
    let finalize: js_sys::Function = finalize.dyn_into()?;
    finalize.call0(muxer)?;

    // target.buffer is an ArrayBuffer
    let buffer = Reflect::get(target, &"buffer".into())?;
    let u8arr = Uint8Array::new(&buffer);
    Ok(u8arr.to_vec())
}
