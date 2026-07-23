// Output Range combo — sits at the end of the band-affected group in the
// Hearing bar, after the mode picker. Shows + edits how the active
// playback mode maps the input frequency focus into the 0–2000 Hz
// target listening range.
//
//   [Output ÷8 ▾]
//
// The popup is a two-column layout: mode-aware preset/slider controls
// on the left, and a vertical 0–2000 Hz "output gutter" on the right
// that visualises the current output band and is itself draggable.
//
// Two playback styles are handled in Phase 1:
//   • Divide (TE / PS / PV / ZC) — output_freq = input_freq / factor
//   • Linear shift (Heterodyne)  — output_freq = |input_freq − carrier|
//
// Each style edits the same underlying signals the mode's own popup
// edits; this component just exposes them through an output-side
// perspective. The Snap toggle constrains dragging to canonical stops
// (powers of 2 / 10 for factors; multiples of 5 kHz for carriers).

use crate::state::store_fields::*;
use leptos::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use crate::components::mode_button::{format_factor_value, format_freq_khz, output_freq};
use crate::components::popup::{Align, PopupPanel, Side};
use crate::dsp::pitch_shift::PitchFactor;
use crate::state::{AppState, LayerPanel, OutputSnap, PlaybackMode};

/// Top of the gutter scale (Hz). Bottom is always 0. Larger than typical
/// human-listening max so we cover the "audible-ish" output range bats
/// often land in after a divide.
const GUTTER_MAX_HZ: f64 = 10_000.0;

/// Tick marks on the gutter (Hz). Sparse enough to stay readable inside
/// the popup but dense enough to read pin positions at a glance.
const GUTTER_TICKS: [i32; 6] = [0, 2_000, 4_000, 6_000, 8_000, 10_000];

/// Canonical divide factors for the Standard snap mode.
const STANDARD_FACTORS: [f64; 8] = [2.0, 4.0, 8.0, 10.0, 16.0, 32.0, 64.0, 128.0];
/// Equal-chroma snap: powers of 2 only (preserves pitch class).
const CHROMA_FACTORS: [f64; 7] = [2.0, 4.0, 8.0, 16.0, 32.0, 64.0, 128.0];

/// Carrier snap step (Hz) for heterodyne mode when Snap is on.
const HET_SNAP_STEP_HZ: f64 = 5_000.0;

fn toggle_panel(state: &AppState, panel: LayerPanel) {
    state.panels.layer_panel_open().update(|p| {
        *p = if *p == Some(panel) { None } else { Some(panel) };
    });
}

/// Logical style picked by the current playback mode.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Style {
    /// No mapping — output is the same as input (Normal mode).
    Passthrough,
    /// output = input / factor (TE, PS, PV, ZC).
    Divide,
    /// output = |input − carrier| (Heterodyne).
    LinearShift,
}

fn style_for(mode: PlaybackMode) -> Style {
    match mode {
        PlaybackMode::Normal => Style::Passthrough,
        PlaybackMode::Heterodyne => Style::LinearShift,
        PlaybackMode::TimeExpansion
        | PlaybackMode::PitchShift
        | PlaybackMode::PhaseVocoder
        | PlaybackMode::ZeroCrossing => Style::Divide,
    }
}

// `transform.*_factor()` are distinct store-subfield types, so we can't return
// a single `RwSignal<f64>` handle selected at runtime. These mode-dispatch
// value/action helpers replace the former `factor_signal`/`auto_signal`.

/// Whether `mode` has a divide-factor (TE/PS/PV/ZC).
fn has_factor(mode: PlaybackMode) -> bool {
    matches!(
        mode,
        PlaybackMode::TimeExpansion
            | PlaybackMode::PitchShift
            | PlaybackMode::PhaseVocoder
            | PlaybackMode::ZeroCrossing
    )
}

