// Mode radio group — 5 joined buttons selecting playback algorithm.
//
//   [1:1] [HET] [TE] [PS] [ZC]
//
// • First click on a non-selected button: select that mode (and turn HFR
//   on if it was off — except for 1:1, which is the "no-op" mode).
// • Second click on the already-selected button: open that mode's
//   settings popup. The settings panel only contains controls relevant
//   to the chosen mode (no more giant mode picker inside).
// • The selected button shows a small corner indicator (◢) when it has
//   a settings popup available (HET/TE/PS/ZC). 1:1 has no settings.
// • PS is a bucket: both PitchShift and PhaseVocoder live under it. The
//   PS settings popup has an "Algorithm" toggle to switch between them.
//
// The cross-cutting Effects (focus-stack default range, smart-auto
// factor, ZC display save/restore, bandpass driving filter settings) all
// live in this component because they're playback-mode-coupled. They
// were previously inside `ModeButton`.

use crate::components::popup::{Align, PopupPanel, Side};
use crate::dsp::pitch_shift::PitchFactor;
use crate::state::store_fields::*;
use crate::state::{
    AppState, BandpassMode, BandpassRange, FilterQuality, LayerPanel, PlaybackMode,
    SpectrogramHandle,
};
use leptos::prelude::*;

fn toggle_panel(state: &AppState, panel: LayerPanel) {
    state.panels.layer_panel_open().update(|p| {
        *p = if *p == Some(panel) { None } else { Some(panel) };
    });
}

/// Compute the best auto factor for shifting BandFF center into audible range.
/// Prefers 8x, then other 2^n, then 10, then best integer, then exact ratio.
fn smart_auto_factor(band_ff_lo: f64, band_ff_hi: f64, max_factor: f64) -> f64 {
    let band_ff_center = (band_ff_lo + band_ff_hi) / 2.0;
    if band_ff_center <= 0.0 {
        return 10.0;
    }

    let target = 3000.0;
    let shifting_down = band_ff_center > target;

    if shifting_down {
        let ideal = band_ff_center / target;

        let preferred: &[f64] = &[8.0, 4.0, 16.0, 2.0, 32.0, 10.0];
        let comfortable = |f: f64| {
            let out = band_ff_center / f;
            (1000.0..=6000.0).contains(&out)
        };

        for &f in preferred {
            if f <= max_factor && comfortable(f) {
                return f;
            }
        }

        let best_int = ideal.round().clamp(2.0, max_factor);
        if comfortable(best_int) {
            return best_int;
        }
        ideal.clamp(2.0, max_factor)
    } else {
        let ideal = target / band_ff_center;
        let preferred: &[f64] = &[8.0, 4.0, 16.0, 2.0, 32.0, 10.0];
        let comfortable = |f: f64| {
            let out = band_ff_center * f;
            (1000.0..=6000.0).contains(&out)
        };

        for &f in preferred {
            if f <= max_factor && comfortable(f) {
                return -f;
            }
        }

        let best_int = ideal.round().clamp(2.0, max_factor);
        if comfortable(best_int) {
            return -best_int;
        }

        -(ideal.clamp(2.0, max_factor))
    }
}

pub(crate) fn output_freq(input: f64, factor: f64) -> f64 {
    if factor > 0.0 {
        input / factor
    } else if factor < 0.0 {
        input * factor.abs()
    } else {
        input
    }
}

pub(crate) fn format_freq_khz(f: f64) -> String {
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

pub(crate) fn format_factor_value(f: f64) -> String {
    let abs = f.abs();
    let num = if (abs - abs.round()).abs() < 0.001 {
        format!("{}", abs.round() as i32)
    } else {
        format!("{:.1}", abs)
    };
    if f < -1.0 {
        format!("\u{00f7}{}", num)
    } else {
        num
    }
}

fn parse_factor_input(s: &str) -> Option<f64> {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix('\u{00f7}') {
        rest.trim().parse::<f64>().ok().map(|v| -v.abs())
    } else {
        s.parse::<f64>().ok()
    }
}

/// Logical "bucket" for the mode radio group. PS bucket spans the
/// PitchShift and PhaseVocoder playback modes; everything else maps 1:1.
// Ord follows declaration order (Normal < Het < Te < Ps < Zc) — the canonical
// display order used to keep the extra play buttons in a stable position.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ModeBucket {
    Normal,
    Het,
    Te,
    Ps,
    Zc,
}

