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
use crate::state::{AppState, ListenMode, MicAcquisitionState, MicBackend};
use crate::dsp::heterodyne::RealtimeHet;
use crate::dsp::pitch_shift::pitch_shift_realtime;
use crate::dsp::phase_vocoder::phase_vocoder_pitch_shift;
use crate::dsp::zc_divide::zc_divide;
use crate::tauri_bridge::{get_tauri_internals, tauri_invoke, tauri_invoke_no_args};
use std::cell::RefCell;

/// Build IPC args for mic_stop_recording / usb_stop_recording,
/// including optional GPS location and device model fields from state.
fn build_stop_recording_args(state: &AppState) -> JsValue {
    let args = js_sys::Object::new();
    if let Some(loc) = state.recording_location.get_untracked() {
        let _ = js_sys::Reflect::set(&args, &JsValue::from_str("locLatitude"), &JsValue::from_f64(loc.latitude));
        let _ = js_sys::Reflect::set(&args, &JsValue::from_str("locLongitude"), &JsValue::from_f64(loc.longitude));
        if let Some(e) = loc.elevation {
            let _ = js_sys::Reflect::set(&args, &JsValue::from_str("locElevation"), &JsValue::from_f64(e));
        }
        if let Some(a) = loc.accuracy {
            let _ = js_sys::Reflect::set(&args, &JsValue::from_str("locAccuracy"), &JsValue::from_f64(a));
        }
    }
    if state.device_model_enabled.get_untracked() {
        if let Some(make) = state.cached_device_make.get_untracked() {
            let _ = js_sys::Reflect::set(&args, &JsValue::from_str("deviceMake"), &JsValue::from_str(&make));
        }
        if let Some(model) = state.cached_device_model.get_untracked() {
            let _ = js_sys::Reflect::set(&args, &JsValue::from_str("deviceModel"), &JsValue::from_str(&model));
        }
    }
    // Pass the app version so the Tauri backend can embed the correct version in GUANO
    let _ = js_sys::Reflect::set(&args, &JsValue::from_str("appVersion"), &JsValue::from_str(env!("CARGO_PKG_VERSION")));
    args.into()
}

// ── Thread-local state: Web Audio mode ──────────────────────────────────