/// Current divide-factor for `mode` (None for Normal/Het).
fn factor_value(state: &AppState, mode: PlaybackMode) -> Option<f64> {
    match mode {
        PlaybackMode::TimeExpansion => Some(state.transform.te_factor().get()),
        PlaybackMode::PitchShift => Some(state.transform.ps_factor().get().signed()),
        PlaybackMode::PhaseVocoder => Some(state.transform.pv_factor().get().signed()),
        PlaybackMode::ZeroCrossing => Some(state.transform.zc_factor().get()),
        _ => None,
    }
}

/// Untracked divide-factor for `mode`.
fn factor_value_untracked(state: &AppState, mode: PlaybackMode) -> Option<f64> {
    match mode {
        PlaybackMode::TimeExpansion => Some(state.transform.te_factor().get_untracked()),
        PlaybackMode::PitchShift => Some(state.transform.ps_factor().get_untracked().signed()),
        PlaybackMode::PhaseVocoder => Some(state.transform.pv_factor().get_untracked().signed()),
        PlaybackMode::ZeroCrossing => Some(state.transform.zc_factor().get_untracked()),
        _ => None,
    }
}

/// Set the divide-factor for `mode` (no-op for Normal/Het).
fn set_factor(state: &AppState, mode: PlaybackMode, v: f64) {
    match mode {
        PlaybackMode::TimeExpansion => state.transform.te_factor().set(v),
        PlaybackMode::PitchShift => state.transform.ps_factor().set(PitchFactor::from_signed(v)),
        PlaybackMode::PhaseVocoder => state.transform.pv_factor().set(PitchFactor::from_signed(v)),
        PlaybackMode::ZeroCrossing => state.transform.zc_factor().set(v),
        _ => {}
    }
}

/// Auto-derive-from-BandFF flag for `mode` (None where N/A).
fn auto_value(state: &AppState, mode: PlaybackMode) -> Option<bool> {
    match mode {
        PlaybackMode::TimeExpansion => Some(state.transform.te_factor_auto().get()),
        PlaybackMode::PitchShift => Some(state.transform.ps_factor_auto().get()),
        PlaybackMode::PhaseVocoder => Some(state.transform.pv_factor_auto().get()),
        PlaybackMode::Heterodyne => Some(state.transform.het_freq_auto().get()),
        _ => None,
    }
}

/// Set the auto flag for `mode` (no-op where N/A).
fn set_auto(state: &AppState, mode: PlaybackMode, v: bool) {
    match mode {
        PlaybackMode::TimeExpansion => state.transform.te_factor_auto().set(v),
        PlaybackMode::PitchShift => state.transform.ps_factor_auto().set(v),
        PlaybackMode::PhaseVocoder => state.transform.pv_factor_auto().set(v),
        PlaybackMode::Heterodyne => state.transform.het_freq_auto().set(v),
        _ => {}
    }
}

/// Toggle the auto flag for `mode`.
fn toggle_auto(state: &AppState, mode: PlaybackMode) {
    match mode {
        PlaybackMode::TimeExpansion => state.transform.te_factor_auto().update(|v| *v = !*v),
        PlaybackMode::PitchShift => state.transform.ps_factor_auto().update(|v| *v = !*v),
        PlaybackMode::PhaseVocoder => state.transform.pv_factor_auto().update(|v| *v = !*v),
        PlaybackMode::Heterodyne => state.transform.het_freq_auto().update(|v| *v = !*v),
        _ => {}
    }
}

fn band(state: &AppState) -> (f64, f64) {
    (
        state.filter.band_ff_freq_lo().get(),
        state.filter.band_ff_freq_hi().get(),
    )
}

/// Whether this mode supports a compound scale + shift mapping.
/// Only the PS bucket (PitchShift / PhaseVocoder) does today — the
/// `ps_shift_hz` signal is honoured by `apply_dsp_mode`'s PS/PV
/// branches via a heterodyne pre-shift stage.
fn supports_shift(mode: PlaybackMode) -> bool {
    matches!(mode, PlaybackMode::PitchShift | PlaybackMode::PhaseVocoder)
}

