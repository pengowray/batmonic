//! Backend-specific microphone operations.
//!
//! Abstracts the three mic backends (Browser/Web Audio, cpal/Tauri native,
//! Raw USB) behind an `ActiveBackend` enum with uniform open/close/record/listen
//! methods. All thread-local state for each backend lives here.

use leptos::prelude::*;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::AudioContext;
use crate::state::{AppState, MicAcquisitionState, MicBackend};
use crate::dsp::heterodyne::RealtimeHet;
use crate::tauri_bridge::{get_tauri_internals, tauri_invoke, tauri_invoke_no_args};
use std::cell::RefCell;

// ── Thread-local state: Web Audio mode ──────────────────────────────────

thread_local! {
    static MIC_CTX: RefCell<Option<AudioContext>> = const { RefCell::new(None) };
    static MIC_STREAM: RefCell<Option<web_sys::MediaStream>> = const { RefCell::new(None) };
    static MIC_PROCESSOR: RefCell<Option<web_sys::ScriptProcessorNode>> = const { RefCell::new(None) };
    static MIC_BUFFER: RefCell<Vec<f32>> = const { RefCell::new(Vec::new()) };
    static MIC_HANDLER: RefCell<Option<Closure<dyn FnMut(web_sys::AudioProcessingEvent)>>> = RefCell::new(None);
    static WEB_RT_HET: RefCell<RealtimeHet> = RefCell::new(RealtimeHet::new());
}

// ── Thread-local state: Native mode (shared by cpal AND USB) ────────────

thread_local! {
    /// Whether a native mic (cpal or USB) is currently open.
    static NATIVE_MIC_OPEN: RefCell<Option<NativeMode>> = const { RefCell::new(None) };
    /// AudioContext for HET playback (output only, no mic input).
    static HET_CTX: RefCell<Option<AudioContext>> = const { RefCell::new(None) };
    /// Next scheduled playback time for HET audio buffers.
    static HET_NEXT_TIME: RefCell<f64> = const { RefCell::new(0.0) };
    /// Keep the event listener closure alive.
    static TAURI_EVENT_CLOSURE: RefCell<Option<Closure<dyn FnMut(JsValue)>>> = RefCell::new(None);
    /// Unlisten function returned by Tauri event subscription.
    static TAURI_UNLISTEN: RefCell<Option<js_sys::Function>> = const { RefCell::new(None) };
    /// Accumulated recording samples on the frontend for native modes (cpal/USB).
    static NATIVE_REC_BUFFER: RefCell<Vec<f32>> = const { RefCell::new(Vec::new()) };
    /// Realtime heterodyne processor for native modes.
    static NATIVE_RT_HET: RefCell<RealtimeHet> = RefCell::new(RealtimeHet::new());
}

// ── Thread-local state: USB-specific ────────────────────────────────────

thread_local! {
    /// Keep the USB stream error event listener closure alive.
    static USB_ERROR_CLOSURE: RefCell<Option<Closure<dyn FnMut(JsValue)>>> = RefCell::new(None);
}

/// Which native mode is active (stored in NATIVE_MIC_OPEN).
#[derive(Clone, Copy, Debug, PartialEq)]
enum NativeMode {
    Cpal,
    Usb,
}

// ── ActiveBackend enum ──────────────────────────────────────────────────

/// Runtime mic backend, used internally by the recording system.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ActiveBackend {
    Browser,
    Cpal,
    RawUsb,
}

impl From<MicBackend> for ActiveBackend {
    fn from(b: MicBackend) -> Self {
        match b {
            MicBackend::Browser => ActiveBackend::Browser,
            MicBackend::Cpal => ActiveBackend::Cpal,
            MicBackend::RawUsb => ActiveBackend::RawUsb,
        }
    }
}

/// Result of stopping a recording.
pub enum StopResult {
    /// Browser mode: raw samples extracted from the JS callback buffer.
    Samples { samples: Vec<f32>, sample_rate: u32 },
    /// Native (cpal/USB) mode: parsed result from Tauri command.
    TauriResult(TauriRecordingResult),
    /// Recording produced no usable data.
    Empty,
    /// An error occurred while stopping.
    Error(String),
}

/// Parsed recording result returned by `mic_stop_recording` / `usb_stop_recording`.
pub struct TauriRecordingResult {
    pub filename: String,
    pub saved_path: String,
    pub sample_rate: u32,
    pub bits_per_sample: u16,
    pub is_float: bool,
    pub duration_secs: f64,
    pub samples: Vec<f32>,
}

