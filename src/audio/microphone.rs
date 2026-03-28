//! Microphone control: unified record/listen API.
//!
//! This module provides the public API for microphone recording and listening.
//! Backend-specific operations (Web Audio, cpal, USB) are delegated to
//! `mic_backend::ActiveBackend`. Finalization is handled by `live_recording`.

use leptos::prelude::*;
use wasm_bindgen::prelude::*;
use crate::state::{AppState, MicStrategy, MicBackend, MicAcquisitionState, MicPendingAction};
use crate::audio::mic_backend::{ActiveBackend, StopResult};
use crate::audio::live_recording::FinalizeParams;
use crate::tauri_bridge::{tauri_invoke, tauri_invoke_no_args};

// ── Tauri IPC query helpers ─────────────────────────────────────────────

/// Request Android RECORD_AUDIO runtime permission via Tauri plugin.
/// Returns true if granted, false if denied or not on Android.
pub async fn request_audio_permission_tauri(state: &AppState) -> bool {
    if !state.is_tauri {
        return true;
    }
    state.log_debug("info", "Requesting RECORD_AUDIO permission via plugin...");
    match tauri_invoke("plugin:usb-audio|requestAudioPermission",
        &js_sys::Object::new().into()).await {
        Ok(result) => {
            let granted = js_sys::Reflect::get(&result, &JsValue::from_str("granted"))
                .ok().and_then(|v| v.as_bool()).unwrap_or(false);
            if granted {
                state.log_debug("info", "RECORD_AUDIO permission granted");
            } else {
                state.log_debug("error", "RECORD_AUDIO permission denied");
                state.show_error_toast("Microphone permission denied");
            }
            granted
        }
        Err(e) => {
            state.log_debug("warn", format!("requestAudioPermission failed (may not be Android): {}", e));
            true // Non-fatal on desktop Tauri
        }
    }
}

/// Query the default cpal input device's supported sample rates without opening the mic.
/// Updates `state.mic_supported_rates` with the result.
pub async fn query_cpal_supported_rates(state: &AppState) {
    if !state.is_tauri {
        return;
    }
    let result = match tauri_invoke_no_args("mic_list_devices").await {
        Ok(v) => v,
        Err(_) => return,
    };
    let devices = js_sys::Array::from(&result);
    for i in 0..devices.length() {
        let dev = devices.get(i);
        let is_default = js_sys::Reflect::get(&dev, &JsValue::from_str("is_default"))
            .ok()
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !is_default {
            continue;
        }
        let ranges = match js_sys::Reflect::get(&dev, &JsValue::from_str("sample_rate_ranges")).ok() {
            Some(v) => js_sys::Array::from(&v),
            None => continue,
        };
        let mut rates = std::collections::BTreeSet::new();
        for j in 0..ranges.length() {
            let range = ranges.get(j);
            let min = js_sys::Reflect::get(&range, &JsValue::from_str("min"))
                .ok().and_then(|v| v.as_f64()).unwrap_or(0.0) as u32;
            let max = js_sys::Reflect::get(&range, &JsValue::from_str("max"))
                .ok().and_then(|v| v.as_f64()).unwrap_or(0.0) as u32;
            rates.insert(min);
            rates.insert(max);
            for &r in &[44100, 48000, 96000, 192000, 256000, 384000, 500000] {
                if r >= min && r <= max {
                    rates.insert(r);
                }
            }
        }
        let rates_vec: Vec<u32> = rates.into_iter().collect();
        if !rates_vec.is_empty() {
            state.mic_supported_rates.set(rates_vec);
        }
        break;
    }
}

