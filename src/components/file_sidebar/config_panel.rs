use crate::canvas::spectrogram_renderer::Colormap;
use crate::state::store_fields::*;
use crate::state::{AppState, ChromaColormap, ShieldStyle};
use leptos::prelude::*;
use wasm_bindgen::JsCast;

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
        state.view.follow_cursor().set(checked);
        if checked {
            state.view.follow_suspended().set(false);
            state.view.follow_visible_since().set(None);
        }
    };

    let on_colormap_change = move |ev: web_sys::Event| {
        let target = ev.target().unwrap();
        let select: web_sys::HtmlSelectElement = target.unchecked_into();
        state
            .spect
            .colormap_preference()
            .set(parse_colormap_pref(&select.value()));
        state
            .viewmode
            .tile_ready_signal()
            .update(|n| *n = n.wrapping_add(1));
    };

    let on_hfr_colormap_change = move |ev: web_sys::Event| {
        let target = ev.target().unwrap();
        let select: web_sys::HtmlSelectElement = target.unchecked_into();
        state
            .spect
            .hfr_colormap_preference()
            .set(parse_colormap_pref(&select.value()));
        state
            .viewmode
            .tile_ready_signal()
            .update(|n| *n = n.wrapping_add(1));
    };

    let is_tauri = state.is_tauri;
    let _ = is_tauri; // used in the view

    view! {
        <div class="sidebar-panel">
            // Recording settings moved to Record menu in bottom toolbar

            <div class="setting-group">
                <div class="setting-group-title">"Playback"</div>
                <div class="setting-row">
                    <span class="setting-label">"Follow cursor"</span>
                    <input
                        type="checkbox"
                        class="setting-checkbox"
                        prop:checked=move || state.view.follow_cursor().get()
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
                        <option value="viridis" selected=move || state.spect.colormap_preference().get() == Colormap::Viridis>"Viridis"</option>
                        <option value="inferno" selected=move || state.spect.colormap_preference().get() == Colormap::Inferno>"Inferno"</option>
                        <option value="magma" selected=move || state.spect.colormap_preference().get() == Colormap::Magma>"Magma"</option>
                        <option value="plasma" selected=move || state.spect.colormap_preference().get() == Colormap::Plasma>"Plasma"</option>
                        <option value="cividis" selected=move || state.spect.colormap_preference().get() == Colormap::Cividis>"Cividis"</option>
                        <option value="turbo" selected=move || state.spect.colormap_preference().get() == Colormap::Turbo>"Turbo"</option>
                        <option value="greyscale" selected=move || state.spect.colormap_preference().get() == Colormap::Greyscale>"Greyscale"</option>
                    </select>
                </div>
                <div class="setting-row">
                    <span class="setting-label">"HFR color scheme"</span>
                    <select
                        class="setting-select"
                        on:change=on_hfr_colormap_change
                    >
                        <option value="viridis" selected=move || state.spect.hfr_colormap_preference().get() == Colormap::Viridis>"Viridis"</option>
                        <option value="inferno" selected=move || state.spect.hfr_colormap_preference().get() == Colormap::Inferno>"Inferno"</option>
                        <option value="magma" selected=move || state.spect.hfr_colormap_preference().get() == Colormap::Magma>"Magma"</option>
                        <option value="plasma" selected=move || state.spect.hfr_colormap_preference().get() == Colormap::Plasma>"Plasma"</option>
                        <option value="cividis" selected=move || state.spect.hfr_colormap_preference().get() == Colormap::Cividis>"Cividis"</option>
                        <option value="turbo" selected=move || state.spect.hfr_colormap_preference().get() == Colormap::Turbo>"Turbo"</option>
                        <option value="greyscale" selected=move || state.spect.hfr_colormap_preference().get() == Colormap::Greyscale>"Greyscale"</option>
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
                            state.chroma.colormap().set(mode);
                        }
                    >
                        <option value="pitch_class" selected=move || state.chroma.colormap().get() == ChromaColormap::PitchClass>"Pitch Class"</option>
                        <option value="solid" selected=move || state.chroma.colormap().get() == ChromaColormap::Solid>"Solid"</option>
                        <option value="warm" selected=move || state.chroma.colormap().get() == ChromaColormap::Warm>"Warm"</option>
                        <option value="octave" selected=move || state.chroma.colormap().get() == ChromaColormap::Octave>"Octave"</option>
                        <option value="flow" selected=move || state.chroma.colormap().get() == ChromaColormap::Flow>"Flow"</option>
                    </select>
                </div>
                <div class="setting-row">
                    <span class="setting-label">"Show clock time"</span>
                    <input
                        type="checkbox"
                        class="setting-checkbox"
                        prop:checked=move || state.timeline.show_clock_time().get()
                        on:change=move |ev: web_sys::Event| {
                            let target = ev.target().unwrap();
                            let input: web_sys::HtmlInputElement = target.unchecked_into();
                            state.timeline.show_clock_time().set(input.checked());
                        }
                        prop:disabled=move || {
                            state.current_file()
                                .and_then(|f| f.recording_start_epoch_ms())
                                .is_none()
                        }
                    />
                </div>
                <div class="setting-row">
                    <span class="setting-label">"Freq flags"</span>
                    <select
                        class="setting-select"
                        on:change=move |ev: web_sys::Event| {
                            let target = ev.target().unwrap();
                            let select: web_sys::HtmlSelectElement = target.unchecked_into();
                            let style = ShieldStyle::from_key(&select.value());
                            state.viewmode.shield_style().set(style);
                            crate::settings::set_raw(crate::settings::keys::SHIELD_STYLE, style.key());
                        }
                    >
                        {ShieldStyle::ALL.iter().map(|&s| view! {
                            <option
                                value=s.key()
                                selected=move || state.viewmode.shield_style().get() == s
                            >{s.label()}</option>
                        }).collect::<Vec<_>>()}
                    </select>
                </div>
                <div class="setting-row">
                    <span class="setting-label">"Status bar"</span>
                    <input
                        type="checkbox"
                        class="setting-checkbox"
                        prop:checked=move || state.panels.show_status_bar().get()
                        on:change=move |ev: web_sys::Event| {
                            let target = ev.target().unwrap();
                            let input: web_sys::HtmlInputElement = target.unchecked_into();
                            let show = input.checked();
                            state.panels.show_status_bar().set(show);
                            crate::settings::set_bool(crate::settings::keys::SHOW_STATUS_BAR, show);
                        }
                    />
                </div>
            </div>

            {move || {
                if state.is_tauri {
                    view! {
                        <div class="setting-group">
                            <div class="setting-group-title">"Location"</div>
                            <div class="setting-row">
                                <span class="setting-label">"Location tags"</span>
                                <input
                                    type="checkbox"
                                    class="setting-checkbox"
                                    prop:checked=move || state.recording_meta.gps_enabled().get()
                                    on:change=move |ev: web_sys::Event| {
                                        let target = ev.target().unwrap();
                                        let input: web_sys::HtmlInputElement = target.unchecked_into();
                                        let checked = input.checked();
                                        state.recording_meta.gps_enabled().set(checked);
                                        crate::settings::set_bool(crate::settings::keys::GPS_ENABLED, checked);
                                    }
                                />
                            </div>
                            <div class="setting-hint">"Add location data to recorded audio"</div>
                            {move || {
                                if !state.recording_meta.gps_enabled().get() {
                                    return view! { <span></span> }.into_any();
                                }
                                let count = state.recording_meta.home_wifi_ssids().with(|list| list.len());
                                if count > 0 {
                                    view! {
                                        <div class="setting-hint" style="color: #8cf;">
                                            {format!("{} privacy zone{} configured", count, if count == 1 { "" } else { "s" })}
                                        </div>
                                    }.into_any()
                                } else {
                                    view! { <span></span> }.into_any()
                                }
                            }}
                            <div style="padding: 4px 12px 8px;">
                                <button
                                    class="analysis-full-btn privacy-settings-btn"
                                    on:click=move |_| { state.dialogs.privacy_settings().set(true); }
                                >"Privacy settings\u{2026}"</button>
                            </div>
                        </div>
                    }.into_any()
                } else {
                    view! { <span></span> }.into_any()
                }
            }}

            <div class="setting-group">
                <div class="setting-group-title">"Selection"</div>
                <div class="setting-row">
                    <span class="setting-label">"Auto-band: Selection"</span>
                    <input
                        type="checkbox"
                        class="setting-checkbox"
                        prop:checked=move || state.annotations.selection_auto_focus().get()
                        on:change=move |ev: web_sys::Event| {
                            let target = ev.target().unwrap();
                            let input: web_sys::HtmlInputElement = target.unchecked_into();
                            state.annotations.selection_auto_focus().set(input.checked());
                        }
                    />
                </div>
                <div class="setting-row">
                    <span class="setting-label">"Auto-band: Annotation"</span>
                    <input
                        type="checkbox"
                        class="setting-checkbox"
                        prop:checked=move || state.annotations.auto_focus().get()
                        on:change=move |ev: web_sys::Event| {
                            let target = ev.target().unwrap();
                            let input: web_sys::HtmlInputElement = target.unchecked_into();
                            state.annotations.auto_focus().set(input.checked());
                        }
                    />
                </div>
                <div class="setting-row">
                    <span class="setting-label">"Auto-band: Bat Book"</span>
                    <input
                        type="checkbox"
                        class="setting-checkbox"
                        prop:checked=move || state.bat_book.auto_focus().get()
                        on:change=move |ev: web_sys::Event| {
                            let target = ev.target().unwrap();
                            let input: web_sys::HtmlInputElement = target.unchecked_into();
                            state.bat_book.auto_focus().set(input.checked());
                        }
                    />
                </div>
            </div>

            <div class="setting-group">
                <div class="setting-group-title">"Beta"</div>
                <div class="setting-row">
                    <span class="setting-label">"Enable projects"</span>
                    <input
                        type="checkbox"
                        class="setting-checkbox"
                        prop:checked=move || state.project.enabled().get()
                        on:change=move |ev: web_sys::Event| {
                            let target = ev.target().unwrap();
                            let input: web_sys::HtmlInputElement = target.unchecked_into();
                            let checked = input.checked();
                            state.project.enabled().set(checked);
                            crate::settings::set_bool(crate::settings::keys::PROJECTS_ENABLED, checked);
                        }
                    />
                </div>
            </div>

        </div>
    }
}
