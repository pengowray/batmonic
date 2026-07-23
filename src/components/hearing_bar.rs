// Hearing Bar — the "what comes out the speakers" strip between the
// Overview and the main canvas.
//
// Layout:
//
//   ┌─── band-affected overline ─────────────────────────────────┐ │
//   │ [HFR]  [Range ▼]  [1:1 HET TE PS ZC]  [PASS ▼]            │ │ [Gain] [NR] [Notch]
//   └────────────────────────────────────────────────────────────┘ │
//        ^band-cell HFR toggle (above the band gutter)
//
// • The BAND cell (above the gutter) is the HFR on/off toggle, with
//   per-letter brightness on "HFR": grey when off, amber when on, with
//   the leading "H" dimmed when the active band sits entirely below
//   24 kHz (audible-only filter).
// • Range dropdown shows the current band range or "OFF"; it hosts the
//   band-presets picker.
// • Mode radio group selects playback algorithm. Clicking the already-
//   selected mode opens its settings panel (corner indicator).
// • PASS (BandpassCombo) is part of the band-affected group; the overline
//   above its slot turns grey when its range is locked (not following).
//
// The Listen button lives in the bottom toolbar's capture group alongside
// Record (and the Mic / New-recording bookends) — see `bottom_toolbar.rs`.

use crate::state::store_fields::*;
use leptos::prelude::*;

use crate::audio::streaming_playback::PV_MODE_BOOST_DB;
use crate::components::combo_button::ComboButton;
use crate::components::hfr_button::RangeButton;
use crate::components::mode_button::ModeRadioGroup;
use crate::components::noise_combos::{NotchCombo, NrCombo};
use crate::dsp::filters::BandMode;
use crate::state::{
    AppState, BandpassMode, BandpassRange, Bar, FilterQuality, GainMode, LayerPanel, PeakSource,
    PlaybackMode,
};

fn toggle_panel(state: &AppState, panel: LayerPanel) {
    state.panels.layer_panel_open().update(|p| {
        *p = if *p == Some(panel) { None } else { Some(panel) };
    });
}

// Band presets and their helpers moved into `hfr_button.rs` — they're
// the body of HfrButton's right-half dropdown now.

fn layer_opt_class_simple(active: bool) -> &'static str {
    if active {
        "layer-panel-opt sel"
    } else {
        "layer-panel-opt"
    }
}