impl TauriRecordingResult {
    /// Parse from JsValue returned by Tauri IPC.
    pub fn from_js(result: &JsValue) -> Option<Self> {
        let filename = js_sys::Reflect::get(result, &JsValue::from_str("filename"))
            .ok().and_then(|v| v.as_string())
            .unwrap_or_else(|| "recording.wav".into());
        let sample_rate = js_sys::Reflect::get(result, &JsValue::from_str("sample_rate"))
            .ok().and_then(|v| v.as_f64())
            .unwrap_or(48000.0) as u32;
        let bits_per_sample = js_sys::Reflect::get(result, &JsValue::from_str("bits_per_sample"))
            .ok().and_then(|v| v.as_f64())
            .unwrap_or(16.0) as u16;
        let is_float = js_sys::Reflect::get(result, &JsValue::from_str("is_float"))
            .ok().and_then(|v| v.as_bool())
            .unwrap_or(false);
        let duration_secs = js_sys::Reflect::get(result, &JsValue::from_str("duration_secs"))
            .ok().and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let saved_path = js_sys::Reflect::get(result, &JsValue::from_str("saved_path"))
            .ok().and_then(|v| v.as_string())
            .unwrap_or_default();

        let samples_js = js_sys::Reflect::get(result, &JsValue::from_str("samples_f32"))
            .unwrap_or(JsValue::NULL);
        let samples_array = js_sys::Array::from(&samples_js);
        let samples: Vec<f32> = (0..samples_array.length())
            .map(|i| samples_array.get(i).as_f64().unwrap_or(0.0) as f32)
            .collect();

        if samples.is_empty() {
            return None;
        }

        Some(TauriRecordingResult {
            filename,
            saved_path,
            sample_rate,
            bits_per_sample,
            is_float,
            duration_secs,
            samples,
        })
    }
}

// ── Public API on ActiveBackend ─────────────────────────────────────────

impl ActiveBackend {
    /// Check if this backend's mic is currently open.
    pub fn is_open(&self) -> bool {
        match self {
            ActiveBackend::Browser => MIC_CTX.with(|c| c.borrow().is_some()),
            ActiveBackend::Cpal => NATIVE_MIC_OPEN.with(|o| *o.borrow() == Some(NativeMode::Cpal)),
            ActiveBackend::RawUsb => NATIVE_MIC_OPEN.with(|o| *o.borrow() == Some(NativeMode::Usb)),
        }
    }

    /// Clear the live sample buffer for this backend.
    pub fn clear_buffer(&self) {
        match self {
            ActiveBackend::Browser => MIC_BUFFER.with(|buf| buf.borrow_mut().clear()),
            ActiveBackend::Cpal | ActiveBackend::RawUsb => {
                NATIVE_REC_BUFFER.with(|buf| buf.borrow_mut().clear());
            }
        }
    }

    /// Open the mic. Returns true on success.
    pub async fn open(&self, state: &AppState) -> bool {
        match self {
            ActiveBackend::Browser => open_web(state).await,
            ActiveBackend::Cpal => open_cpal(state).await,
            ActiveBackend::RawUsb => open_usb(state).await,
        }
    }

    /// Close the mic unconditionally.
    pub async fn close(&self, state: &AppState) {
        match self {
            ActiveBackend::Browser => close_web(state),
            ActiveBackend::Cpal => close_cpal(state).await,
            ActiveBackend::RawUsb => close_usb(state).await,
        }
    }

    /// Close only if not recording and not listening.
    pub async fn maybe_close(&self, state: &AppState) {
        if !state.mic_listening.get_untracked() && !state.mic_recording.get_untracked() {
            self.close(state).await;
        }
    }

    /// Signal the backend to start recording. For browser mode this is a no-op
    /// because the ScriptProcessorNode callback is already accumulating samples.
    pub async fn start_recording(&self, _state: &AppState) -> Result<(), String> {
        match self {
            ActiveBackend::Browser => Ok(()),
            ActiveBackend::Cpal => {
                tauri_invoke_no_args("mic_start_recording").await.map(|_| ())
            }
            ActiveBackend::RawUsb => {
                tauri_invoke_no_args("usb_start_recording").await.map(|_| ())
            }
        }
    }

    /// Stop recording and return the result. For browser mode, extracts samples
    /// from the JS callback buffer. For native modes, calls the Tauri command.
    pub async fn stop_recording(&self, state: &AppState) -> StopResult {
        match self {
            ActiveBackend::Browser => {
                state.mic_recording.set(false);
                state.mic_recording_start_time.set(None);
                let sample_rate = state.mic_sample_rate.get_untracked();
                let samples = MIC_BUFFER.with(|buf| std::mem::take(&mut *buf.borrow_mut()));
                state.mic_samples_recorded.set(0);
                if samples.is_empty() || sample_rate == 0 {
                    log::warn!("No samples recorded (web)");
                    StopResult::Empty
                } else {
                    log::info!("Recording stopped: {} samples ({:.2}s at {} Hz)",
                        samples.len(), samples.len() as f64 / sample_rate as f64, sample_rate);
                    StopResult::Samples { samples, sample_rate }
                }
            }
            ActiveBackend::Cpal => {
                match tauri_invoke_no_args("mic_stop_recording").await {
                    Ok(result) => {
                        match TauriRecordingResult::from_js(&result) {
                            Some(r) => StopResult::TauriResult(r),
                            None => StopResult::Empty,
                        }
                    }
                    Err(e) => StopResult::Error(e),
                }
            }
            ActiveBackend::RawUsb => {
                match tauri_invoke_no_args("usb_stop_recording").await {
                    Ok(result) => {
                        match TauriRecordingResult::from_js(&result) {
                            Some(r) => StopResult::TauriResult(r),
                            None => StopResult::Empty,
                        }
                    }
                    Err(e) => StopResult::Error(e),
                }
            }
        }
    }