impl ModeBucket {
    pub fn from_mode(m: PlaybackMode) -> Self {
        match m {
            PlaybackMode::Normal => ModeBucket::Normal,
            PlaybackMode::Heterodyne => ModeBucket::Het,
            PlaybackMode::TimeExpansion => ModeBucket::Te,
            PlaybackMode::PitchShift | PlaybackMode::PhaseVocoder => ModeBucket::Ps,
            PlaybackMode::ZeroCrossing => ModeBucket::Zc,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            ModeBucket::Normal => "1:1",
            ModeBucket::Het => "HET",
            ModeBucket::Te => "TE",
            ModeBucket::Ps => "PS",
            ModeBucket::Zc => "ZC",
        }
    }
    /// Full mode name, for surfaces with room for words (the phone sheet).
    pub fn name(self) -> &'static str {
        match self {
            ModeBucket::Normal => "Direct",
            ModeBucket::Het => "Heterodyne",
            ModeBucket::Te => "Time expansion",
            ModeBucket::Ps => "Pitch shift",
            ModeBucket::Zc => "Zero crossing",
        }
    }
    /// One-line consequence of picking this mode.
    pub fn description(self) -> &'static str {
        match self {
            ModeBucket::Normal => "Unchanged. Calls above 20 kHz are inaudible",
            ModeBucket::Het => "Mixes the band down to audible, like a handheld bat detector",
            ModeBucket::Te => {
                "Slows playback so calls drop into hearing range. Keeps call shape, takes longer"
            }
            ModeBucket::Ps => "Lowers pitch, keeps duration",
            ModeBucket::Zc => "Synthesised tone at the strongest frequency",
        }
    }
    fn title(self) -> &'static str {
        match self {
            ModeBucket::Normal => "1:1 — Normal playback",
            ModeBucket::Het => "HET — Heterodyne",
            ModeBucket::Te => "TE — Time Expansion",
            ModeBucket::Ps => "PS — Pitch Shift / Phase Vocoder",
            ModeBucket::Zc => "ZC — Zero Crossing",
        }
    }
    /// Whether this mode has a settings popup. 1:1 has nothing useful to show.
    fn has_settings(self) -> bool {
        !matches!(self, ModeBucket::Normal)
    }
    /// Default playback mode when this bucket is freshly selected.
    pub fn default_mode(self) -> PlaybackMode {
        match self {
            ModeBucket::Normal => PlaybackMode::Normal,
            ModeBucket::Het => PlaybackMode::Heterodyne,
            ModeBucket::Te => PlaybackMode::TimeExpansion,
            ModeBucket::Ps => PlaybackMode::PitchShift,
            ModeBucket::Zc => PlaybackMode::ZeroCrossing,
        }
    }
    /// Whether this bucket requires HFR to be on for playback (i.e.
    /// any mode other than Normal/1:1).
    pub fn needs_hfr(self) -> bool {
        !matches!(self, ModeBucket::Normal)
    }
}

