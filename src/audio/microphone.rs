//! Microphone control: unified record/listen API.
//!
//! This module provides the public API for microphone recording and listening.
//! Backend-specific operations (Web Audio, cpal, USB) are delegated to the
//! `MicBackend` methods in `mic_backend`. Finalization is handled by
//! `live_recording`.

use crate::audio::live_recording::FinalizeParams;
use crate::audio::mic_backend::StopResult;
use crate::state::store_fields::*;
use crate::state::{
    AppState, GpsLocation, MicAcquisitionState, MicBackend, MicPendingAction, MicStrategy,
};
use crate::tauri_bridge::{
    tauri_invoke, tauri_invoke_no_args, tauri_invoke_typed_args, tauri_invoke_typed_no_args,
};
use leptos::prelude::*;
use wasm_bindgen::prelude::*;

// ── GPS location acquisition ────────────────────────────────────────────

/// Query the current WiFi SSID from the native plugin.
/// Returns None if not connected or unavailable.
async fn get_wifi_ssid() -> Option<String> {
    let result: oversample_ipc::plugins::WifiSsidResult =
        tauri_invoke_typed_no_args("plugin:geolocation|getWifiSsid")
            .await
            .ok()?;
    result.ssid
}

/// Fetch the Android device make/model from the native plugin.
/// Results are cached in state after first call (cached_device_make / cached_device_model).
async fn fetch_device_model(state: &AppState) {
    if state
        .recording_meta
        .cached_model()
        .get_untracked()
        .is_some()
    {
        return;
    }
    let Ok(result) = tauri_invoke_typed_no_args::<oversample_ipc::plugins::DeviceModelResult>(
        "plugin:geolocation|getDeviceModel",
    )
    .await
    else {
        return;
    };
    if !result.manufacturer.is_empty() {
        state
            .recording_meta
            .cached_make()
            .set(Some(result.manufacturer));
    }
    if !result.model.is_empty() {
        state.recording_meta.cached_model().set(Some(result.model));
    }
}

/// Request a one-shot GPS fix from the native geolocation plugin.
/// Returns None if the plugin is unavailable, permission denied, or location times out.
async fn acquire_gps_location() -> Option<GpsLocation> {
    let result: oversample_ipc::plugins::GeolocationResult =
        tauri_invoke_typed_no_args("plugin:geolocation|getCurrentLocation")
            .await
            .ok()?;
    // Error response (permission_denied, timeout, …) carries no coordinates.
    if result.error.is_some() {
        return None;
    }
    let latitude = result.latitude?;
    let longitude = result.longitude?;
    let elevation = if result.has_altitude.unwrap_or(false) {
        result.altitude
    } else {
        None
    };
    Some(GpsLocation {
        latitude,
        longitude,
        elevation,
        accuracy: result.accuracy,
    })
}

// ── Tauri IPC query helpers ─────────────────────────────────────────────

/// Request Android RECORD_AUDIO runtime permission via Tauri plugin.
/// Returns true if granted, false if denied or not on Android.
pub async fn request_audio_permission_tauri(state: &AppState) -> bool {
    if !state.is_tauri {
        return true;
    }
    state.log_debug("info", "Requesting RECORD_AUDIO permission via plugin...");
    match tauri_invoke_typed_no_args::<oversample_ipc::plugins::PermissionGranted>(
        "plugin:usb-audio|requestAudioPermission",
    )
    .await
    {
        Ok(result) => {
            if result.granted {
                state.log_debug("info", "RECORD_AUDIO permission granted");
            } else {
                state.log_debug("error", "RECORD_AUDIO permission denied");
                state.show_error_toast("Microphone permission denied");
            }
            result.granted
        }
        Err(e) => {
            state.log_debug(
                "warn",
                format!("requestAudioPermission failed (may not be Android): {}", e),
            );
            true // Non-fatal on desktop Tauri
        }
    }
}

/// Proactively request access to a USB audio device when it's picked in the mic
/// chooser, so a later recording starts without a permission prompt (the
/// chooser's original purpose). Idempotent — Android returns granted with no
/// dialog if access was already given. No-op off Tauri.
pub async fn ensure_usb_permission(state: &AppState, device_name: &str) {
    if !state.is_tauri {
        return;
    }
    match tauri_invoke_typed_args::<_, oversample_ipc::plugins::UsbPermissionResult>(
        "plugin:usb-audio|requestUsbPermission",
        &oversample_ipc::plugins::UsbDeviceNameArgs {
            device_name: device_name.to_string(),
        },
    )
    .await
    {
        Ok(r) => state.log_debug(
            "info",
            format!(
                "USB permission for {device_name}: {}",
                if r.granted { "granted" } else { "denied" }
            ),
        ),
        Err(e) => state.log_debug("warn", format!("requestUsbPermission failed: {e}")),
    }
}

/// Query the default cpal input device's supported sample rates without opening the mic.
/// Updates `state.mic.supported_rates()` with the result.
pub async fn query_cpal_supported_rates(state: &AppState) {
    if !state.is_tauri {
        return;
    }
    let result = match tauri_invoke_typed_no_args::<oversample_ipc::mic::DeviceListResult>(
        "mic_list_devices",
    )
    .await
    {
        Ok(v) => v,
        Err(_) => return,
    };
    for dev in &result.devices {
        if !dev.is_default {
            continue;
        }
        let mut rates = std::collections::BTreeSet::new();
        for range in &dev.sample_rate_ranges {
            rates.insert(range.min);
            rates.insert(range.max);
            for &r in &[44100u32, 48000, 96000, 192000, 256000, 384000, 500000] {
                if r >= range.min && r <= range.max {
                    rates.insert(r);
                }
            }
        }
        let rates_vec: Vec<u32> = rates.into_iter().collect();
        if !rates_vec.is_empty() {
            state.mic.supported_rates().set(rates_vec);
        }
        break;
    }
}

