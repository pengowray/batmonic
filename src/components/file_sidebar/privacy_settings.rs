use crate::state::store_fields::*;
use crate::state::AppState;
use crate::tauri_bridge::{tauri_invoke, tauri_invoke_typed_no_args};
use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;

fn persist_home_wifi(state: &AppState) {
    let val = state
        .recording_meta
        .home_wifi_ssids()
        .with_untracked(|list| list.join("\n"));
    crate::settings::set_raw(crate::settings::keys::HOME_WIFI, &val);
}

#[derive(Clone, Copy, PartialEq)]
enum ZoneStatus {
    Loading,
    Active,
    NotOnZone,
    NoWifi,
}

/// Check current WiFi against saved privacy zones.
fn check_zone_status(state: AppState, status: RwSignal<ZoneStatus>) {
    status.set(ZoneStatus::Loading);
    spawn_local(async move {
        let ssid = match tauri_invoke_typed_no_args::<oversample_ipc::plugins::WifiSsidResult>(
            "plugin:geolocation|getWifiSsid",
        )
        .await
        {
            Ok(r) => r.ssid,
            Err(_) => {
                status.set(ZoneStatus::NoWifi);
                return;
            }
        };
        match ssid {
            Some(ssid) => {
                let on_zone = state
                    .recording_meta
                    .home_wifi_ssids()
                    .with_untracked(|list| list.contains(&ssid));
                status.set(if on_zone {
                    ZoneStatus::Active
                } else {
                    ZoneStatus::NotOnZone
                });
            }
            None => status.set(ZoneStatus::NoWifi),
        }
    });
}