/// Clamp the user's stored output-shift to the post-divide low edge so
/// the compound mapping `out = |in/factor − shift|` never folds below
/// zero. Callers (UI math + DSP snapshot) all funnel through this so
/// the visualisation and the actual audio stay in sync.
///
/// `band_lo` of 0 means there's no lower bound to clamp to (e.g. user
/// hasn't set a frequency focus) — in that case we leave the stored
/// value alone and accept that folding can happen.
pub(crate) fn effective_ps_shift(stored: f64, band_lo: f64, factor: f64) -> f64 {
    let s = stored.max(0.0);
    let f = factor.abs().max(1.0);
    if band_lo <= 0.0 {
        return s;
    }
    let div_lo = band_lo / f;
    s.min(div_lo)
}

/// Compute the current effective output range derived from the active
/// mode + its parameters + the BandFF input range. Returns None when
/// there's no meaningful range to draw (e.g. BandFF is empty).
fn current_output_range(state: &AppState, mode: PlaybackMode) -> Option<(f64, f64)> {
    let (in_lo, in_hi) = band(state);
    if in_hi <= in_lo {
        return None;
    }
    let style = style_for(mode);
    match style {
        Style::Passthrough => Some((in_lo, in_hi)),
        Style::Divide => {
            let f = factor_value(state, mode)?;
            // Optional output-side shift for PS/PV — divide first, then
            // subtract. Clamped to the post-divide low edge so the
            // output band never folds below zero.
            let shift = if supports_shift(mode) {
                effective_ps_shift(state.transform.ps_shift_hz().get(), in_lo, f)
            } else {
                0.0
            };
            let a = (output_freq(in_lo, f) - shift).abs();
            let b = (output_freq(in_hi, f) - shift).abs();
            Some(if a < b { (a, b) } else { (b, a) })
        }
        Style::LinearShift => {
            let c = state.transform.het_frequency().get();
            let a = (in_lo - c).abs();
            let b = (in_hi - c).abs();
            Some(if a < b { (a, b) } else { (b, a) })
        }
    }
}

fn divide_shorthand(f: f64) -> String {
    let s = format_factor_value(f);
    if s.starts_with('\u{00f7}') {
        s
    } else {
        format!("\u{00f7}{s}")
    }
}

/// Short mapping label for the toolbar button left side.
fn mapping_shorthand(state: &AppState, mode: PlaybackMode) -> String {
    match style_for(mode) {
        Style::Passthrough => "1:1".into(),
        Style::Divide => {
            let Some(f) = factor_value(state, mode) else {
                return "\u{2014}".into();
            };
            let div = divide_shorthand(f);
            if supports_shift(mode) {
                let shift = state.transform.ps_shift_hz().get();
                if shift.abs() >= 50.0 {
                    return format!("{div} \u{2212}{}", format_freq_khz(shift));
                }
            }
            div
        }
        Style::LinearShift => {
            let c = state.transform.het_frequency().get();
            // Negative because heterodyne shifts the band *down*.
            format!("\u{2212}{}", format_freq_khz(c))
        }
    }
}

/// Snap a free-form factor according to the current snap policy.
fn snap_factor(raw: f64, snap: OutputSnap) -> f64 {
    let abs = raw.abs().max(1.5);
    let snapped_abs = match snap {
        OutputSnap::Free => ((abs * 10.0).round() / 10.0).clamp(1.5, 64.0),
        OutputSnap::Standard => STANDARD_FACTORS
            .iter()
            .copied()
            .min_by(|a, b| (a - abs).abs().partial_cmp(&(b - abs).abs()).unwrap())
            .unwrap_or(8.0),
        OutputSnap::EqualChroma => CHROMA_FACTORS
            .iter()
            .copied()
            .min_by(|a, b| (a - abs).abs().partial_cmp(&(b - abs).abs()).unwrap())
            .unwrap_or(8.0),
    };
    if raw < 0.0 {
        -snapped_abs
    } else {
        snapped_abs
    }
}

