use leptos::prelude::*;
use wasm_bindgen::JsCast;

use crate::state::{AppState, DisplayFilterMode, GainMode, LayerPanel, PlaybackMode};

fn toggle_panel(state: &AppState, panel: LayerPanel) {
    state.layer_panel_open.update(|p| {
        *p = if *p == Some(panel) { None } else { Some(panel) };
    });
}

/// A single row in the DSP filter grid: label + 4-way segmented control + playback indicator.
#[component]
fn DspFilterRow(
    label: &'static str,
    signal: RwSignal<DisplayFilterMode>,
    /// Whether the corresponding playback filter is currently active
    #[prop(into)]
    playback_active: Signal<bool>,
    /// Whether 'custom' is available (greyed out if false)
    custom_available: bool,
    /// Whether 'auto' is available (greyed out if false)
    #[prop(default = true)]
    auto_available: bool,
) -> impl IntoView {
    let modes = DisplayFilterMode::ALL;

    view! {
        <div class="dsp-filter-row">
            <span class="dsp-filter-label">{label}</span>
            <div class="dsp-filter-seg">
                {modes.iter().copied().map(|mode| {
                    let is_custom = mode == DisplayFilterMode::Custom;
                    let is_auto = mode == DisplayFilterMode::Auto;
                    let disabled = (is_custom && !custom_available) || (is_auto && !auto_available);
                    let is_same = mode == DisplayFilterMode::Same;
                    view! {
                        <button
                            class=move || {
                                let sel = signal.get() == mode;
                                match (sel, disabled) {
                                    (true, _) => "sel",
                                    (_, true) => "disabled",
                                    _ => "",
                                }
                            }
                            title=mode.label()
                            disabled=disabled
                            on:click=move |_| {
                                if !disabled {
                                    signal.set(mode);
                                }
                            }
                        >
                            {mode.short_label()}
                            {is_same.then(|| view! {
                                <span class=move || {
                                    if playback_active.get() { "sam-dot active" } else { "sam-dot inactive" }
                                }></span>
                            })}
                        </button>
                    }
                }).collect_view()}
            </div>
            <div class=move || {
                if playback_active.get() { "dsp-filter-indicator active" } else { "dsp-filter-indicator inactive" }
            }></div>
        </div>
    }
}