/// Gain combo (migrated from the bottom toolbar). Controls the playback
/// gain mode (Off / Manual / AutoPeak / Adaptive) plus a manual-boost dB
/// slider that doubles as a quick override when in Auto modes.
#[component]
fn GainCombo() -> impl IntoView {
    let state = expect_context::<AppState>();

    let is_open =
        Signal::derive(move || state.panels.layer_panel_open().get() == Some(LayerPanel::Gain));
    // Live monitoring uses `live_gain_db` (separate from file-playback gain).
    // When live is active, the slider rebinds and the AGC/AutoPeak modes are
    // ignored — the user wanted a simple slider for monitoring.
    let is_live = move || state.mic.listening().get() || state.mic.recording().get();
    let no_file = move || {
        !is_live()
            && state.library.current_index().get().is_none()
            && state.timeline.active().get().is_none()
    };

    let left_class = Signal::derive(move || {
        if no_file() {
            "layer-btn combo-btn-left disabled"
        } else if is_live() {
            if state.gain.live_db().get().abs() > 0.001 {
                "layer-btn combo-btn-left active"
            } else {
                "layer-btn combo-btn-left"
            }
        } else if state.gain.mode().get() != GainMode::Off {
            "layer-btn combo-btn-left active"
        } else {
            "layer-btn combo-btn-left no-annotation"
        }
    });
    let right_class = Signal::derive(move || {
        if no_file() {
            return "layer-btn combo-btn-right disabled";
        }
        // While live, the mode label has no effect on the audio path — dim it.
        let dim = if is_live() || state.gain.mode().get() == GainMode::Off {
            " dim"
        } else {
            ""
        };
        if is_open.get() {
            if dim.is_empty() {
                "layer-btn combo-btn-right open"
            } else {
                "layer-btn combo-btn-right dim open"
            }
        } else if dim.is_empty() {
            "layer-btn combo-btn-right"
        } else {
            "layer-btn combo-btn-right dim"
        }
    });

    let left_value = Signal::derive(move || {
        if is_live() {
            let db = state.gain.live_db().get();
            return if db > 0.0 {
                format!("+{:.0}dB", db)
            } else {
                format!("{:.0}dB", db)
            };
        }
        let mode = state.gain.mode().get();
        let manual_db = state.gain.db().get();
        let pv_boost = if state.playback.mode().get() == PlaybackMode::PhaseVocoder {
            PV_MODE_BOOST_DB
        } else {
            0.0
        };
        match mode {
            GainMode::Off => {
                if pv_boost > 0.0 {
                    format!("+{:.0}dB", pv_boost)
                } else {
                    String::new()
                }
            }
            GainMode::Manual => {
                let total = manual_db + pv_boost;
                if total > 0.0 {
                    format!("+{:.0}dB", total)
                } else {
                    format!("{:.0}dB", total)
                }
            }
            GainMode::AutoPeak => {
                let auto_db = state.compute_auto_gain();
                let total = auto_db + manual_db + pv_boost;
                format!("+{:.0}dB", total)
            }
            GainMode::Adaptive => {
                if manual_db > 0.0 || pv_boost > 0.0 {
                    format!("A+{:.0}", manual_db + pv_boost)
                } else {
                    "Auto".to_string()
                }
            }
        }
    });
    let right_value = Signal::derive(move || {
        if is_live() {
            return "LIVE".to_string();
        }
        match state.gain.mode().get() {
            GainMode::Off => "OFF".to_string(),
            mode => mode.label().to_string(),
        }
    });

    let left_click = Callback::new(move |_: web_sys::MouseEvent| {
        if is_live() {
            // In live mode, the left half just opens/closes the panel — the
            // mode toggle is meaningless because live ignores gain_mode.
            toggle_panel(&state, LayerPanel::Gain);
            return;
        }
        let mode = state.gain.mode().get_untracked();
        if mode == GainMode::Off {
            let last = state.gain.mode_last_auto().get_untracked();
            state.gain.mode().set(last);
            state.gain.auto().set(last.is_auto());
        } else {
            if mode.is_auto() {
                state.gain.mode_last_auto().set(mode);
            }
            state.gain.mode().set(GainMode::Off);
            state.gain.auto().set(false);
        }
    });
    let toggle_menu = Callback::new(move |()| {
        toggle_panel(&state, LayerPanel::Gain);
    });

    view! {
        <ComboButton
            left_label="Gain"
            left_value=left_value
            left_click=left_click
            left_class=left_class
            right_value=right_value
            right_class=right_class
            is_open=is_open
            toggle_menu=toggle_menu
            left_title="Toggle gain"
            right_title="Gain mode"
            menu_direction="below"
            panel_align="left"
            panel_style="min-width: 210px;"
        >
            <button class=move || layer_opt_class_simple(state.gain.mode().get() == GainMode::Off)
                on:click=move |_| {
                    state.gain.mode().set(GainMode::Off);
                    state.gain.auto().set(false);
                    state.panels.layer_panel_open().set(None);
                }
            >"Off"</button>
            <button class=move || layer_opt_class_simple(state.gain.mode().get() == GainMode::Manual)
                on:click=move |_| {
                    state.gain.mode().set(GainMode::Manual);
                    state.gain.auto().set(false);
                    state.panels.layer_panel_open().set(None);
                }
            >"Manual \u{2014} Slider boost only"</button>
            <button class=move || layer_opt_class_simple(state.gain.mode().get() == GainMode::AutoPeak)
                on:click=move |_| {
                    state.gain.mode().set(GainMode::AutoPeak);
                    state.gain.mode_last_auto().set(GainMode::AutoPeak);
                    state.gain.auto().set(true);
                    state.panels.layer_panel_open().set(None);
                }
            >"Peak \u{2014} Normalize to peak"</button>
            <button class=move || layer_opt_class_simple(state.gain.mode().get() == GainMode::Adaptive)
                on:click=move |_| {
                    state.gain.mode().set(GainMode::Adaptive);
                    state.gain.mode_last_auto().set(GainMode::Adaptive);
                    state.gain.auto().set(true);
                    state.panels.layer_panel_open().set(None);
                }
            >"AGC \u{2014} Automatic gain control"</button>
            <Show when=move || state.gain.mode().get() == GainMode::AutoPeak>
                <div class="peak-source-row">
                    <span class="peak-source-label">"Peak from:"</span>
                    <button class=move || if state.gain.peak_source().get() == PeakSource::First30s { "peak-src-btn sel" } else { "peak-src-btn" }
                        on:click=move |_| state.gain.peak_source().set(PeakSource::First30s)
                        title="Peak from first 30 seconds"
                    >"30s"</button>
                    <button class=move || if state.gain.peak_source().get() == PeakSource::FullWave { "peak-src-btn sel" } else { "peak-src-btn" }
                        on:click=move |_| state.gain.peak_source().set(PeakSource::FullWave)
                        title="Peak from entire file"
                    >"Full"</button>
                    <button class=move || {
                        let base = if state.gain.peak_source().get() == PeakSource::Selection { "peak-src-btn sel" } else { "peak-src-btn" };
                        if state.interaction.selection().get().is_none() { format!("{} disabled", base) } else { base.to_string() }
                    }
                        on:click=move |_| {
                            if state.interaction.selection().get_untracked().is_some() {
                                state.gain.peak_source().set(PeakSource::Selection);
                            }
                        }
                        title="Peak from current selection"
                    >"Sel"</button>
                    <button class=move || if state.gain.peak_source().get() == PeakSource::Processed { "peak-src-btn sel" } else { "peak-src-btn" }
                        on:click=move |_| state.gain.peak_source().set(PeakSource::Processed)
                        title="Peak after DSP processing"
                    >"DSP"</button>
                </div>
            </Show>
            <div class="layer-panel-slider-row" style="margin-top: 6px;">
                <span class="slider-label">{move || if is_live() { "Live" } else { "Boost" }}</span>
                <label>{move || {
                    if is_live() {
                        let db = state.gain.live_db().get();
                        return if db > 0.0 { format!("+{:.0}dB", db) } else { format!("{:.0}dB", db) };
                    }
                    let db = state.gain.db().get();
                    let pv = if state.playback.mode().get() == PlaybackMode::PhaseVocoder { PV_MODE_BOOST_DB } else { 0.0 };
                    let total = db + pv;
                    if total > 0.0 { format!("+{:.0}dB", total) }
                    else { format!("{:.0}dB", total) }
                }}</label>
                <input type="range" min="-12" max="60" step="1"
                    prop:value=move || if is_live() {
                        state.gain.live_db().get().to_string()
                    } else {
                        state.gain.db().get().to_string()
                    }
                    on:input=move |ev| {
                        let val: f64 = leptos::prelude::event_target_value(&ev).parse().unwrap_or(0.0);
                        if is_live() {
                            state.gain.live_db().set(val);
                        } else {
                            state.gain.db().set(val);
                            if state.gain.mode().get_untracked() == GainMode::Off && val > 0.0 {
                                state.gain.mode().set(GainMode::Manual);
                            }
                        }
                    }
                    on:dblclick=move |_| {
                        if is_live() {
                            state.gain.live_db().set(0.0);
                        } else {
                            state.gain.db().set(0.0);
                        }
                    }
                />
            </div>
        </ComboButton>
    }
}