fn snap_carrier(raw_hz: f64, snap: OutputSnap) -> f64 {
    let clamped = raw_hz.max(0.0);
    match snap {
        OutputSnap::Free => (clamped / 100.0).round() * 100.0,
        // Both Standard and EqualChroma snap carriers to 5 kHz — there's
        // no meaningful "musical interval" quantisation for an additive
        // shift, so EqualChroma falls back to the same step.
        OutputSnap::Standard | OutputSnap::EqualChroma => {
            (clamped / HET_SNAP_STEP_HZ).round() * HET_SNAP_STEP_HZ
        }
    }
}

#[component]
pub fn OutputRangeCombo() -> impl IntoView {
    let state = expect_context::<AppState>();

    let is_open = Signal::derive(move || {
        state.panels.layer_panel_open().get() == Some(LayerPanel::OutputRange)
    });
    let no_file = move || {
        state.library.current_index().get().is_none() && state.timeline.active().get().is_none()
    };

    let mode = Signal::derive(move || state.playback.mode().get());
    let anchor_ref = NodeRef::<leptos::html::Div>::new();

    let on_click = move |_: web_sys::MouseEvent| {
        if no_file() {
            return;
        }
        toggle_panel(&state, LayerPanel::OutputRange);
    };

    let btn_class = move || {
        let mut cls = String::from("layer-btn out-range-btn");
        if no_file() {
            cls.push_str(" disabled");
        } else {
            cls.push_str(" no-annotation active");
        }
        if is_open.get() {
            cls.push_str(" open");
        }
        cls
    };

    view! {
        <div node_ref=anchor_ref class="out-range-btn-wrap">
            <button
                class=btn_class
                on:click=on_click
                title="Output range \u{2014} how input frequencies are mapped to audible output"
            >
                <span class="out-range-btn-label">
                    <span class="layer-btn-category">"OUT"</span>
                    <span class="layer-btn-value">{move || mapping_shorthand(&state, mode.get())}</span>
                </span>
                <span class="combo-btn-arrow">{"\u{25E2}"}</span>
            </button>
            <PopupPanel
                is_open=is_open
                anchor=anchor_ref
                preferred_side=Side::Below
                preferred_align=Align::Start
                extra_style="min-width: 320px;"
            >
                <OutputRangePopup/>
            </PopupPanel>
        </div>
    }
}

#[component]
fn OutputRangePopup() -> impl IntoView {
    let state = expect_context::<AppState>();

    view! {
        <div class="output-range-popup">
            <div class="output-range-controls">
                <div class="layer-panel-title">"Output range"</div>
                {move || {
                    let mode = state.playback.mode().get();
                    match style_for(mode) {
                        Style::Passthrough => view! {
                            <div class="output-range-empty">
                                "1:1 \u{2014} input plays back unchanged."
                                <br/>
                                "Pick HET, TE, PS or ZC to remap above-audible content."
                            </div>
                        }.into_any(),
                        Style::Divide => view! { <DivideControls mode/> }.into_any(),
                        Style::LinearShift => view! { <LinearShiftControls/> }.into_any(),
                    }
                }}
                <SnapPicker/>
                <BandSummary/>
            </div>
            <OutputGutter/>
        </div>
    }
}