/// Query mic info without opening the mic. Populates device name/type signals.
pub async fn query_mic_info(state: &AppState) {
    if !state.is_tauri {
        return;
    }
    let backend = state.mic_backend.get_untracked();

    match backend {
        Some(MicBackend::RawUsb) => {
            let devices_result = tauri_invoke("plugin:usb-audio|listUsbDevices",
                &js_sys::Object::new().into()).await;
            if let Ok(devices) = devices_result {
                let devices_arr = js_sys::Reflect::get(&devices, &JsValue::from_str("devices"))
                    .ok()
                    .map(|v| js_sys::Array::from(&v))
                    .unwrap_or_default();
                for i in 0..devices_arr.length() {
                    let dev = devices_arr.get(i);
                    let is_audio = js_sys::Reflect::get(&dev, &JsValue::from_str("isAudioDevice"))
                        .ok().and_then(|v| v.as_bool()).unwrap_or(false);
                    if is_audio {
                        let name = js_sys::Reflect::get(&dev, &JsValue::from_str("productName"))
                            .ok().and_then(|v| v.as_string()).unwrap_or_else(|| "USB Audio".into());
                        state.mic_device_name.set(Some(name));
                        state.mic_connection_type.set(Some("USB".to_string()));
                        state.mic_usb_connected.set(true);
                        return;
                    }
                }
            }
            state.mic_usb_connected.set(false);
        }
        Some(MicBackend::Cpal) | None => {
            if let Ok(result) = tauri_invoke_no_args("mic_list_devices").await {
                let devices = js_sys::Array::from(&result);
                for i in 0..devices.length() {
                    let dev = devices.get(i);
                    let is_default = js_sys::Reflect::get(&dev, &JsValue::from_str("is_default"))
                        .ok().and_then(|v| v.as_bool()).unwrap_or(false);
                    if is_default {
                        let name = js_sys::Reflect::get(&dev, &JsValue::from_str("name"))
                            .ok().and_then(|v| v.as_string());
                        if let Some(ref n) = name {
                            let conn = if n.to_lowercase().contains("usb") {
                                "USB"
                            } else if n.to_lowercase().contains("bluetooth") || n.to_lowercase().contains("bt ") {
                                "Bluetooth"
                            } else {
                                "Internal"
                            };
                            state.mic_connection_type.set(Some(conn.to_string()));
                        }
                        state.mic_device_name.set(name);

                        if let Ok(ranges) = js_sys::Reflect::get(&dev, &JsValue::from_str("sample_rate_ranges")) {
                            let ranges = js_sys::Array::from(&ranges);
                            let mut max_rate: u32 = 0;
                            let mut format_str: Option<String> = None;
                            for j in 0..ranges.length() {
                                let range = ranges.get(j);
                                let rmax = js_sys::Reflect::get(&range, &JsValue::from_str("max"))
                                    .ok().and_then(|v| v.as_f64()).unwrap_or(0.0) as u32;
                                if rmax > max_rate {
                                    max_rate = rmax;
                                    format_str = js_sys::Reflect::get(&range, &JsValue::from_str("format"))
                                        .ok().and_then(|v| v.as_string());
                                }
                            }
                            if max_rate > 0 {
                                state.mic_sample_rate.set(max_rate);
                            }
                            if let Some(fmt) = format_str {
                                let bits: u16 = match fmt.as_str() {
                                    "I16" => 16, "I24" => 24, "I32" => 32, "F32" => 32,
                                    _ => 0,
                                };
                                if bits > 0 {
                                    state.mic_bits_per_sample.set(bits);
                                }
                            }
                        }
                        break;
                    }
                }
            }
        }
        Some(MicBackend::Browser) => {}
    }

    // Also check for USB devices to update usb_connected status
    if let Ok(devices) = tauri_invoke("plugin:usb-audio|listUsbDevices",
        &js_sys::Object::new().into()).await {
        let devices_arr = js_sys::Reflect::get(&devices, &JsValue::from_str("devices"))
            .ok()
            .map(|v| js_sys::Array::from(&v))
            .unwrap_or_default();
        let has_audio = (0..devices_arr.length()).any(|i| {
            let dev = devices_arr.get(i);
            js_sys::Reflect::get(&dev, &JsValue::from_str("isAudioDevice"))
                .ok().and_then(|v| v.as_bool()).unwrap_or(false)
        });
        state.mic_usb_connected.set(has_audio);
    }
}

/// Check for USB audio devices and update `mic_usb_connected` signal.
pub async fn check_usb_status(state: &AppState) {
    let devices_result = tauri_invoke("plugin:usb-audio|listUsbDevices",
        &js_sys::Object::new().into()).await;

    if let Ok(devices) = devices_result {
        let devices_arr = js_sys::Reflect::get(&devices, &JsValue::from_str("devices"))
            .ok()
            .map(|v| js_sys::Array::from(&v))
            .unwrap_or_default();

        for i in 0..devices_arr.length() {
            let dev = devices_arr.get(i);
            let is_audio = js_sys::Reflect::get(&dev, &JsValue::from_str("isAudioDevice"))
                .ok().and_then(|v| v.as_bool()).unwrap_or(false);
            if is_audio {
                let product_name = js_sys::Reflect::get(&dev, &JsValue::from_str("productName"))
                    .ok().and_then(|v| v.as_string()).unwrap_or_else(|| "USB Audio".into());
                state.mic_usb_connected.set(true);
                state.show_info_toast(format!("USB mic: {}", product_name));
                return;
            }
        }
    }

    state.mic_usb_connected.set(false);
}

