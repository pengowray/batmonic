use crate::state::store_fields::*;
use crate::state::{
    AppState, MicAcquisitionState, MicBackend, MicDeviceInfo, MicPendingAction, MicStrategy,
};
use crate::tauri_bridge::tauri_invoke_typed_no_args;
use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;

// ── Shared device-selection helpers ──────────────────────────────────────────
// Every device row in the chooser performs the same selection sequence; only the
// backend, selected id, and capability info differ. These collapse the five
// hand-copied click handlers (which had drifted) into one path.

/// Apply a device selection: set backend + strategy + selected device + caps,
/// close the chooser, reset to Idle, and stop any current capture.
fn select_device(
    state: &AppState,
    backend: MicBackend,
    selected: Option<String>,
    device_info: Option<MicDeviceInfo>,
) {
    state.mic.backend().set(Some(backend));
    state.mic.strategy().set(MicStrategy::Selected);
    state.mic.selected_device().set(selected);
    state.mic.device_info().set(device_info);
    state.mic.show_chooser().set(false);
    state.mic.acquisition_state().set(MicAcquisitionState::Idle);
    crate::audio::microphone::stop_all(state);
}

/// Take and clear the action the user deferred when the chooser opened.
fn take_pending_action(state: &AppState) -> Option<MicPendingAction> {
    let pending = state.mic.pending_action().get_untracked();
    state.mic.pending_action().set(None);
    pending
}

/// Run a deferred mic action (the chooser was opened mid-request).
async fn dispatch_pending_action(state: &AppState, action: MicPendingAction) {
    match action {
        MicPendingAction::Listen => crate::audio::microphone::toggle_listen(state).await,
        MicPendingAction::Record => crate::audio::microphone::toggle_record(state).await,
        MicPendingAction::Arm => crate::audio::microphone::arm_live_doc(state).await,
    }
}

/// Non-cpal device rows: dispatch the deferred action (if any) directly. Cpal
/// rows instead spawn a task that first queries supported rates + mic info, so
/// they don't use this.
fn run_pending_action(state: AppState) {
    if let Some(action) = take_pending_action(&state) {
        spawn_local(async move { dispatch_pending_action(&state, action).await });
    }
}

#[derive(Clone, Debug)]
struct CpalDevice {
    name: String,
    is_default: bool,
    rates_summary: String,
    format: String,
    rates: Vec<u32>,
    bit_depths: Vec<u16>,
}

#[derive(Clone, Debug)]
struct UsbDevice {
    device_name: String,
    product_name: String,
    has_permission: bool,
}

#[derive(Clone, Debug)]
struct BrowserDevice {
    device_id: String,
    label: String,
    is_default: bool,
}