    /// Enable or disable live listening. For browser mode this is a no-op
    /// (the ScriptProcessorNode callback checks the signal). For cpal, issues
    /// the `mic_set_listening` command. For USB, no backend command needed.
    pub async fn set_listening(&self, _state: &AppState, enabled: bool) {
        match self {
            ActiveBackend::Browser => { /* callback checks mic_listening signal */ }
            ActiveBackend::Cpal => {
                let args = js_sys::Object::new();
                js_sys::Reflect::set(&args, &"listening".into(),
                    &JsValue::from_bool(enabled)).ok();
                let _ = tauri_invoke("mic_set_listening", &args.into()).await;
            }
            ActiveBackend::RawUsb => { /* USB streams continuously once open */ }
        }
    }
}

// ── Public helpers ──────────────────────────────────────────────────────

/// Borrow the live recording buffer and call `f` with a reference to the samples.
/// Works for both web (MIC_BUFFER) and Tauri (NATIVE_REC_BUFFER) modes.
pub fn with_live_samples<R>(is_tauri: bool, f: impl FnOnce(&[f32]) -> R) -> R {
    if is_tauri {
        NATIVE_REC_BUFFER.with(|buf| f(&buf.borrow()))
    } else {
        MIC_BUFFER.with(|buf| f(&buf.borrow()))
    }
}

/// Extract samples from the native buffer (for error-path finalization).
pub fn take_native_buffer() -> Vec<f32> {
    NATIVE_REC_BUFFER.with(|buf| std::mem::take(&mut *buf.borrow_mut()))
}

// ── Tauri event listeners (private) ─────────────────────────────────────

/// Subscribe to a Tauri event, storing the closure in the shared native thread-local.
fn tauri_listen(event_name: &str, callback: Closure<dyn FnMut(JsValue)>) -> Option<()> {
    let tauri = get_tauri_internals()?;

    let transform_fn = js_sys::Reflect::get(&tauri, &JsValue::from_str("transformCallback")).ok()?;
    let transform_fn = js_sys::Function::from(transform_fn);
    let handler_id = transform_fn.call1(&tauri, callback.as_ref().unchecked_ref()).ok()?;

    let invoke_fn = js_sys::Reflect::get(&tauri, &JsValue::from_str("invoke")).ok()?;
    let invoke_fn = js_sys::Function::from(invoke_fn);

    let args = js_sys::Object::new();
    js_sys::Reflect::set(&args, &"event".into(), &JsValue::from_str(event_name)).ok();
    let target = js_sys::Object::new();
    js_sys::Reflect::set(&target, &"kind".into(), &JsValue::from_str("Any")).ok();
    js_sys::Reflect::set(&args, &"target".into(), &target).ok();
    js_sys::Reflect::set(&args, &"handler".into(), &handler_id).ok();

    invoke_fn
        .call2(&tauri, &JsValue::from_str("plugin:event|listen"), &args)
        .ok();

    TAURI_EVENT_CLOSURE.with(|c| *c.borrow_mut() = Some(callback));
    Some(())
}

/// Subscribe to a USB stream error event (separate thread-local from tauri_listen).
fn tauri_listen_usb_error(event_name: &str, callback: Closure<dyn FnMut(JsValue)>) -> Option<()> {
    let tauri = get_tauri_internals()?;

    let transform_fn = js_sys::Reflect::get(&tauri, &JsValue::from_str("transformCallback")).ok()?;
    let transform_fn = js_sys::Function::from(transform_fn);
    let handler_id = transform_fn.call1(&tauri, callback.as_ref().unchecked_ref()).ok()?;

    let invoke_fn = js_sys::Reflect::get(&tauri, &JsValue::from_str("invoke")).ok()?;
    let invoke_fn = js_sys::Function::from(invoke_fn);

    let args = js_sys::Object::new();
    js_sys::Reflect::set(&args, &"event".into(), &JsValue::from_str(event_name)).ok();
    let target = js_sys::Object::new();
    js_sys::Reflect::set(&target, &"kind".into(), &JsValue::from_str("Any")).ok();
    js_sys::Reflect::set(&args, &"target".into(), &target).ok();
    js_sys::Reflect::set(&args, &"handler".into(), &handler_id).ok();

    invoke_fn
        .call2(&tauri, &JsValue::from_str("plugin:event|listen"), &args)
        .ok();

    USB_ERROR_CLOSURE.with(|c| *c.borrow_mut() = Some(callback));
    Some(())
}

// ── Shared native helpers (used by both cpal and USB) ───────────────────

