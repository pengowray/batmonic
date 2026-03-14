use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::JsValue;
use crate::state::{AppState, MicMode};
use crate::tauri_bridge::{tauri_invoke, tauri_invoke_no_args};

#[derive(Clone, Debug)]
struct CpalDevice {
    name: String,
    is_default: bool,
    rates_summary: String,
    format: String,
}

#[derive(Clone, Debug)]
struct UsbDevice {
    device_name: String,
    product_name: String,
    has_permission: bool,
}

#[component]
pub fn MicChooserModal() -> impl IntoView {
    let state = expect_context::<AppState>();

    let cpal_devices: RwSignal<Vec<CpalDevice>> = RwSignal::new(Vec::new());
    let usb_devices: RwSignal<Vec<UsbDevice>> = RwSignal::new(Vec::new());
    let loading = RwSignal::new(true);

    // Fetch devices on mount
    spawn_local(async move {
        // Fetch cpal devices
        match tauri_invoke_no_args("mic_list_devices").await {
            Ok(val) => {
                let arr = js_sys::Array::from(&val);
                let mut devs = Vec::new();
                for i in 0..arr.length() {
                    let item = arr.get(i);
                    let name = js_sys::Reflect::get(&item, &JsValue::from_str("name"))
                        .ok()
                        .and_then(|v| v.as_string())
                        .unwrap_or_else(|| "Unknown".into());
                    let is_default = js_sys::Reflect::get(&item, &JsValue::from_str("is_default"))
                        .ok()
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);

                    // Parse sample rate ranges
                    let ranges_val = js_sys::Reflect::get(&item, &JsValue::from_str("sample_rate_ranges"))
                        .ok()
                        .unwrap_or(JsValue::UNDEFINED);
                    let ranges_arr = js_sys::Array::from(&ranges_val);
                    let mut rates = Vec::new();
                    let mut formats = std::collections::BTreeSet::new();
                    for j in 0..ranges_arr.length() {
                        let range = ranges_arr.get(j);
                        let min = js_sys::Reflect::get(&range, &JsValue::from_str("min"))
                            .ok().and_then(|v| v.as_f64()).unwrap_or(0.0) as u32;
                        let max = js_sys::Reflect::get(&range, &JsValue::from_str("max"))
                            .ok().and_then(|v| v.as_f64()).unwrap_or(0.0) as u32;
                        let fmt = js_sys::Reflect::get(&range, &JsValue::from_str("format"))
                            .ok().and_then(|v| v.as_string()).unwrap_or_default();
                        if !fmt.is_empty() {
                            formats.insert(fmt);
                        }
                        // Collect common rates within range
                        for &r in &[44100u32, 48000, 96000, 192000, 256000, 384000, 500000] {
                            if r >= min && r <= max && !rates.contains(&r) {
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
                    let format = formats.into_iter().collect::<Vec<_>>().join(", ");

                    devs.push(CpalDevice { name, is_default, rates_summary, format });
                }
                cpal_devices.set(devs);
            }
            Err(e) => log::warn!("Failed to list cpal devices: {}", e),
        }

        // Fetch USB devices (if available)
        let usb_args = js_sys::Object::new();
        match tauri_invoke("plugin:usb-audio|listUsbDevices", &usb_args.into()).await {
            Ok(val) => {
                let devices_val = js_sys::Reflect::get(&val, &JsValue::from_str("devices"))
                    .ok()
                    .unwrap_or(JsValue::UNDEFINED);
                let arr = js_sys::Array::from(&devices_val);
                let mut devs = Vec::new();
                for i in 0..arr.length() {
                    let item = arr.get(i);
                    let is_audio = js_sys::Reflect::get(&item, &JsValue::from_str("isAudioDevice"))
                        .ok().and_then(|v| v.as_bool()).unwrap_or(false);
                    if !is_audio { continue; }
                    let device_name = js_sys::Reflect::get(&item, &JsValue::from_str("deviceName"))
                        .ok().and_then(|v| v.as_string()).unwrap_or_default();
                    let product_name = js_sys::Reflect::get(&item, &JsValue::from_str("productName"))
                        .ok().and_then(|v| v.as_string()).unwrap_or_else(|| device_name.clone());
                    let has_permission = js_sys::Reflect::get(&item, &JsValue::from_str("hasPermission"))
                        .ok().and_then(|v| v.as_bool()).unwrap_or(false);
                    devs.push(UsbDevice { device_name, product_name, has_permission });
                }
                usb_devices.set(devs);
            }
            Err(_) => {} // USB plugin not available (not Android)
        }

        loading.set(false);
    });

    let on_close = move |_: web_sys::MouseEvent| {
        state.show_mic_chooser.set(false);
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

                    // Native Audio (cpal) section
                    {move || {
                        let devs = cpal_devices.get();
                        if devs.is_empty() && !loading.get() {
                            return None;
                        }
                        if devs.is_empty() {
                            return None;
                        }
                        Some(view! {
                            <div class="mic-chooser-group-title">"Native Audio"</div>
                            {devs.into_iter().map(|dev| {
                                let dev_name = dev.name.clone();
                                let dev_name2 = dev.name.clone();
                                let dev_name_for_class = dev.name.clone();
                                let dev_name_for_badge = dev.name.clone();
                                let is_default = dev.is_default;
                                view! {
                                    <div
                                        class=move || {
                                            let sel = state.mic_selected_device.get() == Some(dev_name_for_class.clone())
                                                && state.mic_mode.get() != MicMode::RawUsb;
                                            if sel { "mic-chooser-device selected" } else { "mic-chooser-device" }
                                        }
                                        on:click=move |_| {
                                            let name = dev_name.clone();
                                            state.mic_mode.set(MicMode::Cpal);
                                            state.mic_selected_device.set(Some(name));
                                            state.show_mic_chooser.set(false);
                                            crate::audio::microphone::stop_all(&state);
                                            spawn_local(async move {
                                                crate::audio::microphone::query_cpal_supported_rates(&state).await;
                                                crate::audio::microphone::query_mic_info(&state).await;
                                            });
                                        }
                                    >
                                        <div class="mic-chooser-device-name">
                                            {dev_name2.clone()}
                                            {is_default.then(|| view! {
                                                <span class="mic-chooser-device-badge">"default"</span>
                                            })}
                                            {move || {
                                                let sel = state.mic_selected_device.get() == Some(dev_name_for_badge.clone())
                                                    && state.mic_mode.get() != MicMode::RawUsb;
                                                sel.then(|| view! {
                                                    <span class="mic-chooser-device-badge sel">"selected"</span>
                                                })
                                            }}
                                        </div>
                                        <div class="mic-chooser-device-caps">
                                            {if !dev.rates_summary.is_empty() {
                                                format!("{}", dev.rates_summary)
                                            } else {
                                                "No rates reported".to_string()
                                            }}
                                            {if !dev.format.is_empty() {
                                                format!(" \u{2022} {}", dev.format)
                                            } else {
                                                String::new()
                                            }}
                                        </div>
                                    </div>
                                }
                            }).collect::<Vec<_>>()}
                        })
                    }}

                    // USB (Raw) section
                    {move || {
                        let devs = usb_devices.get();
                        if devs.is_empty() {
                            return None;
                        }
                        Some(view! {
                            <div class="mic-chooser-group-title" style="margin-top: 8px;">"USB (Raw)"</div>
                            {devs.into_iter().map(|dev| {
                                let dev_name = dev.device_name.clone();
                                let dev_name_for_class = dev.device_name.clone();
                                let dev_name_for_badge = dev.device_name.clone();
                                let product = dev.product_name.clone();
                                let has_perm = dev.has_permission;
                                view! {
                                    <div
                                        class=move || {
                                            let sel = state.mic_selected_device.get() == Some(dev_name_for_class.clone())
                                                && state.mic_mode.get() == MicMode::RawUsb;
                                            if sel { "mic-chooser-device selected" } else { "mic-chooser-device" }
                                        }
                                        on:click=move |_| {
                                            let name = dev_name.clone();
                                            state.mic_mode.set(MicMode::RawUsb);
                                            state.mic_selected_device.set(Some(name));
                                            state.show_mic_chooser.set(false);
                                            crate::audio::microphone::stop_all(&state);
                                        }
                                    >
                                        <div class="mic-chooser-device-name">
                                            {product}
                                            {(!has_perm).then(|| view! {
                                                <span class="mic-chooser-device-badge warn">"needs permission"</span>
                                            })}
                                            {move || {
                                                let sel = state.mic_selected_device.get() == Some(dev_name_for_badge.clone())
                                                    && state.mic_mode.get() == MicMode::RawUsb;
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

                    // Browser section
                    {move || {
                        Some(view! {
                            <div class="mic-chooser-group-title" style="margin-top: 8px;">"Browser"</div>
                            <div
                                class=move || if state.mic_mode.get() == MicMode::Browser {
                                    "mic-chooser-device selected"
                                } else {
                                    "mic-chooser-device"
                                }
                                on:click=move |_| {
                                    state.mic_mode.set(MicMode::Browser);
                                    state.mic_selected_device.set(None);
                                    state.show_mic_chooser.set(false);
                                    crate::audio::microphone::stop_all(&state);
                                }
                            >
                                <div class="mic-chooser-device-name">
                                    "Browser microphone"
                                    {move || (state.mic_mode.get() == MicMode::Browser).then(|| view! {
                                        <span class="mic-chooser-device-badge sel">"selected"</span>
                                    })}
                                </div>
                                <div class="mic-chooser-device-caps">"Web Audio API (max ~96 kHz)"</div>
                            </div>
                        })
                    }}

                    // "Use Default" option
                    {move || {
                        Some(view! {
                            <div class="mic-chooser-group-title" style="margin-top: 8px;"></div>
                            <div
                                class=move || {
                                    let sel = state.mic_selected_device.get().is_none()
                                        && state.mic_mode.get() != MicMode::Browser
                                        && state.mic_mode.get() != MicMode::RawUsb;
                                    if sel { "mic-chooser-device selected" } else { "mic-chooser-device" }
                                }
                                on:click=move |_| {
                                    state.mic_mode.set(MicMode::Auto);
                                    state.mic_selected_device.set(None);
                                    state.show_mic_chooser.set(false);
                                    crate::audio::microphone::stop_all(&state);
                                    spawn_local(async move {
                                        let resolved = crate::audio::microphone::resolve_auto_mode(&state).await;
                                        if resolved == Some(MicMode::Cpal) {
                                            crate::audio::microphone::query_cpal_supported_rates(&state).await;
                                        }
                                        crate::audio::microphone::query_mic_info(&state).await;
                                    });
                                }
                            >
                                <div class="mic-chooser-device-name">"Auto (system default)"</div>
                                <div class="mic-chooser-device-caps">"Automatically select the best available device"</div>
                            </div>
                        })
                    }}
                </div>
            </div>
        </div>
    }
}
