use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::JsCast;
use crate::canvas::spectrogram_renderer::Colormap;
use crate::state::{AppState, ChromaColormap, MicMode};
use super::mic_chooser::MicChooserModal;

fn parse_colormap_pref(s: &str) -> Colormap {
    match s {
        "inferno" => Colormap::Inferno,
        "magma" => Colormap::Magma,
        "plasma" => Colormap::Plasma,
        "cividis" => Colormap::Cividis,
        "turbo" => Colormap::Turbo,
        "greyscale" => Colormap::Greyscale,
        _ => Colormap::Viridis,
    }
}

#[component]
pub(super) fn ConfigPanel() -> impl IntoView {
    let state = expect_context::<AppState>();

    let on_follow_cursor = move |ev: web_sys::Event| {
        let target = ev.target().unwrap();
        let input: web_sys::HtmlInputElement = target.unchecked_into();
        let checked = input.checked();
        state.follow_cursor.set(checked);
        if checked {
            state.follow_suspended.set(false);
            state.follow_visible_since.set(None);
        }
    };

    let on_always_show_view_range = move |ev: web_sys::Event| {
        let target = ev.target().unwrap();
        let input: web_sys::HtmlInputElement = target.unchecked_into();
        state.always_show_view_range.set(input.checked());
    };

    let on_colormap_change = move |ev: web_sys::Event| {
        let target = ev.target().unwrap();
        let select: web_sys::HtmlSelectElement = target.unchecked_into();
        state.colormap_preference.set(parse_colormap_pref(&select.value()));
        state.tile_ready_signal.update(|n| *n = n.wrapping_add(1));
    };

    let on_hfr_colormap_change = move |ev: web_sys::Event| {
        let target = ev.target().unwrap();
        let select: web_sys::HtmlSelectElement = target.unchecked_into();
        state.hfr_colormap_preference.set(parse_colormap_pref(&select.value()));
        state.tile_ready_signal.update(|n| *n = n.wrapping_add(1));
    };

    let on_mic_mode_change = move |ev: web_sys::Event| {
        let target = ev.target().unwrap();
        let select: web_sys::HtmlSelectElement = target.unchecked_into();
        let mode = match select.value().as_str() {
            "auto" => MicMode::Auto,
            "cpal" => MicMode::Cpal,
            "raw_usb" => MicMode::RawUsb,
            _ => MicMode::Browser,
        };
        state.mic_mode.set(mode);
        if mode == MicMode::Auto && state.is_tauri {
            // Resolve auto mode first (checks USB, requests permission), then query info
            spawn_local(async move {
                let resolved = crate::audio::microphone::resolve_auto_mode(&state).await;
                if resolved == Some(MicMode::Cpal) {
                    crate::audio::microphone::query_cpal_supported_rates(&state).await;
                }
                crate::audio::microphone::query_mic_info(&state).await;
            });
        } else if mode == MicMode::Cpal && state.is_tauri {
            // Query cpal rates when switching to native audio
            spawn_local(async move {
                crate::audio::microphone::query_cpal_supported_rates(&state).await;
                crate::audio::microphone::query_mic_info(&state).await;
            });
        } else if mode == MicMode::Browser && state.is_tauri {
            // Request RECORD_AUDIO permission when switching to Browser mode on Tauri (Android)
            spawn_local(async move {
                crate::audio::microphone::request_audio_permission_tauri(&state).await;
            });
        }
    };

    let on_max_sr_change = move |ev: web_sys::Event| {
        let target = ev.target().unwrap();
        let select: web_sys::HtmlSelectElement = target.unchecked_into();
        let val: u32 = select.value().parse().unwrap_or(0);
        state.mic_max_sample_rate.set(val);
    };

    let is_tauri = state.is_tauri;

    // Query device info on initial render
    if is_tauri {
        let mode = state.mic_mode.get_untracked();
        if mode == MicMode::Cpal || mode == MicMode::Auto {
            spawn_local(async move {
                crate::audio::microphone::query_cpal_supported_rates(&state).await;
                crate::audio::microphone::query_mic_info(&state).await;
            });
        }
    }

    // Re-query mic info when USB connection status or mic mode changes
    if is_tauri {
        Effect::new(move |_| {
            let _usb = state.mic_usb_connected.get(); // subscribe to USB changes
            let _mode = state.mic_mode.get(); // subscribe to mode changes
            spawn_local(async move {
                crate::audio::microphone::query_mic_info(&state).await;
            });
        });
    }

    // Check if a specific rate is available based on mode + actual device rates
    let rate_available = move |rate: u32| -> bool {
        let mode = state.mic_mode.get();
        let effective = if mode == MicMode::Auto {
            state.mic_effective_mode.get()
        } else {
            mode
        };
        match effective {
            MicMode::Browser => rate <= 96_000,
            MicMode::RawUsb => rate <= 500_000,
            MicMode::Auto | MicMode::Cpal => {
                let rates = state.mic_supported_rates.get();
                if rates.is_empty() {
                    rate <= 192_000
                } else {
                    rates.iter().any(|&r| r >= rate)
                }
            }
        }
    };

    view! {
        <div class="sidebar-panel">
            <div class="setting-group">
                <div class="setting-group-title">"Recording"</div>
                <div class="setting-row">
                    <span class="setting-label">"Mic mode"</span>
                    <select
                        class="setting-select"
                        on:change=on_mic_mode_change
                    >
                        <option value="auto"
                            selected=move || state.mic_mode.get() == MicMode::Auto
                            disabled=move || !is_tauri
                        >"Auto"</option>
                        <option value="browser"
                            selected=move || state.mic_mode.get() == MicMode::Browser
                            disabled=move || is_tauri
                        >{move || if is_tauri { "Browser (unavailable)" } else { "Browser" }}</option>
                        <option value="cpal"
                            selected=move || state.mic_mode.get() == MicMode::Cpal
                            disabled=move || !is_tauri
                        >"Native audio"</option>
                        <option value="raw_usb"
                            selected=move || state.mic_mode.get() == MicMode::RawUsb
                            disabled=move || !is_tauri
                        >"Raw USB"</option>
                    </select>
                </div>
                // Effective mode hint (only when Auto is selected in Tauri)
                {move || {
                    if !is_tauri || state.mic_mode.get() != MicMode::Auto {
                        return None;
                    }
                    let eff = state.mic_effective_mode.get();
                    let needs_perm = state.mic_needs_permission.get();
                    let label = match eff {
                        MicMode::RawUsb if needs_perm => "USB detected (needs permission)",
                        MicMode::RawUsb => "Using: USB (Raw)",
                        MicMode::Cpal => "Using: Native audio",
                        _ => "Using: Browser",
                    };
                    Some(view! {
                        <div class="mic-mode-hint">{label}</div>
                    })
                }}
                // Choose mic button (Tauri only)
                {is_tauri.then(|| view! {
                    <div class="setting-row">
                        <span class="setting-label">"Device"</span>
                        <button
                            class="setting-btn"
                            on:click=move |_| state.show_mic_chooser.set(true)
                        >{move || {
                            match state.mic_selected_device.get() {
                                Some(name) => {
                                    if name.len() > 16 {
                                        format!("{}\u{2026}", &name[..15])
                                    } else {
                                        name
                                    }
                                }
                                None => "Default".to_string(),
                            }
                        }}</button>
                    </div>
                })}
                <div class="setting-row">
                    <span class="setting-label">"Max sample rate"</span>
                    <select
                        class="setting-select"
                        on:change=on_max_sr_change
                    >
                        <option value="0" selected=move || state.mic_max_sample_rate.get() == 0>"Auto (native)"</option>
                        <option value="44100" selected=move || state.mic_max_sample_rate.get() == 44100
                            disabled=move || !rate_available(44100)
                        >"44.1 kHz"</option>
                        <option value="48000" selected=move || state.mic_max_sample_rate.get() == 48000
                            disabled=move || !rate_available(48000)
                        >"48 kHz"</option>
                        <option value="96000" selected=move || state.mic_max_sample_rate.get() == 96000
                            disabled=move || !rate_available(96000)
                        >"96 kHz"</option>
                        <option value="192000" selected=move || state.mic_max_sample_rate.get() == 192000
                            disabled=move || !rate_available(192000)
                        >"192 kHz"</option>
                        <option value="256000" selected=move || state.mic_max_sample_rate.get() == 256000
                            disabled=move || !rate_available(256000)
                        >"256 kHz"</option>
                        <option value="384000" selected=move || state.mic_max_sample_rate.get() == 384000
                            disabled=move || !rate_available(384000)
                        >"384 kHz"</option>
                        <option value="500000" selected=move || state.mic_max_sample_rate.get() == 500000
                            disabled=move || !rate_available(500000)
                        >"500 kHz"</option>
                    </select>
                </div>
                // Mic info display with Refresh button
                {move || {
                    let name = state.mic_device_name.get();
                    let conn = state.mic_connection_type.get();
                    let sr = state.mic_sample_rate.get();
                    let bits = state.mic_bits_per_sample.get();
                    let has_info = name.is_some() || sr > 0;
                    Some(view! {
                        <div class="mic-info">
                            {if has_info {
                                view! {
                                    <div>
                                        {name.map(|n| view! {
                                            <div class="mic-info-row">
                                                <span class="mic-info-label">"Device"</span>
                                                <span class="mic-info-value">{n}</span>
                                            </div>
                                        })}
                                        {conn.map(|c| view! {
                                            <div class="mic-info-row">
                                                <span class="mic-info-label">"Type"</span>
                                                <span class="mic-info-value">{c}</span>
                                            </div>
                                        })}
                                        {(sr > 0).then(|| view! {
                                            <div class="mic-info-row">
                                                <span class="mic-info-label">"Rate"</span>
                                                <span class="mic-info-value">{
                                                    if sr >= 1000 {
                                                        format!("{} kHz", sr / 1000)
                                                    } else {
                                                        format!("{} Hz", sr)
                                                    }
                                                }</span>
                                            </div>
                                        })}
                                        {(bits > 0).then(|| view! {
                                            <div class="mic-info-row">
                                                <span class="mic-info-label">"Depth"</span>
                                                <span class="mic-info-value">{format!("{}-bit", bits)}</span>
                                            </div>
                                        })}
                                    </div>
                                }.into_any()
                            } else {
                                view! { <span></span> }.into_any()
                            }}
                            <button class="setting-btn mic-info-refresh" on:click=move |_| {
                                spawn_local(async move {
                                    let mode = state.mic_mode.get_untracked();
                                    if mode == MicMode::Auto && state.is_tauri {
                                        // Resolve auto mode (checks USB, requests permission)
                                        let resolved = crate::audio::microphone::resolve_auto_mode(&state).await;
                                        if resolved == Some(MicMode::Cpal) {
                                            crate::audio::microphone::query_cpal_supported_rates(&state).await;
                                        }
                                    } else if (mode == MicMode::Cpal || mode == MicMode::Auto) && state.is_tauri {
                                        crate::audio::microphone::query_cpal_supported_rates(&state).await;
                                    }
                                    crate::audio::microphone::query_mic_info(&state).await;
                                });
                            }>{move || if has_info { "Refresh" } else { "Get mic info" }}</button>
                        </div>
                    })
                }}
            </div>

            <div class="setting-group">
                <div class="setting-group-title">"Playback"</div>
                <div class="setting-row">
                    <span class="setting-label">"Follow cursor"</span>
                    <input
                        type="checkbox"
                        class="setting-checkbox"
                        prop:checked=move || state.follow_cursor.get()
                        on:change=on_follow_cursor
                    />
                </div>
            </div>

            <div class="setting-group">
                <div class="setting-group-title">"Display"</div>
                <div class="setting-row">
                    <span class="setting-label">"Color scheme"</span>
                    <select
                        class="setting-select"
                        on:change=on_colormap_change
                    >
                        <option value="viridis" selected=move || state.colormap_preference.get() == Colormap::Viridis>"Viridis"</option>
                        <option value="inferno" selected=move || state.colormap_preference.get() == Colormap::Inferno>"Inferno"</option>
                        <option value="magma" selected=move || state.colormap_preference.get() == Colormap::Magma>"Magma"</option>
                        <option value="plasma" selected=move || state.colormap_preference.get() == Colormap::Plasma>"Plasma"</option>
                        <option value="cividis" selected=move || state.colormap_preference.get() == Colormap::Cividis>"Cividis"</option>
                        <option value="turbo" selected=move || state.colormap_preference.get() == Colormap::Turbo>"Turbo"</option>
                        <option value="greyscale" selected=move || state.colormap_preference.get() == Colormap::Greyscale>"Greyscale"</option>
                    </select>
                </div>
                <div class="setting-row">
                    <span class="setting-label">"HFR color scheme"</span>
                    <select
                        class="setting-select"
                        on:change=on_hfr_colormap_change
                    >
                        <option value="viridis" selected=move || state.hfr_colormap_preference.get() == Colormap::Viridis>"Viridis"</option>
                        <option value="inferno" selected=move || state.hfr_colormap_preference.get() == Colormap::Inferno>"Inferno"</option>
                        <option value="magma" selected=move || state.hfr_colormap_preference.get() == Colormap::Magma>"Magma"</option>
                        <option value="plasma" selected=move || state.hfr_colormap_preference.get() == Colormap::Plasma>"Plasma"</option>
                        <option value="cividis" selected=move || state.hfr_colormap_preference.get() == Colormap::Cividis>"Cividis"</option>
                        <option value="turbo" selected=move || state.hfr_colormap_preference.get() == Colormap::Turbo>"Turbo"</option>
                        <option value="greyscale" selected=move || state.hfr_colormap_preference.get() == Colormap::Greyscale>"Greyscale"</option>
                    </select>
                </div>
                <div class="setting-row">
                    <span class="setting-label">"Chromagram colors"</span>
                    <select
                        class="setting-select"
                        on:change=move |ev: web_sys::Event| {
                            let target = ev.target().unwrap();
                            let select: web_sys::HtmlSelectElement = target.unchecked_into();
                            let mode = match select.value().as_str() {
                                "warm" => ChromaColormap::Warm,
                                "solid" => ChromaColormap::Solid,
                                "octave" => ChromaColormap::Octave,
                                "flow" => ChromaColormap::Flow,
                                _ => ChromaColormap::PitchClass,
                            };
                            state.chroma_colormap.set(mode);
                        }
                    >
                        <option value="pitch_class" selected=move || state.chroma_colormap.get() == ChromaColormap::PitchClass>"Pitch Class"</option>
                        <option value="solid" selected=move || state.chroma_colormap.get() == ChromaColormap::Solid>"Solid"</option>
                        <option value="warm" selected=move || state.chroma_colormap.get() == ChromaColormap::Warm>"Warm"</option>
                        <option value="octave" selected=move || state.chroma_colormap.get() == ChromaColormap::Octave>"Octave"</option>
                        <option value="flow" selected=move || state.chroma_colormap.get() == ChromaColormap::Flow>"Flow"</option>
                    </select>
                </div>
                <div class="setting-row">
                    <span class="setting-label">"Always show view range"</span>
                    <input
                        type="checkbox"
                        class="setting-checkbox"
                        prop:checked=move || state.always_show_view_range.get()
                        on:change=on_always_show_view_range
                    />
                </div>
                <div class="setting-row">
                    <span class="setting-label">"Show clock time"</span>
                    <input
                        type="checkbox"
                        class="setting-checkbox"
                        prop:checked=move || state.show_clock_time.get()
                        on:change=move |ev: web_sys::Event| {
                            let target = ev.target().unwrap();
                            let input: web_sys::HtmlInputElement = target.unchecked_into();
                            state.show_clock_time.set(input.checked());
                        }
                        prop:disabled=move || {
                            state.current_file()
                                .and_then(|f| f.recording_start_epoch_ms())
                                .is_none()
                        }
                    />
                </div>
                <div class="setting-row">
                    <span class="setting-label">"Max freq"</span>
                    <select
                        class="setting-select"
                        on:change=move |ev: web_sys::Event| {
                            let target = ev.target().unwrap();
                            let select: web_sys::HtmlSelectElement = target.unchecked_into();
                            let freq = match select.value().as_str() {
                                "auto" => None,
                                v => v.parse::<f64>().ok().map(|khz| khz * 1000.0),
                            };
                            state.max_display_freq.set(freq);
                            state.min_display_freq.set(None);
                        }
                        prop:value=move || match state.max_display_freq.get() {
                            None => "auto".to_string(),
                            Some(hz) => format!("{}", (hz / 1000.0) as u32),
                        }
                    >
                        <option value="auto">"Auto"</option>
                        <option value="50">"50 kHz"</option>
                        <option value="100">"100 kHz"</option>
                        <option value="150">"150 kHz"</option>
                        <option value="200">"200 kHz"</option>
                        <option value="250">"250 kHz"</option>
                    </select>
                </div>
            </div>

            // Mic chooser modal (rendered here, uses position:fixed so DOM location doesn't matter)
            {move || state.show_mic_chooser.get().then(|| view! { <MicChooserModal /> })}
        </div>
    }
}