/// Bandpass + EQ combo. Migrated from HfrButton's dropdown body. Owns
/// `bandpass_mode` (Off/Auto/On), `bandpass_range` (FollowFocus/Custom),
/// the four `filter_db_*` band sliders, `filter_quality`, and
/// `filter_band_mode` (3 vs 4 band split).
#[component]
fn BandpassCombo() -> impl IntoView {
    let state = expect_context::<AppState>();

    let is_open =
        Signal::derive(move || state.panels.layer_panel_open().get() == Some(LayerPanel::Bandpass));
    let no_file = move || {
        state.library.current_index().get().is_none() && state.timeline.active().get().is_none()
    };

    let active = Signal::derive(move || state.filter.bandpass_mode().get() != BandpassMode::Off);

    let left_class = Signal::derive(move || {
        if no_file() {
            "layer-btn combo-btn-left disabled"
        } else if active.get() {
            "layer-btn combo-btn-left no-annotation active"
        } else {
            "layer-btn combo-btn-left no-annotation"
        }
    });
    let right_class = Signal::derive(move || {
        if no_file() {
            return "layer-btn combo-btn-right disabled";
        }
        let dim = if !active.get() { " dim" } else { "" };
        if is_open.get() {
            if dim.is_empty() {
                "layer-btn combo-btn-right open"
            } else {
                "layer-btn combo-btn-right dim open"
            }
        } else if dim.is_empty() {
            "layer-btn combo-btn-right"
        } else {
            "layer-btn combo-btn-right dim"
        }
    });

    // Left half just labels the button. Right half shows the bandpass
    // mode (OFF/AUTO/ON); when the configured range *differs* from the
    // FF/HFR band (i.e. locked to something else), the differing range
    // appears as a small caption above the mode word.
    let left_value = Signal::derive(|| "PASS".to_string());

    let range_differs = Signal::derive(move || {
        let lo = state.filter.freq_low().get();
        let hi = state.filter.freq_high().get();
        if hi <= lo {
            return false;
        }
        let ff_lo = state.filter.band_ff_freq_lo().get();
        let ff_hi = state.filter.band_ff_freq_hi().get();
        if ff_hi <= ff_lo {
            return true;
        } // no FF set, but bandpass has a range
        (lo - ff_lo).abs() > 50.0 || (hi - ff_hi).abs() > 50.0
    });
    let right_label = Signal::derive(move || {
        // Hide the range caption when:
        //   (A) bandpass is off — nothing to describe,
        //   (B) the filter isn't actually active (e.g. Auto+HET, which
        //       leaves filter_enabled = false even though bandpass_mode
        //       is Auto — showing the stale filter range here looked
        //       like "Follow is broken" because it didn't update with
        //       the HFR band),
        //   (C) the range tracks the HFR band exactly — redundant with
        //       the Range dropdown.
        if state.filter.bandpass_mode().get() == BandpassMode::Off {
            return String::new();
        }
        if !state.filter.enabled().get() {
            return String::new();
        }
        if !range_differs.get() {
            return String::new();
        }
        let lo = state.filter.freq_low().get();
        let hi = state.filter.freq_high().get();
        format!("{:.1}\u{2013}{:.1}", lo / 1000.0, hi / 1000.0)
    });
    let right_value = Signal::derive(move || match state.filter.bandpass_mode().get() {
        BandpassMode::Off => "OFF".to_string(),
        BandpassMode::Auto => "AUTO".to_string(),
        BandpassMode::On => "ON".to_string(),
    });

    let left_click = Callback::new(move |_: web_sys::MouseEvent| {
        if no_file() {
            return;
        }
        let mode = state.filter.bandpass_mode().get_untracked();
        if mode == BandpassMode::Off {
            // Turn on: prefer Auto when HFR is on (band-following), else On.
            let next = if state.viewmode.focus_stack().get_untracked().hfr_enabled() {
                BandpassMode::Auto
            } else {
                BandpassMode::On
            };
            state.filter.bandpass_mode().set(next);
        } else {
            state.filter.bandpass_mode().set(BandpassMode::Off);
        }
    });
    let toggle_menu = Callback::new(move |()| {
        toggle_panel(&state, LayerPanel::Bandpass);
    });

    // Generic over the signal handle so it accepts store subfields
    // (`state.filter.db_above()` etc.), which are each a distinct type.
    fn make_db_handler<S>(state: AppState, signal: S) -> impl Fn(web_sys::Event) + Copy + 'static
    where
        S: leptos::prelude::Set<Value = f64> + Copy + 'static,
    {
        move |ev: web_sys::Event| {
            use wasm_bindgen::JsCast;
            let input: web_sys::HtmlInputElement = ev.target().unwrap().unchecked_into();
            if let Ok(val) = input.value().parse::<f64>() {
                if state.filter.bandpass_mode().get_untracked() == BandpassMode::Auto {
                    state.filter.bandpass_mode().set(BandpassMode::On);
                }
                signal.set(val);
            }
        }
    }
    let on_above_change = make_db_handler(state, state.filter.db_above());
    let on_selected_change = make_db_handler(state, state.filter.db_selected());
    let on_harmonics_change = make_db_handler(state, state.filter.db_harmonics());
    let on_below_change = make_db_handler(state, state.filter.db_below());

    let on_quality_click = move |q: FilterQuality| {
        move |_: web_sys::MouseEvent| {
            if state.filter.bandpass_mode().get_untracked() == BandpassMode::Auto {
                state.filter.bandpass_mode().set(BandpassMode::On);
            }
            state.filter.quality().set(q);
        }
    };
    let on_band_click = move |b: BandMode| {
        move |_: web_sys::MouseEvent| {
            if state.filter.bandpass_mode().get_untracked() == BandpassMode::Auto {
                state.filter.bandpass_mode().set(BandpassMode::On);
            }
            state.filter.band_mode().set(b);
        }
    };

    view! {
        <ComboButton
            left_label=""
            left_value=left_value
            left_click=left_click
            left_class=left_class
            right_value=right_value
            right_class=right_class
            right_label=right_label
            is_open=is_open
            toggle_menu=toggle_menu
            left_title="Toggle bandpass / EQ"
            right_title="Bandpass mode"
            menu_direction="below"
            panel_align="left"
            panel_style="min-width: 240px;"
        >
            <div class="layer-panel-title">"Bandpass"</div>
            <div style="display: flex; gap: 2px; padding: 0 6px 4px;">
                <Show when=move || state.viewmode.hfr_enabled().get()>
                    <button class=move || layer_opt_class_simple(state.filter.bandpass_mode().get() == BandpassMode::Auto)
                        on:click=move |_| state.filter.bandpass_mode().set(BandpassMode::Auto)
                    >"AUTO"</button>
                </Show>
                <button class=move || layer_opt_class_simple(state.filter.bandpass_mode().get() == BandpassMode::Off)
                    on:click=move |_| state.filter.bandpass_mode().set(BandpassMode::Off)
                >"OFF"</button>
                <button class=move || layer_opt_class_simple(state.filter.bandpass_mode().get() == BandpassMode::On)
                    on:click=move |_| {
                        if !state.viewmode.focus_stack().get_untracked().hfr_enabled() {
                            state.viewmode.focus_stack().update(|s| s.set_saved_playback_mode(Some(PlaybackMode::Normal)));
                            state.toggle_hfr();
                        }
                        state.filter.bandpass_mode().set(BandpassMode::On);
                    }
                >"ON"</button>
            </div>
            <Show when=move || {
                let bp = state.filter.bandpass_mode().get();
                bp == BandpassMode::On
                    || (bp == BandpassMode::Auto && state.band_ff_range().is_some())
            }>
                <div style="display: flex; gap: 2px; padding: 0 6px 2px;">
                    <button class=move || layer_opt_class_simple(state.filter.bandpass_range().get() == BandpassRange::FollowFocus)
                        on:click=move |_| state.filter.bandpass_range().set(BandpassRange::FollowFocus)
                        title="Range tracks the active FF/HFR focus"
                    >"Follow"</button>
                    <button class=move || layer_opt_class_simple(state.filter.bandpass_range().get() == BandpassRange::Locked)
                        on:click=move |_| state.filter.bandpass_range().set(BandpassRange::Locked)
                        title="Lock the range here \u{2014} won't track focus changes"
                    >"Locked"</button>
                </div>
                <div class="bandpass-range-readout">
                    {move || format!("{:.1}\u{2013}{:.1} kHz",
                        state.filter.freq_low().get() / 1000.0,
                        state.filter.freq_high().get() / 1000.0
                    )}
                    <Show when=move || state.filter.bandpass_range().get() == BandpassRange::Locked>
                        <span class="bandpass-lock-icon" title="Locked">{"\u{00A0}\u{1F512}"}</span>
                    </Show>
                </div>
                <div style="display: flex; gap: 2px; padding: 0 6px 2px;">
                    <button class=move || layer_opt_class_simple(state.filter.quality().get() == FilterQuality::Fast)
                        on:click=on_quality_click(FilterQuality::Fast)
                        title="IIR band-split \u{2014} low latency, softer edges"
                    >"Fast"</button>
                    <button class=move || layer_opt_class_simple(state.filter.quality().get() == FilterQuality::Spectral)
                        on:click=on_quality_click(FilterQuality::Spectral)
                        title="FFT spectral EQ \u{2014} sharp edges, higher latency"
                    >"HQ"</button>
                    <span style="width: 8px;"></span>
                    <button class=move || layer_opt_class_simple(state.filter.band_mode().get() == BandMode::ThreeBand)
                        on:click=on_band_click(BandMode::ThreeBand)
                    >"3"</button>
                    <button class=move || layer_opt_class_simple(state.filter.band_mode().get() == BandMode::FourBand)
                        on:click=on_band_click(BandMode::FourBand)
                    >"4"</button>
                </div>
                <div class="layer-panel-slider-row"
                    on:mouseenter=move |_| state.filter.hovering_band().set(Some(3))
                    on:mouseleave=move |_| state.filter.hovering_band().set(None)
                >
                    <label>"Above"</label>
                    <input type="range" min="-60" max="6" step="1"
                        prop:value=move || state.filter.db_above().get().to_string()
                        on:input=on_above_change
                    />
                    <span>{move || format!("{:.0}", state.filter.db_above().get())}</span>
                </div>
                <Show when=move || { state.filter.band_mode().get().has_harmonics() }>
                    <div class="layer-panel-slider-row"
                        on:mouseenter=move |_| state.filter.hovering_band().set(Some(2))
                        on:mouseleave=move |_| state.filter.hovering_band().set(None)
                    >
                        <label>"Harm"</label>
                        <input type="range" min="-60" max="6" step="1"
                            prop:value=move || state.filter.db_harmonics().get().to_string()
                            on:input=on_harmonics_change
                        />
                        <span>{move || format!("{:.0}", state.filter.db_harmonics().get())}</span>
                    </div>
                </Show>
                <div class="layer-panel-slider-row"
                    on:mouseenter=move |_| state.filter.hovering_band().set(Some(1))
                    on:mouseleave=move |_| state.filter.hovering_band().set(None)
                >
                    <label>"Band"</label>
                    <input type="range" min="-60" max="6" step="1"
                        prop:value=move || state.filter.db_selected().get().to_string()
                        on:input=on_selected_change
                    />
                    <span>{move || format!("{:.0}", state.filter.db_selected().get())}</span>
                </div>
                <div class="layer-panel-slider-row"
                    on:mouseenter=move |_| state.filter.hovering_band().set(Some(0))
                    on:mouseleave=move |_| state.filter.hovering_band().set(None)
                >
                    <label>"Below"</label>
                    <input type="range" min="-60" max="6" step="1"
                        prop:value=move || state.filter.db_below().get().to_string()
                        on:input=on_below_change
                    />
                    <span>{move || format!("{:.0}", state.filter.db_below().get())}</span>
                </div>
            </Show>
        </ComboButton>
    }
}