#[component]
fn DivideControls(#[prop(into)] mode: Signal<PlaybackMode>) -> impl IntoView {
    let state = expect_context::<AppState>();

    let preset_values: [(f64, &str); 8] = [
        (2.0, "\u{00f7}2"),
        (4.0, "\u{00f7}4"),
        (8.0, "\u{00f7}8"),
        (10.0, "\u{00f7}10"),
        (16.0, "\u{00f7}16"),
        (32.0, "\u{00f7}32"),
        (64.0, "\u{00f7}64"),
        (128.0, "\u{00f7}128"),
    ];

    let click_preset = move |value: f64| {
        let m = mode.get_untracked();
        if has_factor(m) {
            set_auto(&state, m, false);
            set_factor(&state, m, value);
        }
    };

    view! {
        <div class="output-range-section-label">"Divide factor"</div>
        <div class="output-range-presets">
            {move || {
                let m = mode.get();
                let on_auto = auto_value(&state, m).unwrap_or(false);
                view! {
                    <button
                        class=if on_auto { "factor-preset auto on" } else { "factor-preset auto" }
                        on:click=move |_| {
                            toggle_auto(&state, m);
                        }
                        title="Auto-pick factor from current band"
                    >"Auto"</button>
                }
            }}
            {preset_values.iter().map(|&(val, label)| {
                let sel = Signal::derive(move || {
                    let m = mode.get();
                    let f = factor_value(&state, m).unwrap_or(0.0);
                    let auto = auto_value(&state, m).unwrap_or(false);
                    (f.abs() - val).abs() < 0.01 && !auto
                });
                let click = move |_: web_sys::MouseEvent| click_preset(val);
                view! {
                    <button
                        class=move || if sel.get() { "factor-preset sel" } else { "factor-preset" }
                        on:click=click
                    >{label}</button>
                }
            }).collect::<Vec<_>>()}
        </div>
        // ── Optional output-side shift (PS / PV only) ──
        // The user thinks/sets in output-Hz space ("shift the output
        // down by 500 Hz"); the DSP applies it as a post-pitch
        // heterodyne, which keeps the LP cutoff small and well-behaved.
        // Equivalent input-side shift would be `shift × factor`, but
        // we never ask the user to convert.
        {move || {
            if !supports_shift(mode.get()) { return view! { <span></span> }.into_any(); }
            let on_shift_input = move |ev: web_sys::Event| {
                use wasm_bindgen::JsCast;
                let el: web_sys::HtmlInputElement = ev.target().unwrap().unchecked_into();
                if let Ok(khz) = el.value().parse::<f64>() {
                    state.transform.ps_shift_hz().set((khz * 1000.0).max(0.0));
                }
            };
            view! {
                <div class="output-range-section-label"
                    title="Subtract a fixed offset from the OUTPUT frequency, applied after the pitch divide. Compound mapping: out = |in/factor − shift|. Example: input 30–50 kHz ÷8 lands at 3.75–6.25 kHz; setting Output shift to 3 kHz brings it down to 0.75–3.25 kHz."
                >"Output shift"<span class="output-range-help">"\u{2139}"</span></div>
                <div class="output-range-shift-row">
                    <input type="number"
                        class="output-range-num"
                        min="0" max="20" step="0.1"
                        prop:value=move || format!("{:.2}", state.transform.ps_shift_hz().get() / 1000.0)
                        on:input=on_shift_input
                        title="Subtract this many kHz from the output (after pitch divide)"
                    />
                    <span class="output-range-num-suffix">"kHz"</span>
                    <button
                        class="auto-toggle"
                        on:click=move |_| state.transform.ps_shift_hz().set(0.0)
                        title="Clear output shift (back to pure divide)"
                    >"0"</button>
                </div>
                <div class="output-range-presets">
                    {[0.0, 0.5, 1.0, 2.0, 3.0, 5.0].iter().map(|&khz| {
                        let click = move |_: web_sys::MouseEvent| {
                            state.transform.ps_shift_hz().set(khz * 1000.0);
                        };
                        let sel = Signal::derive(move || {
                            (state.transform.ps_shift_hz().get() - khz * 1000.0).abs() < 50.0
                        });
                        let label = if khz == 0.0 {
                            "0".to_string()
                        } else if khz < 1.0 {
                            format!("\u{2212}{:.1}k", khz)
                        } else {
                            format!("\u{2212}{}k", khz as i32)
                        };
                        view! {
                            <button
                                class=move || if sel.get() { "factor-preset sel" } else { "factor-preset" }
                                on:click=click
                            >{label}</button>
                        }
                    }).collect::<Vec<_>>()}
                </div>
            }.into_any()
        }}
    }
}