/// Create and resume the HET playback AudioContext, resetting the scheduling state.
async fn setup_het_context(state: &AppState) -> bool {
    let het_ctx = match AudioContext::new() {
        Ok(c) => c,
        Err(e) => {
            log::error!("Failed to create HET AudioContext: {:?}", e);
            state.status_message.set(Some("Failed to initialize audio output".into()));
            return false;
        }
    };
    if let Ok(promise) = het_ctx.resume() {
        let _ = JsFuture::from(promise).await;
    }
    HET_CTX.with(|c| *c.borrow_mut() = Some(het_ctx));
    HET_NEXT_TIME.with(|t| *t.borrow_mut() = 0.0);
    NATIVE_RT_HET.with(|h| h.borrow_mut().reset());
    true
}

/// Create the chunk handler closure used by both cpal and USB native backends.
/// The closure accumulates samples in NATIVE_REC_BUFFER and handles HET listening.
fn create_native_chunk_handler(state: AppState) -> Closure<dyn FnMut(JsValue)> {
    let state_cb = state;
    Closure::<dyn FnMut(JsValue)>::new(move |event: JsValue| {
        let payload = match js_sys::Reflect::get(&event, &JsValue::from_str("payload")) {
            Ok(p) => p,
            Err(_) => return,
        };

        let array = js_sys::Array::from(&payload);
        let len = array.length() as usize;
        if len == 0 {
            return;
        }

        let input_data: Vec<f32> = (0..len)
            .map(|i| array.get(i as u32).as_f64().unwrap_or(0.0) as f32)
            .collect();

        // Accumulate samples for live waterfall display during recording OR listening
        if state_cb.mic_recording.get_untracked() || state_cb.mic_listening.get_untracked() {
            NATIVE_REC_BUFFER.with(|buf| buf.borrow_mut().extend_from_slice(&input_data));
            if state_cb.mic_recording.get_untracked() {
                state_cb.mic_samples_recorded.update(|n| *n += len);
            }
        }

        // HET listening: process and play through speakers
        if state_cb.mic_listening.get_untracked() {
            let sr = state_cb.mic_sample_rate.get_untracked();
            let het_freq = state_cb.listen_het_frequency.get_untracked();
            let het_cutoff = state_cb.listen_het_cutoff.get_untracked();
            let mut out_data = vec![0.0f32; len];
            NATIVE_RT_HET.with(|h| {
                h.borrow_mut().process(&input_data, &mut out_data, sr, het_freq, het_cutoff);
            });

            // Schedule playback via AudioBuffer
            HET_CTX.with(|ctx_cell| {
                let ctx_ref = ctx_cell.borrow();
                let Some(ctx) = ctx_ref.as_ref() else { return };
                let Ok(buffer) = ctx.create_buffer(1, len as u32, sr as f32) else { return };
                let _ = buffer.copy_to_channel(&out_data, 0);
                let Ok(source) = ctx.create_buffer_source() else { return };
                source.set_buffer(Some(&buffer));
                let _ = source.connect_with_audio_node(&ctx.destination());

                let current_time = ctx.current_time();
                let next_time = HET_NEXT_TIME.with(|t| *t.borrow());
                let start = if next_time > current_time { next_time } else { current_time };
                let _ = source.start_with_when(start);

                let duration = len as f64 / sr as f64;
                HET_NEXT_TIME.with(|t| *t.borrow_mut() = start + duration);
            });
        }
    })
}

/// Clean up all native thread-local state (HET context, event closures, buffer).
fn cleanup_native_state() {
    TAURI_EVENT_CLOSURE.with(|c| { c.borrow_mut().take(); });
    TAURI_UNLISTEN.with(|u| { u.borrow_mut().take(); });

    HET_CTX.with(|c| {
        if let Some(ctx) = c.borrow_mut().take() {
            let _ = ctx.close();
        }
    });

    NATIVE_RT_HET.with(|h| h.borrow_mut().reset());
    NATIVE_REC_BUFFER.with(|buf| buf.borrow_mut().clear());
    crate::canvas::live_waterfall::clear();
}

// ── Browser (Web Audio) backend ─────────────────────────────────────────

