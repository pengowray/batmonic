// XForm toggle — a standalone combo button in the View bar. The left half
// toggles XForm display-processing on/off; the right half opens the "Display
// Processing" panel (per-stage EQ / Notch / NR / Xform / Gain / Resam modes +
// custom sub-controls + a dynamic blurb describing what's currently applied).
//
// XForm used to be a separate `MainView::XformedSpec` that duplicated all the
// spectrogram settings. It is now an independent toggle layered over any
// spectro-like view (Spectrogram / Flow / Chromagram / Resonators): the
// spectrogram settings stay in the Spectrogram menu and apply to both.

use crate::state::store_fields::*;
use leptos::prelude::*;
use wasm_bindgen::JsCast;

use crate::components::app::{disable_xform, enable_xform};
use crate::components::combo_button::ComboButton;
use crate::components::display_filter_button::DspFilterRow;
use crate::state::{AppState, DisplayFilterMode, GainMode, LayerPanel, PlaybackMode};

fn toggle_panel(state: &AppState, panel: LayerPanel) {
    state.panels.layer_panel_open().update(|p| {
        *p = if *p == Some(panel) { None } else { Some(panel) };
    });
}

#[component]
pub fn XformButton() -> impl IntoView {
    let state = expect_context::<AppState>();

    let is_open =
        Signal::derive(move || state.panels.layer_panel_open().get() == Some(LayerPanel::Xform));
    let no_file = move || {
        state.library.current_index().get().is_none() && state.timeline.active().get().is_none()
    };
    // XForm only transforms the spectrogram, so it's inert on Wave / ZC views.
    let supported = move || state.viewmode.main_view().get().is_spectrogram();
    let enabled = Signal::derive(move || state.display.xform_enabled().get());

    // Toggle XForm on/off (no-op when no file or on a non-spectro view).
    let toggle_xform = move || {
        if no_file() || !supported() {
            return;
        }
        if state.display.xform_enabled().get_untracked() {
            disable_xform(&state);
        } else {
            enable_xform(&state);
        }
    };

    // ── Combo-button chrome ──
    let left_class = Signal::derive(move || {
        if no_file() || !supported() {
            "layer-btn combo-btn-left disabled"
        } else if enabled.get() {
            "layer-btn combo-btn-left active"
        } else {
            "layer-btn combo-btn-left"
        }
    });
    let right_class = Signal::derive(move || {
        if no_file() || !supported() {
            return "layer-btn combo-btn-right disabled";
        }
        let dim = !enabled.get();
        match (is_open.get(), dim) {
            (true, false) => "layer-btn combo-btn-right open",
            (true, true) => "layer-btn combo-btn-right dim open",
            (false, false) => "layer-btn combo-btn-right",
            (false, true) => "layer-btn combo-btn-right dim",
        }
    });

    // Compact summary of the active stages, shown on the left value line.
    let left_value = Signal::derive(move || {
        if !enabled.get() || !supported() {
            return String::new();
        }
        let mut parts: Vec<&'static str> = Vec::new();
        if state.display.eq().get() {
            parts.push("EQ");
        }
        if state.display.noise_filter().get() {
            parts.push("NF");
        }
        if state.display.transform().get() {
            parts.push(match state.playback.mode().get() {
                PlaybackMode::Heterodyne => "HET",
                PlaybackMode::TimeExpansion => "TE",
                PlaybackMode::PitchShift => "PS",
                PlaybackMode::PhaseVocoder => "PV",
                PlaybackMode::ZeroCrossing => "ZC",
                PlaybackMode::Normal => "XF",
            });
        }
        if state.display.gain_boost().get().abs() >= 0.5 {
            parts.push("G");
        }
        if state.display.decimate_effective().get() > 0 {
            parts.push("RS");
        }
        if parts.is_empty() {
            "raw".to_string()
        } else {
            parts.join("+")
        }
    });
    let right_value = Signal::derive(move || {
        if enabled.get() {
            "ON".to_string()
        } else {
            "OFF".to_string()
        }
    });

    let toggle_menu = Callback::new(move |()| {
        if no_file() || !supported() {
            return;
        }
        toggle_panel(&state, LayerPanel::Xform);
    });
    let left_click = Callback::new(move |_ev: web_sys::MouseEvent| toggle_xform());

    // ── Playback-active indicators (drive the green dots on each DSP row) ──
    let eq_active = Signal::derive(move || state.filter.enabled().get());
    let notch_active = Signal::derive(move || state.notch.enabled().get());
    let nr_active = Signal::derive(move || state.noise_reduce.enabled().get());
    let transform_active =
        Signal::derive(move || state.playback.mode().get() != PlaybackMode::Normal);
    let gain_active = Signal::derive(move || state.gain.mode().get() != GainMode::Off);
    let decim_active = Signal::derive(move || false);

    let browser_is_resampling = Signal::derive(move || {
        let bsr = state.display.browser_sample_rate().get();
        if bsr == 0 {
            return false;
        }
        let files = state.library.files().get();
        let idx = state.library.current_index().get();
        let file_rate = idx
            .and_then(|i| files.get(i))
            .map(|f| f.audio.sample_rate)
            .unwrap_or(0);
        if file_rate == 0 {
            return false;
        }
        let decim = state.display.decimate_effective().get();
        let effective = if decim > 0 && decim < file_rate {
            crate::dsp::filters::decimated_rate(file_rate, decim)
        } else {
            file_rate
        };
        effective != bsr
    });

    let resam_tooltip = Signal::derive(move || {
        let bsr = state.display.browser_sample_rate().get();
        if bsr == 0 {
            return String::new();
        }
        let files = state.library.files().get();
        let idx = state.library.current_index().get();
        let file_rate = idx
            .and_then(|i| files.get(i))
            .map(|f| f.audio.sample_rate)
            .unwrap_or(0);
        if file_rate == 0 {
            return String::new();
        }
        let decim = state.display.decimate_effective().get();
        let effective = if decim > 0 && decim < file_rate {
            crate::dsp::filters::decimated_rate(file_rate, decim)
        } else {
            file_rate
        };
        if effective != bsr {
            format!("Browser resampling {}Hz to {}Hz output", effective, bsr)
        } else {
            format!("Output matches browser rate ({}Hz)", bsr)
        }
    });

    let show_nr_custom = Signal::derive(move || {
        state.display.xform_enabled().get()
            && state.display.filter_nr().get() == DisplayFilterMode::Custom
    });
    let show_decim_custom = Signal::derive(move || {
        state.display.xform_enabled().get()
            && state.display.filter_decimate().get() == DisplayFilterMode::Custom
    });

    // Dynamic blurb describing what the transformed spectrogram is showing,
    // composed from the *resolved* display_* signals so it tracks reality.
    let blurb = Signal::derive(move || {
        if !state.display.xform_enabled().get() {
            return "Off — the spectrogram shows the raw signal. Turn XForm on to \
                    view the signal as transformed for playback (heterodyne, gain, \
                    filtering, …)."
                .to_string();
        }
        let mut parts: Vec<String> = Vec::new();
        if state.display.eq().get() {
            parts.push("bandpass/EQ".to_string());
        }
        if state.display.noise_filter().get() {
            parts.push("noise filtering".to_string());
        }
        if state.display.transform().get() {
            match state.playback.mode().get() {
                PlaybackMode::Heterodyne => {
                    let f = state.transform.het_frequency().get();
                    parts.push(format!("heterodyne @ {:.0} kHz", f / 1000.0));
                }
                PlaybackMode::TimeExpansion => {
                    let n = state.transform.te_factor().get();
                    parts.push(format!("{:.0}\u{00D7} time-expansion", n));
                }
                PlaybackMode::PitchShift => parts.push("pitch-shift".to_string()),
                PlaybackMode::PhaseVocoder => parts.push("phase-vocoder shift".to_string()),
                PlaybackMode::ZeroCrossing => parts.push("zero-crossing".to_string()),
                PlaybackMode::Normal => {}
            }
        }
        let boost = state.display.gain_boost().get();
        if boost.abs() >= 0.5 {
            parts.push(format!("{:+.0} dB gain", boost));
        }
        let decim = state.display.decimate_effective().get();
        if decim > 0 {
            let files = state.library.files().get();
            let idx = state.library.current_index().get();
            let file_rate = idx
                .and_then(|i| files.get(i))
                .map(|f| f.audio.sample_rate)
                .unwrap_or(0);
            let effective = if file_rate > 0 && decim < file_rate {
                crate::dsp::filters::decimated_rate(file_rate, decim)
            } else {
                decim
            };
            parts.push(format!("resample to {:.0} kHz", effective as f64 / 1000.0));
        }
        if parts.is_empty() {
            "Showing the spectrogram of the unmodified signal — no processing stages \
             are active. Switch a row below to \"sam\" to mirror playback."
                .to_string()
        } else {
            format!("Spectrogram of the signal after {}.", join_clauses(&parts))
        }
    });

    view! {
        <ComboButton
            left_label="XForm"
            left_value=left_value
            left_click=left_click
            left_class=left_class
            right_value=right_value
            right_class=right_class
            is_open=is_open
            toggle_menu=toggle_menu
            left_title="Toggle XForm: view the spectrogram of the signal as transformed for playback"
            right_title="Display Processing settings"
            panel_style="min-width: 230px;"
        >
            <div class="xform-blurb">{move || blurb.get()}</div>
            <div class="layer-panel-title">"Display Processing"</div>
            <div class="dsp-filter-row dsp-filter-header">
                <span class="dsp-filter-label"></span>
                <div class="dsp-filter-seg">
                    <span>"off"</span>
                    <span>"aut"</span>
                    <span>"sam"</span>
                    <span>"cst"</span>
                </div>
                <div class="dsp-filter-indicator-header" title="Playback active">
                    {"\u{1F50A}"}
                </div>
            </div>
            <DspFilterRow label="EQ" signal=state.display.filter_eq() playback_active=eq_active custom_available=false />
            <DspFilterRow label="Notch" signal=state.display.filter_notch() playback_active=notch_active custom_available=false auto_available=false />
            <DspFilterRow label="NR" signal=state.display.filter_nr() playback_active=nr_active custom_available=false />
            <DspFilterRow label="Xform" signal=state.display.filter_transform() playback_active=transform_active custom_available=false auto_available=false />
            <DspFilterRow label="Gain" signal=state.display.filter_gain() playback_active=gain_active custom_available=true />
            <DspFilterRow label="Resam" signal=state.display.filter_decimate() playback_active=decim_active custom_available=true browser_resampling=browser_is_resampling sam_tooltip=resam_tooltip />

            // Custom NR strength
            {move || show_nr_custom.get().then(|| {
                let strength = state.display.nr_strength();
                view! {
                    <div class="dsp-custom-section">
                        <div class="dsp-custom-title">"NR Strength"</div>
                        <div class="dsp-custom-slider-row">
                            <input
                                type="range"
                                class="setting-range"
                                min="0" max="2" step="0.05"
                                prop:value=move || strength.get().to_string()
                                on:input=move |ev: web_sys::Event| {
                                    let target = ev.target().unwrap();
                                    let input: web_sys::HtmlInputElement = target.unchecked_into();
                                    if let Ok(v) = input.value().parse::<f64>() {
                                        strength.set(v);
                                    }
                                }
                                on:dblclick=move |_| strength.set(0.8)
                            />
                            <span class="dsp-custom-value">{move || format!("{:.2}", strength.get())}</span>
                        </div>
                    </div>
                }
            })}

            // Custom decimate rate
            {move || show_decim_custom.get().then(|| {
                let rate = state.display.decimate_rate();
                let rates: [(u32, &str); 4] = [
                    (44100, "44.1k"),
                    (48000, "48k"),
                    (96000, "96k"),
                    (192000, "192k"),
                ];
                view! {
                    <div class="dsp-custom-section">
                        <div class="dsp-custom-title">"Decimate Rate"</div>
                        <div class="dsp-filter-seg" style="justify-content: center; gap: 2px; padding: 2px 4px;">
                            {rates.into_iter().map(|(r, label)| {
                                view! {
                                    <button
                                        class=move || if rate.get() == r { "sel" } else { "" }
                                        on:click=move |_| rate.set(r)
                                    >{label}</button>
                                }
                            }).collect_view()}
                        </div>
                    </div>
                }
            })}
        </ComboButton>
    }
}

/// Join clause fragments into "a", "a and b", or "a, b and c".
fn join_clauses(parts: &[String]) -> String {
    match parts.len() {
        0 => String::new(),
        1 => parts[0].clone(),
        2 => format!("{} and {}", parts[0], parts[1]),
        _ => {
            let (last, head) = parts.split_last().unwrap();
            format!("{} and {}", head.join(", "), last)
        }
    }
}