/// Mode radio-button group on the Hearing Bar. Replaces the old `ModeButton`
/// combo (a single button that hosted both the mode toggle and its settings).
#[component]
pub fn ModeRadioGroup(#[prop(default = false)] list_layout: bool) -> impl IntoView {
    let state = expect_context::<AppState>();

    // ── Effect A: Ensure a default user range when HFR is first enabled ──
    Effect::new(move || {
        let stack = state.viewmode.focus_stack().get();
        if stack.hfr_enabled() {
            let eff = stack.effective_range();
            if !eff.is_active() {
                let files = state.library.files().get();
                let idx = state.library.current_index().get();
                let file = idx.and_then(|i| files.get(i));
                let nyquist = file.map(|f| f.spectrogram.max_freq).unwrap_or(96_000.0);

                let species_range = file.and_then(|f| {
                    use crate::bat_book::auto_resolve;
                    let favourites = state.bat_book.favourites().get_untracked();
                    let resolved = auto_resolve::resolve_auto(Some(f), &favourites);
                    let species_id = resolved.matched_species_id?;
                    let entry = auto_resolve::find_entry_in_manifest(resolved.region, &species_id)
                        .or_else(|| auto_resolve::find_entry_any_book(&species_id))?;
                    if entry.freq_lo_hz > 0.0 && entry.freq_hi_hz > entry.freq_lo_hz {
                        Some((entry.freq_lo_hz, entry.freq_hi_hz.min(nyquist)))
                    } else {
                        None
                    }
                });

                let (lo, hi) = species_range.unwrap_or((18_000.0, nyquist));
                state.set_band_ff_range(lo, hi);
            }
        }
    });

    // ── Effect B: BandFF range → auto parameter values (smart auto) ──
    Effect::new(move || {
        let band_ff_lo = state.filter.band_ff_freq_lo().get();
        let band_ff_hi = state.filter.band_ff_freq_hi().get();

        if band_ff_hi <= band_ff_lo {
            return;
        }

        let band_ff_bandwidth = band_ff_hi - band_ff_lo;

        // Auto LP cutoff: half the focus band, capped. Computed first so the
        // carrier anchor below can sit one cutoff above the band's low edge.
        let auto_cutoff = (band_ff_bandwidth / 2.0).min(15_000.0);
        if state.transform.het_cutoff_auto().get_untracked() {
            state.transform.het_cutoff().set(auto_cutoff);
        }
        if state.transform.het_freq_auto().get_untracked() {
            // Anchor the comb at the BOTTOM of the focus band: the lowest
            // carrier's lower LP edge lands on band_lo, and any comb teeth tile
            // upward from there. For a single full-band carrier this equals the
            // band centre, so single-carrier auto behaviour is unchanged.
            let cutoff = if state.transform.het_cutoff_auto().get_untracked() {
                auto_cutoff
            } else {
                state.transform.het_cutoff().get_untracked()
            };
            state.transform.het_frequency().set(band_ff_lo + cutoff);
        }

        if state.transform.te_factor_auto().get_untracked() {
            let te = smart_auto_factor(band_ff_lo, band_ff_hi, 40.0);
            state.transform.te_factor().set(te);
        }
        if state.transform.ps_factor_auto().get_untracked() {
            let ps = smart_auto_factor(band_ff_lo, band_ff_hi, 20.0);
            state
                .transform
                .ps_factor()
                .set(PitchFactor::from_signed(ps));
        }
        if state.transform.pv_factor_auto().get_untracked() {
            let pv = smart_auto_factor(band_ff_lo, band_ff_hi, 20.0);
            state
                .transform
                .pv_factor()
                .set(PitchFactor::from_signed(pv));
        }
    });

    // ── Effect B2: Comb-auto recompute ──
    Effect::new(move || {
        if !state.transform.het_comb_auto().get() {
            return;
        }
        let band_ff_lo = state.filter.band_ff_freq_lo().get();
        let band_ff_hi = state.filter.band_ff_freq_hi().get();
        let cutoff = state.transform.het_cutoff().get();

        if band_ff_hi <= band_ff_lo {
            return;
        }
        let band_ff_bandwidth = band_ff_hi - band_ff_lo;
        let cutoff = cutoff.max(1_000.0);
        let spacing = (cutoff * 2.0).max(5_000.0);
        let count = ((band_ff_bandwidth / spacing).ceil() as u32).clamp(1, 5);
        if (state.transform.het_comb_spacing().get_untracked() - spacing).abs() > 0.5 {
            state.transform.het_comb_spacing().set(spacing);
        }
        if state.transform.het_comb_count().get_untracked() != count {
            state.transform.het_comb_count().set(count);
        }
    });

    // ── Effect C: ZC mode display settings save/restore ──
    {
        let prev_mode = RwSignal::new(state.playback.mode().get_untracked());
        Effect::new(move || {
            let mode = state.playback.mode().get();
            let old = prev_mode.get_untracked();
            if mode == old {
                return;
            }
            prev_mode.set(mode);

            let was_zc = old == PlaybackMode::ZeroCrossing;
            let is_zc = mode == PlaybackMode::ZeroCrossing;

            if was_zc && !is_zc {
                state
                    .display
                    .zc_saved_auto_gain()
                    .set(state.display.auto_gain().get_untracked());
                state
                    .display
                    .zc_saved_eq()
                    .set(state.display.eq().get_untracked());
                state
                    .display
                    .zc_saved_noise_filter()
                    .set(state.display.noise_filter().get_untracked());

                state
                    .display
                    .auto_gain()
                    .set(state.display.normal_saved_auto_gain().get_untracked());
                state
                    .display
                    .eq()
                    .set(state.display.normal_saved_eq().get_untracked());
                state
                    .display
                    .noise_filter()
                    .set(state.display.normal_saved_noise_filter().get_untracked());
            } else if !was_zc && is_zc {
                state
                    .display
                    .normal_saved_auto_gain()
                    .set(state.display.auto_gain().get_untracked());
                state
                    .display
                    .normal_saved_eq()
                    .set(state.display.eq().get_untracked());
                state
                    .display
                    .normal_saved_noise_filter()
                    .set(state.display.noise_filter().get_untracked());

                state
                    .display
                    .auto_gain()
                    .set(state.display.zc_saved_auto_gain().get_untracked());
                state
                    .display
                    .eq()
                    .set(state.display.zc_saved_eq().get_untracked());
                state
                    .display
                    .noise_filter()
                    .set(state.display.zc_saved_noise_filter().get_untracked());
            }
        });
    }

    // ── Effect D: bandpass_mode + bandpass_range + playback_mode → filter settings ──
    Effect::new(move || {
        let bp_mode = state.filter.bandpass_mode().get();
        let bp_range = state.filter.bandpass_range().get();
        let band_ff_lo = state.filter.band_ff_freq_lo().get();
        let band_ff_hi = state.filter.band_ff_freq_hi().get();
        let playback_mode = state.playback.mode().get();

        match bp_mode {
            BandpassMode::Off => {
                state.filter.enabled().set(false);
            }
            BandpassMode::Auto => {
                let has_ff = band_ff_hi > band_ff_lo;
                match playback_mode {
                    PlaybackMode::Heterodyne => {
                        state.filter.enabled().set(false);
                    }
                    PlaybackMode::ZeroCrossing => {
                        state.filter.enabled().set(has_ff);
                        if has_ff {
                            state.filter.freq_low().set(band_ff_lo);
                            state.filter.freq_high().set(band_ff_hi);
                            state.filter.quality().set(FilterQuality::Spectral);
                            state.filter.db_below().set(-60.0);
                            state.filter.db_selected().set(0.0);
                            state.filter.db_above().set(-60.0);
                        }
                    }
                    _ => {
                        state.filter.enabled().set(has_ff);
                        if has_ff {
                            state.filter.freq_low().set(band_ff_lo);
                            state.filter.freq_high().set(band_ff_hi);
                            state.filter.quality().set(FilterQuality::Spectral);
                            state.filter.db_below().set(-60.0);
                            state.filter.db_selected().set(0.0);
                            state.filter.db_above().set(-60.0);
                        }
                    }
                }
            }
            BandpassMode::On => {
                state.filter.enabled().set(true);
                if bp_range == BandpassRange::FollowFocus && band_ff_hi > band_ff_lo {
                    state.filter.freq_low().set(band_ff_lo);
                    state.filter.freq_high().set(band_ff_hi);
                }
            }
        }
    });

    // ── Settings popup state ──
    let settings_open =
        Signal::derive(move || state.panels.layer_panel_open().get() == Some(LayerPanel::HfrMode));
    let no_file = move || {
        state.library.current_index().get().is_none() && state.timeline.active().get().is_none()
    };
    let muting =
        Signal::derive(move || state.mic.listening().get() && state.mic.mute_output().get());

    let row_ref = NodeRef::<leptos::html::Div>::new();

    // Whether a bucket is part of the user's current selection (the
    // active playback mode, plus any extras added via ctrl+click).
    let bucket_is_selected = move |bucket: ModeBucket| -> bool {
        let active = ModeBucket::from_mode(state.playback.mode().get());
        if active == bucket {
            return true;
        }
        let extras = state.playback.modes_extra().get();
        extras.iter().any(|m| ModeBucket::from_mode(*m) == bucket)
    };

    let select_bucket = move |bucket: ModeBucket, multi: bool| {
        if no_file() {
            return;
        }
        let active = ModeBucket::from_mode(state.playback.mode().get_untracked());

        if multi {
            // Ctrl+click: toggle this bucket in/out of the multi-selection.
            // The active playback_mode bucket can't be removed by
            // ctrl-clicking it (we'd be left with nothing to play) —
            // ctrl-click on the active bucket is a no-op in that case.
            let bucket_mode = bucket.default_mode();
            let mut extras = state.playback.modes_extra().get_untracked();
            let extras_has = extras.iter().any(|m| ModeBucket::from_mode(*m) == bucket);
            if bucket == active {
                if extras_has {
                    // Already deduped — clean up just in case.
                    extras.retain(|m| ModeBucket::from_mode(*m) != bucket);
                    state.playback.modes_extra().set(extras);
                }
                return;
            }
            if extras_has {
                extras.retain(|m| ModeBucket::from_mode(*m) != bucket);
            } else {
                extras.push(bucket_mode);
                // Selecting an HFR-requiring bucket turns HFR on so the
                // mode is actually playable.
                if bucket.needs_hfr() && !state.viewmode.focus_stack().get_untracked().hfr_enabled()
                {
                    state
                        .viewmode
                        .focus_stack()
                        .update(|s| s.set_saved_playback_mode(Some(bucket_mode)));
                    state.toggle_hfr();
                }
            }
            state.playback.modes_extra().set(extras);
            return;
        }

        // Plain click on the already-active bucket → open settings popup.
        if active == bucket {
            if bucket.has_settings() {
                toggle_panel(&state, LayerPanel::HfrMode);
            } else {
                state.panels.layer_panel_open().set(None);
            }
            return;
        }
        // Plain click on a different bucket: replace the entire selection.
        let mode = bucket.default_mode();
        if bucket.needs_hfr() && !state.viewmode.focus_stack().get_untracked().hfr_enabled() {
            state
                .viewmode
                .focus_stack()
                .update(|s| s.set_saved_playback_mode(Some(mode)));
            state.toggle_hfr();
        } else {
            state
                .viewmode
                .focus_stack()
                .update(|s| s.set_saved_playback_mode(Some(mode)));
        }
        state.playback.mode().set(mode);
        // A plain click clears any extras the user had added; if they
        // want multi-select they need to ctrl-click.
        state.playback.modes_extra().set(Vec::new());
        state.panels.layer_panel_open().set(None);
    };

    // True when the active band is entirely above human hearing — picking
    // 1:1 in that case would leave nothing audible. Drives the warning
    // badge on the 1:1 button (and an underline on the Play button via
    // bottom_toolbar.rs).
    let band_inaudible = Signal::derive(move || {
        let lo = state.filter.band_ff_freq_lo().get();
        let hi = state.filter.band_ff_freq_hi().get();
        hi > lo && lo >= 20_000.0
    });

    let mode_button = move |bucket: ModeBucket| {
        let label = bucket.label();
        let title = bucket.title();
        let class_sig = Signal::derive(move || {
            let active = ModeBucket::from_mode(state.playback.mode().get());
            let on = state.viewmode.hfr_enabled().get();
            let mut s = String::from("layer-btn mode-radio-btn");
            // Active bucket: same logic as before — Normal is "active"
            // when HFR is off or mode is Normal; others when HFR is on
            // and matches.
            let is_active = if bucket == ModeBucket::Normal {
                !on || active == ModeBucket::Normal
            } else {
                on && active == bucket
            };
            // Selected = active OR present in the multi-selection extras.
            let is_selected = is_active || bucket_is_selected(bucket);
            if is_selected {
                s.push_str(" selected");
                if is_active {
                    // Only the active mode gets the corner-arrow indicator
                    // (clicking it again opens its settings popup).
                    s.push_str(" active-mode");
                    if bucket.has_settings() {
                        s.push_str(" has-settings");
                    }
                    if settings_open.get() && bucket.has_settings() {
                        s.push_str(" open");
                    }
                } else {
                    // Multi-selected but not the live mode — paint it
                    // dimmer to make the "playing right now" mode obvious.
                    s.push_str(" multi-selected");
                }
            }
            if muting.get() && is_active && bucket != ModeBucket::Normal {
                s.push_str(" mute-blink");
            }
            // Warning badge on 1:1 when the band is ultrasound-only.
            if bucket == ModeBucket::Normal && band_inaudible.get() {
                s.push_str(" inaudible-warning");
            }
            if no_file() {
                s.push_str(" disabled");
            }
            s
        });
        let dyn_title = Signal::derive(move || {
            let base = if bucket == ModeBucket::Normal && band_inaudible.get() {
                "1:1 — Normal playback. Warning: band is above human hearing; this mode won\u{2019}t make it audible.".to_string()
            } else {
                title.to_string()
            };
            format!(
                "{}\n(Ctrl-click to add/remove from multi-play selection)",
                base
            )
        });
        view! {
            <button class=move || class_sig.get()
                title=move || dyn_title.get()
                on:click=move |ev: web_sys::MouseEvent| {
                    let multi = ev.ctrl_key() || ev.meta_key();
                    select_bucket(bucket, multi);
                }
            >
                <span class="mode-radio-label">{label}</span>
                <span class="mode-radio-corner">{"\u{25E2}"}</span>
            </button>
        }
    };

    // Phone "Hear" sheet: one row per mode with the full name and a
    // one-line consequence; the abbreviation chip ties it to the desktop
    // radio group. Same select_bucket path, so HFR/ZC side effects hold.
    let mode_row = move |bucket: ModeBucket| {
        let class_sig = Signal::derive(move || {
            let active = ModeBucket::from_mode(state.playback.mode().get());
            let on = state.viewmode.hfr_enabled().get();
            let is_active = if bucket == ModeBucket::Normal {
                !on || active == ModeBucket::Normal
            } else {
                on && active == bucket
            };
            let mut s = String::from("hear-mode-row");
            if is_active {
                s.push_str(" sel");
            } else if bucket_is_selected(bucket) {
                s.push_str(" multi-selected");
            }
            if no_file() {
                s.push_str(" disabled");
            }
            s
        });
        view! {
            <button class=move || class_sig.get()
                title=bucket.title()
                on:click=move |ev: web_sys::MouseEvent| {
                    let multi = ev.ctrl_key() || ev.meta_key();
                    select_bucket(bucket, multi);
                }
            >
                <span class="hear-mode-text">
                    <span class="hear-mode-name">{bucket.name()}</span>
                    <span class="hear-mode-desc">{bucket.description()}</span>
                </span>
                <span class="hear-mode-abbr">{bucket.label()}</span>
            </button>
        }
    };

    if list_layout {
        return view! {
            <div node_ref=row_ref class="hear-mode-list"
                on:click=|ev: web_sys::MouseEvent| ev.stop_propagation()
                on:touchstart=|ev: web_sys::TouchEvent| ev.stop_propagation()
            >
                {mode_row(ModeBucket::Normal)}
                {mode_row(ModeBucket::Het)}
                {mode_row(ModeBucket::Te)}
                {mode_row(ModeBucket::Ps)}
                {mode_row(ModeBucket::Zc)}

                <PopupPanel
                    is_open=settings_open
                    anchor=row_ref
                    preferred_side=Side::Above
                    preferred_align=Align::Start
                    extra_style="min-width: 220px;"
                >
                    <ModeSettingsBody/>
                </PopupPanel>
            </div>
        }
        .into_any();
    }

    // Settings popup content is keyed off the current playback mode.
    view! {
        <div node_ref=row_ref class="mode-radio-group"
            on:click=|ev: web_sys::MouseEvent| ev.stop_propagation()
            on:touchstart=|ev: web_sys::TouchEvent| ev.stop_propagation()
        >
            {mode_button(ModeBucket::Normal)}
            {mode_button(ModeBucket::Het)}
            {mode_button(ModeBucket::Te)}
            {mode_button(ModeBucket::Ps)}
            {mode_button(ModeBucket::Zc)}

            <PopupPanel
                is_open=settings_open
                anchor=row_ref
                preferred_side=Side::Below
                preferred_align=Align::Start
                extra_style="min-width: 220px;"
            >
                <ModeSettingsBody/>
            </PopupPanel>
        </div>
    }
    .into_any()
}