// NotchCombo and NrCombo live in `noise_combos.rs` — they contain the
// Detect / Sensitivity / Bands / Learn / Strength controls migrated
// from the deleted right-sidebar Noise Filter tab.

/// The BAND cell that sits above the band gutter is now an HFR on/off
/// toggle. The cell's label is the literal "HFR" with per-letter brightness
/// driven by classes on the cell itself (formerly applied to a wrapper
/// around the old HfrButton's left half).
#[component]
fn BandHfrCell() -> impl IntoView {
    let state = expect_context::<AppState>();
    let no_file = move || {
        state.library.current_index().get().is_none() && state.timeline.active().get().is_none()
    };

    let cell_class = Signal::derive(move || {
        let on = state.viewmode.hfr_enabled().get();
        let h_dim = on && {
            let lo = state.filter.band_ff_freq_lo().get();
            let hi = state.filter.band_ff_freq_hi().get();
            hi > lo && hi < 24_000.0
        };
        let mut s = String::from("band-cell band-cell-hfr");
        if on {
            s.push_str(" hfr-on");
        }
        if h_dim {
            s.push_str(" hfr-h-dim");
        }
        if no_file() {
            s.push_str(" disabled");
        }
        s
    });

    let title = Signal::derive(move || {
        if state.viewmode.hfr_enabled().get() {
            "High-frequency reception is ON — click to disable.".to_string()
        } else {
            "Enable high-frequency reception — process and listen to the band selected on the gutter below.".to_string()
        }
    });

    view! {
        <button class=move || cell_class.get()
            title=move || title.get()
            on:click=move |_: web_sys::MouseEvent| {
                if !no_file() { state.toggle_hfr(); }
            }
        >
            <span class="band-cell-hfr-label">"HFR"</span>
        </button>
    }
}