async fn open_web(state: &AppState) -> bool {
    if MIC_CTX.with(|c| c.borrow().is_some()) {
        return true;
    }

    state.log_debug("info", "open_web: opening browser mic...");

    let window = match web_sys::window() {
        Some(w) => w,
        None => {
            state.log_debug("error", "open_web: no window object");
            return false;
        }
    };
    let navigator = window.navigator();
    let media_devices = match navigator.media_devices() {
        Ok(md) => md,
        Err(e) => {
            state.log_debug("error", format!("open_web: no media devices: {:?}", e));
            state.status_message.set(Some("Microphone not available on this device".into()));
            return false;
        }
    };

    let constraints = web_sys::MediaStreamConstraints::new();
    let audio_opts = js_sys::Object::new();
    js_sys::Reflect::set(&audio_opts, &"echoCancellation".into(), &JsValue::FALSE).ok();
    js_sys::Reflect::set(&audio_opts, &"noiseSuppression".into(), &JsValue::FALSE).ok();
    js_sys::Reflect::set(&audio_opts, &"autoGainControl".into(), &JsValue::FALSE).ok();
    constraints.set_audio(&audio_opts.into());

    let promise = match media_devices.get_user_media_with_constraints(&constraints) {
        Ok(p) => p,
        Err(e) => {
            log::error!("getUserMedia failed: {:?}", e);
            state.status_message.set(Some("Microphone not available".into()));
            return false;
        }
    };

    state.log_debug("info", "open_web: calling getUserMedia...");
    let stream_js = match JsFuture::from(promise).await {
        Ok(s) => {
            state.log_debug("info", "open_web: getUserMedia succeeded");
            s
        }
        Err(e) => {
            state.log_debug("error", format!("open_web: getUserMedia denied: {:?}", e));
            state.status_message.set(Some("Microphone permission denied".into()));
            return false;
        }
    };

    let stream: web_sys::MediaStream = match stream_js.dyn_into() {
        Ok(s) => s,
        Err(_) => {
            log::error!("Failed to cast MediaStream");
            return false;
        }
    };

    let ctx = match AudioContext::new() {
        Ok(c) => c,
        Err(e) => {
            log::error!("Failed to create AudioContext: {:?}", e);
            state.status_message.set(Some("Failed to initialize audio".into()));
            return false;
        }
    };

    if let Ok(promise) = ctx.resume() {
        let _ = JsFuture::from(promise).await;
    }

    let sample_rate = ctx.sample_rate() as u32;
    state.mic_sample_rate.set(sample_rate);
    state.mic_device_name.set(Some("Browser microphone".into()));
    state.mic_connection_type.set(None);
    let source = match ctx.create_media_stream_source(&stream) {
        Ok(s) => s,
        Err(e) => {
            log::error!("Failed to create MediaStreamSource: {:?}", e);
            return false;
        }
    };

    let processor = match ctx.create_script_processor_with_buffer_size_and_number_of_input_channels_and_number_of_output_channels(4096, 1, 1) {
        Ok(p) => p,
        Err(e) => {
            log::error!("Failed to create ScriptProcessorNode: {:?}", e);
            return false;
        }
    };

    if let Err(e) = source.connect_with_audio_node(&processor) {
        log::error!("Failed to connect source -> processor: {:?}", e);
        return false;
    }
    if let Err(e) = processor.connect_with_audio_node(&ctx.destination()) {
        log::error!("Failed to connect processor -> destination: {:?}", e);
        return false;
    }

    WEB_RT_HET.with(|h| h.borrow_mut().reset());

    let state_cb = *state;
    let handler = Closure::<dyn FnMut(web_sys::AudioProcessingEvent)>::new(move |ev: web_sys::AudioProcessingEvent| {
        let input_buffer = match ev.input_buffer() {
            Ok(b) => b,
            Err(_) => return,
        };
        let output_buffer = match ev.output_buffer() {
            Ok(b) => b,
            Err(_) => return,
        };

        let input_data = match input_buffer.get_channel_data(0) {
            Ok(d) => d,
            Err(_) => return,
        };

        if state_cb.mic_listening.get_untracked() {
            let sr = state_cb.mic_sample_rate.get_untracked();
            let het_freq = state_cb.listen_het_frequency.get_untracked();
            let het_cutoff = state_cb.listen_het_cutoff.get_untracked();
            let mut out_data = vec![0.0f32; input_data.len()];
            WEB_RT_HET.with(|h| {
                h.borrow_mut().process(&input_data, &mut out_data, sr, het_freq, het_cutoff);
            });
            let _ = output_buffer.copy_to_channel(&out_data, 0);
        } else {
            let zeros = vec![0.0f32; input_data.len()];
            let _ = output_buffer.copy_to_channel(&zeros, 0);
        }

        // Accumulate samples for live waterfall display during recording OR listening
        if state_cb.mic_recording.get_untracked() || state_cb.mic_listening.get_untracked() {
            MIC_BUFFER.with(|buf| {
                buf.borrow_mut().extend_from_slice(&input_data);
                if state_cb.mic_recording.get_untracked() {
                    state_cb.mic_samples_recorded.set(buf.borrow().len());
                }
            });
        }
    });

    processor.set_onaudioprocess(Some(handler.as_ref().unchecked_ref()));

    MIC_CTX.with(|c| *c.borrow_mut() = Some(ctx));
    MIC_STREAM.with(|s| *s.borrow_mut() = Some(stream));
    MIC_PROCESSOR.with(|p| *p.borrow_mut() = Some(processor));
    MIC_HANDLER.with(|h| *h.borrow_mut() = Some(handler));

    log::info!("Web mic opened at {} Hz", sample_rate);
    true
}