/// The body of the mode settings popup — switches content by current playback mode.
#[component]
fn ModeSettingsBody() -> impl IntoView {
    let state = expect_context::<AppState>();

    let on_te_change = move |ev: web_sys::Event| {
        use wasm_bindgen::JsCast;
        let input: web_sys::HtmlInputElement = ev.target().unwrap().unchecked_into();
        if let Ok(val) = input.value().parse::<f64>() {
            state.transform.te_factor_auto().set(false);
            state.playback.mode().set(PlaybackMode::TimeExpansion);
            state.transform.te_factor().set(val);
        }
    };

    let on_ps_change = move |ev: web_sys::Event| {
        use wasm_bindgen::JsCast;
        let input: web_sys::HtmlInputElement = ev.target().unwrap().unchecked_into();
        if let Ok(val) = input.value().parse::<f64>() {
            state.transform.ps_factor_auto().set(false);
            state.playback.mode().set(PlaybackMode::PitchShift);
            state
                .transform
                .ps_factor()
                .set(PitchFactor::from_signed(val));
        }
    };

    let on_pv_change = move |ev: web_sys::Event| {
        use wasm_bindgen::JsCast;
        let input: web_sys::HtmlInputElement = ev.target().unwrap().unchecked_into();
        if let Ok(val) = input.value().parse::<f64>() {
            state.transform.pv_factor_auto().set(false);
            state.playback.mode().set(PlaybackMode::PhaseVocoder);
            state
                .transform
                .pv_factor()
                .set(PitchFactor::from_signed(val));
        }
    };

    let on_zc_change = move |ev: web_sys::Event| {
        use wasm_bindgen::JsCast;
        let input: web_sys::HtmlInputElement = ev.target().unwrap().unchecked_into();
        if let Ok(val) = input.value().parse::<f64>() {
            state.playback.mode().set(PlaybackMode::ZeroCrossing);
            state.transform.zc_factor().set(val);
        }
    };

    let on_te_text = move |ev: web_sys::Event| {
        use wasm_bindgen::JsCast;
        let input: web_sys::HtmlInputElement = ev.target().unwrap().unchecked_into();
        if let Some(val) = parse_factor_input(&input.value()) {
            state.transform.te_factor_auto().set(false);
            state.transform.te_factor().set(val);
        }
    };

    let on_ps_text = move |ev: web_sys::Event| {
        use wasm_bindgen::JsCast;
        let input: web_sys::HtmlInputElement = ev.target().unwrap().unchecked_into();
        if let Some(val) = parse_factor_input(&input.value()) {
            state.transform.ps_factor_auto().set(false);
            state
                .transform
                .ps_factor()
                .set(PitchFactor::from_signed(val));
        }
    };

    let on_pv_text = move |ev: web_sys::Event| {
        use wasm_bindgen::JsCast;
        let input: web_sys::HtmlInputElement = ev.target().unwrap().unchecked_into();
        if let Some(val) = parse_factor_input(&input.value()) {
            state.transform.pv_factor_auto().set(false);
            state
                .transform
                .pv_factor()
                .set(PitchFactor::from_signed(val));
        }
    };

    // Generic over the signal handle so it accepts both `RwSignal` and store
    // subfields (`state.transform.te_factor()` etc.), which are distinct types.
    fn make_preset_click<F, A, V>(
        state: AppState,
        factor_signal: F,
        auto_signal: A,
        mode: PlaybackMode,
        value: V,
    ) -> impl Fn(web_sys::MouseEvent) + Copy + 'static
    where
        F: leptos::prelude::Set<Value = V> + Copy + 'static,
        A: leptos::prelude::Set<Value = bool> + Copy + 'static,
        V: Copy + 'static,
    {
        move |_: web_sys::MouseEvent| {
            auto_signal.set(false);
            factor_signal.set(value);
            state.playback.mode().set(mode);
        }
    }

    let preset_values: [(f64, &str); 6] = [
        (-2.0, "\u{00f7}2x"),
        (2.0, "2x"),
        (4.0, "4x"),
        (8.0, "8x"),
        (10.0, "10x"),
        (16.0, "16x"),
    ];

    fn set_output_highlight<F, V>(
        state: AppState,
        factor_signal: F,
    ) -> impl Fn(web_sys::MouseEvent) + Copy + 'static
    where
        F: leptos::prelude::GetUntracked<Value = V> + Copy + 'static,
        V: Into<f64>,
    {
        move |_: web_sys::MouseEvent| {
            let f: f64 = factor_signal.get_untracked().into();
            let band_ff_lo = state.filter.band_ff_freq_lo().get_untracked();
            let band_ff_hi = state.filter.band_ff_freq_hi().get_untracked();
            if band_ff_hi > band_ff_lo {
                let out_lo = output_freq(band_ff_lo, f);
                let out_hi = output_freq(band_ff_hi, f);
                let (lo, hi) = if out_lo < out_hi {
                    (out_lo, out_hi)
                } else {
                    (out_hi, out_lo)
                };
                state.viewmode.output_freq_highlight().set(Some((lo, hi)));
            }
        }
    }
    let clear_output_highlight = move |_: web_sys::MouseEvent| {
        state.viewmode.output_freq_highlight().set(None);
    };

    view! {
        <>
            // ── Listening-muted notice (unchanged from old ModeButton) ──
            <Show when=move || state.mic.listening().get() && state.mic.mute_output().get()>
                <div style="padding: 6px 8px; background: rgba(255, 200, 0, 0.12); border: 1px solid rgba(255, 200, 0, 0.5); border-radius: 4px; margin: 0 4px 6px;">
                    <div style="font-size: 11px; color: #ffcc00; line-height: 1.3; margin-bottom: 4px;">
                        "Listening output is muted (warm-up). Spectrogram still updates."
                    </div>
                    <button class="layer-panel-opt"
                        style="width: 100%;"
                        on:click=move |_| state.mic.mute_output().set(false)
                        title="Unmute speaker output"
                    >"Unmute"</button>
                </div>
            </Show>

            // ── Body: per-mode controls ──
            {move || {
                let mode = state.playback.mode().get();
                match mode {
                    PlaybackMode::Heterodyne => view! {
                        <div class="layer-panel-title">"Heterodyne"</div>
                        <div class="layer-panel-slider-row het-text-row"
                            on:mouseenter=move |_| {
                                state.transform.het_interacting().set(true);
                                state.interaction.spec_hover_handle().set(Some(SpectrogramHandle::HetBandUpper));
                            }
                            on:mouseleave=move |_| {
                                state.transform.het_interacting().set(false);
                                state.interaction.spec_hover_handle().set(None);
                            }
                        >
                            <label title="Low-pass cutoff (kHz) applied to each carrier — controls how wide a band around the carrier you hear. Drag the cyan band edges on the spectrogram to adjust.">"LP cutoff"</label>
                            <span class="het-value">{move || format!("{:.1} kHz", state.transform.het_cutoff().get() / 1000.0)}</span>
                            <button class=move || if state.transform.het_cutoff_auto().get() { "auto-toggle on" } else { "auto-toggle" }
                                on:click=move |_| state.transform.het_cutoff_auto().update(|v| *v = !*v)
                                title="Toggle auto LP cutoff"
                            >"A"</button>
                        </div>
                        <div class="layer-panel-slider-row">
                            <label title="Number of heterodyne carriers — 1 is classic; more carriers tile upward from the low edge to cover a wider band.">"Carriers"</label>
                            {(1u32..=5).map(|n| {
                                view! {
                                    <button class=move || {
                                        let auto = state.transform.het_comb_auto().get();
                                        let sel = !auto && state.transform.het_comb_count().get() == n;
                                        if sel { "factor-preset sel" }
                                        else if auto && state.transform.het_comb_count().get() == n { "factor-preset auto-derived" }
                                        else { "factor-preset" }
                                    }
                                        on:click=move |_| {
                                            state.transform.het_comb_auto().set(false);
                                            state.transform.het_comb_count().set(n);
                                        }
                                    >{n.to_string()}</button>
                                }
                            }).collect::<Vec<_>>()}
                            <button class=move || if state.transform.het_comb_auto().get() { "auto-toggle on" } else { "auto-toggle" }
                                on:click=move |_| state.transform.het_comb_auto().update(|v| *v = !*v)
                                title="Auto-fit carrier count + spacing to the focus range"
                            >"A"</button>
                        </div>
                        <Show when=move || { state.transform.het_comb_count().get() > 1 }>
                            <div class="layer-panel-slider-row het-text-row">
                                <label title="Spacing between adjacent carriers (Hz).">"Spacing"</label>
                                <span class="het-value">{move || format!("{:.0} kHz", state.transform.het_comb_spacing().get() / 1000.0)}</span>
                                <input type="range" min="5" max="100" step="1"
                                    prop:value=move || (state.transform.het_comb_spacing().get() / 1000.0).round().to_string()
                                    on:input=move |ev| {
                                        let v: f64 = leptos::prelude::event_target_value(&ev).parse().unwrap_or(30.0);
                                        state.transform.het_comb_auto().set(false);
                                        state.transform.het_comb_spacing().set(v * 1000.0);
                                    }
                                    on:dblclick=move |_| {
                                        let two_cutoff = (state.transform.het_cutoff().get_untracked() * 2.0).clamp(5_000.0, 100_000.0);
                                        state.transform.het_comb_spacing().set(two_cutoff);
                                    }
                                />
                            </div>
                        </Show>
                    }.into_any(),

                    PlaybackMode::TimeExpansion => view! {
                        <div class="layer-panel-title">"Time Expansion"</div>
                        <div class="layer-panel-slider-row">
                            <label>"Factor"</label>
                            <input type="range" min="-40" max="40" step="1"
                                prop:value=move || (state.transform.te_factor().get() as i32).to_string()
                                on:input=on_te_change
                            />
                            <input type="text" class="factor-input"
                                prop:value=move || format_factor_value(state.transform.te_factor().get())
                                on:change=on_te_text
                                on:focus=move |ev: web_sys::FocusEvent| {
                                    use wasm_bindgen::JsCast;
                                    if let Some(input) = ev.target().and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok()) {
                                        input.select();
                                    }
                                }
                                title="Enter a custom factor (e.g. 10, 7.5, \u{00f7}2)"
                            />
                            <button class=move || if state.transform.te_factor_auto().get() { "auto-toggle on" } else { "auto-toggle" }
                                on:click=move |_| state.transform.te_factor_auto().update(|v| *v = !*v)
                                title="Auto: picks best factor for audible output"
                            >"A"</button>
                        </div>
                        <div class="factor-presets">
                            {preset_values.iter().map(|&(val, label)| {
                                let on_click = make_preset_click(state, state.transform.te_factor(), state.transform.te_factor_auto(), PlaybackMode::TimeExpansion, val);
                                let is_sel = move || (state.transform.te_factor().get() - val).abs() < 0.01 && !state.transform.te_factor_auto().get();
                                view! {
                                    <button class=move || if is_sel() { "factor-preset sel" } else { "factor-preset" }
                                        on:click=on_click
                                    >{label}</button>
                                }
                            }).collect::<Vec<_>>()}
                        </div>
                        <Show when=move || state.band_ff_range().is_some()>
                            <div class="freq-summary">
                                <div>"Input: "{move || format!("{}\u{2013}{}", format_freq_khz(state.filter.band_ff_freq_lo().get()), format_freq_khz(state.filter.band_ff_freq_hi().get()))}</div>
                                <div class="freq-summary-output"
                                    on:mouseenter=set_output_highlight(state, state.transform.te_factor())
                                    on:mouseleave=clear_output_highlight
                                >"Output: "{move || {
                                    let f = state.transform.te_factor().get();
                                    let lo = output_freq(state.filter.band_ff_freq_lo().get(), f);
                                    let hi = output_freq(state.filter.band_ff_freq_hi().get(), f);
                                    let (lo, hi) = if lo < hi { (lo, hi) } else { (hi, lo) };
                                    format!("{}\u{2013}{}", format_freq_khz(lo), format_freq_khz(hi))
                                }}</div>
                            </div>
                        </Show>
                    }.into_any(),

                    PlaybackMode::PitchShift | PlaybackMode::PhaseVocoder => {
                        // PS bucket — shared settings panel for PitchShift +
                        // PhaseVocoder, with an Algorithm toggle at the top.
                        view! {
                            <div class="layer-panel-title">"Pitch Shift"</div>
                            <div class="layer-panel-slider-row algo-row">
                                <label>"Algorithm"</label>
                                <button class=move || if state.playback.mode().get() == PlaybackMode::PitchShift { "auto-toggle on" } else { "auto-toggle" }
                                    on:click=move |_| state.playback.mode().set(PlaybackMode::PitchShift)
                                    title="Standard pitch shift — lower latency"
                                >"PS"</button>
                                <button class=move || if state.playback.mode().get() == PlaybackMode::PhaseVocoder { "auto-toggle on" } else { "auto-toggle" }
                                    on:click=move |_| state.playback.mode().set(PlaybackMode::PhaseVocoder)
                                    title="Phase Vocoder — preserves transients better, higher quality"
                                >"PV"</button>
                            </div>
                            {move || if state.playback.mode().get() == PlaybackMode::PhaseVocoder {
                                view! {
                                    <div class="layer-panel-slider-row">
                                        <label>"Factor"</label>
                                        <input type="range" min="-20" max="20" step="1"
                                            prop:value=move || (state.transform.pv_factor().get().signed() as i32).to_string()
                                            on:input=on_pv_change
                                        />
                                        <input type="text" class="factor-input"
                                            prop:value=move || format_factor_value(state.transform.pv_factor().get().signed())
                                            on:change=on_pv_text
                                            on:focus=move |ev: web_sys::FocusEvent| {
                                                use wasm_bindgen::JsCast;
                                                if let Some(input) = ev.target().and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok()) {
                                                    input.select();
                                                }
                                            }
                                            title="Enter a custom factor (e.g. 10, 7.5, \u{00f7}2)"
                                        />
                                        <button class=move || if state.transform.pv_factor_auto().get() { "auto-toggle on" } else { "auto-toggle" }
                                            on:click=move |_| state.transform.pv_factor_auto().update(|v| *v = !*v)
                                            title="Auto: picks best factor for audible output"
                                        >"A"</button>
                                    </div>
                                    <div class="factor-presets">
                                        {preset_values.iter().map(|&(val, label)| {
                                            let on_click = make_preset_click(state, state.transform.pv_factor(), state.transform.pv_factor_auto(), PlaybackMode::PhaseVocoder, PitchFactor::from_signed(val));
                                            let is_sel = move || (state.transform.pv_factor().get().signed() - val).abs() < 0.01 && !state.transform.pv_factor_auto().get();
                                            view! {
                                                <button class=move || if is_sel() { "factor-preset sel" } else { "factor-preset" }
                                                    on:click=on_click
                                                >{label}</button>
                                            }
                                        }).collect::<Vec<_>>()}
                                    </div>
                                }.into_any()
                            } else {
                                view! {
                                    <div class="layer-panel-slider-row">
                                        <label>"Factor"</label>
                                        <input type="range" min="-20" max="20" step="1"
                                            prop:value=move || (state.transform.ps_factor().get().signed() as i32).to_string()
                                            on:input=on_ps_change
                                        />
                                        <input type="text" class="factor-input"
                                            prop:value=move || format_factor_value(state.transform.ps_factor().get().signed())
                                            on:change=on_ps_text
                                            on:focus=move |ev: web_sys::FocusEvent| {
                                                use wasm_bindgen::JsCast;
                                                if let Some(input) = ev.target().and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok()) {
                                                    input.select();
                                                }
                                            }
                                            title="Enter a custom factor (e.g. 10, 7.5, \u{00f7}2)"
                                        />
                                        <button class=move || if state.transform.ps_factor_auto().get() { "auto-toggle on" } else { "auto-toggle" }
                                            on:click=move |_| state.transform.ps_factor_auto().update(|v| *v = !*v)
                                            title="Auto: picks best factor for audible output"
                                        >"A"</button>
                                    </div>
                                    <div class="factor-presets">
                                        {preset_values.iter().map(|&(val, label)| {
                                            let on_click = make_preset_click(state, state.transform.ps_factor(), state.transform.ps_factor_auto(), PlaybackMode::PitchShift, PitchFactor::from_signed(val));
                                            let is_sel = move || (state.transform.ps_factor().get().signed() - val).abs() < 0.01 && !state.transform.ps_factor_auto().get();
                                            view! {
                                                <button class=move || if is_sel() { "factor-preset sel" } else { "factor-preset" }
                                                    on:click=on_click
                                                >{label}</button>
                                            }
                                        }).collect::<Vec<_>>()}
                                    </div>
                                }.into_any()
                            }}
                            <Show when=move || state.band_ff_range().is_some()>
                                <div class="freq-summary">
                                    <div>"Input: "{move || format!("{}\u{2013}{}", format_freq_khz(state.filter.band_ff_freq_lo().get()), format_freq_khz(state.filter.band_ff_freq_hi().get()))}</div>
                                    <div class="freq-summary-output"
                                        on:mouseenter=move |ev: web_sys::MouseEvent| {
                                            // pv_factor()/ps_factor() are distinct subfield types, so
                                            // dispatch in separate branches rather than a unified handle.
                                            if state.playback.mode().get_untracked() == PlaybackMode::PhaseVocoder {
                                                set_output_highlight(state, state.transform.pv_factor())(ev);
                                            } else {
                                                set_output_highlight(state, state.transform.ps_factor())(ev);
                                            }
                                        }
                                        on:mouseleave=clear_output_highlight
                                    >"Output: "{move || {
                                        let mode = state.playback.mode().get();
                                        let f = if mode == PlaybackMode::PhaseVocoder { state.transform.pv_factor().get().signed() } else { state.transform.ps_factor().get().signed() };
                                        let lo = output_freq(state.filter.band_ff_freq_lo().get(), f);
                                        let hi = output_freq(state.filter.band_ff_freq_hi().get(), f);
                                        let (lo, hi) = if lo < hi { (lo, hi) } else { (hi, lo) };
                                        format!("{}\u{2013}{}", format_freq_khz(lo), format_freq_khz(hi))
                                    }}</div>
                                </div>
                            </Show>
                            <div class="layer-panel-slider-row">
                                <label>"Quality"</label>
                                <button class=move || if !state.transform.pv_hq().get() { "auto-toggle on" } else { "auto-toggle" }
                                    on:click=move |_| state.transform.pv_hq().set(false)
                                    title="Standard mode — uses filter warmup to reduce boundary clicks"
                                >"Std"</button>
                                <button class=move || if state.transform.pv_hq().get() { "auto-toggle on" } else { "auto-toggle" }
                                    on:click=move |_| state.transform.pv_hq().set(true)
                                    title="HQ mode — overlapping crossfade eliminates boundary clicks"
                                >"HQ"</button>
                            </div>
                        }.into_any()
                    },

                    PlaybackMode::ZeroCrossing => view! {
                        <div class="layer-panel-title">"Zero Crossing"</div>
                        <div class="layer-panel-slider-row">
                            <label>"Division"</label>
                            <input type="range" min="2" max="32" step="1"
                                prop:value=move || (state.transform.zc_factor().get() as u32).to_string()
                                on:input=on_zc_change
                            />
                            <span>{move || format!("\u{00f7}{}", state.transform.zc_factor().get() as u32)}</span>
                        </div>
                    }.into_any(),

                    PlaybackMode::Normal => view! {
                        // 1:1 has no settings. The inaudible-band warning,
                        // when applicable, is the only thing worth showing.
                        {move || (state.filter.band_ff_freq_lo().get() >= 20_000.0).then(|| {
                            view! {
                                <div style="padding: 8px 10px; font-size: 11px; color: #e0a030; line-height: 1.35;">
                                    "Band is above human hearing. 1:1 mode won\u{2019}t make it audible — try HET, TE, or PS."
                                </div>
                            }
                        })}
                    }.into_any(),
                }
            }}
        </>
    }
}