#[component]
fn LinearShiftControls() -> impl IntoView {
    let state = expect_context::<AppState>();

    let on_input = move |ev: web_sys::Event| {
        use wasm_bindgen::JsCast;
        let el: web_sys::HtmlInputElement = ev.target().unwrap().unchecked_into();
        if let Ok(khz) = el.value().parse::<f64>() {
            state.transform.het_freq_auto().set(false);
            state.transform.het_frequency().set(khz * 1000.0);
        }
    };

    view! {
        <div class="output-range-section-label">"Carrier (shift)"</div>
        <div class="output-range-shift-row">
            <input type="number"
                class="output-range-num"
                min="0" max="500" step="0.5"
                prop:value=move || format!("{:.1}", state.transform.het_frequency().get() / 1000.0)
                on:input=on_input
            />
            <span class="output-range-num-suffix">"kHz"</span>
            <button
                class=move || if state.transform.het_freq_auto().get() { "auto-toggle on" } else { "auto-toggle" }
                on:click=move |_| state.transform.het_freq_auto().update(|v| *v = !*v)
                title="Auto-derive carrier from current band"
            >"A"</button>
        </div>
        <div class="output-range-presets">
            {[10.0, 20.0, 40.0, 80.0].iter().map(|&khz| {
                let click = move |_: web_sys::MouseEvent| {
                    state.transform.het_freq_auto().set(false);
                    state.transform.het_frequency().set(khz * 1000.0);
                };
                let sel = Signal::derive(move || {
                    !state.transform.het_freq_auto().get()
                        && (state.transform.het_frequency().get() - khz * 1000.0).abs() < 100.0
                });
                view! {
                    <button
                        class=move || if sel.get() { "factor-preset sel" } else { "factor-preset" }
                        on:click=click
                    >{format!("\u{2212}{}", khz as i32)}{"k"}</button>
                }
            }).collect::<Vec<_>>()}
        </div>
    }
}

#[component]
fn BandSummary() -> impl IntoView {
    let state = expect_context::<AppState>();
    view! {
        <div class="output-range-summary">
            <div>
                <span class="dim">"in   "</span>
                {move || {
                    let (lo, hi) = band(&state);
                    if hi > lo { format!("{}\u{2013}{}", format_freq_khz(lo), format_freq_khz(hi)) }
                    else { "\u{2014}".into() }
                }}
            </div>
            // Intermediate "after divide, before output shift" range —
            // only shown for PS / PV when a shift is active. With the
            // effective-shift clamp the post-shift output never folds
            // below zero, but if the user's stored shift exceeds what
            // the current factor/BandFF allows, we annotate "(capped)"
            // so they can see why the value isn't fully in play.
            {move || {
                let mode = state.playback.mode().get();
                if !supports_shift(mode) { return view! { <span></span> }.into_any(); }
                let stored_shift = state.transform.ps_shift_hz().get();
                if stored_shift.abs() < 50.0 { return view! { <span></span> }.into_any(); }
                let (in_lo, in_hi) = band(&state);
                if in_hi <= in_lo { return view! { <span></span> }.into_any(); }
                let Some(f) = factor_value(&state, mode) else {
                    return view! { <span></span> }.into_any();
                };
                let post_lo = output_freq(in_lo, f);
                let post_hi = output_freq(in_hi, f);
                let (mlo, mhi) = if post_lo < post_hi { (post_lo, post_hi) } else { (post_hi, post_lo) };
                let effective = effective_ps_shift(stored_shift, in_lo, f);
                let suffix = if (effective - stored_shift).abs() > 1.0 {
                    format!(" (cap {})", format_freq_khz(effective))
                } else { String::new() };
                view! {
                    <div>
                        <span class="dim">"div  "</span>
                        {format!("{}\u{2013}{}{}", format_freq_khz(mlo), format_freq_khz(mhi), suffix)}
                    </div>
                }.into_any()
            }}
            <div>
                <span class="dim">"out  "</span>
                {move || {
                    let mode = state.playback.mode().get();
                    match current_output_range(&state, mode) {
                        Some((lo, hi)) => format!("{}\u{2013}{}", format_freq_khz(lo), format_freq_khz(hi)),
                        None => "\u{2014}".into(),
                    }
                }}
            </div>
        </div>
    }
}

/// Which input frequency the drag pins to the cursor's output Hz.
/// Lo / Hi anchor a single endpoint; Center moves the whole band.
#[derive(Copy, Clone)]
enum DragAnchor {
    Lo,
    Hi,
    Center,
}

