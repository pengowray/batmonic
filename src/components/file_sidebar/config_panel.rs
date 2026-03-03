use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::JsCast;
use crate::state::{AppState, ChromaColormap, ColormapPreference, MicMode};

fn parse_colormap_pref(s: &str) -> ColormapPreference {
    match s {
        "inferno" => ColormapPreference::Inferno,
        "magma" => ColormapPreference::Magma,
        "plasma" => ColormapPreference::Plasma,
        "cividis" => ColormapPreference::Cividis,
        "turbo" => ColormapPreference::Turbo,
        "greyscale" => ColormapPreference::Greyscale,
        _ => ColormapPreference::Viridis,
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
            "cpal" => MicMode::Cpal,
            "raw_usb" => MicMode::RawUsb,
            _ => MicMode::Browser,
        };
        state.mic_mode.set(mode);
        // Query actual supported rates when switching to cpal
        if mode == MicMode::Cpal && state.is_tauri {
            spawn_local(async move {
                crate::audio::microphone::query_cpal_supported_rates(&state).await;
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

    // Query cpal rates on initial render if in cpal mode
    if is_tauri && state.mic_mode.get_untracked() == MicMode::Cpal {
        spawn_local(async move {
            crate::audio::microphone::query_cpal_supported_rates(&state).await;
        });
    }

    // Check if a specific rate is available based on mode + actual device rates
    let rate_available = move |rate: u32| -> bool {
        match state.mic_mode.get() {
            MicMode::Browser => rate <= 96_000,
            MicMode::RawUsb => rate <= 500_000,
            MicMode::Cpal => {
                let rates = state.mic_supported_rates.get();
                if rates.is_empty() {
                    // Not yet queried — show all rates up to 192kHz (cpal ceiling)
                    rate <= 192_000
                } else {
                    // Only show rates the device actually supports
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
                        <option value="browser"
                            selected=move || state.mic_mode.get() == MicMode::Browser
                        >"Browser"</option>
                        <option value="cpal"
                            selected=move || state.mic_mode.get() == MicMode::Cpal
                            disabled=move || !is_tauri
                        >"Native audio"</option>
                        <option value="raw_usb"
                            selected=move || state.mic_mode.get() == MicMode::RawUsb
                            disabled=move || !is_tauri
                        >{move || if is_tauri { "Raw USB (Recommended)" } else { "Raw USB" }}</option>
                    </select>
                </div>
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
                        <option value="viridis" selected=move || state.colormap_preference.get() == ColormapPreference::Viridis>"Viridis"</option>
                        <option value="inferno" selected=move || state.colormap_preference.get() == ColormapPreference::Inferno>"Inferno"</option>
                        <option value="magma" selected=move || state.colormap_preference.get() == ColormapPreference::Magma>"Magma"</option>
                        <option value="plasma" selected=move || state.colormap_preference.get() == ColormapPreference::Plasma>"Plasma"</option>
                        <option value="cividis" selected=move || state.colormap_preference.get() == ColormapPreference::Cividis>"Cividis"</option>
                        <option value="turbo" selected=move || state.colormap_preference.get() == ColormapPreference::Turbo>"Turbo"</option>
                        <option value="greyscale" selected=move || state.colormap_preference.get() == ColormapPreference::Greyscale>"Greyscale"</option>
                    </select>
                </div>
                <div class="setting-row">
                    <span class="setting-label">"HFR color scheme"</span>
                    <select
                        class="setting-select"
                        on:change=on_hfr_colormap_change
                    >
                        <option value="viridis" selected=move || state.hfr_colormap_preference.get() == ColormapPreference::Viridis>"Viridis"</option>
                        <option value="inferno" selected=move || state.hfr_colormap_preference.get() == ColormapPreference::Inferno>"Inferno"</option>
                        <option value="magma" selected=move || state.hfr_colormap_preference.get() == ColormapPreference::Magma>"Magma"</option>
                        <option value="plasma" selected=move || state.hfr_colormap_preference.get() == ColormapPreference::Plasma>"Plasma"</option>
                        <option value="cividis" selected=move || state.hfr_colormap_preference.get() == ColormapPreference::Cividis>"Cividis"</option>
                        <option value="turbo" selected=move || state.hfr_colormap_preference.get() == ColormapPreference::Turbo>"Turbo"</option>
                        <option value="greyscale" selected=move || state.hfr_colormap_preference.get() == ColormapPreference::Greyscale>"Greyscale"</option>
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
        </div>
    }
}