// ── Backend resolution ──────────────────────────────────────────────────

/// Convert `state.mic_backend` to `ActiveBackend`.
fn resolve_active_backend(state: &AppState) -> Option<ActiveBackend> {
    state.mic_backend.get_untracked().map(ActiveBackend::from)
}

/// Open the appropriate mic backend based on a resolved MicBackend.
async fn open_backend(state: &AppState, backend: MicBackend) -> bool {
    ActiveBackend::from(backend).open(state).await
}

// ── Unified mic acquisition ─────────────────────────────────────────────

/// Unified mic acquisition. Called by both toggle_record and toggle_listen.
/// Returns the resolved MicBackend when the mic is ready, or None if the user
/// cancelled, permission was denied, or the mic failed to open.
pub async fn acquire_mic(state: &AppState, action: MicPendingAction) -> Option<MicBackend> {
    // If mic is already open and streaming, return current backend immediately
    if state.mic_acquisition_state.get_untracked() == MicAcquisitionState::Ready {
        if let Some(backend) = state.mic_backend.get_untracked() {
            let still_open = ActiveBackend::from(backend).is_open();
            if still_open {
                return Some(backend);
            }
            // Backend closed unexpectedly — fall through to re-acquire
            state.mic_acquisition_state.set(MicAcquisitionState::Idle);
        }
    }

    let strategy = state.mic_strategy.get_untracked();

    match strategy {
        MicStrategy::None => {
            state.log_debug("info", "acquire_mic: strategy=None, mic disabled");
            None
        }
        MicStrategy::Browser => {
            state.mic_acquisition_state.set(MicAcquisitionState::Acquiring);
            let t0 = js_sys::Date::now();
            if ActiveBackend::Browser.open(state).await {
                let elapsed = js_sys::Date::now() - t0;
                state.mic_permission_dialog_shown.set(elapsed > 1500.0);
                state.mic_backend.set(Some(MicBackend::Browser));
                state.mic_acquisition_state.set(MicAcquisitionState::Ready);
                Some(MicBackend::Browser)
            } else {
                state.mic_acquisition_state.set(MicAcquisitionState::Failed);
                state.mic_strategy.set(MicStrategy::Ask);
                state.mic_backend.set(None);
                state.mic_device_info.set(None);
                state.mic_selected_device.set(None);
                state.status_message.set(Some("Browser mic failed. Please choose a microphone.".into()));
                None
            }
        }
        MicStrategy::Selected => {
            if let Some(backend) = state.mic_backend.get_untracked() {
                state.mic_acquisition_state.set(MicAcquisitionState::Acquiring);
                let t0 = js_sys::Date::now();
                if open_backend(state, backend).await {
                    let elapsed = js_sys::Date::now() - t0;
                    state.mic_permission_dialog_shown.set(elapsed > 1500.0);
                    state.mic_acquisition_state.set(MicAcquisitionState::Ready);
                    return Some(backend);
                } else {
                    state.mic_strategy.set(MicStrategy::Ask);
                    state.mic_backend.set(None);
                    state.mic_device_info.set(None);
                    state.mic_selected_device.set(None);
                    state.mic_acquisition_state.set(MicAcquisitionState::Idle);
                    state.status_message.set(Some("Microphone failed. Please choose again.".into()));
                    return None;
                }
            }
            // No backend remembered despite Selected — fall back to Ask
            state.mic_strategy.set(MicStrategy::Ask);
            state.mic_pending_action.set(Some(action));
            state.mic_acquisition_state.set(MicAcquisitionState::AwaitingChoice);
            state.show_mic_chooser.set(true);
            None
        }
        MicStrategy::Ask => {
            state.mic_pending_action.set(Some(action));
            state.mic_acquisition_state.set(MicAcquisitionState::AwaitingChoice);
            state.show_mic_chooser.set(true);
            None
        }
    }
}