#[component]
fn SnapPicker() -> impl IntoView {
    let state = expect_context::<AppState>();
    let opts: [(OutputSnap, &str, &str); 3] = [
        (OutputSnap::Free, "Free", "No snap \u{2014} continuous factor / carrier"),
        (OutputSnap::Standard, "Std", "Snap to standard factors (\u{00f7}2 \u{00f7}4 \u{00f7}8 \u{00f7}10 \u{00f7}16 \u{00f7}32) or 5 kHz carrier steps"),
        (OutputSnap::EqualChroma, "Chroma", "Snap to powers of 2 only \u{2014} preserves pitch intervals"),
    ];
    view! {
        <div class="output-range-snap-row">
            <span class="output-range-snap-label">"Snap"</span>
            <div class="output-range-snap-group">
                {opts.iter().map(|&(val, label, title)| {
                    let sel = Signal::derive(move || state.viewmode.output_snap().get() == val);
                    let click = move |_: web_sys::MouseEvent| state.viewmode.output_snap().set(val);
                    view! {
                        <button
                            class=move || if sel.get() { "snap-btn sel" } else { "snap-btn" }
                            on:click=click
                            title=title
                        >{label}</button>
                    }
                }).collect::<Vec<_>>()}
            </div>
        </div>
    }
}