#[component]
pub fn MicChooserModal() -> impl IntoView {
    let state = expect_context::<AppState>();

    let cpal_devices: RwSignal<Vec<CpalDevice>> = RwSignal::new(Vec::new());
    let cpal_host_name: RwSignal<String> = RwSignal::new(String::new());
    let usb_devices: RwSignal<Vec<UsbDevice>> = RwSignal::new(Vec::new());
    let browser_devices: RwSignal<Vec<BrowserDevice>> = RwSignal::new(Vec::new());
    let browser_needs_permission = RwSignal::new(false);
    let loading = RwSignal::new(true);

    // Enumerate browser audio input devices (non-Tauri only)
    if !state.is_tauri {
        spawn_local(async move {
            enumerate_browser_devices(browser_devices, browser_needs_permission).await;
        });
    }

    // Fetch cpal + USB device lists. Wrapped in a closure so it runs on mount AND
    // re-runs on a USB hot-plug while the chooser is open (Effect below), so a
    // freshly-attached mic shows up without reopening the dialog.
    let refresh_devices = move || {
        spawn_local(async move {
            // Fetch cpal devices via the typed IPC boundary (oversample_ipc::mic).
            match tauri_invoke_typed_no_args::<oversample_ipc::mic::DeviceListResult>(
                "mic_list_devices",
            )
            .await
            {
                Ok(result) => {
                    cpal_host_name.set(result.host_name);
                    let mut devs = Vec::new();
                    for dev in result.devices {
                        // Collect the common rates each device's ranges support,
                        // plus the distinct sample-format tags.
                        let mut rates = Vec::new();
                        let mut formats = std::collections::BTreeSet::new();
                        for range in &dev.sample_rate_ranges {
                            if !range.format.is_empty() {
                                formats.insert(range.format.clone());
                            }
                            for &r in &[44100u32, 48000, 96000, 192000, 256000, 384000, 500000] {
                                if r >= range.min && r <= range.max && !rates.contains(&r) {
                                    rates.push(r);
                                }
                            }
                        }
                        rates.sort();
                        let rates_summary = rates
                            .iter()
                            .map(|&r| {
                                if r >= 1000 {
                                    format!("{}k", r / 1000)
                                } else {
                                    format!("{}", r)
                                }
                            })
                            .collect::<Vec<_>>()
                            .join(", ");
                        // Derive bit depths from format strings before consuming formats
                        let bit_depths: Vec<u16> = formats
                            .iter()
                            .filter_map(|f| {
                                if f.contains("I16") {
                                    Some(16)
                                } else if f.contains("I24") {
                                    Some(24)
                                } else if f.contains("I32") {
                                    Some(32)
                                } else if f.contains("F32") {
                                    Some(32)
                                } else {
                                    None
                                }
                            })
                            .collect();
                        let format = formats.into_iter().collect::<Vec<_>>().join(", ");
                        devs.push(CpalDevice {
                            name: dev.name,
                            is_default: dev.is_default,
                            rates_summary,
                            format,
                            rates,
                            bit_depths,
                        });
                    }
                    cpal_devices.set(devs);
                }
                Err(e) => log::warn!("Failed to list cpal devices: {}", e),
            }

            // Fetch USB devices (if available)
            if let Ok(result) = tauri_invoke_typed_no_args::<
                oversample_ipc::plugins::UsbDeviceListResult,
            >("plugin:usb-audio|listUsbDevices")
            .await
            {
                let devs = result
                    .devices
                    .into_iter()
                    .filter(|d| d.is_audio_device)
                    .map(|d| {
                        let product_name = if d.product_name.is_empty() {
                            d.device_name.clone()
                        } else {
                            d.product_name
                        };
                        UsbDevice {
                            device_name: d.device_name,
                            product_name,
                            has_permission: d.has_permission,
                        }
                    })
                    .collect();
                usb_devices.set(devs);
            }

            loading.set(false);
        });
    };
    refresh_devices();

    // Re-enumerate device lists when a USB device is hot-plugged while the chooser
    // is open, so the just-attached mic appears in the list immediately.
    Effect::new(move |_| {
        let _ = state.mic.hotplug_seq().get();
        if state.mic.show_chooser().get_untracked() {
            refresh_devices();
        }
    });

    let on_close = move |_: web_sys::MouseEvent| {
        state.mic.show_chooser().set(false);
        state.mic.pending_action().set(None);
        if state.mic.acquisition_state().get_untracked() == MicAcquisitionState::AwaitingChoice {
            state.mic.acquisition_state().set(MicAcquisitionState::Idle);
        }
    };

    let on_content_click = move |ev: web_sys::MouseEvent| {
        ev.stop_propagation();
    };

    view! {
        <div class="xc-modal-overlay" on:click=on_close>
            <div class="xc-modal" style="width: min(90vw, 480px);" on:click=on_content_click>
                <div class="xc-modal-header">
                    <span class="xc-modal-title">"Choose Microphone"</span>
                    <button class="xc-modal-close" on:click=on_close>{"\u{00D7}"}</button>
                </div>

                <div style="padding: 8px 16px; overflow-y: auto; max-height: 60vh;">
                    {move || loading.get().then(|| view! {
                        <div style="color: #888; padding: 16px; text-align: center;">"Loading devices\u{2026}"</div>
                    })}

                    // USB (Raw) section — shown first (ideal for bat detectors)
                    {move || {
                        let devs = usb_devices.get();
                        if devs.is_empty() {
                            return None;
                        }
                        Some(view! {
                            <div class="mic-chooser-group-title">"USB (Raw)"</div>
                            <div class="mic-chooser-group-subtitle">"Recommended for bat detectors"</div>
                            {devs.into_iter().map(|dev| {
                                let dev_name = dev.device_name.clone();
                                let dev_name_for_class = dev.device_name.clone();
                                let dev_name_for_badge = dev.device_name.clone();
                                let product = dev.product_name.clone();
                                let product_for_click = dev.product_name.clone();
                                let has_perm = dev.has_permission;
                                view! {
                                    <div
                                        class=move || {
                                            let sel = state.mic.selected_device().get() == Some(dev_name_for_class.clone())
                                                && state.mic.backend().get() == Some(MicBackend::RawUsb);
                                            if sel { "mic-chooser-device selected" } else { "mic-chooser-device" }
                                        }
                                        on:click=move |_| {
                                            select_device(&state, MicBackend::RawUsb, Some(dev_name.clone()), Some(MicDeviceInfo {
                                                name: product_for_click.clone(),
                                                connection_type: "USB".to_string(),
                                                supported_rates: vec![44100, 48000, 96000, 192000, 256000, 384000, 500000],
                                                supported_bit_depths: vec![16],
                                                max_channels: 1,
                                            }));
                                            // Front-load USB access on selection so a later recording
                                            // starts without a permission prompt (the chooser's purpose).
                                            // A deferred record/listen requests permission itself, so
                                            // only prompt here when nothing is pending.
                                            match take_pending_action(&state) {
                                                Some(action) => spawn_local(async move {
                                                    dispatch_pending_action(&state, action).await;
                                                }),
                                                None => {
                                                    let name = dev_name.clone();
                                                    spawn_local(async move {
                                                        crate::audio::microphone::ensure_usb_permission(&state, &name).await;
                                                    });
                                                }
                                            }
                                        }
                                    >
                                        <div class="mic-chooser-device-name">
                                            {product}
                                            {(!has_perm).then(|| view! {
                                                <span class="mic-chooser-device-badge warn">"needs permission"</span>
                                            })}
                                            {move || {
                                                let sel = state.mic.selected_device().get() == Some(dev_name_for_badge.clone())
                                                    && state.mic.backend().get() == Some(MicBackend::RawUsb);
                                                sel.then(|| view! {
                                                    <span class="mic-chooser-device-badge sel">"selected"</span>
                                                })
                                            }}
                                        </div>
                                        <div class="mic-chooser-device-caps">"Direct USB streaming (up to 500 kHz)"</div>
                                    </div>
                                }
                            }).collect::<Vec<_>>()}
                        })
                    }}

                    // Native Audio (cpal) section
                    {move || {
                        let devs = cpal_devices.get();
                        if devs.is_empty() {
                            return None;
                        }
                        let host = cpal_host_name.get();
                        let title = if host.is_empty() {
                            "System audio".to_string()
                        } else {
                            let category = match host.as_str() {
                                "Oboe" => "Android audio",
                                _ => "System audio",
                            };
                            format!("{}: {}", category, host)
                        };
                        Some(view! {
                            <div class="mic-chooser-group-title" style="margin-top: 8px;">{title}</div>
                            {devs.into_iter().map(|dev| {
                                let dev_name = dev.name.clone();
                                let dev_name2 = dev.name.clone();
                                let dev_name_for_class = dev.name.clone();
                                let dev_name_for_badge = dev.name.clone();
                                let dev_name_for_bits = dev.name.clone();
                                let dev_name_for_sel = dev.name.clone();
                                let dev_name_for_selval = dev.name.clone();
                                let is_default = dev.is_default;
                                let click_rates = dev.rates.clone();
                                let click_bit_depths = dev.bit_depths.clone();
                                view! {
                                    <div
                                        class=move || {
                                            let sel = state.mic.selected_device().get() == Some(dev_name_for_class.clone())
                                                && state.mic.backend().get() == Some(MicBackend::Cpal);
                                            if sel { "mic-chooser-device selected" } else { "mic-chooser-device" }
                                        }
                                        on:click=move |_| {
                                            let name = dev_name.clone();
                                            select_device(&state, MicBackend::Cpal, Some(name.clone()), Some(MicDeviceInfo {
                                                name,
                                                connection_type: "Native".to_string(),
                                                supported_rates: click_rates.clone(),
                                                supported_bit_depths: click_bit_depths.clone(),
                                                max_channels: 2,
                                            }));
                                            let pending = take_pending_action(&state);
                                            spawn_local(async move {
                                                crate::audio::microphone::query_cpal_supported_rates(&state).await;
                                                crate::audio::microphone::query_mic_info(&state).await;
                                                if let Some(action) = pending {
                                                    dispatch_pending_action(&state, action).await;
                                                }
                                            });
                                        }
                                    >
                                        <div class="mic-chooser-device-name">
                                            {dev_name2.clone()}
                                            {is_default.then(|| view! {
                                                <span class="mic-chooser-device-badge">"default"</span>
                                            })}
                                            {move || {
                                                let sel = state.mic.selected_device().get() == Some(dev_name_for_badge.clone())
                                                    && state.mic.backend().get() == Some(MicBackend::Cpal);
                                                sel.then(|| view! {
                                                    <span class="mic-chooser-device-badge sel">"selected"</span>
                                                })
                                            }}
                                        </div>
                                        <div class="mic-chooser-device-caps">
                                            {if !dev.rates_summary.is_empty() {
                                                dev.rates_summary.to_string()
                                            } else {
                                                "No rates reported".to_string()
                                            }}
                                            {if !dev.format.is_empty() {
                                                format!(" \u{2022} {}", dev.format)
                                            } else {
                                                String::new()
                                            }}
                                            // Auto-detected effective bit depth, remembered per-device.
                                            // Shows when the device delivers fewer real bits than its
                                            // container (e.g. a 24-bit interface in a 32-bit stream).
                                            {move || {
                                                state.mic.bit_depths()
                                                    .with(|m| m.get(&dev_name_for_bits).copied())
                                                    .map(|bits| format!(" \u{2022} appears {bits}-bit"))
                                                    .unwrap_or_default()
                                            }}
                                        </div>
                                        // Per-device bit-depth override. Auto uses the auto-detected
                                        // depth; an explicit choice forces it (12-bit zero-pads).
                                        // stop_propagation so using the control doesn't select the device.
                                        <div
                                            class="mic-chooser-device-caps"
                                            on:click=|ev: web_sys::MouseEvent| ev.stop_propagation()
                                        >
                                            "Record at: "
                                            <select
                                                prop:value=move || state.mic.bit_depth_override()
                                                    .with(|m| m.get(&dev_name_for_selval).map(|b| b.to_string()))
                                                    .unwrap_or_else(|| "auto".to_string())
                                                on:change=move |ev| {
                                                    let v = event_target_value(&ev);
                                                    let dev = dev_name_for_sel.clone();
                                                    state.mic.bit_depth_override().update(|m| {
                                                        match v.parse::<u16>() {
                                                            Ok(b) => { m.insert(dev, b); }
                                                            Err(_) => { m.remove(&dev); }
                                                        }
                                                    });
                                                    let map = state.mic.bit_depth_override().get_untracked();
                                                    crate::settings::set_mic_bit_depth_overrides(&map);
                                                }
                                            >
                                                <option value="auto">"Auto"</option>
                                                <option value="12">"12-bit"</option>
                                                <option value="16">"16-bit"</option>
                                                <option value="24">"24-bit"</option>
                                                <option value="32">"32-bit"</option>
                                            </select>
                                        </div>
                                    </div>
                                }
                            }).collect::<Vec<_>>()}
                        })
                    }}

                    // Browser section (hidden on Tauri)
                    {move || {
                        if state.is_tauri { return None; }
                        let devs = browser_devices.get();
                        let needs_perm = browser_needs_permission.get();
                        Some(view! {
                            <div class="mic-chooser-group-title" style="margin-top: 8px;">"Browser Audio Devices"</div>

                            // Permission request button (shown when devices have no labels)
                            {needs_perm.then(|| view! {
                                <div
                                    class="mic-chooser-device"
                                    style="cursor: pointer; text-align: center;"
                                    on:click=move |_| {
                                        spawn_local(async move {
                                            request_browser_permission(browser_devices, browser_needs_permission).await;
                                        });
                                    }
                                >
                                    <div class="mic-chooser-device-name" style="color: #4af;">"Grant microphone access to see devices"</div>
                                    <div class="mic-chooser-device-caps">"Browser will prompt for permission"</div>
                                </div>
                            })}

                            // Enumerated browser devices
                            {devs.into_iter().map(|dev| {
                                let dev_id = dev.device_id.clone();
                                let dev_id_for_class = dev.device_id.clone();
                                let dev_id_for_badge = dev.device_id.clone();
                                let label = dev.label.clone();
                                let is_default = dev.is_default;
                                view! {
                                    <div
                                        class=move || {
                                            let sel = state.mic.selected_device().get() == Some(dev_id_for_class.clone())
                                                && state.mic.backend().get() == Some(MicBackend::Browser);
                                            if sel { "mic-chooser-device selected" } else { "mic-chooser-device" }
                                        }
                                        on:click=move |_| {
                                            select_device(&state, MicBackend::Browser, Some(dev_id.clone()), Some(MicDeviceInfo {
                                                name: label.clone(),
                                                connection_type: "Browser".to_string(),
                                                supported_rates: vec![44100, 48000, 96000],
                                                supported_bit_depths: vec![32],
                                                max_channels: 1,
                                            }));
                                            run_pending_action(state);
                                        }
                                    >
                                        <div class="mic-chooser-device-name">
                                            {label.clone()}
                                            {is_default.then(|| view! {
                                                <span class="mic-chooser-device-badge">"default"</span>
                                            })}
                                            {move || {
                                                let sel = state.mic.selected_device().get() == Some(dev_id_for_badge.clone())
                                                    && state.mic.backend().get() == Some(MicBackend::Browser);
                                                sel.then(|| view! {
                                                    <span class="mic-chooser-device-badge sel">"selected"</span>
                                                })
                                            }}
                                        </div>
                                        <div class="mic-chooser-device-caps">"Web Audio API"</div>
                                    </div>
                                }
                            }).collect::<Vec<_>>()}

                            // Fallback: "Browser default" option (always shown)
                            <div
                                class=move || {
                                    let sel = state.mic.selected_device().get().is_none()
                                        && state.mic.backend().get() == Some(MicBackend::Browser);
                                    if sel { "mic-chooser-device selected" } else { "mic-chooser-device" }
                                }
                                on:click=move |_| {
                                    select_device(&state, MicBackend::Browser, None, Some(MicDeviceInfo {
                                        name: "Browser microphone".to_string(),
                                        connection_type: "Browser".to_string(),
                                        supported_rates: vec![44100, 48000, 96000],
                                        supported_bit_depths: vec![32],
                                        max_channels: 1,
                                    }));
                                    run_pending_action(state);
                                }
                            >
                                <div class="mic-chooser-device-name">
                                    "Browser default"
                                    {move || {
                                        let sel = state.mic.selected_device().get().is_none()
                                            && state.mic.backend().get() == Some(MicBackend::Browser);
                                        sel.then(|| view! {
                                            <span class="mic-chooser-device-badge sel">"selected"</span>
                                        })
                                    }}
                                </div>
                                <div class="mic-chooser-device-caps">"Let browser choose (Web Audio API)"</div>
                            </div>
                        })
                    }}

                    // "Use Default" option (Tauri only — cpal not available in browser)
                    {move || {
                        if !state.is_tauri { return None; }
                        Some(view! {
                            <div class="mic-chooser-group-title" style="margin-top: 8px;"></div>
                            <div
                                class=move || {
                                    let sel = state.mic.selected_device().get().is_none()
                                        && state.mic.backend().get() != Some(MicBackend::Browser)
                                        && state.mic.backend().get() != Some(MicBackend::RawUsb);
                                    if sel { "mic-chooser-device selected" } else { "mic-chooser-device" }
                                }
                                on:click=move |_| {
                                    select_device(&state, MicBackend::Cpal, None, None);
                                    let pending = take_pending_action(&state);
                                    spawn_local(async move {
                                        crate::audio::microphone::query_cpal_supported_rates(&state).await;
                                        crate::audio::microphone::query_mic_info(&state).await;
                                        if let Some(action) = pending {
                                            dispatch_pending_action(&state, action).await;
                                        }
                                    });
                                }
                            >
                                <div class="mic-chooser-device-name">"System default"</div>
                                <div class="mic-chooser-device-caps">"Automatically selected device"</div>
                            </div>
                        })
                    }}
                </div>
            </div>
        </div>
    }
}