// ── Unified flows (private) ─────────────────────────────────────────────

/// Start recording with the given backend (mic already open).
async fn do_start_recording(state: &AppState, backend: ActiveBackend) {
    backend.clear_buffer();
    // Clear tile caches so previous file's spectrogram doesn't flash during recording
    crate::canvas::tile_cache::clear_all_caches();
    match backend.start_recording(state).await {
        Ok(()) => {
            state.mic_samples_recorded.set(0);
            state.mic_recording.set(true);
            state.mic_recording_start_time.set(Some(js_sys::Date::now()));
            let sr = state.mic_sample_rate.get_untracked();
            let file_idx = start_live_recording(state, sr);
            spawn_live_processing_loop(*state, file_idx, sr);
            spawn_smooth_scroll_animation(*state);
            log::info!("Recording started ({:?})", backend);
        }
        Err(e) => {
            log::error!("start_recording failed: {}", e);
            state.status_message.set(Some(format!("Failed to start recording: {}", e)));
        }
    }
}

/// Stop recording and finalize.
async fn do_stop_recording(state: &AppState, backend: ActiveBackend) {
    state.mic_recording.set(false);
    state.mic_recording_start_time.set(None);
    state.mic_samples_recorded.set(0);

    let bits_per_sample = state.mic_bits_per_sample.get_untracked();

    let result = backend.stop_recording(state).await;
    match result {
        StopResult::Samples { samples, sample_rate } => {
            finalize_recording(FinalizeParams {
                samples, sample_rate, bits_per_sample, is_float: false,
                saved_path: String::new(),
            }, *state);
        }
        StopResult::TauriResult(rec) => {
            finalize_recording(FinalizeParams {
                samples: rec.samples,
                sample_rate: rec.sample_rate,
                bits_per_sample: rec.bits_per_sample,
                is_float: rec.is_float,
                saved_path: rec.saved_path,
            }, *state);
        }
        StopResult::Empty => {
            log::warn!("No samples recorded");
            cleanup_failed_recording(state);
        }
        StopResult::Error(e) => {
            log::error!("stop_recording failed: {}", e);
            state.status_message.set(Some(format!("Recording failed: {}", e)));
            cleanup_failed_recording(state);
        }
    }

    backend.maybe_close(state).await;
}

/// Start listening with the given backend (mic already open).
async fn do_start_listening(state: &AppState, backend: ActiveBackend) {
    backend.set_listening(state, true).await;
    backend.clear_buffer();
    state.mic_listening.set(true);
    let sr = state.mic_sample_rate.get_untracked();
    spawn_live_processing_loop(*state, 0, sr);
    spawn_smooth_scroll_animation(*state);
}

/// Stop listening.
async fn do_stop_listening(state: &AppState, backend: ActiveBackend) {
    state.mic_listening.set(false);
    crate::canvas::live_waterfall::clear();
    backend.clear_buffer();
    backend.set_listening(state, false).await;
    backend.maybe_close(state).await;
}

// ── Public API ──────────────────────────────────────────────────────────

/// Toggle live HET listening on/off.
pub async fn toggle_listen(state: &AppState) {
    // If already listening, stop
    if state.mic_listening.get_untracked() {
        state.log_debug("info", "toggle_listen: stopping");
        if let Some(backend) = resolve_active_backend(state) {
            do_stop_listening(state, backend).await;
        } else {
            // Fallback: just clear signals
            state.mic_listening.set(false);
            crate::canvas::live_waterfall::clear();
        }
        return;
    }

    // Acquire mic (unified flow)
    let mic_backend = match acquire_mic(state, MicPendingAction::Listen).await {
        Some(b) => b,
        None => {
            state.log_debug("info", "toggle_listen: acquire_mic returned None (chooser shown or failed)");
            return;
        }
    };

    let backend = ActiveBackend::from(mic_backend);
    state.log_debug("info", format!("toggle_listen: backend={:?}, starting listen", backend));
    do_start_listening(state, backend).await;
}