#[component]
fn OutputGutter() -> impl IntoView {
    let state = expect_context::<AppState>();
    let gutter_ref = NodeRef::<leptos::html::Div>::new();

    let highlight_style = move || {
        let mode = state.playback.mode().get();
        let Some((lo, hi)) = current_output_range(&state, mode) else {
            return "display: none;".to_string();
        };
        let lo_pct = (lo.clamp(0.0, GUTTER_MAX_HZ) / GUTTER_MAX_HZ * 100.0).min(100.0);
        let hi_pct = (hi.clamp(0.0, GUTTER_MAX_HZ) / GUTTER_MAX_HZ * 100.0).min(100.0);
        format!(
            "bottom: {lo_pct:.2}%; height: {:.2}%;",
            (hi_pct - lo_pct).max(0.5)
        )
    };

    let band_label = move || mapping_shorthand(&state, state.playback.mode().get());

    let pin_input_to = move |target_hz: f64, anchor: f64| {
        let mode = state.playback.mode().get_untracked();
        let snap = state.viewmode.output_snap().get_untracked();
        let target = target_hz.clamp(1.0, GUTTER_MAX_HZ);
        match style_for(mode) {
            Style::Passthrough => {}
            Style::Divide => {
                if has_factor(mode) {
                    set_auto(&state, mode, false);
                    // Honour any active output shift so the back-solve
                    // matches the compound mapping the user sees:
                    //   target = |anchor/factor − shift|   (post-shift)
                    // Solve for factor in the unfolded branch:
                    //   factor = anchor / (target + shift)
                    let stored_shift = if supports_shift(mode) {
                        state.transform.ps_shift_hz().get_untracked()
                    } else {
                        0.0
                    };
                    let band_lo = state.filter.band_ff_freq_lo().get_untracked();
                    let current_f = factor_value_untracked(&state, mode).unwrap_or(0.0);
                    let shift = effective_ps_shift(stored_shift, band_lo, current_f);
                    let denom = (target + shift).max(1.0);
                    let raw = anchor / denom;
                    set_factor(&state, mode, snap_factor(raw, snap));
                }
            }
            Style::LinearShift => {
                state.transform.het_freq_auto().set(false);
                let raw = anchor - target;
                state.transform.het_frequency().set(snap_carrier(raw, snap));
            }
        }
    };

    let start_drag = move |ev: web_sys::PointerEvent, which: DragAnchor| {
        ev.prevent_default();
        ev.stop_propagation();
        let Some(el) = gutter_ref.get_untracked() else {
            return;
        };
        let rect = el.get_bounding_client_rect();
        let height = rect.height().max(1.0);
        let top = rect.top();

        let (in_lo, in_hi) = band(&state);
        if in_hi <= in_lo {
            return;
        }
        let anchor = match which {
            DragAnchor::Lo => in_lo,
            DragAnchor::Hi => in_hi,
            DragAnchor::Center => (in_lo + in_hi) / 2.0,
        };

        let hz_from_client_y = move |client_y: f64| -> f64 {
            let rel = ((client_y - top) / height).clamp(0.0, 1.0);
            (1.0 - rel) * GUTTER_MAX_HZ
        };

        pin_input_to(hz_from_client_y(ev.client_y() as f64), anchor);

        let win = web_sys::window().unwrap();
        let move_slot: Rc<RefCell<Option<Closure<dyn FnMut(web_sys::PointerEvent)>>>> =
            Rc::new(RefCell::new(None));
        let up_slot: Rc<RefCell<Option<Closure<dyn FnMut(web_sys::PointerEvent)>>>> =
            Rc::new(RefCell::new(None));

        let move_cb =
            Closure::<dyn FnMut(web_sys::PointerEvent)>::new(move |e: web_sys::PointerEvent| {
                pin_input_to(hz_from_client_y(e.client_y() as f64), anchor);
            });
        let win_clone = win.clone();
        let move_slot_clone = Rc::clone(&move_slot);
        let up_slot_clone = Rc::clone(&up_slot);
        let up_cb =
            Closure::<dyn FnMut(web_sys::PointerEvent)>::new(move |_: web_sys::PointerEvent| {
                if let Some(m) = move_slot_clone.borrow_mut().take() {
                    let _ = win_clone.remove_event_listener_with_callback(
                        "pointermove",
                        m.as_ref().unchecked_ref(),
                    );
                }
                if let Some(u) = up_slot_clone.borrow_mut().take() {
                    let _ = win_clone.remove_event_listener_with_callback(
                        "pointerup",
                        u.as_ref().unchecked_ref(),
                    );
                }
            });
        let _ =
            win.add_event_listener_with_callback("pointermove", move_cb.as_ref().unchecked_ref());
        let _ = win.add_event_listener_with_callback("pointerup", up_cb.as_ref().unchecked_ref());
        *move_slot.borrow_mut() = Some(move_cb);
        *up_slot.borrow_mut() = Some(up_cb);
    };

    let on_gutter_down = move |ev: web_sys::PointerEvent| start_drag(ev, DragAnchor::Center);
    let on_lo_down = move |ev: web_sys::PointerEvent| start_drag(ev, DragAnchor::Lo);
    let on_hi_down = move |ev: web_sys::PointerEvent| start_drag(ev, DragAnchor::Hi);
    let on_band_down = move |ev: web_sys::PointerEvent| start_drag(ev, DragAnchor::Center);

    view! {
        <div class="output-gutter-wrap">
            <div class="output-gutter"
                node_ref=gutter_ref
                on:pointerdown=on_gutter_down
            >
                <div class="output-gutter-ticks">
                    {GUTTER_TICKS.iter().map(|&hz| {
                        let pct = hz as f64 / GUTTER_MAX_HZ * 100.0;
                        let label = if hz >= 1000 {
                            format!("{}k", hz / 1000)
                        } else {
                            hz.to_string()
                        };
                        view! {
                            <div class="output-gutter-tick" style=format!("bottom: {pct:.2}%;")>
                                <span class="output-gutter-tick-label">{label}</span>
                            </div>
                        }
                    }).collect::<Vec<_>>()}
                </div>
                <div class="output-gutter-band"
                    style=highlight_style
                    on:pointerdown=on_band_down
                >
                    <span class="output-gutter-band-label">{band_label}</span>
                    <div class="output-gutter-handle output-gutter-handle-hi"
                        on:pointerdown=on_hi_down
                        title="Drag to anchor the high end of the input band to this output Hz"
                    ></div>
                    <div class="output-gutter-handle output-gutter-handle-lo"
                        on:pointerdown=on_lo_down
                        title="Drag to anchor the low end of the input band to this output Hz"
                    ></div>
                </div>
            </div>
        </div>
    }
}
