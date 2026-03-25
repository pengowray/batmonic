use leptos::prelude::*;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::AudioContext;
use crate::state::{AppState, FileSettings, LoadedFile, MicMode};
use crate::audio::source::InMemorySource;
use crate::types::{AudioData, FileMetadata, SpectrogramData};
use crate::dsp::fft::compute_preview;
use crate::dsp::heterodyne::RealtimeHet;
use std::cell::RefCell;
use std::sync::Arc;

// ── Thread-local state: Web Audio mode ──────────────────────────────────

thread_local! {
    static MIC_CTX: RefCell<Option<AudioContext>> = const { RefCell::new(None) };
    static MIC_STREAM: RefCell<Option<web_sys::MediaStream>> = const { RefCell::new(None) };
    static MIC_PROCESSOR: RefCell<Option<web_sys::ScriptProcessorNode>> = const { RefCell::new(None) };
    static MIC_BUFFER: RefCell<Vec<f32>> = const { RefCell::new(Vec::new()) };
    static MIC_HANDLER: RefCell<Option<Closure<dyn FnMut(web_sys::AudioProcessingEvent)>>> = RefCell::new(None);
    static RT_HET: RefCell<RealtimeHet> = RefCell::new(RealtimeHet::new());
}

// ── Thread-local state: Tauri native mode ───────────────────────────────

thread_local! {
    /// Whether the Tauri native mic is currently open
    static TAURI_MIC_OPEN: RefCell<bool> = const { RefCell::new(false) };
    /// AudioContext for HET playback (output only, no mic input)
    static HET_CTX: RefCell<Option<AudioContext>> = const { RefCell::new(None) };
    /// Next scheduled playback time for HET audio buffers
    static HET_NEXT_TIME: RefCell<f64> = const { RefCell::new(0.0) };
    /// Keep the event listener closure alive
    static TAURI_EVENT_CLOSURE: RefCell<Option<Closure<dyn FnMut(JsValue)>>> = RefCell::new(None);
    /// Unlisten function returned by Tauri event subscription
    static TAURI_UNLISTEN: RefCell<Option<js_sys::Function>> = const { RefCell::new(None) };
}

// ── Thread-local state: Live recording buffer (Tauri) ────────────────

thread_local! {
    /// Accumulated recording samples on the frontend for Tauri modes (cpal/USB).
    /// In browser mode, MIC_BUFFER serves this purpose instead.
    static TAURI_REC_BUFFER: RefCell<Vec<f32>> = const { RefCell::new(Vec::new()) };
}

/// Borrow the live recording buffer and call `f` with a reference to the samples.
/// Works for both web (MIC_BUFFER) and Tauri (TAURI_REC_BUFFER) modes.
pub fn with_live_samples<R>(is_tauri: bool, f: impl FnOnce(&[f32]) -> R) -> R {
    if is_tauri {
        TAURI_REC_BUFFER.with(|buf| f(&buf.borrow()))
    } else {
        MIC_BUFFER.with(|buf| f(&buf.borrow()))
    }
}

// ── Thread-local state: Raw USB mode ─────────────────────────────────

thread_local! {
    /// Whether the USB stream is currently open
    static USB_MIC_OPEN: RefCell<bool> = const { RefCell::new(false) };
    /// Keep the USB stream error event listener closure alive
    static USB_ERROR_CLOSURE: RefCell<Option<Closure<dyn FnMut(JsValue)>>> = RefCell::new(None);
}

// ── Tauri IPC helpers ───────────────────────────────────────────────────

use crate::tauri_bridge::{get_tauri_internals, tauri_invoke, tauri_invoke_no_args};