#[component]
pub fn PrivacySettingsModal() -> impl IntoView {
    let state = expect_context::<AppState>();

    let zone_status = RwSignal::new(ZoneStatus::Loading);
    let adding = RwSignal::new(false);
    let confirm_clear = RwSignal::new(false);

    // One-time check on modal open
    check_zone_status(state, zone_status);

    let on_close = move |_: web_sys::MouseEvent| {
        state.dialogs.privacy_settings().set(false);
    };

    let on_content_click = move |ev: web_sys::MouseEvent| {
        ev.stop_propagation();
    };

    let on_refresh = move |_: web_sys::MouseEvent| {
        check_zone_status(state, zone_status);
    };

    let on_add_network = move |_: web_sys::MouseEvent| {
        adding.set(true);
        spawn_local(async move {
            // Request location permission first (needed for WiFi SSID on Android)
            let perm_ok = tauri_invoke(
                "plugin:geolocation|getCurrentLocation",
                &JsValue::from(js_sys::Object::new()),
            )
            .await
            .is_ok();
            if !perm_ok {
                state.show_info_toast("Location permission is needed to detect WiFi network");
                adding.set(false);
                return;
            }
            // Read WiFi SSID
            let ssid_result =
                tauri_invoke_typed_no_args::<oversample_ipc::plugins::WifiSsidResult>(
                    "plugin:geolocation|getWifiSsid",
                )
                .await;
            adding.set(false);
            let ssid = match ssid_result {
                Ok(r) => r.ssid,
                Err(_) => {
                    state.show_info_toast("Could not read WiFi network");
                    return;
                }
            };
            if let Some(ssid) = ssid {
                let already = state
                    .recording_meta
                    .home_wifi_ssids()
                    .with_untracked(|list| list.contains(&ssid));
                if !already {
                    state
                        .recording_meta
                        .home_wifi_ssids()
                        .update(|list| list.push(ssid));
                    persist_home_wifi(&state);
                    state.show_info_toast("Privacy zone added");
                } else {
                    state.show_info_toast("Network already added");
                }
                // Refresh status after adding
                check_zone_status(state, zone_status);
            } else {
                state.show_info_toast("Not connected to WiFi");
            }
        });
    };

    let on_clear = move |_: web_sys::MouseEvent| {
        if confirm_clear.get_untracked() {
            state.recording_meta.home_wifi_ssids().set(Vec::new());
            persist_home_wifi(&state);
            confirm_clear.set(false);
            state.show_info_toast("All privacy zones cleared");
            check_zone_status(state, zone_status);
        } else {
            confirm_clear.set(true);
            // Reset after 3 seconds
            if let Some(w) = web_sys::window() {
                let cb = Closure::once(move || confirm_clear.set(false));
                let _ = w.set_timeout_with_callback_and_timeout_and_arguments_0(
                    cb.as_ref().unchecked_ref(),
                    3000,
                );
                cb.forget();
            }
        }
    };

    view! {
        <div class="xc-modal-overlay" on:click=on_close>
            <div class="xc-modal" style="width: min(90vw, 480px);" on:click=on_content_click>
                <div class="xc-modal-header">
                    <span class="xc-modal-title">"Privacy Settings"</span>
                    <button class="xc-modal-close" on:click=on_close>{"\u{00D7}"}</button>
                </div>

                <div style="overflow-y: auto; max-height: 70vh;">
                    // ── Privacy Zones section ──
                    <div class="privacy-section">
                        <div class="privacy-section-title">"Privacy Zones"</div>
                        <div class="privacy-section-desc">
                            "When connected to a privacy zone network, GPS location won\u{2019}t be added to recordings. "
                            "Useful for avoiding location tags on test recordings at home."
                        </div>

                        // Live status banner
                        <div class="privacy-status-banner">
                            {move || match zone_status.get() {
                                ZoneStatus::Loading => view! {
                                    <span style="color: #888;">"Checking\u{2026}"</span>
                                }.into_any(),
                                ZoneStatus::Active => view! {
                                    <span style="color: #f88;">"GPS paused \u{2014} on a privacy zone network"</span>
                                }.into_any(),
                                ZoneStatus::NotOnZone => view! {
                                    <span style="color: #8c8;">"Not on a privacy zone network"</span>
                                }.into_any(),
                                ZoneStatus::NoWifi => view! {
                                    <span style="color: #888;">"Not connected to WiFi"</span>
                                }.into_any(),
                            }}
                            <button class="privacy-refresh-btn" on:click=on_refresh title="Check current network">
                                "Check now"
                            </button>
                        </div>

                        // Saved networks list
                        {move || {
                            let ssids = state.recording_meta.home_wifi_ssids().get();
                            if ssids.is_empty() {
                                view! {
                                    <div class="privacy-ssid-list">
                                        <div style="color: #666; font-style: italic;">"No privacy zones configured"</div>
                                    </div>
                                }.into_any()
                            } else {
                                view! {
                                    <div class="privacy-ssid-list">
                                        {ssids.into_iter().map(|ssid| view! {
                                            <div class="privacy-ssid-item">{ssid}</div>
                                        }).collect::<Vec<_>>()}
                                    </div>
                                }.into_any()
                            }
                        }}

                        // Action buttons
                        <button
                            class="privacy-btn"
                            on:click=on_add_network
                            disabled=move || adding.get()
                        >
                            {move || if adding.get() { "Requesting permission\u{2026}" } else { "Add current network" }}
                        </button>

                        {move || {
                            let count = state.recording_meta.home_wifi_ssids().with(|list| list.len());
                            if count > 0 {
                                view! {
                                    <button
                                        class="privacy-btn danger"
                                        on:click=on_clear
                                    >
                                        {move || if confirm_clear.get() { "Tap again to confirm" } else { "Clear all privacy zones" }}
                                    </button>
                                }.into_any()
                            } else {
                                view! { <span></span> }.into_any()
                            }
                        }}
                    </div>

                    // ── Recording Metadata section ──
                    <div class="privacy-section">
                        <div class="privacy-section-title">"Recording Metadata"</div>
                        <div class="setting-row" style="padding: 8px 0;">
                            <span class="setting-label" style="font-size: 13px;">"Include phone model"</span>
                            <input
                                type="checkbox"
                                class="setting-checkbox"
                                prop:checked=move || state.recording_meta.device_model_enabled().get()
                                on:change=move |ev: web_sys::Event| {
                                    let target = ev.target().unwrap();
                                    let input: web_sys::HtmlInputElement = target.unchecked_into();
                                    let checked = input.checked();
                                    state.recording_meta.device_model_enabled().set(checked);
                                    crate::settings::set_bool(crate::settings::keys::DEVICE_MODEL, checked);
                                }
                            />
                        </div>
                        <div class="privacy-section-desc" style="margin-top: 0;">
                            "Adds device manufacturer and model to recording metadata"
                        </div>
                    </div>
                </div>
            </div>
        </div>
    }
}