/// Toggle recording on/off. When stopping, finalizes the recording.
pub async fn toggle_record(state: &AppState) {
    // If already recording, stop
    if state.mic_recording.get_untracked() {
        state.log_debug("info", "toggle_record: stopping");
        if let Some(backend) = resolve_active_backend(state) {
            do_stop_recording(state, backend).await;
        }
        return;
    }

    // If already listening, the mic is ready — go straight to recording
    if state.mic_listening.get_untracked() {
        if let Some(backend) = resolve_active_backend(state) {
            state.log_debug("info", format!("toggle_record: already listening, starting immediate with {:?}", backend));
            do_start_recording(state, backend).await;
            return;
        }
    }

    // Acquire mic (unified flow)
    let mic_backend = match acquire_mic(state, MicPendingAction::Record).await {
        Some(b) => b,
        None => {
            state.log_debug("info", "toggle_record: acquire_mic returned None (chooser shown or failed)");
            return;
        }
    };

    let backend = ActiveBackend::from(mic_backend);

    // If OS permission dialog was shown (detected by timing), skip our dialog
    if state.mic_permission_dialog_shown.get_untracked() {
        state.log_debug("info", format!("toggle_record: backend={:?}, permission dialog detected, starting immediately", backend));
        do_start_recording(state, backend).await;
    } else {
        // Show "Ready to record" dialog — user must confirm
        state.log_debug("info", format!("toggle_record: backend={:?}, showing Ready to Record dialog", backend));
        state.record_ready_state.set(crate::state::RecordReadyState::AwaitingConfirmation);
    }
}

/// Called by the "Ready to record" dialog's OK button.
pub async fn confirm_record_start(state: &AppState) {
    state.record_ready_state.set(crate::state::RecordReadyState::None);
    if let Some(backend) = resolve_active_backend(state) {
        do_start_recording(state, backend).await;
    }
}

/// Called by the "Ready to record" dialog's Cancel button.
pub fn cancel_record_start(state: &AppState) {
    state.record_ready_state.set(crate::state::RecordReadyState::None);
}

/// Stop both listening and recording, close mic.
pub fn stop_all(state: &AppState) {
    let backend = resolve_active_backend(state).or_else(|| {
        // Legacy: infer from what's open
        if ActiveBackend::RawUsb.is_open() {
            Some(ActiveBackend::RawUsb)
        } else if ActiveBackend::Cpal.is_open() {
            Some(ActiveBackend::Cpal)
        } else {
            None
        }
    });

    let state_copy = *state;
    let bits_per_sample = state.mic_bits_per_sample.get_untracked();

    match backend {
        Some(b) => {
            wasm_bindgen_futures::spawn_local(async move {
                if state_copy.mic_recording.get_untracked() {
                    state_copy.mic_recording.set(false);
                    state_copy.mic_recording_start_time.set(None);
                    state_copy.mic_samples_recorded.set(0);

                    let result = b.stop_recording(&state_copy).await;
                    match result {
                        StopResult::Samples { samples, sample_rate } => {
                            finalize_recording(FinalizeParams {
                                samples, sample_rate, bits_per_sample, is_float: false,
                                saved_path: String::new(),
                            }, state_copy);
                        }
                        StopResult::TauriResult(rec) => {
                            finalize_recording(FinalizeParams {
                                samples: rec.samples,
                                sample_rate: rec.sample_rate,
                                bits_per_sample: rec.bits_per_sample,
                                is_float: rec.is_float,
                                saved_path: rec.saved_path,
                            }, state_copy);
                        }
                        StopResult::Error(e) => {
                            log::error!("stop_recording failed: {}", e);
                            cleanup_failed_recording(&state_copy);
                        }
                        StopResult::Empty => {
                            cleanup_failed_recording(&state_copy);
                        }
                    }
                }
                state_copy.mic_listening.set(false);
                b.close(&state_copy).await;
                state_copy.mic_acquisition_state.set(MicAcquisitionState::Idle);
            });
        }
        None => {
            // No backend known — just clear state
            state.mic_listening.set(false);
            state.mic_recording.set(false);
            state.mic_recording_start_time.set(None);
            wasm_bindgen_futures::spawn_local(async move {
                ActiveBackend::Browser.close(&state_copy).await;
                state_copy.mic_acquisition_state.set(MicAcquisitionState::Idle);
            });
        }
    }
}

// Re-export from split modules
pub use crate::audio::wav_encoder::{encode_wav, download_wav};
pub(crate) use crate::audio::live_recording::{
    start_live_recording, spawn_live_processing_loop,
    spawn_smooth_scroll_animation, finalize_recording,
    cleanup_failed_recording,
};