/// Request Android RECORD_AUDIO runtime permission via Tauri plugin.
/// Call this when the user selects "Browser" mic mode on Tauri (Android).
/// Returns true if granted, false if denied or not on Android.
pub async fn request_audio_permission_tauri(state: &AppState) -> bool {
    if !state.is_tauri {
        return true; // Not needed on web
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
    // Result is Vec<DeviceInfo>; find the default device's rates
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
        // Parse sample_rate_ranges to collect supported rates
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
    let mode = state.mic_mode.get_untracked();
    let effective = if mode == MicMode::Auto {
        state.mic_effective_mode.get_untracked()
    } else {
        mode
    };

    match effective {
        MicMode::RawUsb => {
            // Query USB device info
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
        MicMode::Cpal | MicMode::Auto => {
            // Query cpal default device info
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

                        // Extract native sample rate and format from the device's supported ranges
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
                            // Parse format string to set bit depth
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
        _ => {}
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

/// Resolve Auto mode to either RawUsb or Cpal based on USB device availability.
/// Requests USB permission proactively if a USB audio device is found.
/// Returns `None` when USB is present but permission was denied (caller should not proceed).
pub async fn resolve_auto_mode(state: &AppState) -> Option<MicMode> {
    state.log_debug("info", "resolve_auto_mode: checking for USB devices...");

    // Check for USB audio devices
    let devices_result = tauri_invoke("plugin:usb-audio|listUsbDevices",
        &js_sys::Object::new().into()).await;

    if let Err(ref e) = devices_result {
        state.log_debug("warn", format!("resolve_auto_mode: listUsbDevices failed: {}", e));
    }

    if let Ok(devices) = devices_result {
        let devices_arr = js_sys::Reflect::get(&devices, &JsValue::from_str("devices"))
            .ok()
            .map(|v| js_sys::Array::from(&v))
            .unwrap_or_default();

        state.log_debug("info", format!("resolve_auto_mode: found {} USB device(s)", devices_arr.length()));

        for i in 0..devices_arr.length() {
            let dev = devices_arr.get(i);
            let is_audio = js_sys::Reflect::get(&dev, &JsValue::from_str("isAudioDevice"))
                .ok().and_then(|v| v.as_bool()).unwrap_or(false);
            if !is_audio {
                state.log_debug("info", format!("resolve_auto_mode: device {} is not audio, skipping", i));
                continue;
            }

            let device_name = js_sys::Reflect::get(&dev, &JsValue::from_str("deviceName"))
                .ok().and_then(|v| v.as_string());
            let product_name = js_sys::Reflect::get(&dev, &JsValue::from_str("productName"))
                .ok().and_then(|v| v.as_string()).unwrap_or_else(|| "USB Audio".into());
            let has_permission = js_sys::Reflect::get(&dev, &JsValue::from_str("hasPermission"))
                .ok().and_then(|v| v.as_bool()).unwrap_or(false);

            state.log_debug("info", format!("resolve_auto_mode: audio device '{}', has_permission={}", product_name, has_permission));
            state.mic_usb_connected.set(true);

            if has_permission {
                state.mic_effective_mode.set(MicMode::RawUsb);
                state.mic_needs_permission.set(false);
                state.show_info_toast(format!("USB mic: {}", product_name));
                return Some(MicMode::RawUsb);
            }

            // Request permission proactively
            if let Some(ref dev_name) = device_name {
                state.log_debug("info", format!("resolve_auto_mode: requesting USB permission for '{}'...", dev_name));
                let perm_args = js_sys::Object::new();
                js_sys::Reflect::set(&perm_args, &JsValue::from_str("deviceName"),
                    &JsValue::from_str(dev_name)).ok();
                match tauri_invoke("plugin:usb-audio|requestUsbPermission", &perm_args.into()).await {
                    Ok(result) => {
                        let granted = js_sys::Reflect::get(&result, &JsValue::from_str("granted"))
                            .ok().and_then(|v| v.as_bool()).unwrap_or(false);
                        if granted {
                            state.log_debug("info", "resolve_auto_mode: USB permission granted");
                            state.mic_effective_mode.set(MicMode::RawUsb);
                            state.mic_needs_permission.set(false);
                            state.show_info_toast(format!("USB mic: {}", product_name));
                            return Some(MicMode::RawUsb);
                        } else {
                            state.log_debug("warn", "resolve_auto_mode: USB permission denied, not falling back");
                            state.show_error_toast("Raw USB mic permission denied");
                            state.mic_effective_mode.set(MicMode::RawUsb);
                            state.mic_needs_permission.set(true);
                            return None;
                        }
                    }
                    Err(e) => {
                        state.log_debug("error", format!("resolve_auto_mode: USB permission request failed: {}", e));
                        state.show_error_toast("USB permission request failed");
                        state.mic_effective_mode.set(MicMode::RawUsb);
                        state.mic_needs_permission.set(true);
                        return None;
                    }
                }
            } else {
                state.log_debug("warn", "resolve_auto_mode: no deviceName, cannot request permission");
                state.mic_effective_mode.set(MicMode::RawUsb);
                state.mic_needs_permission.set(true);
                return None;
            }
        }
    }

    // No USB audio device found — fall back to native audio
    state.log_debug("info", "resolve_auto_mode: no USB audio device, falling back to Cpal (native audio)");
    state.mic_usb_connected.set(false);
    state.mic_needs_permission.set(false);
    state.mic_effective_mode.set(MicMode::Cpal);
    Some(MicMode::Cpal)
}

/// Check for USB devices and permission state WITHOUT requesting permission.
/// Sets `mic_effective_mode` and `mic_needs_permission` accordingly.
/// Used at startup and on hotplug to avoid showing permission dialog unprompted.
pub async fn check_auto_mode_no_request(state: &AppState) -> MicMode {
    state.log_debug("info", "check_auto_mode_no_request: checking USB devices...");

    let devices_result = tauri_invoke("plugin:usb-audio|listUsbDevices",
        &js_sys::Object::new().into()).await;

    if let Err(ref e) = devices_result {
        state.log_debug("warn", format!("check_auto_mode_no_request: listUsbDevices failed: {}", e));
    }

    if let Ok(devices) = devices_result {
        let devices_arr = js_sys::Reflect::get(&devices, &JsValue::from_str("devices"))
            .ok()
            .map(|v| js_sys::Array::from(&v))
            .unwrap_or_default();

        for i in 0..devices_arr.length() {
            let dev = devices_arr.get(i);
            let is_audio = js_sys::Reflect::get(&dev, &JsValue::from_str("isAudioDevice"))
                .ok().and_then(|v| v.as_bool()).unwrap_or(false);
            if !is_audio { continue; }

            let product_name = js_sys::Reflect::get(&dev, &JsValue::from_str("productName"))
                .ok().and_then(|v| v.as_string()).unwrap_or_else(|| "USB Audio".into());
            let has_permission = js_sys::Reflect::get(&dev, &JsValue::from_str("hasPermission"))
                .ok().and_then(|v| v.as_bool()).unwrap_or(false);

            state.mic_usb_connected.set(true);

            if has_permission {
                state.log_debug("info", format!("check_auto_mode_no_request: '{}' already permitted", product_name));
                state.mic_effective_mode.set(MicMode::RawUsb);
                state.mic_needs_permission.set(false);
                state.show_info_toast(format!("USB mic: {}", product_name));
                return MicMode::RawUsb;
            } else {
                state.log_debug("info", format!("check_auto_mode_no_request: '{}' needs permission", product_name));
                state.mic_effective_mode.set(MicMode::RawUsb);
                state.mic_needs_permission.set(true);
                state.show_info_toast(format!("USB mic detected: {}", product_name));
                return MicMode::RawUsb;
            }
        }
    }

    state.mic_usb_connected.set(false);
    state.mic_needs_permission.set(false);
    state.mic_effective_mode.set(MicMode::Cpal);
    MicMode::Cpal
}

/// Subscribe to a Tauri event, storing the closure in thread-local state.
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

    // Store the closure so it's not dropped
    TAURI_EVENT_CLOSURE.with(|c| *c.borrow_mut() = Some(callback));

    Some(())
}

/// Subscribe to a USB stream error event, using a separate thread-local from tauri_listen.
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

// ── Web Audio mode (existing implementation) ────────────────────────────

fn web_mic_is_open() -> bool {
    MIC_CTX.with(|c| c.borrow().is_some())
}

async fn ensure_mic_open_web(state: &AppState) -> bool {
    if web_mic_is_open() {
        return true;
    }

    state.log_debug("info", "ensure_mic_open_web: opening browser mic...");

    let window = match web_sys::window() {
        Some(w) => w,
        None => {
            state.log_debug("error", "ensure_mic_open_web: no window object");
            return false;
        }
    };
    let navigator = window.navigator();
    let media_devices = match navigator.media_devices() {
        Ok(md) => md,
        Err(e) => {
            state.log_debug("error", format!("ensure_mic_open_web: no media devices: {:?}", e));
            state.status_message.set(Some("Microphone not available on this device".into()));
            return false;
        }
    };

    let constraints = web_sys::MediaStreamConstraints::new();
    // Disable browser audio processing that destroys non-speech signals
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

    state.log_debug("info", "ensure_mic_open_web: calling getUserMedia...");
    let stream_js = match JsFuture::from(promise).await {
        Ok(s) => {
            state.log_debug("info", "ensure_mic_open_web: getUserMedia succeeded");
            s
        }
        Err(e) => {
            state.log_debug("error", format!("ensure_mic_open_web: getUserMedia denied: {:?}", e));
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

    // Resume context in case it started suspended (async gap breaks user gesture chain)
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

    RT_HET.with(|h| h.borrow_mut().reset());

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
            RT_HET.with(|h| {
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

fn close_mic_web(state: &AppState) {
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
    RT_HET.with(|h| h.borrow_mut().reset());
    crate::canvas::live_waterfall::clear();

    state.mic_samples_recorded.set(0);
    // Don't clear mic info signals — persist for settings display
    log::info!("Web mic closed");
}

fn maybe_close_mic_web(state: &AppState) {
    if !state.mic_listening.get_untracked() && !state.mic_recording.get_untracked() {
        close_mic_web(state);
    }
}

fn stop_recording_web(state: &AppState) -> Option<(Vec<f32>, u32)> {
    state.mic_recording.set(false);
    state.mic_recording_start_time.set(None);
    let sample_rate = state.mic_sample_rate.get_untracked();
    let samples = MIC_BUFFER.with(|buf| std::mem::take(&mut *buf.borrow_mut()));
    state.mic_samples_recorded.set(0);

    if samples.is_empty() || sample_rate == 0 {
        log::warn!("No samples recorded");
        return None;
    }

    log::info!("Recording stopped: {} samples ({:.2}s at {} Hz)",
        samples.len(), samples.len() as f64 / sample_rate as f64, sample_rate);
    Some((samples, sample_rate))
}

// ── Tauri native mode ───────────────────────────────────────────────────

fn tauri_mic_is_open() -> bool {
    TAURI_MIC_OPEN.with(|o| *o.borrow())
}

/// Open the mic via cpal in the Tauri backend.
async fn ensure_mic_open_tauri(state: &AppState) -> bool {
    if tauri_mic_is_open() {
        return true;
    }

    let max_sr = state.mic_max_sample_rate.get_untracked();
    let selected_device = state.mic_selected_device.get_untracked();
    let args = js_sys::Object::new();
    if max_sr > 0 {
        js_sys::Reflect::set(
            &args,
            &JsValue::from_str("maxSampleRate"),
            &JsValue::from_f64(max_sr as f64),
        )
        .ok();
    }
    if let Some(ref name) = selected_device {
        js_sys::Reflect::set(
            &args,
            &JsValue::from_str("deviceName"),
            &JsValue::from_str(name),
        )
        .ok();
    }
    let result = match tauri_invoke("mic_open", &args.into()).await {
        Ok(v) => v,
        Err(e) => {
            log::warn!("Native mic failed ({}), falling back to Web Audio", e);
            state.status_message.set(Some(format!("Native mic unavailable: {}", e)));
            return ensure_mic_open_web(state).await;
        }
    };

    // Parse MicInfo from the response
    let sample_rate = js_sys::Reflect::get(&result, &JsValue::from_str("sample_rate"))
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(48000.0) as u32;
    let bits_per_sample = js_sys::Reflect::get(&result, &JsValue::from_str("bits_per_sample"))
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(16.0) as u16;
    let device_name = js_sys::Reflect::get(&result, &JsValue::from_str("device_name"))
        .ok()
        .and_then(|v| v.as_string())
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
    // Classify connection type from device name
    let conn_type = if device_name.to_lowercase().contains("usb") {
        "USB"
    } else if device_name.to_lowercase().contains("bluetooth") || device_name.to_lowercase().contains("bt ") {
        "Bluetooth"
    } else {
        "Internal"
    };
    state.mic_connection_type.set(Some(conn_type.to_string()));

    // Setup HET playback AudioContext (output only)
    let het_ctx = match AudioContext::new() {
        Ok(c) => c,
        Err(e) => {
            log::error!("Failed to create HET AudioContext: {:?}", e);
            state.status_message.set(Some("Failed to initialize audio output".into()));
            return false;
        }
    };
    // Resume context in case it started suspended (async gap breaks user gesture chain)
    if let Ok(promise) = het_ctx.resume() {
        let _ = JsFuture::from(promise).await;
    }
    HET_CTX.with(|c| *c.borrow_mut() = Some(het_ctx));
    HET_NEXT_TIME.with(|t| *t.borrow_mut() = 0.0);
    RT_HET.with(|h| h.borrow_mut().reset());

    // Setup event listener for audio chunks from the backend
    let state_cb = *state;
    let chunk_handler = Closure::<dyn FnMut(JsValue)>::new(move |event: JsValue| {
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
            TAURI_REC_BUFFER.with(|buf| buf.borrow_mut().extend_from_slice(&input_data));
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
            RT_HET.with(|h| {
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
    });

    tauri_listen("mic-audio-chunk", chunk_handler);

    TAURI_MIC_OPEN.with(|o| *o.borrow_mut() = true);
    log::info!("Native mic opened: {} at {} Hz, {}-bit", device_name, sample_rate, bits_per_sample);
    true
}

async fn close_mic_tauri(state: &AppState) {
    // Tell backend to stop streaming and close mic
    if let Err(e) = tauri_invoke_no_args("mic_close").await {
        log::error!("mic_close failed: {}", e);
    }

    // Clean up event listener
    TAURI_EVENT_CLOSURE.with(|c| { c.borrow_mut().take(); });
    TAURI_UNLISTEN.with(|u| { u.borrow_mut().take(); });

    // Close HET playback context
    HET_CTX.with(|c| {
        if let Some(ctx) = c.borrow_mut().take() {
            let _ = ctx.close();
        }
    });

    RT_HET.with(|h| h.borrow_mut().reset());
    TAURI_MIC_OPEN.with(|o| *o.borrow_mut() = false);
    TAURI_REC_BUFFER.with(|buf| buf.borrow_mut().clear());
    crate::canvas::live_waterfall::clear();

    state.mic_samples_recorded.set(0);
    // Don't clear mic info signals — persist for settings display
    log::info!("Native mic closed");
}

async fn maybe_close_mic_tauri(state: &AppState) {
    if !state.mic_listening.get_untracked() && !state.mic_recording.get_untracked() {
        close_mic_tauri(state).await;
    }
}

/// Toggle listening in Tauri mode.
async fn toggle_listen_tauri(state: &AppState) {
    if state.mic_listening.get_untracked() {
        state.mic_listening.set(false);
        // Waterfall + processing loop will self-stop via the is_listening check
        crate::canvas::live_waterfall::clear();
        TAURI_REC_BUFFER.with(|buf| buf.borrow_mut().clear());
        // Tell backend to stop streaming audio chunks
        let args = js_sys::Object::new();
        js_sys::Reflect::set(&args, &"listening".into(), &JsValue::FALSE).ok();
        let _ = tauri_invoke("mic_set_listening", &args.into()).await;
        maybe_close_mic_tauri(state).await;
    } else if ensure_mic_open_tauri(state).await {
        // Tell backend to start streaming audio chunks
        let args = js_sys::Object::new();
        js_sys::Reflect::set(&args, &"listening".into(), &JsValue::TRUE).ok();
        let _ = tauri_invoke("mic_set_listening", &args.into()).await;
        state.mic_listening.set(true);
        // Start live waterfall processing loop for listening
        TAURI_REC_BUFFER.with(|buf| buf.borrow_mut().clear());
        let sr = state.mic_sample_rate.get_untracked();
        // file_index 0 is a dummy — listening doesn't create a file
        spawn_live_processing_loop(*state, 0, sr);
        spawn_smooth_scroll_animation(*state);
    }
}

/// Toggle recording in Tauri mode.
async fn toggle_record_tauri(state: &AppState) {
    if state.mic_recording.get_untracked() {
        // Stop recording
        state.mic_recording.set(false);
        state.mic_recording_start_time.set(None);
        state.mic_samples_recorded.set(0);

        match tauri_invoke_no_args("mic_stop_recording").await {
            Ok(result) => {
                finalize_recording_tauri(result, *state);
            }
            Err(e) => {
                log::error!("mic_stop_recording failed: {}", e);
                state.status_message.set(Some(format!("Recording failed: {}", e)));
            }
        }

        maybe_close_mic_tauri(state).await;
    } else {
        // Start recording
        if ensure_mic_open_tauri(state).await {
            TAURI_REC_BUFFER.with(|buf| buf.borrow_mut().clear());
            match tauri_invoke_no_args("mic_start_recording").await {
                Ok(_) => {
                    state.mic_samples_recorded.set(0);
                    state.mic_recording.set(true);
                    state.mic_recording_start_time.set(Some(js_sys::Date::now()));
                    let sr = state.mic_sample_rate.get_untracked();
                    let file_idx = start_live_recording(state, sr);
                    spawn_live_processing_loop(*state, file_idx, sr);
                    spawn_smooth_scroll_animation(*state);
                    log::info!("Native recording started");
                }
                Err(e) => {
                    log::error!("mic_start_recording failed: {}", e);
                    state.status_message.set(Some(format!("Failed to start recording: {}", e)));
                }
            }
        }
    }
}

/// Stop all in Tauri mode.
async fn stop_all_tauri(state: &AppState) {
    if state.mic_recording.get_untracked() {
        state.mic_recording.set(false);
        state.mic_recording_start_time.set(None);
        match tauri_invoke_no_args("mic_stop_recording").await {
            Ok(result) => {
                finalize_recording_tauri(result, *state);
            }
            Err(e) => {
                log::error!("mic_stop_recording failed: {}", e);
            }
        }
    }
    state.mic_listening.set(false);
    close_mic_tauri(state).await;
}

/// Build a LoadedFile from the Tauri RecordingResult and add to state.
/// If a live file exists (from live visualization), updates it in-place.
fn finalize_recording_tauri(result: JsValue, state: AppState) {
    use crate::canvas::{spectral_store, tile_cache};

    let filename = js_sys::Reflect::get(&result, &JsValue::from_str("filename"))
        .ok()
        .and_then(|v| v.as_string())
        .unwrap_or_else(|| "recording.wav".into());
    let sample_rate = js_sys::Reflect::get(&result, &JsValue::from_str("sample_rate"))
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(48000.0) as u32;
    let bits_per_sample = js_sys::Reflect::get(&result, &JsValue::from_str("bits_per_sample"))
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(16.0) as u16;
    let is_float = js_sys::Reflect::get(&result, &JsValue::from_str("is_float"))
        .ok()
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let duration_secs = js_sys::Reflect::get(&result, &JsValue::from_str("duration_secs"))
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let saved_path = js_sys::Reflect::get(&result, &JsValue::from_str("saved_path"))
        .ok()
        .and_then(|v| v.as_string())
        .unwrap_or_default();

    // Extract f32 samples for frontend display
    let samples_js = js_sys::Reflect::get(&result, &JsValue::from_str("samples_f32"))
        .unwrap_or(JsValue::NULL);
    let samples_array = js_sys::Array::from(&samples_js);
    let samples: Vec<f32> = (0..samples_array.length())
        .map(|i| samples_array.get(i).as_f64().unwrap_or(0.0) as f32)
        .collect();

    if samples.is_empty() {
        log::warn!("No samples in recording result");
        return;
    }

    log::info!("Native recording: {} samples ({:.2}s at {} Hz, {}-bit{}), saved to {}",
        samples.len(), duration_secs, sample_rate, bits_per_sample,
        if is_float { " float" } else { "" }, saved_path);

    // Build GUANO metadata for display in metadata panel
    let guano = {
        use crate::audio::guano::GuanoMetadata;
        let now = js_sys::Date::new_0();
        let start_ms = now.get_time() - (duration_secs * 1000.0);
        let start = js_sys::Date::new(&JsValue::from_f64(start_ms));
        let timestamp = format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
            start.get_full_year(), start.get_month() + 1, start.get_date(),
            start.get_hours(), start.get_minutes(), start.get_seconds(),
        );
        let version = env!("CARGO_PKG_VERSION");
        let mut g = GuanoMetadata::new();
        g.add("GUANO|Version", "1.0");
        g.add("Timestamp", &timestamp);
        g.add("Length", &format!("{:.6}", duration_secs));
        g.add("Samplerate", &sample_rate.to_string());
        g.add("Make", "Oversample");
        g.add("Firmware Version", version);
        g.add("Original Filename", &filename);
        g
    };

    let samples: Arc<Vec<f32>> = samples.into();
    let source = Arc::new(InMemorySource {
        samples: samples.clone(),
        raw_samples: None,
        sample_rate,
        channels: 1,
    });
    let audio = AudioData {
        samples,
        source,
        sample_rate,
        channels: 1,
        duration_secs,
        metadata: FileMetadata {
            file_size: 0,
            format: "REC",
            bits_per_sample,
            is_float,
            guano: Some(guano),
            data_offset: None,
            data_size: None,
        },
    };

    let preview = compute_preview(&audio, 256, 128);
    let audio_for_stft = audio.clone();

    // Check if a live file exists from live visualization
    let live_idx = state.mic_live_file_idx.get_untracked();
    state.mic_live_file_idx.set(None);

    let file_index;
    let name_check;

    if let Some(idx) = live_idx {
        // Update the existing live file in-place
        file_index = idx;
        name_check = state.files.with_untracked(|files| {
            files.get(idx).map(|f| f.name.clone()).unwrap_or_default()
        });

        // Clear progressive tiles and spectral store
        tile_cache::clear_file(file_index);
        spectral_store::clear_file(file_index);

        state.files.update(|files| {
            if let Some(f) = files.get_mut(file_index) {
                f.audio = audio;
                f.preview = Some(preview);
                f.is_recording = false; // Already saved by backend
            }
        });
    } else {
        // No live file — create a new one (fallback)
        name_check = filename.clone();
        let total_cols = if audio_for_stft.samples.len() >= 2048 {
            (audio_for_stft.samples.len() - 2048) / 512 + 1
        } else {
            0
        };
        let placeholder_spec = SpectrogramData {
            columns: Vec::new().into(),
            total_columns: total_cols,
            freq_resolution: sample_rate as f64 / 2048.0,
            time_resolution: 512.0 / sample_rate as f64,
            max_freq: sample_rate as f64 / 2.0,
            sample_rate,
        };

        let mut idx = 0;
        state.files.update(|files| {
            idx = files.len();
            files.push(LoadedFile {
                name: filename,
                audio,
                spectrogram: placeholder_spec,
                preview: Some(preview),
                overview_image: None,
                xc_metadata: None,
                xc_hashes: None,
                is_recording: false, // Already saved by backend
                settings: FileSettings::default(),
                add_order: idx,
                last_modified_ms: None,
                identity: None,
                file_handle: None,
                cached_peak_db: None,
                cached_full_peak_db: None,
                read_only: false,
                had_sidecar: false,
            });
        });
        file_index = idx;
        state.current_file_index.set(Some(file_index));
    }

    // Set file handle to saved path for hash computation, and compute Layer 1 identity
    if !saved_path.is_empty() {
        state.files.update(|files| {
            if let Some(f) = files.get_mut(file_index) {
                f.file_handle = Some(crate::audio::streaming_source::FileHandle::TauriPath(saved_path.clone()));
            }
        });
    }
    let num_samples_est = (duration_secs * sample_rate as f64).ceil() as u64;
    let estimated_size = 44 + num_samples_est * (bits_per_sample as u64 / 8);
    crate::file_identity::start_identity_computation(
        state, file_index, name_check.clone(), estimated_size, None,
        None, None, None,
    );

    // Zoom to fit the entire recording
    let canvas_w = state.spectrogram_canvas_width.get_untracked();
    let final_time_res = 512.0 / sample_rate as f64;
    state.zoom_level.set(crate::viewport::fit_zoom(canvas_w, final_time_res, duration_secs));
    state.scroll_offset.set(0.0);

    // Async chunked spectrogram computation with final normalization
    spawn_spectrogram_computation(audio_for_stft, name_check, file_index, state);
}

// ── Raw USB mode ────────────────────────────────────────────────────────

fn usb_mic_is_open() -> bool {
    USB_MIC_OPEN.with(|o| *o.borrow())
}

/// Open the USB mic: enumerate devices, request permission, open device, start stream.
async fn ensure_mic_open_usb(state: &AppState) -> bool {
    if usb_mic_is_open() {
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

    // Find first audio device
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

    // Step 3: Open device via Kotlin plugin (returns fd + endpoint info)
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

    // Parse the response
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
            // Close the Kotlin connection on failure
            let _ = tauri_invoke("plugin:usb-audio|closeUsbDevice",
                &js_sys::Object::new().into()).await;
            return false;
        }
    }

    state.mic_sample_rate.set(sample_rate);
    // Parse bit depth from USB device info if available, default to 16
    let usb_bits = js_sys::Reflect::get(&device_info, &JsValue::from_str("bitDepth"))
        .ok().and_then(|v| v.as_f64()).unwrap_or(16.0) as u16;
    state.mic_bits_per_sample.set(usb_bits);

    // Setup HET playback AudioContext (same as cpal mode)
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
    RT_HET.with(|h| h.borrow_mut().reset());

    // Setup event listener for audio chunks (same event as cpal mode)
    let state_cb = *state;
    let chunk_handler = Closure::<dyn FnMut(JsValue)>::new(move |event: JsValue| {
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

        // Accumulate samples for live waterfall during recording OR listening
        if state_cb.mic_recording.get_untracked() || state_cb.mic_listening.get_untracked() {
            TAURI_REC_BUFFER.with(|buf| buf.borrow_mut().extend_from_slice(&input_data));
            if state_cb.mic_recording.get_untracked() {
                state_cb.mic_samples_recorded.update(|n| *n += len);
            }
        }

        if state_cb.mic_listening.get_untracked() {
            let sr = state_cb.mic_sample_rate.get_untracked();
            let het_freq = state_cb.listen_het_frequency.get_untracked();
            let het_cutoff = state_cb.listen_het_cutoff.get_untracked();
            let mut out_data = vec![0.0f32; len];
            RT_HET.with(|h| {
                h.borrow_mut().process(&input_data, &mut out_data, sr, het_freq, het_cutoff);
            });

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
    });

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
        state_err.mic_effective_mode.set(MicMode::Cpal);

        USB_MIC_OPEN.with(|o| *o.borrow_mut() = false);

        // Finalize any in-progress recording with whatever samples we have
        if was_recording {
            let sr = state_err.mic_sample_rate.get_untracked();
            let samples = TAURI_REC_BUFFER.with(|buf| std::mem::take(&mut *buf.borrow_mut()));
            if !samples.is_empty() && sr > 0 {
                finalize_live_recording(samples, sr, state_err);
            }
        }

        // Clean up HET context
        HET_CTX.with(|c| {
            if let Some(ctx) = c.borrow_mut().take() {
                let _ = ctx.close();
            }
        });
        RT_HET.with(|h| h.borrow_mut().reset());
        TAURI_REC_BUFFER.with(|buf| buf.borrow_mut().clear());
    });
    tauri_listen_usb_error("usb-stream-error", error_handler);

    USB_MIC_OPEN.with(|o| *o.borrow_mut() = true);
    state.mic_device_name.set(Some(product_name.clone()));
    state.mic_connection_type.set(Some("USB (Raw)".to_string()));
    log::info!("USB mic opened: {} at {} Hz", product_name, sample_rate);
    true
}

async fn close_mic_usb(state: &AppState) {
    // Stop USB stream in Rust backend
    if let Err(e) = tauri_invoke_no_args("usb_stop_stream").await {
        log::error!("usb_stop_stream failed: {}", e);
    }

    // Close USB device in Kotlin
    let _ = tauri_invoke("plugin:usb-audio|closeUsbDevice",
        &js_sys::Object::new().into()).await;

    // Clean up event listeners
    TAURI_EVENT_CLOSURE.with(|c| { c.borrow_mut().take(); });
    TAURI_UNLISTEN.with(|u| { u.borrow_mut().take(); });
    USB_ERROR_CLOSURE.with(|c| { c.borrow_mut().take(); });

    // Close HET playback context
    HET_CTX.with(|c| {
        if let Some(ctx) = c.borrow_mut().take() {
            let _ = ctx.close();
        }
    });

    RT_HET.with(|h| h.borrow_mut().reset());
    USB_MIC_OPEN.with(|o| *o.borrow_mut() = false);
    TAURI_REC_BUFFER.with(|buf| buf.borrow_mut().clear());
    crate::canvas::live_waterfall::clear();

    state.mic_samples_recorded.set(0);
    // Don't clear mic info signals — persist for settings display
    log::info!("USB mic closed");
}

async fn maybe_close_mic_usb(state: &AppState) {
    if !state.mic_listening.get_untracked() && !state.mic_recording.get_untracked() {
        close_mic_usb(state).await;
    }
}

async fn toggle_listen_usb(state: &AppState) {
    if state.mic_listening.get_untracked() {
        state.mic_listening.set(false);
        crate::canvas::live_waterfall::clear();
        TAURI_REC_BUFFER.with(|buf| buf.borrow_mut().clear());
        maybe_close_mic_usb(state).await;
    } else if ensure_mic_open_usb(state).await {
        TAURI_REC_BUFFER.with(|buf| buf.borrow_mut().clear());
        state.mic_listening.set(true);
        let sr = state.mic_sample_rate.get_untracked();
        spawn_live_processing_loop(*state, 0, sr);
        spawn_smooth_scroll_animation(*state);
    }
}

async fn toggle_record_usb(state: &AppState) {
    if state.mic_recording.get_untracked() {
        // Stop recording
        state.mic_recording.set(false);
        state.mic_recording_start_time.set(None);
        state.mic_samples_recorded.set(0);

        match tauri_invoke_no_args("usb_stop_recording").await {
            Ok(result) => {
                finalize_recording_tauri(result, *state);
            }
            Err(e) => {
                log::error!("usb_stop_recording failed: {}", e);
                state.status_message.set(Some(format!("Recording failed: {}", e)));
            }
        }

        maybe_close_mic_usb(state).await;
    } else {
        // Start recording
        if ensure_mic_open_usb(state).await {
            TAURI_REC_BUFFER.with(|buf| buf.borrow_mut().clear());
            match tauri_invoke_no_args("usb_start_recording").await {
                Ok(_) => {
                    state.mic_samples_recorded.set(0);
                    state.mic_recording.set(true);
                    state.mic_recording_start_time.set(Some(js_sys::Date::now()));
                    let sr = state.mic_sample_rate.get_untracked();
                    let file_idx = start_live_recording(state, sr);
                    spawn_live_processing_loop(*state, file_idx, sr);
                    spawn_smooth_scroll_animation(*state);
                    log::info!("USB recording started");
                }
                Err(e) => {
                    log::error!("usb_start_recording failed: {}", e);
                    state.status_message.set(Some(format!("Failed to start recording: {}", e)));
                }
            }
        }
    }
}

async fn stop_all_usb(state: &AppState) {
    if state.mic_recording.get_untracked() {
        state.mic_recording.set(false);
        state.mic_recording_start_time.set(None);
        match tauri_invoke_no_args("usb_stop_recording").await {
            Ok(result) => {
                finalize_recording_tauri(result, *state);
            }
            Err(e) => {
                log::error!("usb_stop_recording failed: {}", e);
            }
        }
    }
    state.mic_listening.set(false);
    close_mic_usb(state).await;
}

// ── Public API (routes by mic_mode) ─────────────────────────────────────

/// Resolve the effective mic mode, handling Auto by resolving on first use.
/// Returns `None` when USB is present but permission was denied (caller should not proceed).
async fn effective_mode(state: &AppState) -> Option<MicMode> {
    let mode = state.mic_mode.get_untracked();
    if mode == MicMode::Auto && state.is_tauri {
        // If a mic is already open, use whichever backend is active
        if usb_mic_is_open() {
            return Some(MicMode::RawUsb);
        }
        if tauri_mic_is_open() {
            return Some(MicMode::Cpal);
        }
        // Resolve: check for USB device, request permission; None if denied
        resolve_auto_mode(state).await
    } else {
        Some(mode)
    }
}

/// Toggle live HET listening on/off.
pub async fn toggle_listen(state: &AppState) {
    let mode = match effective_mode(state).await {
        Some(m) => m,
        None => {
            state.log_debug("info", "toggle_listen: permission denied, aborting");
            return;
        }
    };
    state.log_debug("info", format!("toggle_listen: mode={:?}, listening={}", mode, state.mic_listening.get_untracked()));
    match mode {
        MicMode::RawUsb if state.is_tauri => toggle_listen_usb(state).await,
        MicMode::Cpal if state.is_tauri => toggle_listen_tauri(state).await,
        _ => {
            // Browser mode or non-Tauri fallback
            if state.mic_listening.get_untracked() {
                state.mic_listening.set(false);
                crate::canvas::live_waterfall::clear();
                MIC_BUFFER.with(|buf| buf.borrow_mut().clear());
                maybe_close_mic_web(state);
            } else if ensure_mic_open_web(state).await {
                MIC_BUFFER.with(|buf| buf.borrow_mut().clear());
                state.mic_listening.set(true);
                // Start live waterfall processing loop for listening
                let sr = state.mic_sample_rate.get_untracked();
                spawn_live_processing_loop(*state, 0, sr);
                spawn_smooth_scroll_animation(*state);
            }
        }
    }
}

/// Toggle recording on/off. When stopping, finalizes the recording.
pub async fn toggle_record(state: &AppState) {
    let mode = match effective_mode(state).await {
        Some(m) => m,
        None => {
            state.log_debug("info", "toggle_record: permission denied, aborting");
            return;
        }
    };
    state.log_debug("info", format!("toggle_record: mode={:?}, recording={}", mode, state.mic_recording.get_untracked()));
    match mode {
        MicMode::RawUsb if state.is_tauri => toggle_record_usb(state).await,
        MicMode::Cpal if state.is_tauri => toggle_record_tauri(state).await,
        _ => {
            // Browser mode or non-Tauri fallback
            if state.mic_recording.get_untracked() {
                if let Some((samples, sr)) = stop_recording_web(state) {
                    finalize_live_recording(samples, sr, *state);
                }
                maybe_close_mic_web(state);
            } else if ensure_mic_open_web(state).await {
                MIC_BUFFER.with(|buf| buf.borrow_mut().clear());
                state.mic_samples_recorded.set(0);
                state.mic_recording.set(true);
                state.mic_recording_start_time.set(Some(js_sys::Date::now()));
                let sr = state.mic_sample_rate.get_untracked();
                let file_idx = start_live_recording(state, sr);
                spawn_live_processing_loop(*state, file_idx, sr);
                spawn_smooth_scroll_animation(*state);
                log::info!("Recording started");
            }
        }
    }
}

/// Stop both listening and recording, close mic.
pub fn stop_all(state: &AppState) {
    let mode = state.mic_mode.get_untracked();
    let is_tauri = state.is_tauri;

    // For Auto mode, check which backend is actually open
    let effective = if mode == MicMode::Auto && is_tauri {
        if usb_mic_is_open() {
            MicMode::RawUsb
        } else if tauri_mic_is_open() {
            MicMode::Cpal
        } else {
            state.mic_effective_mode.get_untracked()
        }
    } else {
        mode
    };

    match effective {
        MicMode::RawUsb if is_tauri => {
            let state = *state;
            wasm_bindgen_futures::spawn_local(async move {
                stop_all_usb(&state).await;
            });
        }
        MicMode::Cpal if is_tauri => {
            let state = *state;
            wasm_bindgen_futures::spawn_local(async move {
                stop_all_tauri(&state).await;
            });
        }
        _ => {
            if state.mic_recording.get_untracked() {
                if let Some((samples, sr)) = stop_recording_web(state) {
                    finalize_live_recording(samples, sr, *state);
                }
            }
            state.mic_listening.set(false);
            state.mic_recording.set(false);
            state.mic_recording_start_time.set(None);
            close_mic_web(state);
        }
    }
}

// Re-export from split modules
pub use crate::audio::wav_encoder::{encode_wav, download_wav};
pub(crate) use crate::audio::live_recording::{
    start_live_recording, spawn_live_processing_loop,
    spawn_smooth_scroll_animation, finalize_live_recording,
    spawn_spectrogram_computation,
};