/// Floating DSP filter button with dropdown panel for per-stage display processing control.
#[component]
pub fn DisplayFilterButton() -> impl IntoView {
    use crate::components::combo_button::ComboButton;
    let state = expect_context::<AppState>();

    let is_open = Signal::derive(move || {
        state.layer_panel_open.get() == Some(LayerPanel::DisplayFilter)
    });

    let enabled = state.display_filter_enabled;

    let left_class = Signal::derive(move || {
        let base = if enabled.get() { "layer-btn combo-btn-left active" } else { "layer-btn combo-btn-left" };
        if is_open.get() {
            if enabled.get() { "layer-btn combo-btn-left active open" } else { "layer-btn combo-btn-left open" }
        } else {
            base
        }
    });

    let right_class = Signal::derive(move || {
        let dim = if !enabled.get() { " dim" } else { "" };
        if is_open.get() {
            if dim.is_empty() { "layer-btn combo-btn-right open" } else { "layer-btn combo-btn-right dim open" }
        } else {
            if dim.is_empty() { "layer-btn combo-btn-right" } else { "layer-btn combo-btn-right dim" }
        }
    });

    let left_click = Callback::new(move |_: web_sys::MouseEvent| {
        enabled.update(|v| *v = !*v);
    });

    // Summary: count of non-Off stages, or "OFF" when master is disabled
    let right_value = Signal::derive(move || {
        if !enabled.get() {
            return "OFF".to_string();
        }
        let count = [
            state.display_filter_eq.get(),
            state.display_filter_notch.get(),
            state.display_filter_nr.get(),
            state.display_filter_transform.get(),
            state.display_filter_gain.get(),
        ].iter().filter(|m| **m != DisplayFilterMode::Off).count();
        format!("{}/5", count)
    });

    let toggle_menu = Callback::new(move |()| {
        toggle_panel(&state, LayerPanel::DisplayFilter);
    });

    // Playback active indicators
    let eq_active = Signal::derive(move || state.filter_enabled.get());
    let notch_active = Signal::derive(move || state.notch_enabled.get());
    let nr_active = Signal::derive(move || state.noise_reduce_enabled.get());
    let transform_active = Signal::derive(move || state.playback_mode.get() != PlaybackMode::Normal);
    let gain_active = Signal::derive(move || state.gain_mode.get() != GainMode::Off);

    // Whether custom NR or Gain sections should show
    let show_nr_custom = Signal::derive(move || {
        enabled.get() && state.display_filter_nr.get() == DisplayFilterMode::Custom
    });
    let show_gain_custom = Signal::derive(move || {
        enabled.get() && state.display_filter_gain.get() == DisplayFilterMode::Custom
    });

    view! {
        <ComboButton
            left_label="DSP"
            left_value=Signal::derive(|| String::new())
            left_click=left_click
            left_class=left_class
            right_value=right_value
            right_class=right_class
            is_open=is_open
            toggle_menu=toggle_menu
            left_title="Toggle display processing on/off"
            right_title="Display processing settings"
            panel_style="min-width: 240px;"
        >
            <div class="layer-panel-title">"Display Processing"</div>

            // Column headers
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

            <DspFilterRow label="EQ" signal=state.display_filter_eq playback_active=eq_active custom_available=false />
            <DspFilterRow label="Notch" signal=state.display_filter_notch playback_active=notch_active custom_available=false auto_available=false />
            <DspFilterRow label="NR" signal=state.display_filter_nr playback_active=nr_active custom_available=true />
            <DspFilterRow label="Xform" signal=state.display_filter_transform playback_active=transform_active custom_available=false auto_available=false />
            <DspFilterRow label="Gain" signal=state.display_filter_gain playback_active=gain_active custom_available=true />

            // Custom NR section
            {move || show_nr_custom.get().then(|| {
                let strength = state.display_nr_strength;
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

            // Custom Gain section
            {move || show_gain_custom.get().then(|| {
                let gain = state.display_custom_gain_db;
                view! {
                    <div class="dsp-custom-section">
                        <div class="dsp-custom-title">"Display Gain"</div>
                        <div class="dsp-custom-slider-row">
                            <input
                                type="range"
                                class="setting-range"
                                min="-40" max="40" step="1"
                                prop:value=move || gain.get().to_string()
                                on:input=move |ev: web_sys::Event| {
                                    let target = ev.target().unwrap();
                                    let input: web_sys::HtmlInputElement = target.unchecked_into();
                                    if let Ok(v) = input.value().parse::<f32>() {
                                        gain.set(v);
                                    }
                                }
                                on:dblclick=move |_| gain.set(0.0)
                            />
                            <span class="dsp-custom-value">{move || {
                                let v = gain.get();
                                if v >= 0.0 { format!("+{:.0} dB", v) } else { format!("{:.0} dB", v) }
                            }}</span>
                        </div>
                    </div>
                }
            })}

            // ── Intensity sliders (Gain / Range / Contrast) ──
            <div class="dsp-custom-section">
                <div class="dsp-custom-title">"Intensity"</div>
                <div class="dsp-custom-slider-row">
                    <span class="dsp-slider-label">"Gain"</span>
                    <input
                        type="range"
                        class="setting-range"
                        min="-40" max="40" step="1"
                        prop:value=move || state.spect_gain_db.get().to_string()
                        on:input=move |ev: web_sys::Event| {
                            let target = ev.target().unwrap();
                            let input: web_sys::HtmlInputElement = target.unchecked_into();
                            if let Ok(v) = input.value().parse::<f32>() {
                                state.spect_gain_db.set(v);
                                // Switch to Custom gain when user touches the slider
                                if state.display_filter_enabled.get_untracked() {
                                    state.display_filter_gain.set(DisplayFilterMode::Custom);
                                }
                                state.display_auto_gain.set(false);
                            }
                        }
                        on:dblclick=move |_| state.spect_gain_db.set(0.0)
                    />
                    <span class="dsp-custom-value">{move || {
                        let dsp_on = state.display_filter_enabled.get();
                        let gain_mode = state.display_filter_gain.get();
                        if dsp_on && gain_mode == DisplayFilterMode::Off {
                            "off".to_string()
                        } else if state.display_auto_gain.get() {
                            "auto".to_string()
                        } else if dsp_on && gain_mode == DisplayFilterMode::Same {
                            "same".to_string()
                        } else {
                            format!("{:+.0} dB", state.spect_gain_db.get())
                        }
                    }}</span>
                </div>
                <div class="dsp-custom-slider-row">
                    <span class="dsp-slider-label">"Range"</span>
                    <input
                        type="range"
                        class="setting-range"
                        min="20" max="120" step="5"
                        prop:value=move || state.spect_range_db.get().to_string()
                        on:input=move |ev: web_sys::Event| {
                            let target = ev.target().unwrap();
                            let input: web_sys::HtmlInputElement = target.unchecked_into();
                            if let Ok(v) = input.value().parse::<f32>() {
                                state.spect_range_db.set(v);
                                state.spect_floor_db.set(-v);
                            }
                        }
                        on:dblclick=move |_| {
                            state.spect_range_db.set(120.0);
                            state.spect_floor_db.set(-120.0);
                        }
                    />
                    <span class="dsp-custom-value">{move || format!("{:.0} dB", state.spect_range_db.get())}</span>
                </div>
                <div class="dsp-custom-slider-row">
                    <span class="dsp-slider-label">"Contrast"</span>
                    <input
                        type="range"
                        class="setting-range"
                        min="0.2" max="3.0" step="0.05"
                        prop:value=move || state.spect_gamma.get().to_string()
                        on:input=move |ev: web_sys::Event| {
                            let target = ev.target().unwrap();
                            let input: web_sys::HtmlInputElement = target.unchecked_into();
                            if let Ok(v) = input.value().parse::<f32>() {
                                state.spect_gamma.set(v);
                            }
                        }
                        on:dblclick=move |_| state.spect_gamma.set(1.0)
                    />
                    <span class="dsp-custom-value">{move || {
                        let g = state.spect_gamma.get();
                        if g == 1.0 { "linear".to_string() } else { format!("{:.2}", g) }
                    }}</span>
                </div>
                <div style="text-align: right; padding-top: 4px;">
                    <button
                        class="layer-panel-opt"
                        style="display: inline; width: auto; padding: 2px 8px; font-size: 9px;"
                        on:click=move |_| {
                            state.spect_gain_db.set(0.0);
                            state.spect_floor_db.set(-120.0);
                            state.spect_range_db.set(120.0);
                            state.spect_gamma.set(1.0);
                            state.display_auto_gain.set(false);
                            state.display_eq.set(false);
                            state.display_noise_filter.set(false);
                            // Reset DSP filter modes to defaults
                            state.display_filter_eq.set(DisplayFilterMode::Off);
                            state.display_filter_notch.set(DisplayFilterMode::Off);
                            state.display_filter_nr.set(DisplayFilterMode::Auto);
                            state.display_filter_transform.set(DisplayFilterMode::Off);
                            state.display_filter_gain.set(DisplayFilterMode::Auto);
                            state.display_nr_strength.set(0.8);
                            state.display_custom_gain_db.set(0.0);
                        }
                    >"Reset"</button>
                </div>
            </div>
        </ComboButton>
    }
}
