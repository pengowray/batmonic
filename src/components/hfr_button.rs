use leptos::prelude::*;
use crate::state::{AppState, BandpassMode, BandpassRange, FilterQuality, LayerPanel, PlaybackMode, SpectrogramHandle};
use crate::components::combo_button::ComboButton;

fn layer_opt_class(active: bool) -> &'static str {
    if active { "layer-panel-opt sel" } else { "layer-panel-opt" }
}

fn toggle_panel(state: &AppState, panel: LayerPanel) {
    state.layer_panel_open.update(|p| {
        *p = if *p == Some(panel) { None } else { Some(panel) };
    });
}

/// Compute the best auto factor for shifting FF center into audible range.
/// Prefers 8x, then other 2^n, then 10, then best integer, then exact ratio.
fn smart_auto_factor(ff_lo: f64, ff_hi: f64, max_factor: f64) -> f64 {
    let ff_center = (ff_lo + ff_hi) / 2.0;
    if ff_center <= 0.0 { return 10.0; }

    let target = 3000.0;
    let shifting_down = ff_center > target;

    if shifting_down {
        let ideal = ff_center / target;

        // Check preferred factors in order: 8, then other 2^n, then 10
        let preferred: &[f64] = &[8.0, 4.0, 16.0, 2.0, 32.0, 10.0];
        let comfortable = |f: f64| {
            let out = ff_center / f;
            (1000.0..=6000.0).contains(&out)
        };

        for &f in preferred {
            if f <= max_factor && comfortable(f) { return f; }
        }

        // Best integer
        let best_int = ideal.round().clamp(2.0, max_factor);
        if comfortable(best_int) { return best_int; }

        // Exact ratio
        ideal.clamp(2.0, max_factor)
    } else {
        // Sub-audible: need to pitch up (negative factor)
        let ideal = target / ff_center;
        let preferred: &[f64] = &[8.0, 4.0, 16.0, 2.0, 32.0, 10.0];
        let comfortable = |f: f64| {
            let out = ff_center * f;
            (1000.0..=6000.0).contains(&out)
        };

        for &f in preferred {
            if f <= max_factor && comfortable(f) { return -f; }
        }

        let best_int = ideal.round().clamp(2.0, max_factor);
        if comfortable(best_int) { return -best_int; }

        -(ideal.clamp(2.0, max_factor))
    }
}

/// Compute output frequency given input frequency and factor.
/// Positive factor = divide (shift down), negative = multiply (shift up).
fn output_freq(input: f64, factor: f64) -> f64 {
    if factor > 0.0 { input / factor }
    else if factor < 0.0 { input * factor.abs() }
    else { input }
}

/// Format a frequency for display (e.g. "45.0k", "1.8k", "800").
fn format_freq_khz(f: f64) -> String {
    if f >= 1000.0 {
        let khz = f / 1000.0;
        if (khz - khz.round()).abs() < 0.05 {
            format!("{}k", khz.round() as i32)
        } else {
            format!("{:.1}k", khz)
        }
    } else {
        format!("{:.0}", f)
    }
}

/// Format a factor value for display in the text input.
fn format_factor_value(f: f64) -> String {
    let abs = f.abs();
    let num = if (abs - abs.round()).abs() < 0.001 {
        format!("{}", abs.round() as i32)
    } else {
        format!("{:.1}", abs)
    };
    if f < -1.0 {
        format!("\u{00f7}{}", num) // ÷N
    } else {
        num
    }
}

/// Parse a user-entered factor string. Accepts "10", "10.5", "-2", "÷2", etc.
fn parse_factor_input(s: &str) -> Option<f64> {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix('\u{00f7}') {
        // ÷N → negative factor
        rest.trim().parse::<f64>().ok().map(|v| -v.abs())
    } else {
        s.parse::<f64>().ok()
    }
}