/// Query mic info without opening the mic. Populates device name/type signals.
pub async fn query_mic_info(state: &AppState) {
    if !state.is_tauri {
        return;
    }
    let backend = state.mic.backend().get_untracked();

    match backend {
        Some(MicBackend::RawUsb) => {
            if let Ok(result) = tauri_invoke_typed_no_args::<
                oversample_ipc::plugins::UsbDeviceListResult,
            >("plugin:usb-audio|listUsbDevices")
            .await
            {
                if let Some(dev) = result.devices.iter().find(|d| d.is_audio_device) {
                    state.mic.device_name().set(Some(dev.product_name.clone()));
                    state.mic.connection_type().set(Some("USB".to_string()));
                    state.mic.usb_connected().set(true);
                    return;
                }
            }
            state.mic.usb_connected().set(false);
        }
        Some(MicBackend::Cpal) | None => {
            if let Ok(result) = tauri_invoke_typed_no_args::<oversample_ipc::mic::DeviceListResult>(
                "mic_list_devices",
            )
            .await
            {
                for dev in &result.devices {
                    if dev.is_default {
                        let n = dev.name.to_lowercase();
                        let conn = if n.contains("usb") {
                            "USB"
                        } else if n.contains("bluetooth") || n.contains("bt ") {
                            "Bluetooth"
                        } else {
                            "Internal"
                        };
                        state.mic.connection_type().set(Some(conn.to_string()));
                        state.mic.device_name().set(Some(dev.name.clone()));

                        let mut max_rate: u32 = 0;
                        let mut format_str: Option<String> = None;
                        for range in &dev.sample_rate_ranges {
                            if range.max > max_rate {
                                max_rate = range.max;
                                format_str = Some(range.format.clone());
                            }
                        }
                        if max_rate > 0 {
                            state.mic.sample_rate().set(max_rate);
                        }
                        if let Some(fmt) = format_str {
                            let bits: u16 = match fmt.as_str() {
                                "I16" => 16,
                                "I24" => 24,
                                "I32" => 32,
                                "F32" => 32,
                                _ => 0,
                            };
                            if bits > 0 {
                                state.mic.bits_per_sample().set(bits);
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
    if let Ok(result) = tauri_invoke_typed_no_args::<oversample_ipc::plugins::UsbDeviceListResult>(
        "plugin:usb-audio|listUsbDevices",
    )
    .await
    {
        state
            .mic
            .usb_connected()
            .set(result.devices.iter().any(|d| d.is_audio_device));
    }
}

/// Check for USB audio devices and update `mic_usb_connected` signal.
pub async fn check_usb_status(state: &AppState) {
    if let Ok(result) = tauri_invoke_typed_no_args::<oversample_ipc::plugins::UsbDeviceListResult>(
        "plugin:usb-audio|listUsbDevices",
    )
    .await
    {
        if let Some(dev) = result.devices.iter().find(|d| d.is_audio_device) {
            state.mic.usb_connected().set(true);
            state.show_info_toast(format!("USB mic: {}", dev.product_name));
            return;
        }
    }

    state.mic.usb_connected().set(false);
}

/// Handle a USB hot-plug event pushed from the Android plugin via the
/// `window.__oversampleUsbHotplug` global (see app.rs). `event` is "attached" or
/// "detached". Replaces the old 3-second `checkUsbStatus` poll, so events are no
/// longer coalesced/dropped and arrive even while recording.
pub async fn handle_usb_hotplug(state: &AppState, event: &str, product: &str, device_name: &str) {
    state.log_debug(
        "info",
        format!("USB hotplug: {event} — {product} ({device_name})"),
    );
    // Let an already-open mic chooser re-enumerate its device lists.
    state.mic.hotplug_seq().update(|n| *n = n.wrapping_add(1));
    match event {
        "attached" => {
            // Name the device so the usb_connected effect's toast is accurate,
            // badge the mic button, and mark connected (the effect re-shows the
            // "Mic detected" chip + toasts).
            if !product.is_empty() {
                state.mic.device_name().set(Some(product.to_string()));
            }
            state.mic.new_device_available().set(true);
            state.mic.usb_connected().set(true);
            // Let the device finish enumerating, then refine device info.
            crate::web_util::sleep_ms(500).await;
            query_mic_info(state).await;
        }
        "detached" => {
            // Was the mic we're showing/using this USB device? (Direct-USB backend,
            // or a device_info marked USB.) If so, reset it below so the UI updates.
            let was_active_usb = state.mic.backend().get_untracked() == Some(MicBackend::RawUsb)
                || state
                    .mic
                    .device_info()
                    .get_untracked()
                    .map(|i| i.connection_type == "USB")
                    .unwrap_or(false);

            if state.mic.recording().get_untracked() {
                // A mid-recording unplug can't continue — stop (and finalize) it.
                toggle_record(state).await;
                state.show_info_toast("USB mic disconnected — recording stopped");
            } else {
                // Tear down a live-listen stream that's reading from the gone device.
                if was_active_usb && state.mic.listening().get_untracked() {
                    stop_all(state);
                }
                state.show_info_toast("USB mic disconnected");
            }

            // Clear the shown/selected mic so the button (mic_value reads
            // device_info) stops displaying the gone device and the user is
            // re-prompted to pick one.
            if was_active_usb {
                state.mic.backend().set(None);
                state.mic.selected_device().set(None);
                state.mic.device_info().set(None);
                state.mic.acquisition_state().set(MicAcquisitionState::Idle);
            }
            state.mic.new_device_available().set(false);
            state.mic.usb_connected().set(false);
            query_mic_info(state).await;
        }
        other => {
            state.log_debug("warn", format!("USB hotplug: unknown event '{other}'"));
        }
    }
}

// ── Android foreground audio service ─────────────────────────────────────

/// Start (or update the notification of) the Android foreground audio service so
/// capture/monitoring survive the app being backgrounded. `mode` is "listening"
/// or "recording". No-op off mobile Tauri; best-effort (logged, not surfaced).
/// MUST be reached from a foreground user gesture — Android 14+ forbids starting
/// a microphone foreground service from the background.
async fn start_foreground_service(state: &AppState, mode: &str) {
    if !state.is_tauri || !state.status.is_mobile().get_untracked() {
        return;
    }
    let args = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&args, &JsValue::from_str("mode"), &JsValue::from_str(mode));
    if let Err(e) = tauri_invoke("plugin:audio-service|startForegroundAudio", &args.into()).await {
        state.log_debug("warn", format!("startForegroundAudio failed: {}", e));
    }
}

/// Stop the Android foreground audio service. No-op off mobile Tauri.
pub(crate) async fn stop_foreground_service(state: &AppState) {
    if !state.is_tauri || !state.status.is_mobile().get_untracked() {
        return;
    }
    if let Err(e) = tauri_invoke_no_args("plugin:audio-service|stopForegroundAudio").await {
        state.log_debug("warn", format!("stopForegroundAudio failed: {}", e));
    }
}

/// Persist that we've surfaced the notification-permission rationale so it never
/// re-prompts. Shared by the rationale modal and the no-op fast path below.
pub(crate) fn mark_notif_asked(state: &AppState) {
    state.dialogs.notif_perm_asked().set(true);
    crate::settings::set_bool(crate::settings::keys::NOTIF_PERM_ASKED, true);
}

/// Invoke the native POST_NOTIFICATIONS request (called after the in-app
/// rationale, so the OS prompt is never shown cold). No-op off mobile Tauri.
pub(crate) async fn request_notification_permission(state: &AppState) {
    if !state.is_tauri || !state.status.is_mobile().get_untracked() {
        return;
    }
    let _ = tauri_invoke_no_args("plugin:audio-service|requestNotificationPermission").await;
}

/// During mic setup on mobile Tauri, decide whether to surface the notification
/// rationale before the OS asks. We ask the plugin whether a runtime request is
/// needed (API 33+) and not yet granted; only then do we show the rationale.
/// Persists `notif_perm_asked` so this happens at most once. Best-effort.
async fn maybe_prompt_notifications(state: &AppState) {
    if !state.is_tauri || !state.status.is_mobile().get_untracked() {
        return;
    }
    if state.dialogs.notif_perm_asked().get_untracked() {
        return;
    }
    let status = match tauri_invoke_typed_no_args::<
        oversample_ipc::plugins::NotificationPermissionStatus,
    >("plugin:audio-service|isNotificationPermissionGranted")
    .await
    {
        Ok(v) => v,
        Err(_) => return, // plugin unavailable — leave the Listen-time path as-is
    };
    if !status.runtime_required || status.granted {
        // Older Android (no runtime permission) or already granted — nothing to
        // ask; record it so we don't re-check on every acquisition.
        mark_notif_asked(state);
        return;
    }
    state.dialogs.notif_rationale().set(true);
}

// ── Backend resolution ──────────────────────────────────────────────────

/// The currently selected mic backend, if any.
fn resolve_active_backend(state: &AppState) -> Option<MicBackend> {
    state.mic.backend().get_untracked()
}

/// Open the appropriate mic backend based on a resolved MicBackend.
async fn open_backend(state: &AppState, backend: MicBackend) -> bool {
    backend.open(state).await
}

// ── Unified mic acquisition ─────────────────────────────────────────────

/// Unified mic acquisition. Called by both toggle_record and toggle_listen.
/// Returns the resolved MicBackend when the mic is ready, or None if the user
/// cancelled, permission was denied, or the mic failed to open.
pub async fn acquire_mic(state: &AppState, action: MicPendingAction) -> Option<MicBackend> {
    // If mic is already open and streaming, return current backend immediately
    if state.mic.acquisition_state().get_untracked() == MicAcquisitionState::Ready {
        if let Some(backend) = state.mic.backend().get_untracked() {
            let still_open = backend.is_open();
            if still_open {
                return Some(backend);
            }
            // Backend closed unexpectedly — fall through to re-acquire
            state.mic.acquisition_state().set(MicAcquisitionState::Idle);
        }
    }

    let strategy = state.mic.strategy().get_untracked();

    match strategy {
        MicStrategy::None => {
            state.log_debug("info", "acquire_mic: strategy=None, mic disabled");
            None
        }
        MicStrategy::Browser => {
            state
                .mic
                .acquisition_state()
                .set(MicAcquisitionState::Acquiring);
            let t0 = js_sys::Date::now();
            if MicBackend::Browser.open(state).await {
                let elapsed = js_sys::Date::now() - t0;
                state.mic.permission_dialog_shown().set(elapsed > 1500.0);
                state.mic.backend().set(Some(MicBackend::Browser));
                state
                    .mic
                    .acquisition_state()
                    .set(MicAcquisitionState::Ready);
                let st = *state;
                wasm_bindgen_futures::spawn_local(async move {
                    maybe_prompt_notifications(&st).await;
                });
                Some(MicBackend::Browser)
            } else {
                state
                    .mic
                    .acquisition_state()
                    .set(MicAcquisitionState::Failed);
                state.mic.strategy().set(MicStrategy::Ask);
                state.mic.backend().set(None);
                state.mic.device_info().set(None);
                state.mic.selected_device().set(None);
                state.status.message().set(Some(
                    "Browser mic failed. Please choose a microphone.".into(),
                ));
                None
            }
        }
        MicStrategy::Selected => {
            if let Some(backend) = state.mic.backend().get_untracked() {
                state
                    .mic
                    .acquisition_state()
                    .set(MicAcquisitionState::Acquiring);
                let t0 = js_sys::Date::now();
                if open_backend(state, backend).await {
                    let elapsed = js_sys::Date::now() - t0;
                    state.mic.permission_dialog_shown().set(elapsed > 1500.0);
                    state
                        .mic
                        .acquisition_state()
                        .set(MicAcquisitionState::Ready);
                    let st = *state;
                    wasm_bindgen_futures::spawn_local(async move {
                        maybe_prompt_notifications(&st).await;
                    });
                    return Some(backend);
                } else {
                    state.mic.strategy().set(MicStrategy::Ask);
                    state.mic.backend().set(None);
                    state.mic.device_info().set(None);
                    state.mic.selected_device().set(None);
                    state.mic.acquisition_state().set(MicAcquisitionState::Idle);
                    state
                        .status
                        .message()
                        .set(Some("Microphone failed. Please choose again.".into()));
                    return None;
                }
            }
            // No backend remembered despite Selected — fall back to Ask
            state.mic.strategy().set(MicStrategy::Ask);
            state.mic.pending_action().set(Some(action));
            state
                .mic
                .acquisition_state()
                .set(MicAcquisitionState::AwaitingChoice);
            state.mic.show_chooser().set(true);
            None
        }
        MicStrategy::Ask => {
            state.mic.pending_action().set(Some(action));
            state
                .mic
                .acquisition_state()
                .set(MicAcquisitionState::AwaitingChoice);
            state.mic.show_chooser().set(true);
            None
        }
    }
}

// ── Unified flows (private) ─────────────────────────────────────────────

/// Start recording with the given backend (mic already open).
async fn do_start_recording(state: &AppState, backend: MicBackend) {
    warn_if_te_for_live(state);
    let was_listening = state.mic.listening().get_untracked();
    let has_listen_file = was_listening && state.mic.live_file_idx().get_untracked().is_some();

    // Fetch device model on first recording (cached for future use)
    if state.is_tauri
        && state.status.is_mobile().get_untracked()
        && state.recording_meta.device_model_enabled().get_untracked()
    {
        let _ = fetch_device_model(state).await;
    }

    // Acquire GPS location if enabled (one-shot, non-blocking).
    // Skip if connected to a home WiFi network (geofencing).
    if state.recording_meta.gps_enabled().get_untracked()
        && state.is_tauri
        && state.status.is_mobile().get_untracked()
    {
        let on_home_wifi = if state
            .recording_meta
            .home_wifi_ssids()
            .with_untracked(|list| !list.is_empty())
        {
            get_wifi_ssid()
                .await
                .map(|ssid| {
                    state
                        .recording_meta
                        .home_wifi_ssids()
                        .with_untracked(|list| list.contains(&ssid))
                })
                .unwrap_or(false)
        } else {
            false
        };
        if on_home_wifi {
            log::info!("On home WiFi — skipping location embedding");
            state.recording_meta.location().set(None);
        } else {
            state
                .recording_meta
                .location()
                .set(acquire_gps_location().await);
        }
    } else {
        state.recording_meta.location().set(None);
    }

    // Detect armed-doc reuse before any backend I/O so we can rename it to the
    // proper batcap_*.wav name BEFORE start_recording runs. The recovery
    // sidecar/.wav.part filename is built from the file at mic_live_file_idx
    // inside build_start_recording_args, so promoting after start_recording
    // would leave the recovery file with the armed "Live mic (HH:MM:SS)" name.
    let armed_idx = if !has_listen_file {
        // Fresh record (not converting an active listen): collapse stale empty
        // live placeholders and reuse the surviving one (armed doc or a stale
        // listen entry) instead of spawning a second recording file.
        let keep = state.mic.live_file_idx().get_untracked();
        prune_empty_live_placeholders(state, keep);
        state
            .mic
            .live_file_idx()
            .get_untracked()
            .filter(|&idx| is_reusable_live_doc(state, idx))
    } else {
        None
    };
    if let Some(idx) = armed_idx {
        // Reset a stale listen entry's accumulated display state before turning
        // it into a recording (no-op for an already-armed doc).
        convert_listen_to_armed(state);
        promote_armed_to_recording(state, idx);
    }

    if !has_listen_file {
        // Fresh recording — clear buffer, tiles, and any stale pre-roll count
        backend.clear_buffer();
        crate::canvas::tile_cache::clear_all_caches();
        state.mic.preroll_samples().set(0);
        state.mic.listening().set(false);
    } else {
        // Listen→record: keep mic_listening=true during the await so the
        // processing loop doesn't exit (it checks `!recording && !listening`).
        // We'll clear it after mic_recording is set to true.
        crate::canvas::tile_cache::clear_all_caches();
        // Rename the listening file from its placeholder ("Listening") to the
        // proper batcap_*.wav name BEFORE start_recording — the recovery
        // sidecar/.wav.part and the Android MediaStore entry are both built
        // from `mic_live_file_idx`'s current name. With the placeholder, the
        // MediaStore entry ends up `DISPLAY_NAME=Listening` against MIME
        // `audio/wav`, which Android either rejects or saves invisibly.
        let sr = state.mic.sample_rate().get_untracked();
        rename_listen_to_recording(state, sr);
    }

    match backend.start_recording(state).await {
        Ok(()) => {
            // Reset frequency display so the waterfall shows the full mic range.
            state.view.min_display_freq().set(None);
            state.view.max_display_freq().set(None);
            state.mic.samples_recorded().set(0);
            state.mic.recording().set(true);
            // Now safe to clear listening — recording is active, loop won't exit.
            state.mic.listening().set(false);
            state
                .mic
                .recording_start_time()
                .set(Some(js_sys::Date::now()));
            // Keep capturing if the app is backgrounded (Android foreground service).
            start_foreground_service(state, "recording").await;
            let sr = state.mic.sample_rate().get_untracked();

            let file_idx = if has_listen_file {
                // Convert the existing listening file into a recording file.
                // Don't respawn the processing loop or recreate the waterfall —
                // the listen loop is already running on the same buffer and its
                // exit condition (`!recording && !listening`) is no longer true
                // because we just set mic_recording=true.  Recreating the
                // waterfall would discard all accumulated columns and cause a
                // visible flash/glitch.
                convert_listen_to_recording(state, sr)
            } else if let Some(idx) = armed_idx {
                // Armed doc was renamed before start_recording above; just
                // start the live processing loop on it.
                set_live_recording_zoom(state, sr);
                spawn_live_processing_loop(*state, idx, sr);
                spawn_smooth_scroll_animation(*state);
                idx
            } else {
                let idx = start_live_recording(state, sr);
                spawn_live_processing_loop(*state, idx, sr);
                spawn_smooth_scroll_animation(*state);
                idx
            };
            log::info!(
                "Recording started ({:?}, pre-roll={}, file_idx={})",
                backend,
                has_listen_file,
                file_idx
            );
        }
        Err(e) => {
            log::error!("start_recording failed: {}", e);
            state
                .status
                .message()
                .set(Some(format!("Failed to start recording: {}", e)));
            // A shared-storage (Android MediaStore) entry + fd may have been
            // reserved before start failed; delete the orphaned pending row so
            // it doesn't linger as a 0-byte file. (The detached fd itself can
            // only be reclaimed by the OS — see SharedFdGuard for the native
            // stop-path leak fix.)
            if state.is_tauri {
                crate::audio::mic_backend::cancel_shared_entry().await;
            }
            // If we were listening, clean up the orphaned listen file
            if has_listen_file {
                state.mic.listening().set(false);
                cleanup_listen_file(state);
            }
            // If we promoted an armed doc to recording above, roll it back so
            // is_reusable_live_doc() recognizes it again on the next attempt.
            if let Some(idx) = armed_idx {
                revert_recording_to_armed(state, idx);
            }
        }
    }

    // Release the start-debounce gate so the next Record press is honoured.
    // Cleared on both Ok and Err paths — `mic_recording` is what really tells
    // the next press to stop instead of start, but the gate covers the gap
    // between user gesture and that flag flipping.
    state.mic.starting_recording().set(false);
}

/// Convert a Tauri recording result into FinalizeParams. Pre-roll is now seeded
/// into the on-disk recording on the native side (from the listening ring), so
/// every Tauri recording — pre-roll or not — comes back as either a saved path
/// (streaming-to-disk / shared storage) or to-memory samples, and finalizes
/// through the same path. The pre-roll cue marker is placed in
/// `finalize_streaming_tauri_recording` from `mic_preroll_samples`.
fn tauri_result_to_params(rec: crate::audio::mic_backend::TauriRecordingResult) -> FinalizeParams {
    FinalizeParams {
        samples: rec.samples,
        sample_rate: rec.sample_rate,
        bits_per_sample: rec.bits_per_sample,
        is_float: rec.is_float,
        saved_path: rec.saved_path,
        file_size: rec.file_size_bytes,
    }
}

/// Dispatch a StopResult into finalize_recording or cleanup.
/// Shared by both `do_stop_recording` and `stop_all`.
fn handle_stop_result(result: StopResult, state: &AppState) {
    let bits_per_sample = state.mic.bits_per_sample().get_untracked();
    match result {
        StopResult::Samples {
            samples,
            sample_rate,
        } => {
            finalize_recording(
                FinalizeParams {
                    samples,
                    sample_rate,
                    bits_per_sample,
                    is_float: false,
                    saved_path: String::new(),
                    file_size: None,
                },
                *state,
            );
        }
        StopResult::TauriResult(rec) => {
            finalize_recording(tauri_result_to_params(rec), *state);
        }
        StopResult::Empty => {
            log::warn!("No samples recorded");
            cleanup_failed_recording(state);
        }
        StopResult::Error(e) => {
            log::error!("stop_recording failed: {}", e);
            state
                .status
                .message()
                .set(Some(format!("Recording failed: {}", e)));
            cleanup_failed_recording(state);
        }
    }
}

/// Stop recording and finalize.
async fn do_stop_recording(state: &AppState, backend: MicBackend) {
    let was_listening = state.mic.listening().get_untracked();
    state.mic.recording().set(false);
    state.mic.recording_start_time().set(None);
    state.mic.samples_recorded().set(0);

    let result = backend.stop_recording(state).await;
    handle_stop_result(result, state);

    if was_listening {
        // Listen overlay was active during the recording. finalize_recording
        // cleared mic_live_file_idx, which causes the processing loop to exit
        // and leaves listening "on" but with no live file or waterfall. Kick
        // off a fresh listen session to restore the live visualization.
        // (do_start_listening re-issues the foreground service as "listening".)
        do_start_listening(state, backend).await;
    } else {
        backend.maybe_close(state).await;
        stop_foreground_service(state).await;
    }
}

/// Start listening with the given backend (mic already open).
async fn do_start_listening(state: &AppState, backend: MicBackend) {
    warn_if_te_for_live(state);
    // Reset frequency display so the waterfall shows the full mic range
    // (not a zoomed range from a previously-open high-SR file).
    state.view.min_display_freq().set(None);
    state.view.max_display_freq().set(None);
    // Clear buffer and DSP state BEFORE enabling listening to prevent stale
    // audio from a previous listen session leaking into the new one. Also stop
    // any still-scheduled playback and reset the schedule cursor so a backlog
    // from a previous (possibly backgrounded) session can't carry over.
    backend.clear_buffer();
    backend.clear_dsp_state();
    crate::audio::mic_backend::stop_het_playback();
    // Set the frontend signal early so the chunk handler accepts data
    // as soon as the native side starts streaming.
    state.mic.listening().set(true);
    backend.set_listening(state, true).await;
    let sr = state.mic.sample_rate().get_untracked();
    // Clear tile caches so previous file's spectrogram doesn't flash
    crate::canvas::tile_cache::clear_all_caches();

    // Collapse any stale, empty live-mic placeholders so the file list never
    // accumulates more than one. Keep the current live entry (if any) as the
    // reuse candidate; prune fixes up mic_live_file_idx for any removals.
    let keep = state.mic.live_file_idx().get_untracked();
    prune_empty_live_placeholders(state, keep);
    // Reuse the existing live placeholder — an armed doc OR a stale listen
    // entry that wasn't cleanly converted — if one is present; otherwise
    // create a new transient listening file. The reuse path lets the user
    // pre-configure HFR settings before pressing Listen, without spawning a
    // second file.
    let reuse = state
        .mic
        .live_file_idx()
        .get_untracked()
        .filter(|&idx| is_reusable_live_doc(state, idx));
    let file_idx = if let Some(idx) = reuse {
        // If it's a stale listen entry, reset its accumulated display state
        // (spectrogram/overview/duration) to a clean armed shape first — a
        // no-op for an already-armed doc.
        convert_listen_to_armed(state);
        promote_armed_to_listening(state, idx);
        idx
    } else {
        start_live_listening(state, sr)
    };

    spawn_live_processing_loop(*state, file_idx, sr);
    spawn_smooth_scroll_animation(*state);
    // Keep monitoring alive if the app is backgrounded (Android foreground service).
    start_foreground_service(state, "listening").await;
}

/// Stop listening. Leaves the live file in place as an empty "armed" doc
/// (mic stays open) so the user can adjust HFR / band and press Listen or
/// Record again without re-acquiring the mic or creating a new entry.
async fn do_stop_listening(state: &AppState, backend: MicBackend) {
    state.mic.listening().set(false);
    // Stop scheduled playback immediately so Stop is instant (no backlog tail).
    crate::audio::mic_backend::stop_het_playback();
    crate::canvas::live_waterfall::clear();
    convert_listen_to_armed(state);
    backend.clear_buffer();
    backend.set_listening(state, false).await;
    // Tear down the foreground service unless a recording is still running.
    if !state.mic.recording().get_untracked() {
        stop_foreground_service(state).await;
    }
    // Intentionally not calling backend.maybe_close — keep the mic warm
    // for the armed doc.
}

// ── Public API ──────────────────────────────────────────────────────────

/// Toggle live HET listening on/off.
pub async fn toggle_listen(state: &AppState) {
    // If a recording is in progress, treat listen as a pure overlay: flip
    // mic_listening + reset HET DSP state, but leave the recording buffer,
    // live file, and processing loop untouched. Otherwise calling
    // do_start_listening / do_stop_listening here would clear_buffer() (wiping
    // in-progress recording samples) and replace mic_live_file_idx with a new
    // "Listening" entry — corrupting the file list.
    if state.mic.recording().get_untracked() {
        if let Some(backend) = resolve_active_backend(state) {
            let enabling = !state.mic.listening().get_untracked();
            state.log_debug(
                "info",
                format!(
                    "toggle_listen: recording active, {} HET overlay only",
                    if enabling { "enabling" } else { "disabling" },
                ),
            );
            if enabling {
                // Reset HET state so we don't hear stale audio from a prior session
                backend.clear_dsp_state();
            } else {
                // Stop the listen overlay's scheduled playback immediately.
                crate::audio::mic_backend::stop_het_playback();
            }
            state.mic.listening().set(enabling);
            backend.set_listening(state, enabling).await;
        }
        return;
    }

    // If already listening, stop
    if state.mic.listening().get_untracked() {
        state.log_debug("info", "toggle_listen: stopping");
        if let Some(backend) = resolve_active_backend(state) {
            do_stop_listening(state, backend).await;
        } else {
            // Fallback: just clear signals
            state.mic.listening().set(false);
            crate::canvas::live_waterfall::clear();
            cleanup_listen_file(state);
        }
        return;
    }

    // Acquire mic (unified flow)
    let mic_backend = match acquire_mic(state, MicPendingAction::Listen).await {
        Some(b) => b,
        None => {
            state.log_debug(
                "info",
                "toggle_listen: acquire_mic returned None (chooser shown or failed)",
            );
            return;
        }
    };

    let backend = mic_backend;
    state.log_debug(
        "info",
        format!("toggle_listen: backend={:?}, starting listen", backend),
    );
    do_start_listening(state, backend).await;
}

/// Reset the DSP state on whichever mic backend is currently active. Use this
/// when a live-audio parameter change (mode switch, filter knob) would
/// otherwise leak stale buffer contents into the new mode (PS/PV overlap
/// buffers, HET filter delay lines, IIR warmup tail).
pub fn clear_live_dsp_state(state: &AppState) {
    if let Some(backend) = resolve_active_backend(state) {
        backend.clear_dsp_state();
    }
}

/// Show a one-shot toast if the user starts live audio while PlaybackMode is
/// TimeExpansion. TE relies on AudioContext sample-rate tricks that can't work
/// for an unbounded live stream — `process_live_audio` falls through to
/// passthrough, but the user should know it's doing nothing.
fn warn_if_te_for_live(state: &AppState) {
    if state.playback.mode().get_untracked() == crate::state::PlaybackMode::TimeExpansion {
        state.show_info_toast(
            "Time-expansion isn't applicable to live audio — playing back at 1:1.",
        );
    }
}

/// Open the mic and create an empty live document — but don't start listening
/// or recording. Lets the user adjust HFR mode/range/bandpass first, then
/// press Listen or Record on the armed doc. If a non-empty live doc already
/// exists, this is a no-op (we don't want to discard in-progress audio).
///
/// Triggered from the file panel's "+ New live recording" button.
pub async fn arm_live_doc(state: &AppState) {
    // If we already have a live doc that's currently streaming, refuse — don't
    // step on an in-progress listen or recording session.
    if state.mic.listening().get_untracked() || state.mic.recording().get_untracked() {
        state.show_error_toast("Already listening or recording.");
        return;
    }

    // Collapse stale empty live placeholders; keep the current one (if any)
    // as the reuse candidate.
    let keep = state.mic.live_file_idx().get_untracked();
    prune_empty_live_placeholders(state, keep);
    // If a reusable live placeholder already exists, reset it to the idle
    // "armed" shape (a stale listen entry gets cleared) and navigate to it
    // instead of making a second one — pressing +New repeatedly is idempotent.
    if let Some(idx) = state.mic.live_file_idx().get_untracked() {
        if is_reusable_live_doc(state, idx) {
            convert_listen_to_armed(state); // no-op unless it's a stale listen entry
            state.library.current_index().set(Some(idx));
            return;
        }
    }

    let mic_backend = match acquire_mic(state, MicPendingAction::Arm).await {
        Some(b) => b,
        None => {
            state.log_debug("info", "arm_live_doc: acquire_mic returned None");
            return;
        }
    };
    let backend = mic_backend;
    state.log_debug(
        "info",
        format!("arm_live_doc: mic acquired ({:?})", backend),
    );

    // Reset DSP state and tile caches so the empty doc starts clean.
    backend.clear_buffer();
    backend.clear_dsp_state();
    crate::canvas::tile_cache::clear_all_caches();

    let sr = state.mic.sample_rate().get_untracked().max(48_000);
    let _ = start_live_armed(state, sr);
}

/// RAII guard for the `starting_recording` debounce gate.
///
/// The gate blocks a second Record tap while a start flow is mid-flight (see
/// [`toggle_record`]). It must be cleared on *every* terminal path, and the flow
/// has several early returns plus a cross-call hold for the "Ready to record"
/// dialog — historically each exit cleared the flag by hand, so any new early
/// return risked leaving it stuck `true` (every later Record tap then silently
/// swallowed with a "Recording is starting…" toast). This guard clears the flag
/// on drop; call [`StartGate::disarm`] to hand the gate off to a path that clears
/// it itself — `do_start_recording`'s tail, or the dialog's confirm/cancel.
struct StartGate {
    state: AppState,
    armed: bool,
}

impl StartGate {
    /// Set the gate and arm a guard that clears it on drop.
    fn engage(state: &AppState) -> Self {
        state.mic.starting_recording().set(true);
        Self {
            state: *state,
            armed: true,
        }
    }

    /// Adopt a gate already set by an earlier call (the dialog hold), arming the
    /// guard so the current flow's early returns still release it.
    fn adopt(state: &AppState) -> Self {
        Self {
            state: *state,
            armed: true,
        }
    }

    /// Hand off responsibility for clearing the gate; drop becomes a no-op.
    fn disarm(mut self) {
        self.armed = false;
    }
}

impl Drop for StartGate {
    fn drop(&mut self) {
        if self.armed {
            self.state.mic.starting_recording().set(false);
        }
    }
}

/// Toggle recording on/off. When stopping, finalizes the recording.
pub async fn toggle_record(state: &AppState) {
    // If already recording, stop
    if state.mic.recording().get_untracked() {
        state.log_debug("info", "toggle_record: stopping");
        if let Some(backend) = resolve_active_backend(state) {
            do_stop_recording(state, backend).await;
        }
        return;
    }

    // Debounce: ignore the second tap of a rapid double-press. Without this
    // guard, a user impatient with the "invisible" acquire_mic / IPC phase
    // could fire two parallel start flows — each calling
    // `mic_start_recording` and `try_create_shared_fd`, leaving Android with
    // two MediaStore entries (one stuck IS_PENDING=1 → `.pending`, the other
    // saved as "…(1).wav").
    if state.mic.starting_recording().get_untracked() {
        state.log_debug("info", "toggle_record: ignored — start already in flight");
        state.show_info_toast("Recording is starting\u{2026}");
        return;
    }
    // Engage the start-debounce gate. The guard clears it on every early return
    // below; `disarm()` hands it off to a path that clears it itself.
    let gate = StartGate::engage(state);

    // If already listening, the mic is ready — go straight to recording
    if state.mic.listening().get_untracked() {
        if let Some(backend) = resolve_active_backend(state) {
            state.log_debug(
                "info",
                format!(
                    "toggle_record: already listening, starting immediate with {:?}",
                    backend
                ),
            );
            gate.disarm(); // do_start_recording clears the gate at its tail
            do_start_recording(state, backend).await;
            return;
        }
    }

    // Acquire mic (unified flow)
    let mic_backend = match acquire_mic(state, MicPendingAction::Record).await {
        Some(b) => b,
        None => {
            state.log_debug(
                "info",
                "toggle_record: acquire_mic returned None (chooser shown or failed)",
            );
            return; // `gate` drops → releases the debounce gate
        }
    };

    let backend = mic_backend;

    // If OS permission dialog was shown (detected by timing), skip our dialog
    if state.mic.permission_dialog_shown().get_untracked() {
        state.log_debug(
            "info",
            format!(
                "toggle_record: backend={:?}, permission dialog detected, starting immediately",
                backend
            ),
        );
        gate.disarm(); // do_start_recording clears the gate at its tail
        do_start_recording(state, backend).await;
    } else {
        // Show "Ready to record" dialog — user must confirm. The gate stays set
        // so a stray tap during the dialog can't kick off a second flow;
        // confirm_record_start / cancel_record_start own clearing it.
        state.log_debug(
            "info",
            format!(
                "toggle_record: backend={:?}, showing Ready to Record dialog",
                backend
            ),
        );
        state
            .mic
            .record_ready_state()
            .set(crate::state::RecordReadyState::AwaitingConfirmation);
        gate.disarm(); // dialog confirm/cancel now owns the gate
    }
}

/// Toggle recording via long-press while listening.  Works even in ListenOnly mode.
/// Captures the current listen buffer as pre-roll and records the buffer length
/// so a WAV cue marker can be written at finalization time.
pub async fn toggle_record_with_preroll(state: &AppState) {
    // If already recording, just stop (same as toggle_record)
    if state.mic.recording().get_untracked() {
        if let Some(backend) = resolve_active_backend(state) {
            do_stop_recording(state, backend).await;
        }
        return;
    }

    // Debounce against rapid double-press (see `toggle_record` for why).
    if state.mic.starting_recording().get_untracked() {
        state.log_debug(
            "info",
            "toggle_record_with_preroll: ignored — start already in flight",
        );
        return;
    }
    let gate = StartGate::engage(state);

    // Must be listening to have a pre-roll buffer
    if !state.mic.listening().get_untracked() {
        // Not listening — fall back to normal toggle_record. Release the gate
        // *before* re-dispatching so toggle_record's own debounce check doesn't
        // bounce; that call then owns the gate.
        drop(gate);
        toggle_record(state).await;
        return;
    }

    // Capture the current listen buffer length as pre-roll.
    // Compensate for audio accumulated during the hold gesture: the user's intent
    // is to capture the buffer state from when they *started* pressing, not when
    // the 400ms timeout fired.
    let raw_preroll = crate::audio::mic_backend::with_live_samples(state.is_tauri, |s| s.len());
    let gesture_start = state.mic.gesture_start_ms().get_untracked();
    state.mic.gesture_start_ms().set(None); // consume
    let sample_rate = state.mic.sample_rate().get_untracked();
    let preroll = if let Some(start_ms) = gesture_start {
        let gesture_ms = (js_sys::Date::now() - start_ms).max(0.0);
        let gesture_samples = ((gesture_ms / 1000.0) * sample_rate as f64).round() as usize;
        raw_preroll.saturating_sub(gesture_samples)
    } else {
        raw_preroll
    };
    // The listen buffer keeps ~2 s of headroom beyond the user-requested
    // duration (see trim logic in live_recording) specifically so that
    // gesture-time compensation above doesn't eat into what the user asked
    // for. Clamp back down to the requested duration here so long presses
    // don't produce *more* pre-roll than the setting promises.
    let requested_secs = state.mic.preroll_buffer_secs().get_untracked().max(2) as usize;
    let requested_samples = requested_secs.saturating_mul(sample_rate as usize);
    let preroll = preroll.min(requested_samples);
    state.mic.preroll_samples().set(preroll);

    if let Some(backend) = resolve_active_backend(state) {
        log::info!(
            "Long-press record: capturing {} pre-roll samples (raw={}, gesture compensated)",
            preroll,
            raw_preroll
        );
        gate.disarm(); // do_start_recording clears the gate at its tail
        do_start_recording(state, backend).await;
    } else {
        // No backend to start with — `gate` drops → releases the debounce gate.
    }
}

/// Called by the "Ready to record" dialog's OK button.
pub async fn confirm_record_start(state: &AppState) {
    state
        .mic
        .record_ready_state()
        .set(crate::state::RecordReadyState::None);
    // The gate is still held from toggle_record's dialog branch; adopt it so the
    // no-backend path below releases it.
    let gate = StartGate::adopt(state);
    if let Some(backend) = resolve_active_backend(state) {
        gate.disarm(); // do_start_recording clears the gate at its tail
        do_start_recording(state, backend).await;
    } else {
        // No backend resolvable — `gate` drops → releases the debounce gate.
    }
}

/// Called by the "Ready to record" dialog's Cancel button.
pub fn cancel_record_start(state: &AppState) {
    state
        .mic
        .record_ready_state()
        .set(crate::state::RecordReadyState::None);
    state.mic.starting_recording().set(false);
}

/// Stop both listening and recording, close mic.
pub fn stop_all(state: &AppState) {
    // Defensive: clear the start-debounce gate in case stop_all is called
    // mid-acquisition (e.g. tab close, app teardown, error recovery).
    state.mic.starting_recording().set(false);

    // Silence any scheduled live playback synchronously so Stop is instant,
    // independent of the async close that follows.
    crate::audio::mic_backend::stop_het_playback();

    let backend = resolve_active_backend(state).or_else(|| {
        // Legacy: infer from what's open
        if MicBackend::RawUsb.is_open() {
            Some(MicBackend::RawUsb)
        } else if MicBackend::Cpal.is_open() {
            Some(MicBackend::Cpal)
        } else {
            None
        }
    });

    let state_copy = *state;

    match backend {
        Some(b) => {
            wasm_bindgen_futures::spawn_local(async move {
                if state_copy.mic.recording().get_untracked() {
                    state_copy.mic.recording().set(false);
                    state_copy.mic.recording_start_time().set(None);
                    state_copy.mic.samples_recorded().set(0);

                    let result = b.stop_recording(&state_copy).await;
                    handle_stop_result(result, &state_copy);
                }
                if state_copy.mic.listening().get_untracked() {
                    state_copy.mic.listening().set(false);
                    cleanup_listen_file(&state_copy);
                }
                crate::canvas::live_waterfall::clear();
                b.close(&state_copy).await;
                stop_foreground_service(&state_copy).await;
                state_copy
                    .mic
                    .acquisition_state()
                    .set(MicAcquisitionState::Idle);
            });
        }
        None => {
            // No backend known — just clear state
            cleanup_listen_file(state);
            state.mic.listening().set(false);
            state.mic.recording().set(false);
            state.mic.recording_start_time().set(None);
            wasm_bindgen_futures::spawn_local(async move {
                MicBackend::Browser.close(&state_copy).await;
                stop_foreground_service(&state_copy).await;
                state_copy
                    .mic
                    .acquisition_state()
                    .set(MicAcquisitionState::Idle);
            });
        }
    }
}

// Re-export from split modules
pub(crate) use crate::audio::live_recording::{
    cleanup_failed_recording, cleanup_listen_file, convert_listen_to_armed,
    convert_listen_to_recording, finalize_recording, is_reusable_live_doc,
    promote_armed_to_listening, promote_armed_to_recording, prune_empty_live_placeholders,
    rename_listen_to_recording, revert_recording_to_armed, set_live_recording_zoom,
    spawn_live_processing_loop, spawn_smooth_scroll_animation, start_live_armed,
    start_live_listening, start_live_recording,
};
pub use crate::audio::wav_encoder::encode_wav;