/// Whether the bandpass is currently "locked" (range is fixed regardless
/// of the active HFR band). Drives the grey overline above PASS.
fn pass_is_locked(state: AppState) -> bool {
    match state.filter.bandpass_mode().get() {
        BandpassMode::Off => false,
        _ => state.filter.bandpass_range().get() == BandpassRange::Locked,
    }
}

/// Hearing Bar. Mounted between the OverviewPanel and the main view.
#[component]
pub fn HearingBar() -> impl IntoView {
    let state = expect_context::<AppState>();
    let bar_ref = NodeRef::<leptos::html::Div>::new();
    let pan = crate::components::bar_pan::BarPan::new(bar_ref);
    let overline_class = Signal::derive(move || {
        let on = state.viewmode.hfr_enabled().get();
        let mut s = String::from("band-affected-row");
        if on {
            s.push_str(" band-on");
        } else {
            s.push_str(" band-off");
        }
        if pass_is_locked(state) {
            s.push_str(" pass-locked");
        }
        s
    });
    view! {
        // stop_propagation guards against future plain <button> children
        // having their panel-open clicks eaten by .main's catch-all.
        // ComboButton children already handle this themselves.
        <div class="hearing-bar"
            node_ref=bar_ref
            class:panel-open=move || matches!(state.panels.layer_panel_open().get().map(LayerPanel::bar), Some(Bar::Hearing))
            on:click=|ev: web_sys::MouseEvent| ev.stop_propagation()
            on:touchstart=|ev: web_sys::TouchEvent| ev.stop_propagation()
            on:pointerdown=move |ev| pan.on_pointerdown(ev)
            on:pointermove=move |ev| pan.on_pointermove(ev)
            on:pointerup=move |ev| pan.on_pointerup(ev)
            on:pointercancel=move |ev| pan.on_pointerup(ev)
            on:wheel=move |ev| pan.on_wheel(ev)
        >
            // Band-affected group — overline border-top spans these four.
            <div class=move || overline_class.get()>
                <BandHfrCell/>
                <div class="bar-controls band-affected-controls">
                    <RangeButton/>
                    <ModeRadioGroup/>
                    <crate::components::output_range_button::OutputRangeCombo/>
                    <BandpassCombo/>
                </div>
            </div>
            <div class="bar-sep"></div>
            <div class="bar-controls">
                <GainCombo/>
                <NrCombo/>
                <NotchCombo/>
            </div>
        </div>
    }
}