#[component]
pub fn HfrButton() -> impl IntoView {
    let state = expect_context::<AppState>();

    // Effect A (HFR toggle) has been removed — its logic now lives in
    // AppState::toggle_hfr() and the focus stack sync Effect in app.rs.

    // ── Ensure default user range when HFR is first enabled with no range ──
    // If the focus stack has no user range set yet when HFR turns on,
    // check for a species in the file's metadata and use its frequency range.
    // Falls back to 18kHz–Nyquist if no species match.
    Effect::new(move || {
        let stack = state.focus_stack.get();
        if stack.hfr_enabled() {
            let eff = stack.effective_range();
            if !eff.is_active() {
                // No range set yet — try species from metadata, then fall back
                let files = state.files.get();
                let idx = state.current_file_index.get();
                let file = idx.and_then(|i| files.get(i));
                let nyquist = file
                    .map(|f| f.spectrogram.max_freq)
                    .unwrap_or(96_000.0);

                let species_range = file.and_then(|f| {
                    use crate::bat_book::auto_resolve;
                    let favourites = state.bat_book_favourites.get_untracked();
                    let resolved = auto_resolve::resolve_auto(Some(f), &favourites);
                    let species_id = resolved.matched_species_id?;
                    let entry = auto_resolve::find_entry_in_manifest(
                        resolved.region, &species_id,
                    ).or_else(|| auto_resolve::find_entry_any_book(&species_id))?;
                    if entry.freq_lo_hz > 0.0 && entry.freq_hi_hz > entry.freq_lo_hz {
                        Some((entry.freq_lo_hz, entry.freq_hi_hz.min(nyquist)))
                    } else {
                        None
                    }
                });

                let (lo, hi) = species_range.unwrap_or((18_000.0, nyquist));
                state.set_ff_range(lo, hi);
            }
        }
    });

    // ── Effect B: FF range → auto parameter values (smart auto) ──
    Effect::new(move || {
        let ff_lo = state.ff_freq_lo.get();
        let ff_hi = state.ff_freq_hi.get();

        if ff_hi <= ff_lo {
            return;
        }

        let ff_center = (ff_lo + ff_hi) / 2.0;
        let ff_bandwidth = ff_hi - ff_lo;

        if state.het_freq_auto.get_untracked() {
            state.het_frequency.set(ff_center);
        }
        if state.het_cutoff_auto.get_untracked() {
            state.het_cutoff.set((ff_bandwidth / 2.0).min(15_000.0));
        }

        if state.te_factor_auto.get_untracked() {
            let te = smart_auto_factor(ff_lo, ff_hi, 40.0);
            state.te_factor.set(te);
        }
        if state.ps_factor_auto.get_untracked() {
            let ps = smart_auto_factor(ff_lo, ff_hi, 20.0);
            state.ps_factor.set(ps);
        }
        if state.pv_factor_auto.get_untracked() {
            let pv = smart_auto_factor(ff_lo, ff_hi, 20.0);
            state.pv_factor.set(pv);
        }
    });

    // ── Effect C: ZC mode display settings save/restore ──
    {
        let prev_mode = RwSignal::new(state.playback_mode.get_untracked());
        Effect::new(move || {
            let mode = state.playback_mode.get();
            let old = prev_mode.get_untracked();
            if mode == old { return; }
            prev_mode.set(mode);

            let was_zc = old == PlaybackMode::ZeroCrossing;
            let is_zc = mode == PlaybackMode::ZeroCrossing;

            if was_zc && !is_zc {
                state.zc_saved_display_auto_gain.set(state.display_auto_gain.get_untracked());
                state.zc_saved_display_eq.set(state.display_eq.get_untracked());
                state.zc_saved_display_noise_filter.set(state.display_noise_filter.get_untracked());

                state.display_auto_gain.set(state.normal_saved_display_auto_gain.get_untracked());
                state.display_eq.set(state.normal_saved_display_eq.get_untracked());
                state.display_noise_filter.set(state.normal_saved_display_noise_filter.get_untracked());
            } else if !was_zc && is_zc {
                state.normal_saved_display_auto_gain.set(state.display_auto_gain.get_untracked());
                state.normal_saved_display_eq.set(state.display_eq.get_untracked());
                state.normal_saved_display_noise_filter.set(state.display_noise_filter.get_untracked());

                state.display_auto_gain.set(state.zc_saved_display_auto_gain.get_untracked());
                state.display_eq.set(state.zc_saved_display_eq.get_untracked());
                state.display_noise_filter.set(state.zc_saved_display_noise_filter.get_untracked());
            }
            // Note: display_filter_enabled and per-stage modes persist across ZC transitions
            // (they are not reset — the resolution Effect re-applies them automatically).
        });
    }

    // ── Effect D: bandpass_mode + bandpass_range + playback_mode → filter settings ──
    Effect::new(move || {
        let bp_mode = state.bandpass_mode.get();
        let bp_range = state.bandpass_range.get();
        let ff_lo = state.ff_freq_lo.get();
        let ff_hi = state.ff_freq_hi.get();
        let playback_mode = state.playback_mode.get();

        match bp_mode {
            BandpassMode::Off => {
                state.filter_enabled.set(false);
            }
            BandpassMode::Auto => {
                let has_ff = ff_hi > ff_lo;
                match playback_mode {
                    PlaybackMode::Heterodyne => {
                        state.filter_enabled.set(false);
                    }
                    PlaybackMode::ZeroCrossing => {
                        state.filter_enabled.set(has_ff);
                        if has_ff {
                            state.filter_freq_low.set(ff_lo);
                            state.filter_freq_high.set(ff_hi);
                            state.filter_quality.set(FilterQuality::Spectral);
                            state.filter_db_below.set(-60.0);
                            state.filter_db_selected.set(0.0);
                            state.filter_db_above.set(-60.0);
                        }
                    }
                    _ => {
                        state.filter_enabled.set(has_ff);
                        if has_ff {
                            state.filter_freq_low.set(ff_lo);
                            state.filter_freq_high.set(ff_hi);
                            state.filter_quality.set(FilterQuality::Spectral);
                            state.filter_db_below.set(-40.0);
                            state.filter_db_selected.set(0.0);
                            state.filter_db_above.set(-40.0);
                        }
                    }
                }
            }
            BandpassMode::On => {
                state.filter_enabled.set(true);
                if bp_range == BandpassRange::FollowFocus && ff_hi > ff_lo {
                    state.filter_freq_low.set(ff_lo);
                    state.filter_freq_high.set(ff_hi);
                }
            }
        }
    });

    // ── ComboButton setup ──
    let is_open = Signal::derive(move || state.layer_panel_open.get() == Some(LayerPanel::HfrMode));

    let no_file = move || state.current_file_index.get().is_none() && state.active_timeline.get().is_none();
    let left_class = Signal::derive(move || {
        if no_file() {
            "layer-btn combo-btn-left no-annotation disabled"
        } else if state.hfr_enabled.get() {
            "layer-btn combo-btn-left no-annotation active"
        } else {
            "layer-btn combo-btn-left no-annotation"
        }
    });
    let right_class = Signal::derive(move || {
        if no_file() { return "layer-btn combo-btn-right disabled"; }
        let dim = if !state.hfr_enabled.get() { " dim" } else { "" };
        if is_open.get() {
            if dim.is_empty() { "layer-btn combo-btn-right open" } else { "layer-btn combo-btn-right dim open" }
        } else if dim.is_empty() { "layer-btn combo-btn-right" } else { "layer-btn combo-btn-right dim" }
    });

    let left_value = Signal::derive(String::new);
    let right_value = Signal::derive(move || {
        if !state.hfr_enabled.get() {
            "OFF".to_string()
        } else {
            match state.playback_mode.get() {
                PlaybackMode::Heterodyne   => "HET".to_string(),
                PlaybackMode::TimeExpansion => "TE".to_string(),
                PlaybackMode::PitchShift   => "PS".to_string(),
                PlaybackMode::PhaseVocoder => "PV".to_string(),
                PlaybackMode::ZeroCrossing => "ZC".to_string(),
                PlaybackMode::Normal       => "1:1".to_string(),
            }
        }
    });

    let left_click = Callback::new(move |_: web_sys::MouseEvent| {
        if state.current_file_index.get_untracked().is_none() && state.active_timeline.get_untracked().is_none() { return; }
        state.toggle_hfr();
    });
    let toggle_menu = Callback::new(move |()| {
        toggle_panel(&state, LayerPanel::HfrMode);
    });

    // ── Dropdown closures (from hfr_mode_button) ──
    let set_mode = |state: AppState, mode: PlaybackMode| {
        move |_: web_sys::MouseEvent| {
            state.focus_stack.update(|s| s.set_saved_playback_mode(Some(mode)));
            if !state.focus_stack.get_untracked().hfr_enabled() {
                state.toggle_hfr(); // enables HFR with saved mode
            }
            state.playback_mode.set(mode);
        }
    };

    // ── Slider change handlers ──
    let on_te_change = move |ev: web_sys::Event| {
        use wasm_bindgen::JsCast;
        let input: web_sys::HtmlInputElement = ev.target().unwrap().unchecked_into();
        if let Ok(val) = input.value().parse::<f64>() {
            state.te_factor_auto.set(false);
            state.playback_mode.set(PlaybackMode::TimeExpansion);
            state.te_factor.set(val);
        }
    };

    let on_ps_change = move |ev: web_sys::Event| {
        use wasm_bindgen::JsCast;
        let input: web_sys::HtmlInputElement = ev.target().unwrap().unchecked_into();
        if let Ok(val) = input.value().parse::<f64>() {
            state.ps_factor_auto.set(false);
            state.playback_mode.set(PlaybackMode::PitchShift);
            state.ps_factor.set(val);
        }
    };

    let on_pv_change = move |ev: web_sys::Event| {
        use wasm_bindgen::JsCast;
        let input: web_sys::HtmlInputElement = ev.target().unwrap().unchecked_into();
        if let Ok(val) = input.value().parse::<f64>() {
            state.pv_factor_auto.set(false);
            state.playback_mode.set(PlaybackMode::PhaseVocoder);
            state.pv_factor.set(val);
        }
    };

    let on_zc_change = move |ev: web_sys::Event| {
        use wasm_bindgen::JsCast;
        let input: web_sys::HtmlInputElement = ev.target().unwrap().unchecked_into();
        if let Ok(val) = input.value().parse::<f64>() {
            state.playback_mode.set(PlaybackMode::ZeroCrossing);
            state.zc_factor.set(val);
        }
    };

    // ── Text input change handlers ──
    let on_te_text = move |ev: web_sys::Event| {
        use wasm_bindgen::JsCast;
        let input: web_sys::HtmlInputElement = ev.target().unwrap().unchecked_into();
        if let Some(val) = parse_factor_input(&input.value()) {
            state.te_factor_auto.set(false);
            state.te_factor.set(val);
        }
    };

    let on_ps_text = move |ev: web_sys::Event| {
        use wasm_bindgen::JsCast;
        let input: web_sys::HtmlInputElement = ev.target().unwrap().unchecked_into();
        if let Some(val) = parse_factor_input(&input.value()) {
            state.ps_factor_auto.set(false);
            state.ps_factor.set(val);
        }
    };

    let on_pv_text = move |ev: web_sys::Event| {
        use wasm_bindgen::JsCast;
        let input: web_sys::HtmlInputElement = ev.target().unwrap().unchecked_into();
        if let Some(val) = parse_factor_input(&input.value()) {
            state.pv_factor_auto.set(false);
            state.pv_factor.set(val);
        }
    };

    // ── Preset click helpers ──
    let make_preset_click = move |factor_signal: RwSignal<f64>, auto_signal: RwSignal<bool>, mode: PlaybackMode, value: f64| {
        move |_: web_sys::MouseEvent| {
            auto_signal.set(false);
            factor_signal.set(value);
            state.playback_mode.set(mode);
        }
    };

    // Preset values: ÷2, 2, 4, 8, 10, 16
    let preset_values: [(f64, &str); 6] = [
        (-2.0, "\u{00f7}2x"),
        (2.0, "2x"),
        (4.0, "4x"),
        (8.0, "8x"),
        (10.0, "10x"),
        (16.0, "16x"),
    ];

    // ── Output freq hover helpers ──
    let set_output_highlight = move |factor_signal: RwSignal<f64>| {
        move |_: web_sys::MouseEvent| {
            let f = factor_signal.get_untracked();
            let ff_lo = state.ff_freq_lo.get_untracked();
            let ff_hi = state.ff_freq_hi.get_untracked();
            if ff_hi > ff_lo {
                let out_lo = output_freq(ff_lo, f);
                let out_hi = output_freq(ff_hi, f);
                let (lo, hi) = if out_lo < out_hi { (out_lo, out_hi) } else { (out_hi, out_lo) };
                state.output_freq_highlight.set(Some((lo, hi)));
            }
        }
    };
    let clear_output_highlight = move |_: web_sys::MouseEvent| {
        state.output_freq_highlight.set(None);
    };

    view! {
            <ComboButton
                left_label="HFR"
                left_value=left_value
                left_click=left_click
                left_class=left_class
                right_value=right_value
                right_class=right_class
                is_open=is_open
                toggle_menu=toggle_menu
                left_title="Toggle High Frequency Range mode"
                right_title="HFR playback mode"
                menu_direction="above"
                panel_style="min-width: 210px;"
            >
                // ── OFF option ──
                <button class=move || layer_opt_class(!state.hfr_enabled.get())
                    on:click=move |_: web_sys::MouseEvent| {
                        if state.focus_stack.get_untracked().hfr_enabled() {
                            state.toggle_hfr();
                        }
                        state.layer_panel_open.set(None);
                    }
                >"OFF"</button>
                <hr />
                // ── Mode selection ──
                <button class=move || layer_opt_class(state.hfr_enabled.get() && state.playback_mode.get() == PlaybackMode::Normal)
                    on:click=set_mode(state, PlaybackMode::Normal)
                >"1:1 \u{2014} Normal"</button>
                <button class=move || layer_opt_class(state.hfr_enabled.get() && state.playback_mode.get() == PlaybackMode::Heterodyne)
                    on:click=set_mode(state, PlaybackMode::Heterodyne)
                >"HET \u{2014} Heterodyne"</button>
                <button class=move || layer_opt_class(state.hfr_enabled.get() && state.playback_mode.get() == PlaybackMode::TimeExpansion)
                    on:click=set_mode(state, PlaybackMode::TimeExpansion)
                >"TE \u{2014} Time Expansion"</button>
                <button class=move || layer_opt_class(state.hfr_enabled.get() && state.playback_mode.get() == PlaybackMode::PitchShift)
                    on:click=set_mode(state, PlaybackMode::PitchShift)
                >"PS \u{2014} Pitch Shift"</button>
                <button class=move || layer_opt_class(state.hfr_enabled.get() && state.playback_mode.get() == PlaybackMode::PhaseVocoder)
                    on:click=set_mode(state, PlaybackMode::PhaseVocoder)
                >"PV \u{2014} Phase Vocoder"</button>
                <button class=move || layer_opt_class(state.hfr_enabled.get() && state.playback_mode.get() == PlaybackMode::ZeroCrossing)
                    on:click=set_mode(state, PlaybackMode::ZeroCrossing)
                >"ZC \u{2014} Zero Crossing"</button>

                // ── Inaudible notice ──
                {move || (state.playback_mode.get() == PlaybackMode::Normal && state.ff_freq_lo.get() >= 20_000.0).then(|| {
                    view! {
                        <div style="padding: 4px 8px; font-size: 10px; color: #e0a030; line-height: 1.3;">
                            "Focus is above human hearing. 1:1 mode won\u{2019}t make it audible"
                        </div>
                    }
                })}

                // ── Adjustment ──
                <Show when=move || state.playback_mode.get() != PlaybackMode::Normal>
                    <hr />
                    <div class="layer-panel-title">"Adjustment"</div>
                    {move || {
                        let mode = state.playback_mode.get();
                        match mode {
                            PlaybackMode::Heterodyne => view! {
                                <div class="layer-panel-slider-row het-text-row"
                                    on:mouseenter=move |_| {
                                        state.het_interacting.set(true);
                                        state.spec_hover_handle.set(Some(SpectrogramHandle::HetCenter));
                                    }
                                    on:mouseleave=move |_| {
                                        state.het_interacting.set(false);
                                        state.spec_hover_handle.set(None);
                                    }
                                >
                                    <label>"Freq"</label>
                                    <span class="het-value">{move || format!("{:.1} kHz", state.het_frequency.get() / 1000.0)}</span>
                                    <button class=move || if state.het_freq_auto.get() { "auto-toggle on" } else { "auto-toggle" }
                                        on:click=move |_| state.het_freq_auto.update(|v| *v = !*v)
                                        title="Toggle auto HET frequency"
                                    >"A"</button>
                                </div>
                                <div class="layer-panel-slider-row het-text-row"
                                    on:mouseenter=move |_| {
                                        state.het_interacting.set(true);
                                        state.spec_hover_handle.set(Some(SpectrogramHandle::HetBandUpper));
                                    }
                                    on:mouseleave=move |_| {
                                        state.het_interacting.set(false);
                                        state.spec_hover_handle.set(None);
                                    }
                                >
                                    <label>"LP cutoff"</label>
                                    <span class="het-value">{move || format!("{:.1} kHz", state.het_cutoff.get() / 1000.0)}</span>
                                    <button class=move || if state.het_cutoff_auto.get() { "auto-toggle on" } else { "auto-toggle" }
                                        on:click=move |_| state.het_cutoff_auto.update(|v| *v = !*v)
                                        title="Toggle auto LP cutoff"
                                    >"A"</button>
                                </div>
                            }.into_any(),

                            PlaybackMode::TimeExpansion => view! {
                                <div class="layer-panel-slider-row">
                                    <label>"Factor"</label>
                                    <input type="range" min="-40" max="40" step="1"
                                        prop:value=move || (state.te_factor.get() as i32).to_string()
                                        on:input=on_te_change
                                    />
                                    <input type="text" class="factor-input"
                                        prop:value=move || format_factor_value(state.te_factor.get())
                                        on:change=on_te_text
                                        on:focus=move |ev: web_sys::FocusEvent| {
                                            use wasm_bindgen::JsCast;
                                            if let Some(input) = ev.target().and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok()) {
                                                input.select();
                                            }
                                        }
                                        title="Enter a custom factor (e.g. 10, 7.5, \u{00f7}2)"
                                    />
                                    <button class=move || if state.te_factor_auto.get() { "auto-toggle on" } else { "auto-toggle" }
                                        on:click=move |_| state.te_factor_auto.update(|v| *v = !*v)
                                        title="Auto: picks best factor for audible output"
                                    >"A"</button>
                                </div>
                                <div class="factor-presets">
                                    {preset_values.iter().map(|&(val, label)| {
                                        let on_click = make_preset_click(state.te_factor, state.te_factor_auto, PlaybackMode::TimeExpansion, val);
                                        let is_sel = move || (state.te_factor.get() - val).abs() < 0.01 && !state.te_factor_auto.get();
                                        view! {
                                            <button class=move || if is_sel() { "factor-preset sel" } else { "factor-preset" }
                                                on:click=on_click
                                            >{label}</button>
                                        }
                                    }).collect::<Vec<_>>()}
                                </div>
                                // Frequency summary
                                <Show when=move || { let (h, l) = (state.ff_freq_hi.get(), state.ff_freq_lo.get()); h > l }>
                                    <div class="freq-summary">
                                        <div>"Input: "{move || format!("{}\u{2013}{}", format_freq_khz(state.ff_freq_lo.get()), format_freq_khz(state.ff_freq_hi.get()))}</div>
                                        <div class="freq-summary-output"
                                            on:mouseenter=set_output_highlight(state.te_factor)
                                            on:mouseleave=clear_output_highlight
                                        >"Output: "{move || {
                                            let f = state.te_factor.get();
                                            let lo = output_freq(state.ff_freq_lo.get(), f);
                                            let hi = output_freq(state.ff_freq_hi.get(), f);
                                            let (lo, hi) = if lo < hi { (lo, hi) } else { (hi, lo) };
                                            format!("{}\u{2013}{}", format_freq_khz(lo), format_freq_khz(hi))
                                        }}</div>
                                    </div>
                                </Show>
                            }.into_any(),

                            PlaybackMode::PitchShift => view! {
                                <div class="layer-panel-slider-row">
                                    <label>"Factor"</label>
                                    <input type="range" min="-20" max="20" step="1"
                                        prop:value=move || (state.ps_factor.get() as i32).to_string()
                                        on:input=on_ps_change
                                    />
                                    <input type="text" class="factor-input"
                                        prop:value=move || format_factor_value(state.ps_factor.get())
                                        on:change=on_ps_text
                                        on:focus=move |ev: web_sys::FocusEvent| {
                                            use wasm_bindgen::JsCast;
                                            if let Some(input) = ev.target().and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok()) {
                                                input.select();
                                            }
                                        }
                                        title="Enter a custom factor (e.g. 10, 7.5, \u{00f7}2)"
                                    />
                                    <button class=move || if state.ps_factor_auto.get() { "auto-toggle on" } else { "auto-toggle" }
                                        on:click=move |_| state.ps_factor_auto.update(|v| *v = !*v)
                                        title="Auto: picks best factor for audible output"
                                    >"A"</button>
                                </div>
                                <div class="factor-presets">
                                    {preset_values.iter().map(|&(val, label)| {
                                        let on_click = make_preset_click(state.ps_factor, state.ps_factor_auto, PlaybackMode::PitchShift, val);
                                        let is_sel = move || (state.ps_factor.get() - val).abs() < 0.01 && !state.ps_factor_auto.get();
                                        view! {
                                            <button class=move || if is_sel() { "factor-preset sel" } else { "factor-preset" }
                                                on:click=on_click
                                            >{label}</button>
                                        }
                                    }).collect::<Vec<_>>()}
                                </div>
                                <Show when=move || { let (h, l) = (state.ff_freq_hi.get(), state.ff_freq_lo.get()); h > l }>
                                    <div class="freq-summary">
                                        <div>"Input: "{move || format!("{}\u{2013}{}", format_freq_khz(state.ff_freq_lo.get()), format_freq_khz(state.ff_freq_hi.get()))}</div>
                                        <div class="freq-summary-output"
                                            on:mouseenter=set_output_highlight(state.ps_factor)
                                            on:mouseleave=clear_output_highlight
                                        >"Output: "{move || {
                                            let f = state.ps_factor.get();
                                            let lo = output_freq(state.ff_freq_lo.get(), f);
                                            let hi = output_freq(state.ff_freq_hi.get(), f);
                                            let (lo, hi) = if lo < hi { (lo, hi) } else { (hi, lo) };
                                            format!("{}\u{2013}{}", format_freq_khz(lo), format_freq_khz(hi))
                                        }}</div>
                                    </div>
                                </Show>
                                <div class="layer-panel-slider-row">
                                    <label>"Quality"</label>
                                    <button class=move || if !state.pv_hq.get() { "auto-toggle on" } else { "auto-toggle" }
                                        on:click=move |_| state.pv_hq.set(false)
                                        title="Standard mode \u{2014} uses filter warmup to reduce boundary clicks"
                                    >"Std"</button>
                                    <button class=move || if state.pv_hq.get() { "auto-toggle on" } else { "auto-toggle" }
                                        on:click=move |_| state.pv_hq.set(true)
                                        title="HQ mode \u{2014} overlapping crossfade eliminates boundary clicks"
                                    >"HQ"</button>
                                </div>
                            }.into_any(),

                            PlaybackMode::PhaseVocoder => view! {
                                <div class="layer-panel-slider-row">
                                    <label>"Factor"</label>
                                    <input type="range" min="-20" max="20" step="1"
                                        prop:value=move || (state.pv_factor.get() as i32).to_string()
                                        on:input=on_pv_change
                                    />
                                    <input type="text" class="factor-input"
                                        prop:value=move || format_factor_value(state.pv_factor.get())
                                        on:change=on_pv_text
                                        on:focus=move |ev: web_sys::FocusEvent| {
                                            use wasm_bindgen::JsCast;
                                            if let Some(input) = ev.target().and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok()) {
                                                input.select();
                                            }
                                        }
                                        title="Enter a custom factor (e.g. 10, 7.5, \u{00f7}2)"
                                    />
                                    <button class=move || if state.pv_factor_auto.get() { "auto-toggle on" } else { "auto-toggle" }
                                        on:click=move |_| state.pv_factor_auto.update(|v| *v = !*v)
                                        title="Auto: picks best factor for audible output"
                                    >"A"</button>
                                </div>
                                <div class="factor-presets">
                                    {preset_values.iter().map(|&(val, label)| {
                                        let on_click = make_preset_click(state.pv_factor, state.pv_factor_auto, PlaybackMode::PhaseVocoder, val);
                                        let is_sel = move || (state.pv_factor.get() - val).abs() < 0.01 && !state.pv_factor_auto.get();
                                        view! {
                                            <button class=move || if is_sel() { "factor-preset sel" } else { "factor-preset" }
                                                on:click=on_click
                                            >{label}</button>
                                        }
                                    }).collect::<Vec<_>>()}
                                </div>
                                <Show when=move || { let (h, l) = (state.ff_freq_hi.get(), state.ff_freq_lo.get()); h > l }>
                                    <div class="freq-summary">
                                        <div>"Input: "{move || format!("{}\u{2013}{}", format_freq_khz(state.ff_freq_lo.get()), format_freq_khz(state.ff_freq_hi.get()))}</div>
                                        <div class="freq-summary-output"
                                            on:mouseenter=set_output_highlight(state.pv_factor)
                                            on:mouseleave=clear_output_highlight
                                        >"Output: "{move || {
                                            let f = state.pv_factor.get();
                                            let lo = output_freq(state.ff_freq_lo.get(), f);
                                            let hi = output_freq(state.ff_freq_hi.get(), f);
                                            let (lo, hi) = if lo < hi { (lo, hi) } else { (hi, lo) };
                                            format!("{}\u{2013}{}", format_freq_khz(lo), format_freq_khz(hi))
                                        }}</div>
                                    </div>
                                </Show>
                                <div class="layer-panel-slider-row">
                                    <label>"Quality"</label>
                                    <button class=move || if !state.pv_hq.get() { "auto-toggle on" } else { "auto-toggle" }
                                        on:click=move |_| state.pv_hq.set(false)
                                        title="Standard mode \u{2014} uses filter warmup to reduce boundary clicks"
                                    >"Std"</button>
                                    <button class=move || if state.pv_hq.get() { "auto-toggle on" } else { "auto-toggle" }
                                        on:click=move |_| state.pv_hq.set(true)
                                        title="HQ mode \u{2014} overlapping crossfade eliminates boundary clicks"
                                    >"HQ"</button>
                                </div>
                            }.into_any(),

                            PlaybackMode::ZeroCrossing => view! {
                                <div class="layer-panel-slider-row">
                                    <label>"Division"</label>
                                    <input type="range" min="2" max="32" step="1"
                                        prop:value=move || (state.zc_factor.get() as u32).to_string()
                                        on:input=on_zc_change
                                    />
                                    <span>{move || format!("\u{00f7}{}", state.zc_factor.get() as u32)}</span>
                                </div>
                            }.into_any(),
                            PlaybackMode::Normal => view! { <span></span> }.into_any(),
                        }
                    }}
                </Show>

                // ── Bandpass ──
                <hr />
                <div class="layer-panel-title">"Bandpass"</div>
                <div style="display: flex; gap: 2px; padding: 0 6px 4px;">
                    <button class=move || layer_opt_class(state.bandpass_mode.get() == BandpassMode::Auto)
                        on:click=move |_| state.bandpass_mode.set(BandpassMode::Auto)
                    >"AUTO"</button>
                    <button class=move || layer_opt_class(state.bandpass_mode.get() == BandpassMode::Off)
                        on:click=move |_| state.bandpass_mode.set(BandpassMode::Off)
                    >"OFF"</button>
                    <button class=move || layer_opt_class(state.bandpass_mode.get() == BandpassMode::On)
                        on:click=move |_| state.bandpass_mode.set(BandpassMode::On)
                    >"ON"</button>
                </div>
                <Show when=move || {
                    let bp = state.bandpass_mode.get();
                    bp == BandpassMode::On
                        || (bp == BandpassMode::Auto && state.ff_freq_hi.get() > state.ff_freq_lo.get())
                }>
                    {
                        let make_db_handler = |signal: RwSignal<f64>| {
                            move |ev: web_sys::Event| {
                                use wasm_bindgen::JsCast;
                                let input: web_sys::HtmlInputElement = ev.target().unwrap().unchecked_into();
                                if let Ok(val) = input.value().parse::<f64>() {
                                    if state.bandpass_mode.get_untracked() == BandpassMode::Auto {
                                        state.bandpass_mode.set(BandpassMode::On);
                                    }
                                    signal.set(val);
                                }
                            }
                        };
                        let on_above_change = make_db_handler(state.filter_db_above);
                        let on_selected_change = make_db_handler(state.filter_db_selected);
                        let on_harmonics_change = make_db_handler(state.filter_db_harmonics);
                        let on_below_change = make_db_handler(state.filter_db_below);

                        let on_quality_click = move |q: FilterQuality| {
                            move |_: web_sys::MouseEvent| {
                                if state.bandpass_mode.get_untracked() == BandpassMode::Auto {
                                    state.bandpass_mode.set(BandpassMode::On);
                                }
                                state.filter_quality.set(q);
                            }
                        };
                        let on_band_click = move |b: u8| {
                            move |_: web_sys::MouseEvent| {
                                if state.bandpass_mode.get_untracked() == BandpassMode::Auto {
                                    state.bandpass_mode.set(BandpassMode::On);
                                }
                                state.filter_band_mode.set(b);
                            }
                        };

                        view! {
                            <div style="display: flex; gap: 2px; padding: 0 6px 2px;">
                                <button class=move || layer_opt_class(state.bandpass_range.get() == BandpassRange::FollowFocus)
                                    on:click=move |_| state.bandpass_range.set(BandpassRange::FollowFocus)
                                >"Focus"</button>
                                <button class=move || layer_opt_class(state.bandpass_range.get() == BandpassRange::Custom)
                                    on:click=move |_| state.bandpass_range.set(BandpassRange::Custom)
                                >"Custom"</button>
                            </div>
                            <div style="padding: 0 8px 2px; font-size: 10px; opacity: 0.7;">
                                {move || format!("{:.1}\u{2013}{:.1} kHz",
                                    state.filter_freq_low.get() / 1000.0,
                                    state.filter_freq_high.get() / 1000.0
                                )}
                            </div>
                            <div style="display: flex; gap: 2px; padding: 0 6px 2px;">
                                <button class=move || layer_opt_class(state.filter_quality.get() == FilterQuality::Fast)
                                    on:click=on_quality_click(FilterQuality::Fast)
                                    title="IIR band-split \u{2014} low latency, softer edges"
                                >"Fast"</button>
                                <button class=move || layer_opt_class(state.filter_quality.get() == FilterQuality::Spectral)
                                    on:click=on_quality_click(FilterQuality::Spectral)
                                    title="FFT spectral EQ \u{2014} sharp edges, higher latency"
                                >"HQ"</button>
                                <span style="width: 8px;"></span>
                                <button class=move || layer_opt_class(state.filter_band_mode.get() == 3)
                                    on:click=on_band_click(3)
                                >"3"</button>
                                <button class=move || layer_opt_class(state.filter_band_mode.get() == 4)
                                    on:click=on_band_click(4)
                                >"4"</button>
                            </div>
                            <div class="layer-panel-slider-row"
                                on:mouseenter=move |_| state.filter_hovering_band.set(Some(3))
                                on:mouseleave=move |_| state.filter_hovering_band.set(None)
                            >
                                <label>"Above"</label>
                                <input type="range" min="-60" max="6" step="1"
                                    prop:value=move || state.filter_db_above.get().to_string()
                                    on:input=on_above_change
                                />
                                <span>{move || format!("{:.0}", state.filter_db_above.get())}</span>
                            </div>
                            <Show when=move || { state.filter_band_mode.get() >= 4 }>
                                <div class="layer-panel-slider-row"
                                    on:mouseenter=move |_| state.filter_hovering_band.set(Some(2))
                                    on:mouseleave=move |_| state.filter_hovering_band.set(None)
                                >
                                    <label>"Harm"</label>
                                    <input type="range" min="-60" max="6" step="1"
                                        prop:value=move || state.filter_db_harmonics.get().to_string()
                                        on:input=on_harmonics_change
                                    />
                                    <span>{move || format!("{:.0}", state.filter_db_harmonics.get())}</span>
                                </div>
                            </Show>
                            <div class="layer-panel-slider-row"
                                on:mouseenter=move |_| state.filter_hovering_band.set(Some(1))
                                on:mouseleave=move |_| state.filter_hovering_band.set(None)
                            >
                                <label>"Focus"</label>
                                <input type="range" min="-60" max="6" step="1"
                                    prop:value=move || state.filter_db_selected.get().to_string()
                                    on:input=on_selected_change
                                />
                                <span>{move || format!("{:.0}", state.filter_db_selected.get())}</span>
                            </div>
                            <div class="layer-panel-slider-row"
                                on:mouseenter=move |_| state.filter_hovering_band.set(Some(0))
                                on:mouseleave=move |_| state.filter_hovering_band.set(None)
                            >
                                <label>"Below"</label>
                                <input type="range" min="-60" max="6" step="1"
                                    prop:value=move || state.filter_db_below.get().to_string()
                                    on:input=on_below_change
                                />
                                <span>{move || format!("{:.0}", state.filter_db_below.get())}</span>
                            </div>
                        }
                    }
                </Show>
            </ComboButton>
    }
}