fn close_web(state: &AppState) {
    MIC_STREAM.with(|s| {
        if let Some(stream) = s.borrow_mut().take() {
            let tracks = stream.get_tracks();
            for i in 0..tracks.length() {
                let track_js = tracks.get(i);
                if let Ok(track) = track_js.dyn_into::<web_sys::MediaStreamTrack>() {
                    track.stop();
                }
            }
        }
    });

    MIC_PROCESSOR.with(|p| {
        if let Some(proc) = p.borrow_mut().take() {
            proc.set_onaudioprocess(None);
            let _ = proc.disconnect();
        }
    });

    MIC_HANDLER.with(|h| { h.borrow_mut().take(); });

    MIC_CTX.with(|c| {
        if let Some(ctx) = c.borrow_mut().take() {
            let _ = ctx.close();
        }
    });

    MIC_BUFFER.with(|buf| buf.borrow_mut().clear());
    WEB_RT_HET.with(|h| h.borrow_mut().reset());
    crate::canvas::live_waterfall::clear();

    state.mic_samples_recorded.set(0);
    log::info!("Web mic closed");
}

// ── cpal (Tauri native) backend ─────────────────────────────────────────

async fn open_cpal(state: &AppState) -> bool {
    if NATIVE_MIC_OPEN.with(|o| *o.borrow() == Some(NativeMode::Cpal)) {
        return true;
    }

    let max_sr = state.mic_max_sample_rate.get_untracked();
    let max_bits = state.mic_max_bit_depth.get_untracked();
    let channel_mode = state.mic_channel_mode.get_untracked();
    let selected_device = state.mic_selected_device.get_untracked();
    let args = js_sys::Object::new();
    if max_sr > 0 {
        js_sys::Reflect::set(&args, &JsValue::from_str("maxSampleRate"),
            &JsValue::from_f64(max_sr as f64)).ok();
    }
    if let Some(ref name) = selected_device {
        js_sys::Reflect::set(&args, &JsValue::from_str("deviceName"),
            &JsValue::from_str(name)).ok();
    }
    if max_bits > 0 {
        js_sys::Reflect::set(&args, &JsValue::from_str("maxBitDepth"),
            &JsValue::from_f64(max_bits as f64)).ok();
    }
    {
        use crate::state::ChannelMode;
        let ch: u16 = match channel_mode {
            ChannelMode::Mono => 1,
            ChannelMode::Stereo => 2,
        };
        js_sys::Reflect::set(&args, &JsValue::from_str("channels"),
            &JsValue::from_f64(ch as f64)).ok();
    }
    let result = match tauri_invoke("mic_open", &args.into()).await {
        Ok(v) => v,
        Err(e) => {
            log::warn!("Native mic failed: {}", e);
            state.status_message.set(Some(format!("Native mic unavailable: {}", e)));
            return false;
        }
    };

    // Parse MicInfo from the response
    let sample_rate = js_sys::Reflect::get(&result, &JsValue::from_str("sample_rate"))
        .ok().and_then(|v| v.as_f64())
        .unwrap_or(48000.0) as u32;
    let bits_per_sample = js_sys::Reflect::get(&result, &JsValue::from_str("bits_per_sample"))
        .ok().and_then(|v| v.as_f64())
        .unwrap_or(16.0) as u16;
    let device_name = js_sys::Reflect::get(&result, &JsValue::from_str("device_name"))
        .ok().and_then(|v| v.as_string())
        .unwrap_or_else(|| "Unknown".into());

    // Parse supported_sample_rates from MicInfo response
    let supported_rates: Vec<u32> = js_sys::Reflect::get(&result, &JsValue::from_str("supported_sample_rates"))
        .ok()
        .and_then(|v| {
            let arr = js_sys::Array::from(&v);
            let mut rates = Vec::new();
            for i in 0..arr.length() {
                if let Some(r) = arr.get(i).as_f64() {
                    rates.push(r as u32);
                }
            }
            if rates.is_empty() { None } else { Some(rates) }
        })
        .unwrap_or_default();
    if !supported_rates.is_empty() {
        state.mic_supported_rates.set(supported_rates);
    }

    state.mic_sample_rate.set(sample_rate);
    state.mic_bits_per_sample.set(bits_per_sample);
    state.mic_device_name.set(Some(device_name.clone()));
    let conn_type = if device_name.to_lowercase().contains("usb") {
        "USB"
    } else if device_name.to_lowercase().contains("bluetooth") || device_name.to_lowercase().contains("bt ") {
        "Bluetooth"
    } else {
        "Internal"
    };
    state.mic_connection_type.set(Some(conn_type.to_string()));

    // Setup HET playback AudioContext and chunk handler
    if !setup_het_context(state).await {
        return false;
    }

    let chunk_handler = create_native_chunk_handler(*state);
    tauri_listen("mic-audio-chunk", chunk_handler);

    NATIVE_MIC_OPEN.with(|o| *o.borrow_mut() = Some(NativeMode::Cpal));
    log::info!("Native mic opened: {} at {} Hz, {}-bit", device_name, sample_rate, bits_per_sample);
    true
}

async fn close_cpal(state: &AppState) {
    if let Err(e) = tauri_invoke_no_args("mic_close").await {
        log::error!("mic_close failed: {}", e);
    }

    cleanup_native_state();
    NATIVE_MIC_OPEN.with(|o| *o.borrow_mut() = None);

    state.mic_samples_recorded.set(0);
    log::info!("Native mic closed");
}

// ── Raw USB backend ─────────────────────────────────────────────────────