/// Enumerate browser audio input devices via `navigator.mediaDevices.enumerateDevices()`.
/// If labels are empty (permission not yet granted), sets `needs_permission` to true.
async fn enumerate_browser_devices(
    devices_signal: RwSignal<Vec<BrowserDevice>>,
    needs_permission: RwSignal<bool>,
) {
    let window = match web_sys::window() {
        Some(w) => w,
        None => return,
    };
    let media_devices = match window.navigator().media_devices() {
        Ok(md) => md,
        Err(_) => return,
    };
    let promise = match media_devices.enumerate_devices() {
        Ok(p) => p,
        Err(_) => return,
    };
    let result = match JsFuture::from(promise).await {
        Ok(v) => v,
        Err(_) => return,
    };

    let arr = js_sys::Array::from(&result);
    let mut devs = Vec::new();
    let mut has_empty_label = false;
    let mut first_audio_input = true;

    for i in 0..arr.length() {
        let item = arr.get(i);
        let kind = js_sys::Reflect::get(&item, &"kind".into())
            .ok()
            .and_then(|v| v.as_string())
            .unwrap_or_default();
        if kind != "audioinput" {
            continue;
        }

        let device_id = js_sys::Reflect::get(&item, &"deviceId".into())
            .ok()
            .and_then(|v| v.as_string())
            .unwrap_or_default();
        let label = js_sys::Reflect::get(&item, &"label".into())
            .ok()
            .and_then(|v| v.as_string())
            .unwrap_or_default();

        if label.is_empty() {
            has_empty_label = true;
            continue; // Skip unlabeled devices — we'll show the permission prompt instead
        }

        // The "default" deviceId or the first audioinput is typically the default
        let is_default = device_id == "default" || first_audio_input;
        first_audio_input = false;

        // Skip the synthetic "default" entry if we already have real devices
        if device_id == "default" {
            continue;
        }

        devs.push(BrowserDevice {
            device_id,
            label,
            is_default: is_default && devs.is_empty(),
        });
    }

    // If we got no labeled devices but there were unlabeled ones, prompt for permission
    if devs.is_empty() && has_empty_label {
        needs_permission.set(true);
    } else {
        needs_permission.set(false);
    }
    devices_signal.set(devs);
}

/// Request mic permission via a temporary getUserMedia call, then re-enumerate.
async fn request_browser_permission(
    devices_signal: RwSignal<Vec<BrowserDevice>>,
    needs_permission: RwSignal<bool>,
) {
    let window = match web_sys::window() {
        Some(w) => w,
        None => return,
    };
    let media_devices = match window.navigator().media_devices() {
        Ok(md) => md,
        Err(_) => return,
    };

    // Request mic access to unlock device labels
    let constraints = web_sys::MediaStreamConstraints::new();
    constraints.set_audio(&JsValue::TRUE);
    let promise = match media_devices.get_user_media_with_constraints(&constraints) {
        Ok(p) => p,
        Err(_) => return,
    };
    if let Ok(stream_js) = JsFuture::from(promise).await {
        // Stop the temporary stream immediately
        if let Ok(stream) = stream_js.dyn_into::<web_sys::MediaStream>() {
            for track in stream.get_tracks().iter() {
                if let Ok(track) = track.dyn_into::<web_sys::MediaStreamTrack>() {
                    track.stop();
                }
            }
        }
    }

    // Re-enumerate now that permission is granted
    enumerate_browser_devices(devices_signal, needs_permission).await;
}