thread_local! {
    static MIC_CTX: RefCell<Option<AudioContext>> = const { RefCell::new(None) };
    static MIC_STREAM: RefCell<Option<web_sys::MediaStream>> = const { RefCell::new(None) };
    static MIC_PROCESSOR: RefCell<Option<web_sys::ScriptProcessorNode>> = const { RefCell::new(None) };
    static MIC_BUFFER: RefCell<Vec<f32>> = const { RefCell::new(Vec::new()) };
    static MIC_HANDLER: RefCell<Option<Closure<dyn FnMut(web_sys::AudioProcessingEvent)>>> = RefCell::new(None);
    static WEB_RT_HET: RefCell<RealtimeHet> = RefCell::new(RealtimeHet::new());
    /// Overlap context state for PS/PV live listening (web).
    static WEB_LISTEN_STATE: RefCell<ListenDspState> = RefCell::new(ListenDspState::new());
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
    /// Overlap context state for PS/PV live listening (native).
    static NATIVE_LISTEN_STATE: RefCell<ListenDspState> = RefCell::new(ListenDspState::new());
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

// ── Live listen DSP dispatch ──────────────────────────────────────────

/// Absolute minimum context for PS/PV (must cover at least one PV FFT window).
const LISTEN_CONTEXT_FLOOR: usize = 4096;

/// Crossfade length (samples) between consecutive output chunks to
/// eliminate any residual boundary discontinuity after overlap-save.
const LISTEN_CROSSFADE: usize = 256;

/// Persistent state for overlap-save PS/PV live processing.
struct ListenDspState {
    /// Accumulated raw input samples (sliding context window).
    context: Vec<f32>,
    /// Tail of the previous returned chunk, used for crossfade.
    prev_tail: Vec<f32>,
}

impl ListenDspState {
    const fn new() -> Self {
        Self { context: Vec::new(), prev_tail: Vec::new() }
    }

    fn clear(&mut self) {
        self.context.clear();
        self.prev_tail.clear();
    }
}

/// Apply the selected listen mode DSP to a chunk of mic input.
///
/// For PS/PV, `dsp_state` accumulates raw input so the batch DSP functions
/// see enough overlap context, then extracts the tail with a crossfade.
fn process_listen_audio(
    input: &[f32],
    mode: ListenMode,
    sample_rate: u32,
    rt_het: &mut RealtimeHet,
    dsp_state: &mut ListenDspState,
    context_chunks: u32,
    het_freq: f64,
    het_cutoff: f64,
    ps_factor: f64,
    pv_factor: f64,
    zc_factor: f64,
) -> Vec<f32> {
    match mode {
        ListenMode::Heterodyne => {
            dsp_state.clear();
            let mut out = vec![0.0f32; input.len()];
            rt_het.process(input, &mut out, sample_rate, het_freq, het_cutoff);
            out
        }
        ListenMode::PitchShift | ListenMode::PhaseVocoder => {
            // Accumulate input into sliding context window
            dsp_state.context.extend_from_slice(input);
            let max_ctx = LISTEN_CONTEXT_FLOOR.max(input.len() * context_chunks as usize);
            if dsp_state.context.len() > max_ctx {
                let excess = dsp_state.context.len() - max_ctx;
                dsp_state.context.drain(..excess);
            }

            // Run the batch DSP on the full context
            let full_output = if mode == ListenMode::PitchShift {
                pitch_shift_realtime(&dsp_state.context, ps_factor)
            } else {
                let mut out = phase_vocoder_pitch_shift(&dsp_state.context, pv_factor);
                let boost = 10.0f32.powf(
                    crate::audio::streaming_playback::PV_MODE_BOOST_DB as f32 / 20.0,
                );
                for s in &mut out {
                    *s *= boost;
                }
                out
            };

            // Extract the last chunk_len samples (deep interior of OLA = clean)
            let chunk_len = input.len();
            let extracted = if full_output.len() >= chunk_len {
                &full_output[full_output.len() - chunk_len..]
            } else {
                &full_output[..]
            };
            let mut result = extracted.to_vec();

            // Crossfade start of this chunk with end of previous chunk
            let fade = LISTEN_CROSSFADE.min(result.len()).min(dsp_state.prev_tail.len());
            for i in 0..fade {
                let t = i as f32 / fade as f32; // 0→1
                result[i] = dsp_state.prev_tail[i] * (1.0 - t) + result[i] * t;
            }

            // Save tail of current result for next crossfade
            dsp_state.prev_tail.clear();
            let tail_start = result.len().saturating_sub(LISTEN_CROSSFADE);
            dsp_state.prev_tail.extend_from_slice(&result[tail_start..]);

            result
        }
        ListenMode::ZeroCrossing => {
            dsp_state.clear();
            zc_divide(input, sample_rate, zc_factor as u32, false)
        }
        ListenMode::Normal => {
            dsp_state.clear();
            input.to_vec()
        }
        ListenMode::ReadyMic => {
            dsp_state.clear();
            vec![0.0f32; input.len()]
        }
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
    /// On mobile Tauri, acquires a ContentResolver fd for direct shared storage write.
    pub async fn start_recording(&self, state: &AppState) -> Result<(), String> {
        match self {
            ActiveBackend::Browser => Ok(()),
            ActiveBackend::Cpal => {
                let fd = try_create_shared_fd(state).await;
                let args = js_sys::Object::new();
                if let Some(fd_val) = fd {
                    js_sys::Reflect::set(&args, &JsValue::from_str("sharedFd"), &JsValue::from_f64(fd_val as f64)).ok();
                }
                tauri_invoke("mic_start_recording", &args.into()).await.map(|_| ())
            }
            ActiveBackend::RawUsb => {
                let fd = try_create_shared_fd(state).await;
                let args = js_sys::Object::new();
                if let Some(fd_val) = fd {
                    js_sys::Reflect::set(&args, &JsValue::from_str("sharedFd"), &JsValue::from_f64(fd_val as f64)).ok();
                }
                tauri_invoke("usb_start_recording", &args.into()).await.map(|_| ())
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
                let args = build_stop_recording_args(state);
                match tauri_invoke("mic_stop_recording", &args).await {
                    Ok(result) => {
                        match TauriRecordingResult::from_js(&result) {
                            Some(r) => {
                                if r.saved_path.starts_with("shared://") {
                                    finalize_shared_entry().await;
                                }
                                StopResult::TauriResult(r)
                            }
                            None => {
                                cancel_shared_entry().await;
                                StopResult::Empty
                            }
                        }
                    }
                    Err(e) => {
                        cancel_shared_entry().await;
                        StopResult::Error(e)
                    }
                }
            }
            ActiveBackend::RawUsb => {
                let args = build_stop_recording_args(state);
                match tauri_invoke("usb_stop_recording", &args).await {
                    Ok(result) => {
                        match TauriRecordingResult::from_js(&result) {
                            Some(r) => {
                                if r.saved_path.starts_with("shared://") {
                                    finalize_shared_entry().await;
                                }
                                StopResult::TauriResult(r)
                            }
                            None => {
                                cancel_shared_entry().await;
                                StopResult::Empty
                            }
                        }
                    }
                    Err(e) => {
                        cancel_shared_entry().await;
                        StopResult::Error(e)
                    }
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

/// Borrow the live recording buffer mutably (e.g. for circular-buffer trimming).
pub fn with_live_samples_mut<R>(is_tauri: bool, f: impl FnOnce(&mut Vec<f32>) -> R) -> R {
    if is_tauri {
        NATIVE_REC_BUFFER.with(|buf| f(&mut buf.borrow_mut()))
    } else {
        MIC_BUFFER.with(|buf| f(&mut buf.borrow_mut()))
    }
}

/// Extract samples from the native buffer (for error-path finalization).
pub fn take_native_buffer() -> Vec<f32> {
    NATIVE_REC_BUFFER.with(|buf| std::mem::take(&mut *buf.borrow_mut()))
}

// ── ContentResolver fd-passing (mobile) ─────────────────────────────────

/// On mobile Tauri, ask the MediaStore plugin to create a pending recording entry
/// and return the raw POSIX fd. Returns `None` on non-mobile, or if the call fails.
async fn try_create_shared_fd(state: &AppState) -> Option<i32> {
    if !state.is_mobile.get_untracked() {
        return None;
    }
    // Build filename from the live file if available, otherwise generate one
    let filename = state.mic_live_file_idx.get_untracked()
        .and_then(|idx| state.files.with_untracked(|f| f.get(idx).map(|f| f.name.clone())))
        .unwrap_or_else(|| {
            let now = js_sys::Date::new_0();
            format!(
                "batcap_{:04}{:02}{:02}_{:02}{:02}{:02}.wav",
                now.get_full_year(), now.get_month() + 1, now.get_date(),
                now.get_hours(), now.get_minutes(), now.get_seconds(),
            )
        });

    let args = js_sys::Object::new();
    js_sys::Reflect::set(&args, &JsValue::from_str("filename"), &JsValue::from_str(&filename)).ok();

    match tauri_invoke("plugin:media-store|createRecordingEntry", &args.into()).await {
        Ok(result) => {
            let fd = js_sys::Reflect::get(&result, &JsValue::from_str("fd"))
                .ok()
                .and_then(|v| v.as_f64())
                .map(|v| v as i32);
            if let Some(fd) = fd {
                log::info!("Got shared storage fd={} for {}", fd, filename);
            }
            fd
        }
        Err(e) => {
            log::warn!("createRecordingEntry failed (will fall back to internal storage): {}", e);
            None
        }
    }
}

/// Tell the MediaStore plugin to finalize (set IS_PENDING=0) the recording entry.
async fn finalize_shared_entry() {
    let args = js_sys::Object::new();
    match tauri_invoke("plugin:media-store|finalizeRecordingEntry", &args.into()).await {
        Ok(_) => log::info!("Finalized shared storage recording entry"),
        Err(e) => log::warn!("finalizeRecordingEntry failed: {}", e),
    }
}

/// Cancel a pending MediaStore recording entry (delete the IS_PENDING=1 row).
/// Called on error paths to avoid orphaned entries. Safe to call when no entry
/// is pending (no-op on the Kotlin side).
pub(crate) async fn cancel_shared_entry() {
    let args = js_sys::Object::new();
    match tauri_invoke("plugin:media-store|cancelRecordingEntry", &args.into()).await {
        Ok(_) => log::info!("Cancelled pending shared storage entry"),
        Err(e) => log::warn!("cancelRecordingEntry failed: {}", e),
    }
}

// ── Tauri event listeners (private) ─────────────────────────────────────

/// Subscribe to a Tauri event, storing the closure in the shared native thread-local.
fn tauri_listen(event_name: &str, callback: Closure<dyn FnMut(JsValue)>) -> Option<()> {
    // If a listener is already registered, reuse it. The closure reads signals
    // dynamically on each invocation, so it works correctly across mic
    // open/close cycles without re-registration.
    if TAURI_EVENT_CLOSURE.with(|c| c.borrow().is_some()) {
        return Some(());
    }

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
    NATIVE_LISTEN_STATE.with(|s| s.borrow_mut().clear());
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

        // Listen mode: process input through selected DSP and play through speakers
        if state_cb.mic_listening.get_untracked() {
            let sr = state_cb.mic_sample_rate.get_untracked();
            let mode = state_cb.listen_mode.get_untracked();
            let out_data = NATIVE_RT_HET.with(|h| {
                NATIVE_LISTEN_STATE.with(|s| {
                    process_listen_audio(
                        &input_data,
                        mode,
                        sr,
                        &mut h.borrow_mut(),
                        &mut s.borrow_mut(),
                        state_cb.listen_context_chunks.get_untracked(),
                        state_cb.listen_het_frequency.get_untracked(),
                        state_cb.listen_het_cutoff.get_untracked(),
                        state_cb.ps_factor.get_untracked(),
                        state_cb.pv_factor.get_untracked(),
                        state_cb.zc_factor.get_untracked(),
                    )
                })
            });

            // Schedule playback via AudioBuffer
            let out_len = out_data.len();
            HET_CTX.with(|ctx_cell| {
                let ctx_ref = ctx_cell.borrow();
                let Some(ctx) = ctx_ref.as_ref() else { return };
                let Ok(buffer) = ctx.create_buffer(1, out_len as u32, sr as f32) else { return };
                let _ = buffer.copy_to_channel(&out_data, 0);
                let Ok(source) = ctx.create_buffer_source() else { return };
                source.set_buffer(Some(&buffer));
                let _ = source.connect_with_audio_node(&ctx.destination());

                let current_time = ctx.current_time();
                let next_time = HET_NEXT_TIME.with(|t| *t.borrow());
                let start = if next_time > current_time { next_time } else { current_time };
                let _ = source.start_with_when(start);

                let duration = out_len as f64 / sr as f64;
                HET_NEXT_TIME.with(|t| *t.borrow_mut() = start + duration);
            });
        }
    })
}

/// Clean up all native thread-local state (HET context, buffer).
/// Note: TAURI_EVENT_CLOSURE is intentionally kept alive — the handler reads
/// signals dynamically and is reused across mic open/close cycles.
fn cleanup_native_state() {
    TAURI_UNLISTEN.with(|u| { u.borrow_mut().take(); });

    HET_CTX.with(|c| {
        if let Some(ctx) = c.borrow_mut().take() {
            let _ = ctx.close();
        }
    });

    NATIVE_RT_HET.with(|h| h.borrow_mut().reset());
    NATIVE_LISTEN_STATE.with(|s| s.borrow_mut().clear());
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
    // If a specific browser device was selected, constrain to that deviceId
    if let Some(device_id) = state.mic_selected_device.get_untracked() {
        if state.mic_backend.get_untracked() == Some(MicBackend::Browser) && !device_id.is_empty() {
            let exact = js_sys::Object::new();
            js_sys::Reflect::set(&exact, &"exact".into(), &JsValue::from_str(&device_id)).ok();
            js_sys::Reflect::set(&audio_opts, &"deviceId".into(), &exact.into()).ok();
        }
    }
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
    let dev_name = state.mic_device_info.get_untracked()
        .map(|info| info.name.clone())
        .unwrap_or_else(|| "Browser microphone".into());
    state.mic_device_name.set(Some(dev_name));
    state.mic_manufacturer.set(None);
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
    WEB_LISTEN_STATE.with(|s| s.borrow_mut().clear());

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
            let mode = state_cb.listen_mode.get_untracked();
            let out_data = WEB_RT_HET.with(|h| {
                WEB_LISTEN_STATE.with(|s| {
                    process_listen_audio(
                        &input_data,
                        mode,
                        sr,
                        &mut h.borrow_mut(),
                        &mut s.borrow_mut(),
                        state_cb.listen_context_chunks.get_untracked(),
                        state_cb.listen_het_frequency.get_untracked(),
                        state_cb.listen_het_cutoff.get_untracked(),
                        state_cb.ps_factor.get_untracked(),
                        state_cb.pv_factor.get_untracked(),
                        state_cb.zc_factor.get_untracked(),
                    )
                })
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
    WEB_LISTEN_STATE.with(|s| s.borrow_mut().clear());
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
    let host_label: Option<String> = js_sys::Reflect::get(&result, &JsValue::from_str("host_name"))
        .ok().and_then(|v| v.as_string())
        .filter(|s| !s.is_empty());

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
    state.mic_manufacturer.set(None);
    // Determine mic interface from the cpal host backend.
    // The host_label comes from the Tauri backend via mic_info.
    let conn_type = if device_name.to_lowercase().contains("usb") {
        "USB"
    } else if device_name.to_lowercase().contains("bluetooth") || device_name.to_lowercase().contains("bt ") {
        "Bluetooth"
    } else {
        // Use the audio host name for native audio interfaces
        host_label.as_deref().unwrap_or("Internal")
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
    let manufacturer_name = js_sys::Reflect::get(&device_info, &JsValue::from_str("manufacturerName"))
        .ok().and_then(|v| v.as_string())
        .filter(|s| !s.is_empty() && s != "Unknown");
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
    let uac_version = js_sys::Reflect::get(&device_info, &JsValue::from_str("uacVersion"))
        .ok().and_then(|v| v.as_f64()).unwrap_or(0.0) as u32;
    js_sys::Reflect::set(&stream_args, &JsValue::from_str("uacVersion"),
        &JsValue::from_f64(uac_version as f64)).ok();

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

        // Cancel any pending shared storage entry (fd was never fully written)
        wasm_bindgen_futures::spawn_local(async { cancel_shared_entry().await });

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
        NATIVE_LISTEN_STATE.with(|s| s.borrow_mut().clear());
        NATIVE_REC_BUFFER.with(|buf| buf.borrow_mut().clear());
    });
    tauri_listen_usb_error("usb-stream-error", error_handler);

    NATIVE_MIC_OPEN.with(|o| *o.borrow_mut() = Some(NativeMode::Usb));
    state.mic_device_name.set(Some(product_name.clone()));
    state.mic_manufacturer.set(manufacturer_name);
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