async fn open_usb(state: &AppState) -> bool {
    if NATIVE_MIC_OPEN.with(|o| *o.borrow() == Some(NativeMode::Usb)) {
        return true;
    }

    // Step 1: List USB devices via Kotlin plugin
    let devices_result = tauri_invoke("plugin:usb-audio|listUsbDevices",
        &js_sys::Object::new().into()).await;
    let devices = match devices_result {
        Ok(v) => v,
        Err(e) => {
            log::warn!("USB device listing failed: {}", e);
            state.status_message.set(Some(format!("USB: {}", e)));
            return false;
        }
    };

    let devices_arr = js_sys::Reflect::get(&devices, &JsValue::from_str("devices"))
        .ok()
        .map(|v| js_sys::Array::from(&v))
        .unwrap_or_default();

    let mut audio_device_name: Option<String> = None;
    let mut has_permission = false;
    for i in 0..devices_arr.length() {
        let dev = devices_arr.get(i);
        let is_audio = js_sys::Reflect::get(&dev, &JsValue::from_str("isAudioDevice"))
            .ok().and_then(|v| v.as_bool()).unwrap_or(false);
        if is_audio {
            audio_device_name = js_sys::Reflect::get(&dev, &JsValue::from_str("deviceName"))
                .ok().and_then(|v| v.as_string());
            has_permission = js_sys::Reflect::get(&dev, &JsValue::from_str("hasPermission"))
                .ok().and_then(|v| v.as_bool()).unwrap_or(false);
            break;
        }
    }

    let device_name = match audio_device_name {
        Some(n) => n,
        None => {
            state.status_message.set(Some("No USB audio device found".into()));
            return false;
        }
    };

    // Step 2: Request permission if needed
    if !has_permission {
        let perm_args = js_sys::Object::new();
        js_sys::Reflect::set(&perm_args, &JsValue::from_str("deviceName"),
            &JsValue::from_str(&device_name)).ok();
        match tauri_invoke("plugin:usb-audio|requestUsbPermission", &perm_args.into()).await {
            Ok(result) => {
                let granted = js_sys::Reflect::get(&result, &JsValue::from_str("granted"))
                    .ok().and_then(|v| v.as_bool()).unwrap_or(false);
                if !granted {
                    state.status_message.set(Some("USB permission denied".into()));
                    return false;
                }
            }
            Err(e) => {
                state.status_message.set(Some(format!("USB permission error: {}", e)));
                return false;
            }
        }
    }

    // Step 3: Open device via Kotlin plugin
    let max_sr = state.mic_max_sample_rate.get_untracked();
    let open_args = js_sys::Object::new();
    js_sys::Reflect::set(&open_args, &JsValue::from_str("deviceName"),
        &JsValue::from_str(&device_name)).ok();
    js_sys::Reflect::set(&open_args, &JsValue::from_str("sampleRate"),
        &JsValue::from_f64(max_sr as f64)).ok();

    let device_info = match tauri_invoke("plugin:usb-audio|openUsbDevice", &open_args.into()).await {
        Ok(v) => v,
        Err(e) => {
            state.status_message.set(Some(format!("USB open failed: {}", e)));
            return false;
        }
    };

    let fd = js_sys::Reflect::get(&device_info, &JsValue::from_str("fd"))
        .ok().and_then(|v| v.as_f64()).unwrap_or(-1.0) as i64;
    let endpoint_address = js_sys::Reflect::get(&device_info, &JsValue::from_str("endpointAddress"))
        .ok().and_then(|v| v.as_f64()).unwrap_or(0.0) as u32;
    let max_packet_size = js_sys::Reflect::get(&device_info, &JsValue::from_str("maxPacketSize"))
        .ok().and_then(|v| v.as_f64()).unwrap_or(0.0) as u32;
    let sample_rate = js_sys::Reflect::get(&device_info, &JsValue::from_str("sampleRate"))
        .ok().and_then(|v| v.as_f64()).unwrap_or(384000.0) as u32;
    let num_channels = js_sys::Reflect::get(&device_info, &JsValue::from_str("numChannels"))
        .ok().and_then(|v| v.as_f64()).unwrap_or(1.0) as u32;
    let product_name = js_sys::Reflect::get(&device_info, &JsValue::from_str("productName"))
        .ok().and_then(|v| v.as_string()).unwrap_or_else(|| "USB Audio".into());
    let interface_number = js_sys::Reflect::get(&device_info, &JsValue::from_str("interfaceNumber"))
        .ok().and_then(|v| v.as_f64()).unwrap_or(0.0) as u32;
    let alternate_setting = js_sys::Reflect::get(&device_info, &JsValue::from_str("alternateSetting"))
        .ok().and_then(|v| v.as_f64()).unwrap_or(0.0) as u32;

    if fd < 0 || endpoint_address == 0 || max_packet_size == 0 {
        state.status_message.set(Some("USB device: invalid fd or endpoint".into()));
        return false;
    }

    // Step 4: Start USB stream in Rust backend
    let stream_args = js_sys::Object::new();
    js_sys::Reflect::set(&stream_args, &JsValue::from_str("fd"),
        &JsValue::from_f64(fd as f64)).ok();
    js_sys::Reflect::set(&stream_args, &JsValue::from_str("endpointAddress"),
        &JsValue::from_f64(endpoint_address as f64)).ok();
    js_sys::Reflect::set(&stream_args, &JsValue::from_str("maxPacketSize"),
        &JsValue::from_f64(max_packet_size as f64)).ok();
    js_sys::Reflect::set(&stream_args, &JsValue::from_str("sampleRate"),
        &JsValue::from_f64(sample_rate as f64)).ok();
    js_sys::Reflect::set(&stream_args, &JsValue::from_str("numChannels"),
        &JsValue::from_f64(num_channels as f64)).ok();
    js_sys::Reflect::set(&stream_args, &JsValue::from_str("deviceName"),
        &JsValue::from_str(&device_name)).ok();
    js_sys::Reflect::set(&stream_args, &JsValue::from_str("interfaceNumber"),
        &JsValue::from_f64(interface_number as f64)).ok();
    js_sys::Reflect::set(&stream_args, &JsValue::from_str("alternateSetting"),
        &JsValue::from_f64(alternate_setting as f64)).ok();

    match tauri_invoke("usb_start_stream", &stream_args.into()).await {
        Ok(_) => {}
        Err(e) => {
            state.status_message.set(Some(format!("USB stream failed: {}", e)));
            let _ = tauri_invoke("plugin:usb-audio|closeUsbDevice",
                &js_sys::Object::new().into()).await;
            return false;
        }
    }

    state.mic_sample_rate.set(sample_rate);
    let usb_bits = js_sys::Reflect::get(&device_info, &JsValue::from_str("bitDepth"))
        .ok().and_then(|v| v.as_f64()).unwrap_or(16.0) as u16;
    state.mic_bits_per_sample.set(usb_bits);

    // Setup HET playback AudioContext and chunk handler (same as cpal)
    if !setup_het_context(state).await {
        return false;
    }

    let chunk_handler = create_native_chunk_handler(*state);
    tauri_listen("mic-audio-chunk", chunk_handler);

    // Listen for USB stream errors (disconnect / ENODEV)
    let state_err = *state;
    let error_handler = Closure::<dyn FnMut(JsValue)>::new(move |event: JsValue| {
        let msg = js_sys::Reflect::get(&event, &JsValue::from_str("payload"))
            .ok()
            .and_then(|v| v.as_string())
            .unwrap_or_else(|| "USB stream error".into());

        state_err.log_debug("error", format!("USB stream error: {}", msg));
        state_err.show_error_toast(&msg);

        let was_recording = state_err.mic_recording.get_untracked();
        state_err.mic_recording.set(false);
        state_err.mic_recording_start_time.set(None);
        state_err.mic_listening.set(false);
        state_err.mic_usb_connected.set(false);
        state_err.mic_backend.set(None);
        state_err.mic_acquisition_state.set(MicAcquisitionState::Failed);

        NATIVE_MIC_OPEN.with(|o| *o.borrow_mut() = None);

        // Finalize any in-progress recording with whatever samples we have
        if was_recording {
            let sr = state_err.mic_sample_rate.get_untracked();
            let samples = take_native_buffer();
            if !samples.is_empty() && sr > 0 {
                crate::audio::live_recording::finalize_recording(
                    crate::audio::live_recording::FinalizeParams {
                        samples, sample_rate: sr,
                        bits_per_sample: state_err.mic_bits_per_sample.get_untracked(),
                        is_float: false,
                        saved_path: String::new(),
                    }, state_err,
                );
            }
        }

        // Clean up HET context
        HET_CTX.with(|c| {
            if let Some(ctx) = c.borrow_mut().take() {
                let _ = ctx.close();
            }
        });
        NATIVE_RT_HET.with(|h| h.borrow_mut().reset());
        NATIVE_REC_BUFFER.with(|buf| buf.borrow_mut().clear());
    });
    tauri_listen_usb_error("usb-stream-error", error_handler);

    NATIVE_MIC_OPEN.with(|o| *o.borrow_mut() = Some(NativeMode::Usb));
    state.mic_device_name.set(Some(product_name.clone()));
    state.mic_connection_type.set(Some("USB (Raw)".to_string()));
    log::info!("USB mic opened: {} at {} Hz", product_name, sample_rate);
    true
}

async fn close_usb(state: &AppState) {
    if let Err(e) = tauri_invoke_no_args("usb_stop_stream").await {
        log::error!("usb_stop_stream failed: {}", e);
    }

    let _ = tauri_invoke("plugin:usb-audio|closeUsbDevice",
        &js_sys::Object::new().into()).await;

    // Also clean up USB error closure
    USB_ERROR_CLOSURE.with(|c| { c.borrow_mut().take(); });

    cleanup_native_state();
    NATIVE_MIC_OPEN.with(|o| *o.borrow_mut() = None);

    state.mic_samples_recorded.set(0);
    log::info!("USB mic closed");
}
