// Bottom toolbar — transport + capture cluster.
//
// Layout:
//   [Play | Mode] [Channel] │ [ Mic | Record | Listen ]
//                              └─── capture group ───┘
//
// The Mic button on the left bookends Record + Listen to communicate
// that selecting a mic is a prerequisite for either. Stopping Listen
// leaves the live document in place as an empty "armed" doc — the user
// can adjust HFR / band, then press Listen or Record again to reuse
// it. (The file panel's "+ New live recording" button creates one
// from scratch.) On mobile the capture group flattens into the grid so
// each button gets its own cell.
//
// Hearing-DSP controls (HFR, Band, EQ, Notch, Gain) live in `HearingBar`.
// Visualization controls (View, Anno, Book, Tool) live in `ViewBar`.

use crate::audio::source::ChannelView;
use crate::audio::{microphone, playback};
use crate::components::combo_button::ComboButton;
use crate::components::icons::Icon;
use crate::components::listen_button::ListenButton;
use crate::components::mode_button::ModeBucket;
use crate::state::store_fields::*;
use crate::state::{
    AppState, Bar, ChannelMode, LayerPanel, MicStrategy, PlayStartMode, PlaybackMode, RecordMode,
};
use leptos::prelude::*;
use wasm_bindgen::prelude::*;

fn layer_opt_class(active: bool) -> &'static str {
    if active {
        "layer-panel-opt sel"
    } else {
        "layer-panel-opt"
    }
}

fn toggle_panel(state: &AppState, panel: LayerPanel) {
    state.panels.layer_panel_open().update(|p| {
        *p = if *p == Some(panel) { None } else { Some(panel) };
    });
}

#[component]
pub fn BottomToolbar() -> impl IntoView {
    let state = expect_context::<AppState>();
    // Always show all buttons; use has_file/is_file_disabled for enable/disable logic
    let has_file = move || true;
    let is_file_disabled = move || {
        state.library.current_index().get().is_none() && state.timeline.active().get().is_none()
    };

    // ── Recording timer ──
    let interval_id: StoredValue<Option<i32>> = StoredValue::new(None);
    Effect::new(move |_| {
        let recording = state.mic.recording().get();
        if recording {
            let cb = Closure::<dyn FnMut()>::new(move || {
                state.mic.timer_tick().update(|n| *n = n.wrapping_add(1));
            });
            if let Some(window) = web_sys::window() {
                if let Ok(id) = window.set_interval_with_callback_and_timeout_and_arguments_0(
                    cb.as_ref().unchecked_ref(),
                    100,
                ) {
                    interval_id.set_value(Some(id));
                }
            }
            cb.forget();
        } else if let Some(id) = interval_id.get_value() {
            if let Some(window) = web_sys::window() {
                window.clear_interval_with_handle(id);
            }
            interval_id.set_value(None);
        }
    });

    // ── Play ComboButton setup ──
    let play_is_open =
        Signal::derive(move || state.panels.layer_panel_open().get() == Some(LayerPanel::PlayMode));

    // True when current mode is 1:1 (Normal) AND the active band is
    // entirely above human hearing — same warning state surfaced as the
    // amber underline on the 1:1 mode radio button.
    let play_inaudible = Signal::derive(move || {
        use crate::state::PlaybackMode;
        if state.playback.mode().get() != PlaybackMode::Normal {
            return false;
        }
        let lo = state.filter.band_ff_freq_lo().get();
        let hi = state.filter.band_ff_freq_hi().get();
        hi > lo && lo >= 20_000.0
    });

    let play_left_class = Signal::derive(move || {
        let no_file = state.library.current_index().get().is_none()
            && state.timeline.active().get().is_none();
        let recording_and_listening = state.mic.recording().get() && state.mic.listening().get();
        if no_file || recording_and_listening {
            "layer-btn combo-btn-left disabled"
        } else if state.playback.is_playing().get() {
            "layer-btn combo-btn-left active"
        } else {
            "layer-btn combo-btn-left"
        }
    });
    let play_right_class = Signal::derive(move || {
        if is_file_disabled() {
            "layer-btn combo-btn-right disabled"
        } else if play_is_open.get() {
            "layer-btn combo-btn-right open"
        } else {
            "layer-btn combo-btn-right"
        }
    });

    // Icon-only face; the mode label (if any) sits above it as the category.
    let play_left_value = Signal::derive(move || String::new());
    let play_left_icon = Signal::derive(move || Some(Icon::Play));

    // Mode label shown on the Play button face (e.g. "HET", "TE"). Shown
    // for every selected mode so the main button and the per-mode extras
    // read consistently. The one exception is a lone, audible 1:1 (Normal
    // with no extras and no inaudible-band warning): there the label is
    // suppressed so just a plain ▶ shows.
    let play_mode_label = Signal::derive(move || {
        let mode = state.playback.mode().get();
        let lone = state.playback.modes_extra().get().is_empty();
        if mode == PlaybackMode::Normal && lone && !play_inaudible.get() {
            String::new()
        } else {
            ModeBucket::from_mode(mode).label().to_string()
        }
    });

    // Empty big-value for the Play combo's right half — the start-position
    // selector now shows only as a small caption above the dropdown caret.
    let play_empty = Signal::derive(move || String::new());

    let play_right_frozen: StoredValue<Option<String>> = StoredValue::new(None);
    let play_pos_label = Signal::derive(move || {
        // Start-position caption (Auto / All / Here / Sel) above the caret.
        // Freeze while playing so scrolling doesn't flicker it.
        if state.playback.is_playing().get() {
            if let Some(frozen) = play_right_frozen.get_value() {
                return frozen;
            }
        }
        let val = match state.playback.start_mode().get() {
            PlayStartMode::All => "All".to_string(),
            PlayStartMode::FromHere => "Here".to_string(),
            PlayStartMode::Selected => "Sel".to_string(),
            PlayStartMode::Auto => {
                // Subscribe to signals that affect auto-play mode for reactivity
                let _sel = state.interaction.selection().get();
                let _ann = state.annotations.selected_ids().get();
                let _scroll = state.view.scroll_offset().get();
                let _zoom = state.view.zoom_level().get();
                if let Some(sel) = playback::effective_selection(&state) {
                    if playback::is_selection_in_viewport(&state, &sel) {
                        "Sel".to_string()
                    } else if _scroll <= 0.0 {
                        "All".to_string()
                    } else {
                        "Here".to_string()
                    }
                } else if _scroll <= 0.0 {
                    "All".to_string()
                } else {
                    "Here".to_string()
                }
            }
        };
        play_right_frozen.set_value(Some(val.clone()));
        val
    });

    // Restore HFR after playback ends if we paused it for a 1:1 play
    // inside a multi-selection. Watches `is_playing` — when it
    // transitions to false and `paused_hfr_for_normal` is set, flip HFR
    // back on. Switching mid-play via another ▶ button also relies on
    // this (do_play_in_mode calls stop() before swapping modes).
    Effect::new(move || {
        let playing = state.playback.is_playing().get();
        if !playing && state.playback.paused_hfr_for_normal().get_untracked() {
            state.playback.paused_hfr_for_normal().set(false);
            if !state.viewmode.hfr_enabled().get_untracked() {
                state.toggle_hfr();
            }
        }
    });

    // Decide which "start position" rule to apply (Auto / All / Here /
    // Sel) and dispatch to the right playback helper. Used by both the
    // primary Play button and the per-mode Play buttons in the
    // multi-mode row.
    let do_start_playback = move || match state.playback.start_mode().get_untracked() {
        PlayStartMode::All => playback::play_from_start(&state),
        PlayStartMode::FromHere => playback::play_from_here(&state),
        PlayStartMode::Selected => {
            if playback::effective_selection(&state).is_some() {
                playback::play(&state);
            } else {
                playback::play_from_start(&state);
            }
        }
        PlayStartMode::Auto => {
            if let Some(sel) = playback::effective_selection(&state) {
                if playback::is_selection_in_viewport(&state, &sel) {
                    playback::play(&state);
                } else if state.view.scroll_offset().get_untracked() <= 0.0 {
                    playback::play_from_start(&state);
                } else {
                    playback::play_from_here(&state);
                }
            } else if state.view.scroll_offset().get_untracked() <= 0.0 {
                playback::play_from_start(&state);
            } else {
                playback::play_from_here(&state);
            }
        }
    };

    // "Play in this mode": stop any current playback (which triggers
    // the HFR-restore Effect if we'd previously paused HFR), then swap
    // playback_mode, handle the special "1:1 needs HFR off" case, and
    // start playback.
    let do_play_in_mode = move |mode: PlaybackMode| {
        let no_file = state.library.current_index().get_untracked().is_none()
            && state.timeline.active().get_untracked().is_none();
        let recording_and_listening =
            state.mic.recording().get_untracked() && state.mic.listening().get_untracked();
        if no_file || recording_and_listening {
            return;
        }
        // Stop anything currently playing (lets the HFR-restore Effect
        // unpause HFR if we'd paused it for a previous 1:1 play).
        if state.playback.is_playing().get_untracked() {
            playback::stop(&state);
        }
        // Swap: the clicked mode becomes the live (main-button) mode and
        // the previously-live mode moves into the extras. This keeps the
        // full multi-selection set intact and guarantees exactly one
        // labelled play button per selected mode (no mode shown twice on
        // the main button AND an extra).
        let old_active = state.playback.mode().get_untracked();
        let new_bucket = ModeBucket::from_mode(mode);
        let old_bucket = ModeBucket::from_mode(old_active);
        if new_bucket != old_bucket {
            state.playback.modes_extra().update(|extras| {
                extras.retain(|m| ModeBucket::from_mode(*m) != new_bucket);
                if !extras
                    .iter()
                    .any(|m| ModeBucket::from_mode(*m) == old_bucket)
                {
                    extras.push(old_active);
                }
            });
        }
        state.playback.mode().set(mode);
        // 1:1 inside a multi-selection that includes HFR modes: HFR is
        // on but 1:1 itself doesn't want it. Pause HFR for the duration
        // of this playback; the Effect above restores it on stop.
        if mode == PlaybackMode::Normal && state.viewmode.hfr_enabled().get_untracked() {
            state.playback.paused_hfr_for_normal().set(true);
            state.toggle_hfr();
        }
        do_start_playback();
    };

    let play_left_click = Callback::new(move |_: web_sys::MouseEvent| {
        let no_file = state.library.current_index().get_untracked().is_none()
            && state.timeline.active().get_untracked().is_none();
        let recording_and_listening =
            state.mic.recording().get_untracked() && state.mic.listening().get_untracked();
        if no_file || recording_and_listening {
            return;
        }
        if state.playback.is_playing().get_untracked() {
            playback::stop(&state);
        } else {
            do_start_playback();
        }
    });
    let play_toggle_menu = Callback::new(move |()| {
        toggle_panel(&state, LayerPanel::PlayMode);
    });

    // ── Record ComboButton setup ──
    let rec_is_open = Signal::derive(move || {
        state.panels.layer_panel_open().get() == Some(LayerPanel::RecordMode)
    });

    let rec_left_class = Signal::derive(move || {
        if state.mic.recording().get() {
            "layer-btn combo-btn-left rec-btn mic-recording"
        } else if state.playback.record_mode().get() == RecordMode::ListenOnly
            || state.mic.strategy().get() == MicStrategy::None
        {
            "layer-btn combo-btn-left rec-btn disabled"
        } else {
            "layer-btn combo-btn-left rec-btn"
        }
    });
    let rec_right_class = Signal::derive(move || {
        if rec_is_open.get() {
            "layer-btn combo-btn-right open"
        } else {
            "layer-btn combo-btn-right"
        }
    });

    let rec_left_value = Signal::derive(move || {
        if state.mic.recording().get() {
            let _ = state.mic.timer_tick().get();
            let start = state
                .mic
                .recording_start_time()
                .get_untracked()
                .unwrap_or(0.0);
            let now = js_sys::Date::now();
            let secs = (now - start) / 1000.0;
            format!("Rec {}", crate::format_time::format_duration_compact(secs))
        } else {
            String::new() // CSS ::before renders the red dot
        }
    });
    let rec_right_value = Signal::derive(move || match state.playback.record_mode().get() {
        RecordMode::ToFile => "File".to_string(),
        RecordMode::ToMemory => "Mem".to_string(),
        RecordMode::ListenOnly => "Listen".to_string(),
    });

    let rec_left_click = Callback::new(move |_: web_sys::MouseEvent| {
        if state.playback.record_mode().get_untracked() == RecordMode::ListenOnly
            || state.mic.strategy().get_untracked() == MicStrategy::None
        {
            return; // greyed out
        }
        let st = state;
        wasm_bindgen_futures::spawn_local(async move {
            microphone::toggle_record(&st).await;
        });
    });
    // Long-press on record button while listening: start recording with pre-roll
    // buffer (works even in ListenOnly mode).
    let rec_long_press = Callback::new(move |_: web_sys::MouseEvent| {
        if state.mic.strategy().get_untracked() == MicStrategy::None {
            return;
        }
        // Only meaningful when currently listening
        if !state.mic.listening().get_untracked() {
            return;
        }
        let st = state;
        wasm_bindgen_futures::spawn_local(async move {
            microphone::toggle_record_with_preroll(&st).await;
        });
    });
    let rec_toggle_menu = Callback::new(move |()| {
        toggle_panel(&state, LayerPanel::RecordMode);
    });

    // ── Drag-to-resize state ──
    let max_height = RwSignal::new(Option::<f64>::None); // None = auto (no constraint)
    let drag_active = StoredValue::new(false);
    let drag_start_y = StoredValue::new(0.0f64);
    let drag_start_height = StoredValue::new(0.0f64);
    let toolbar_node = NodeRef::<leptos::html::Div>::new();

    // Mouse/touch handlers for drag resize
    let on_handle_pointerdown = move |ev: web_sys::PointerEvent| {
        ev.prevent_default();
        ev.stop_propagation();
        drag_active.set_value(true);
        drag_start_y.set_value(ev.client_y() as f64);
        // Get current toolbar height
        if let Some(el) = toolbar_node.get() {
            let rect = el.get_bounding_client_rect();
            drag_start_height.set_value(rect.height());
        }
        // Capture pointer on the target
        if let Some(target) = ev.target() {
            if let Ok(el) = target.dyn_into::<web_sys::Element>() {
                let _ = el.set_pointer_capture(ev.pointer_id());
            }
        }
    };

    let on_handle_pointermove = move |ev: web_sys::PointerEvent| {
        if !drag_active.get_value() {
            return;
        }
        ev.prevent_default();
        let delta = drag_start_y.get_value() - ev.client_y() as f64; // dragging up = positive
        let new_height = (drag_start_height.get_value() + delta).clamp(48.0, 400.0);
        max_height.set(Some(new_height));
    };

    let on_handle_pointerup = move |ev: web_sys::PointerEvent| {
        if !drag_active.get_value() {
            return;
        }
        drag_active.set_value(false);
        // Release pointer capture
        if let Some(target) = ev.target() {
            if let Ok(el) = target.dyn_into::<web_sys::Element>() {
                let _ = el.release_pointer_capture(ev.pointer_id());
            }
        }
    };

    // Double-click resets to auto height
    let on_handle_dblclick = move |_: web_sys::MouseEvent| {
        max_height.set(None);
    };

    view! {
        <div class=move || if state.status.is_mobile().get() { "bottom-toolbar mobile" } else { "bottom-toolbar" }
            class:panel-open=move || matches!(state.panels.layer_panel_open().get().map(LayerPanel::bar), Some(Bar::Transport))
            node_ref=toolbar_node
            style=move || {
                // Inline z-index is the load-bearing lift: keeps combo dropdowns
                // above .main-overlays (z-index: 20) immediately and across all
                // views, regardless of :has() invalidation quirks or Leptos
                // class-diff timing. Matches the CSS-backup rule at line ~1793.
                // Scoped to transport-bar panels so opening a Hearing/View bar
                // popup doesn't also lift this bar (which would otherwise win
                // DOM order and cover the real popup).
                let mut s = if matches!(state.panels.layer_panel_open().get().map(LayerPanel::bar), Some(Bar::Transport)) {
                    String::from("z-index: 25;")
                } else {
                    String::new()
                };
                if let Some(h) = max_height.get() {
                    s.push_str(&format!("max-height: {h}px; overflow-y: auto;"));
                }
                s
            }
            on:click=|ev: web_sys::MouseEvent| ev.stop_propagation()
            on:touchstart=|ev: web_sys::TouchEvent| ev.stop_propagation()
        >
            // ── Drag handle for vertical resize ──
            <div class="bottom-toolbar-drag-handle"
                on:pointerdown=on_handle_pointerdown
                on:pointermove=on_handle_pointermove
                on:pointerup=on_handle_pointerup
                on:pointercancel=on_handle_pointerup
                on:dblclick=on_handle_dblclick
                title="Drag to resize toolbar, double-click to reset"
            >
                <div class="bottom-toolbar-drag-grip"></div>
            </div>

            // ── Extra per-mode Play buttons (multi-mode selection) ──
            // When the user has ctrl-clicked additional modes in the
            // Mode radio group, render one small ▶ button per *extra*
            // selected mode, immediately before the primary Play combo.
            // Clicking one switches `playback_mode` to that mode and
            // starts playback (stopping any current playback first).
            //
            // 1:1 has the same audible-warning underline treatment as
            // the primary button. If 1:1 is among the extras while HFR
            // is on, clicking ▶ 1:1 will temporarily turn HFR off (and
            // restore it on stop) via the Effect above.
            {move || {
                if !has_file() { return None; }
                let mut extras = state.playback.modes_extra().get();
                if extras.is_empty() { return None; }
                // Render in a STABLE canonical order so the extra ▶ buttons keep
                // their positions when one is pressed. do_play_in_mode swaps the
                // pressed mode into the primary combo and the old primary back
                // into the extras (appended), which used to reshuffle the whole
                // row; sorting here means only the swapped pair changes place and
                // every other button stays put (the disorienting part).
                extras.sort_by_key(|m| ModeBucket::from_mode(*m));
                let buttons = extras.into_iter().map(|mode| {
                    let bucket = ModeBucket::from_mode(mode);
                    let label = bucket.label();
                    let is_band_inaudible = mode == PlaybackMode::Normal && {
                        let lo = state.filter.band_ff_freq_lo().get_untracked();
                        let hi = state.filter.band_ff_freq_hi().get_untracked();
                        hi > lo && lo >= 20_000.0
                    };
                    let extra_btn_class = Signal::derive(move || {
                        let mut s = String::from("layer-btn play-mode-extra");
                        if state.playback.is_playing().get() && state.playback.mode().get() == mode {
                            s.push_str(" active");
                        }
                        if is_band_inaudible {
                            s.push_str(" inaudible-warning");
                        }
                        s
                    });
                    let title = format!("Play in {} mode", bucket.label());
                    view! {
                        <button class=move || extra_btn_class.get()
                            title=title
                            on:click=move |_: web_sys::MouseEvent| do_play_in_mode(mode)
                        >
                            <span class="combo-btn-text">
                                <span class="layer-btn-category">{label}</span>
                                <span class="layer-btn-value"><Icon kind=Icon::Play /></span>
                            </span>
                        </button>
                    }
                }).collect_view();
                Some(view! { {buttons} })
            }}

            // ── Play combo button ──
            // The wrapping div carries the `.inaudible-warning` class when
            // the current mode is 1:1 and the active band sits entirely
            // above human hearing. CSS uses it to underline the ▶ glyph
            // (same amber underline as the warning on the 1:1 mode radio
            // button), reminding the user that pressing Play right now
            // won't produce anything audible.
            {move || has_file().then(|| view! {
                <div class:inaudible-warning=move || play_inaudible.get() style="display: contents;">
                <ComboButton
                    left_label=""
                    left_label_dyn=play_mode_label
                    left_value=play_left_value
                    left_icon=play_left_icon
                    left_click=play_left_click
                    left_class=play_left_class
                    right_value=play_empty
                    right_class=play_right_class
                    right_label=play_pos_label
                    is_open=play_is_open
                    toggle_menu=play_toggle_menu
                    left_title="Play / Stop"
                    right_title="Play start position"
                    menu_direction="above"
                    panel_style="min-width: 180px;"
                >
                    <button class=move || layer_opt_class(state.playback.start_mode().get() == PlayStartMode::Auto)
                        on:click=move |_| {
                            state.playback.start_mode().set(PlayStartMode::Auto);
                            state.panels.layer_panel_open().set(None);
                        }
                    >"Auto \u{2014} Selected / Here / All"</button>
                    <button class=move || layer_opt_class(state.playback.start_mode().get() == PlayStartMode::All)
                        on:click=move |_| {
                            state.playback.start_mode().set(PlayStartMode::All);
                            state.panels.layer_panel_open().set(None);
                        }
                    >"All \u{2014} Play from start"</button>
                    <button class=move || layer_opt_class(state.playback.start_mode().get() == PlayStartMode::FromHere)
                        on:click=move |_| {
                            state.playback.start_mode().set(PlayStartMode::FromHere);
                            state.panels.layer_panel_open().set(None);
                        }
                    >"From here \u{2014} Current position"</button>
                    <button
                        class=move || {
                            let active = state.playback.start_mode().get() == PlayStartMode::Selected;
                            let _sel = state.interaction.selection().get();
                            let _ann = state.annotations.selected_ids().get();
                            let enabled = playback::effective_selection(&state).is_some();
                            if !enabled {
                                "layer-panel-opt disabled"
                            } else if active {
                                "layer-panel-opt sel"
                            } else {
                                "layer-panel-opt"
                            }
                        }
                        on:click=move |_| {
                            if playback::effective_selection(&state).is_some() {
                                state.playback.start_mode().set(PlayStartMode::Selected);
                                state.panels.layer_panel_open().set(None);
                            }
                        }
                    >"Selected \u{2014} Play selection"</button>
                </ComboButton>
                </div>
            })}

            // ── Channel / Track selector ──
            // Always rendered; greys out and reads "Mono" when there's only one
            // channel and no multitrack timeline (so its slot is reserved and
            // users can see the option exists).
            {
                let is_multi = move || {
                    let files = state.library.files().get();
                    let idx = state.library.current_index().get();
                    let has_stereo = idx.and_then(|i| files.get(i)).map(|f| f.audio.channels).unwrap_or(1) > 1;
                    let has_mt = state.timeline.active().with(|t| {
                        t.as_ref().map(|tv| !tv.multitrack_groups.is_empty()).unwrap_or(false)
                    });
                    has_stereo || has_mt
                };
                view! {
                <div style="position:relative">
                    <button
                        class=move || {
                            if !is_multi() { "layer-btn disabled" }
                            else if state.panels.layer_panel_open().get() == Some(LayerPanel::Channel) { "layer-btn open" }
                            else { "layer-btn" }
                        }
                        on:click=move |_| { if is_multi() { toggle_panel(&state, LayerPanel::Channel); } }
                        title=move || if is_multi() { "Channel / Track view" } else { "Mono file \u{2014} no channel options" }
                    >
                        <span class="layer-btn-category">"Ch"</span>
                        <span class="layer-btn-value">{move || {
                            if !is_multi() { return "Mono".to_string(); }
                            // Show active track label if in timeline mode with multitrack
                            if let Some(ref track) = state.timeline.active_track().get() {
                                return format!("Trk {}", track);
                            }
                            match state.viewmode.channel_view().get() {
                                ChannelView::Stereo => "Stereo".to_string(),
                                ChannelView::MonoMix => "L+R".to_string(),
                                ChannelView::Channel(0) => "L".to_string(),
                                ChannelView::Channel(1) => "R".to_string(),
                                ChannelView::Difference => "L-R".to_string(),
                                ChannelView::Channel(2) => "Ch3".to_string(),
                                ChannelView::Channel(3) => "Ch4".to_string(),
                                ChannelView::Channel(_) => "Ch?".to_string(),
                            }
                        }}</span>
                    </button>
                    <Show when=move || state.panels.layer_panel_open().get() == Some(LayerPanel::Channel)>
                        {
                            let set_ch = move |cv: ChannelView| {
                                move |_: web_sys::MouseEvent| {
                                    state.viewmode.channel_view().set(cv);
                                    state.timeline.active_track().set(None); // Clear track when switching channel
                                    crate::canvas::tile_cache::clear_all_caches();
                                    state.viewmode.tile_ready_signal().update(|n| *n = n.wrapping_add(1));
                                    state.panels.layer_panel_open().set(None);
                                }
                            };

                            // Check if current file is stereo
                            let files = state.library.files().get_untracked();
                            let idx = state.library.current_index().get_untracked();
                            let is_stereo = idx.and_then(|i| files.get(i)).map(|f| f.audio.channels).unwrap_or(1) > 1;

                            // Get multitrack options from active timeline
                            let mt_groups: Vec<crate::timeline::MultitrackOption> = state.timeline.active().with_untracked(|t| {
                                t.as_ref().map(|tv| tv.multitrack_groups.clone()).unwrap_or_default()
                            });

                            view! {
                                <div class="layer-panel" style="bottom: calc(100% + 4px); left: 0; min-width:100px;">
                                    <div class="layer-panel-title">"Channel"</div>
                                    {if is_stereo {
                                        Some(view! {
                                            <button
                                                class=move || layer_opt_class(state.viewmode.channel_view().get() == ChannelView::Stereo && state.timeline.active_track().with(|t| t.is_none()))
                                                on:click=set_ch(ChannelView::Stereo)
                                            >"Stereo"</button>
                                            <button
                                                class=move || layer_opt_class(state.viewmode.channel_view().get() == ChannelView::MonoMix && state.timeline.active_track().with(|t| t.is_none()))
                                                on:click=set_ch(ChannelView::MonoMix)
                                            >"Mono (L+R)"</button>
                                            <button
                                                class=move || layer_opt_class(state.viewmode.channel_view().get() == ChannelView::Channel(0) && state.timeline.active_track().with(|t| t.is_none()))
                                                on:click=set_ch(ChannelView::Channel(0))
                                            >"Left"</button>
                                            <button
                                                class=move || layer_opt_class(state.viewmode.channel_view().get() == ChannelView::Channel(1) && state.timeline.active_track().with(|t| t.is_none()))
                                                on:click=set_ch(ChannelView::Channel(1))
                                            >"Right"</button>
                                            <button
                                                class=move || layer_opt_class(state.viewmode.channel_view().get() == ChannelView::Difference && state.timeline.active_track().with(|t| t.is_none()))
                                                on:click=set_ch(ChannelView::Difference)
                                            >"Diff (L-R)"</button>
                                        })
                                    } else {
                                        None
                                    }}
                                    {if !mt_groups.is_empty() {
                                        let items: Vec<_> = mt_groups.iter().map(|mt| {
                                            let label = mt.label.clone();
                                            let label2 = label.clone();
                                            let label3 = label.clone();
                                            view! {
                                                <button
                                                    class=move || layer_opt_class(
                                                        state.timeline.active_track().with(|t| t.as_deref() == Some(&label3))
                                                    )
                                                    on:click=move |_: web_sys::MouseEvent| {
                                                        state.timeline.active_track().set(Some(label2.clone()));
                                                        crate::canvas::tile_cache::clear_all_caches();
                                                        state.viewmode.tile_ready_signal().update(|n| *n = n.wrapping_add(1));
                                                        state.panels.layer_panel_open().set(None);
                                                    }
                                                >{format!("Track: {}", label)}</button>
                                            }
                                        }).collect();
                                        Some(view! {
                                            <div class="layer-panel-divider"></div>
                                            <div class="layer-panel-title">"Tracks"</div>
                                            {items}
                                        })
                                    } else {
                                        None
                                    }}
                                </div>
                            }
                        }
                    </Show>
                </div>
                }
            }

            <div class="bottom-toolbar-sep"></div>

            // ── Capture group: [Mic ▼] [Record] [Listen] ──
            // The Mic chip on the left bookends Record + Listen. It's
            // visually lighter than Record/Listen — selecting a mic is a
            // prerequisite, but not the primary action.
            //
            // Click opens a panel hosting mic strategy + device + capture
            // format settings (formerly inside the Record dropdown). The
            // panel also has a "Change mic…" button that opens the full
            // chooser modal for picking among multiple devices.
            <div class="capture-group">
                // ── Mic select chip with dropdown ──
                {
                    let mic_is_open = Signal::derive(move || state.panels.layer_panel_open().get() == Some(LayerPanel::Mic));
                    // LED state: green when a mic is connected and ready to use.
                    // Red dot is rendered separately while recording (CSS pulses).
                    let led_class = Signal::derive(move || {
                        if state.mic.strategy().get() == MicStrategy::None {
                            "mic-led off"
                        } else if state.mic.recording().get() {
                            "mic-led rec"
                        } else if state.mic.listening().get() {
                            "mic-led listening"
                        } else if state.mic.backend().get().is_some() || state.mic.device_info().get().is_some() {
                            "mic-led ready"
                        } else {
                            "mic-led idle"
                        }
                    });
                    // The mic chip never adopts the green "active" tint —
                    // that would compete with Record/Listen for attention.
                    // Listening/recording state is conveyed by the LED dot
                    // (next to the mic icon), which animates on its own.
                    //
                    // "Browser default" counts as a chosen mic (the user
                    // explicitly picked Browser strategy), so it gets the
                    // low-key capsule rather than the amber "empty" nudge.
                    let mic_left_class = Signal::derive(move || {
                        let strat = state.mic.strategy().get();
                        if strat == MicStrategy::None {
                            "layer-btn combo-btn-left mic-select-btn mic-off"
                        } else if state.mic.device_info().get().is_some() || strat == MicStrategy::Browser {
                            "layer-btn combo-btn-left mic-select-btn mic-chosen"
                        } else {
                            "layer-btn combo-btn-left mic-select-btn mic-empty"
                        }
                    });
                    let mic_right_class = Signal::derive(move || {
                        if mic_is_open.get() {
                            "layer-btn combo-btn-right mic-options-btn open"
                        } else if state.mic.listening().get() || state.mic.recording().get() {
                            "layer-btn combo-btn-right mic-options-btn active"
                        } else {
                            "layer-btn combo-btn-right mic-options-btn"
                        }
                    });
                    let mic_value = Signal::derive(move || {
                        match state.mic.strategy().get() {
                            MicStrategy::None => "Off".to_string(),
                            MicStrategy::Browser if state.mic.device_info().get().is_none() => {
                                "Browser default".to_string()
                            }
                            _ => {
                                if let Some(info) = state.mic.device_info().get() {
                                    info.name.clone()
                                } else {
                                    "No mic selected".to_string()
                                }
                            }
                        }
                    });
                    let mic_title = Signal::derive(move || {
                        if state.mic.strategy().get() == MicStrategy::None {
                            "Mic input is disabled. Click for options.".to_string()
                        } else if let Some(info) = state.mic.device_info().get() {
                            let rate = info.supported_rates.iter().max().copied().unwrap_or(0);
                            let rate_str = if rate >= 1000 { format!(" \u{2014} up to {} kHz", rate / 1000) } else { String::new() };
                            format!("Microphone: {}{}", info.name, rate_str)
                        } else {
                            "Choose a microphone and capture settings.".to_string()
                        }
                    });
                    // Combo layout:
                    //   Left half  = the mic identity chip (icon + LED + name).
                    //                Click opens the full chooser modal so the
                    //                user can pick a specific device.
                    //   Right half = caret. Click toggles the options popup
                    //                (strategy + capture format).
                    view! {
                        <div class=move || if mic_is_open.get() { "combo-btn-row mic-combo-row open" } else { "combo-btn-row mic-combo-row" } style="position:relative">
                            <button
                                class=move || mic_left_class.get()
                                title=move || mic_title.get()
                                on:click=move |ev: web_sys::MouseEvent| {
                                    ev.stop_propagation();
                                    // Close the options popup if it happens to be open,
                                    // then surface the chooser modal.
                                    state.panels.layer_panel_open().set(None);
                                    state.mic.pending_action().set(None);
                                    state.mic.show_chooser().set(true);
                                }
                            >
                                <svg class="mic-icon" viewBox="0 0 24 24" aria-hidden="true" fill="currentColor">
                                    <path d="M12 2a3 3 0 0 0-3 3v6a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3zm5 9a1 1 0 0 0-2 0 3 3 0 0 1-6 0 1 1 0 0 0-2 0 5 5 0 0 0 4 4.9V18H8a1 1 0 0 0 0 2h8a1 1 0 0 0 0-2h-3v-2.1A5 5 0 0 0 17 11z"/>
                                </svg>
                                <span class=move || led_class.get() aria-hidden="true"></span>
                                <span class="layer-btn-value fit-text" data-fit-max="13" data-fit-min="9">{move || mic_value.get()}</span>
                            </button>
                            <button
                                class=move || mic_right_class.get()
                                title="Microphone options (strategy, capture format)"
                                on:click=move |ev: web_sys::MouseEvent| {
                                    ev.stop_propagation();
                                    toggle_panel(&state, LayerPanel::Mic);
                                }
                            >
                                <span class="combo-btn-arrow">{"\u{25E2}"}</span>
                            </button>
                            // "New USB mic detected" badge — a glowing dot overhanging the
                            // top-LEFT border, over the mic-select (left) button the user
                            // taps to switch. Sits at the row level (not inside
                            // .combo-btn-left, which clips on mobile); pointer-events:none
                            // so a tap still falls through to the button.
                            <span
                                class="mic-new-badge"
                                title="New USB mic detected — tap to switch"
                                aria-hidden="true"
                                style=move || if state.mic.new_device_available().get() {
                                    "position:absolute; top:-5px; left:2px; width:9px; height:9px; \
                                     border-radius:50%; background:#ff9f1c; \
                                     box-shadow:0 0 6px #ff9f1c, 0 0 0 1px rgba(0,0,0,0.6); \
                                     pointer-events:none; z-index:30;"
                                } else {
                                    "display:none;"
                                }
                            ></span>
                            <Show when=move || mic_is_open.get()>
                                <div class="layer-panel" style="bottom: calc(100% + 4px); left: 0; min-width: 260px;">
                                    // ── Microphone strategy ──
                                    <div class="layer-panel-title">"Microphone"</div>
                                    <div style="display: flex; gap: 2px; padding: 0 6px 4px;">
                                        <button class=move || layer_opt_class(state.mic.strategy().get() == MicStrategy::Ask)
                                            on:click=move |_| {
                                                state.mic.strategy().set(MicStrategy::Ask);
                                                state.mic.backend().set(None);
                                                state.mic.device_info().set(None);
                                                state.mic.selected_device().set(None);
                                            }
                                        >"Ask"</button>
                                        <button class=move || {
                                            if state.mic.strategy().get() == MicStrategy::Selected {
                                                layer_opt_class(true)
                                            } else {
                                                "layer-panel-opt disabled"
                                            }
                                        }
                                            title="Active when a mic has been selected"
                                        >"Selected"</button>
                                        <button class=move || {
                                            if state.is_tauri {
                                                "layer-panel-opt disabled"
                                            } else {
                                                layer_opt_class(state.mic.strategy().get() == MicStrategy::Browser)
                                            }
                                        }
                                            on:click=move |_| {
                                                if !state.is_tauri {
                                                    state.mic.strategy().set(MicStrategy::Browser);
                                                }
                                            }
                                            title=move || if state.is_tauri { "Not available on desktop/mobile" } else { "Use browser Web Audio API" }
                                        >"Browser"</button>
                                        <button class=move || layer_opt_class(state.mic.strategy().get() == MicStrategy::None)
                                            on:click=move |_| state.mic.strategy().set(MicStrategy::None)
                                        >"None"</button>
                                    </div>

                                    // Selected device info + change button
                                    <Show when=move || matches!(state.mic.strategy().get(), MicStrategy::Ask | MicStrategy::Selected)>
                                        <div style="padding: 2px 8px;">
                                            {move || {
                                                let info = state.mic.device_info().get();
                                                match info {
                                                    Some(info) => {
                                                        let rate_str = if !info.supported_rates.is_empty() {
                                                            let max = info.supported_rates.iter().max().copied().unwrap_or(0);
                                                            if max >= 1000 { format!("{}k", max / 1000) } else { format!("{}", max) }
                                                        } else { "?".to_string() };
                                                        let bits_str = if !info.supported_bit_depths.is_empty() {
                                                            let max = info.supported_bit_depths.iter().max().copied().unwrap_or(0);
                                                            format!("{}-bit", max)
                                                        } else { String::new() };
                                                        view! {
                                                            <div style="font-size: 11px; color: #ccc; margin-bottom: 4px;">
                                                                <span style="color: #fff;">{info.name.clone()}</span>
                                                                <br />
                                                                <span style="color: #888;">{format!("{} {} {}", info.connection_type, rate_str, bits_str)}</span>
                                                            </div>
                                                        }.into_any()
                                                    }
                                                    None => view! {
                                                        <div style="font-size: 11px; color: #888; margin-bottom: 4px;">"No mic selected"</div>
                                                    }.into_any()
                                                }
                                            }}
                                        </div>
                                    </Show>

                                    // ── Capture format ──
                                    <hr />
                                    <div class="layer-panel-title">"Capture format"</div>
                                    <div style="padding: 2px 8px;">
                                        <div class="layer-panel-slider-row het-text-row">
                                            <label style="font-size: 11px;">"Max sample rate"</label>
                                            <select style="font-size: 11px; background: #333; color: #ccc; border: 1px solid #555; padding: 1px 2px;"
                                                on:change=move |ev| {
                                                    if let Ok(val) = leptos::prelude::event_target_value(&ev).parse::<u32>() {
                                                        state.mic.max_sample_rate().set(val);
                                                    }
                                                }
                                            >
                                                <option value="0" selected=move || state.mic.max_sample_rate().get() == 0>"Auto"</option>
                                                <option value="44100" selected=move || state.mic.max_sample_rate().get() == 44100>"44.1k"</option>
                                                <option value="48000" selected=move || state.mic.max_sample_rate().get() == 48000>"48k"</option>
                                                <option value="96000" selected=move || state.mic.max_sample_rate().get() == 96000>"96k"</option>
                                                <option value="192000" selected=move || state.mic.max_sample_rate().get() == 192000>"192k"</option>
                                                <option value="256000" selected=move || state.mic.max_sample_rate().get() == 256000>"256k"</option>
                                                <option value="384000" selected=move || state.mic.max_sample_rate().get() == 384000>"384k"</option>
                                                <option value="500000" selected=move || state.mic.max_sample_rate().get() == 500000>"500k"</option>
                                            </select>
                                        </div>
                                        <div class="layer-panel-slider-row het-text-row">
                                            <label style="font-size: 11px;">"Max bit depth"</label>
                                            <select style="font-size: 11px; background: #333; color: #ccc; border: 1px solid #555; padding: 1px 2px;"
                                                on:change=move |ev| {
                                                    if let Ok(val) = leptos::prelude::event_target_value(&ev).parse::<u16>() {
                                                        state.mic.max_bit_depth().set(val);
                                                    }
                                                }
                                            >
                                                <option value="0" selected=move || state.mic.max_bit_depth().get() == 0>"Auto"</option>
                                                <option value="16" selected=move || state.mic.max_bit_depth().get() == 16>"16-bit"</option>
                                                <option value="24" selected=move || state.mic.max_bit_depth().get() == 24>"24-bit"</option>
                                                <option value="32" selected=move || state.mic.max_bit_depth().get() == 32>"32-bit"</option>
                                            </select>
                                        </div>
                                        <div class="layer-panel-slider-row het-text-row">
                                            <label style="font-size: 11px;">"Channels"</label>
                                            <select style="font-size: 11px; background: #333; color: #ccc; border: 1px solid #555; padding: 1px 2px;"
                                                on:change=move |ev| {
                                                    let val = leptos::prelude::event_target_value(&ev);
                                                    state.mic.channel_mode().set(if val == "stereo" { ChannelMode::Stereo } else { ChannelMode::Mono });
                                                }
                                            >
                                                <option value="mono" selected=move || state.mic.channel_mode().get() == ChannelMode::Mono>"Mono"</option>
                                                <option value="stereo" selected=move || state.mic.channel_mode().get() == ChannelMode::Stereo>"Stereo"</option>
                                            </select>
                                        </div>
                                        <div class="layer-panel-slider-row het-text-row">
                                            <label style="font-size: 11px;">"Pre-roll buffer"</label>
                                            <select style="font-size: 11px; background: #333; color: #ccc; border: 1px solid #555; padding: 1px 2px;"
                                                on:change=move |ev| {
                                                    if let Ok(val) = leptos::prelude::event_target_value(&ev).parse::<u32>() {
                                                        state.mic.preroll_buffer_secs().set(val);
                                                    }
                                                }
                                            >
                                                <option value="2" selected=move || state.mic.preroll_buffer_secs().get() == 2>"2s"</option>
                                                <option value="5" selected=move || state.mic.preroll_buffer_secs().get() == 5>"5s"</option>
                                                <option value="10" selected=move || state.mic.preroll_buffer_secs().get() == 10>"10s"</option>
                                                <option value="15" selected=move || state.mic.preroll_buffer_secs().get() == 15>"15s"</option>
                                                <option value="20" selected=move || state.mic.preroll_buffer_secs().get() == 20>"20s"</option>
                                                <option value="30" selected=move || state.mic.preroll_buffer_secs().get() == 30>"30s"</option>
                                            </select>
                                        </div>
                                    </div>
                                </div>
                            </Show>
                        </div>
                    }
                }

                // ── Record combo button ──
                <ComboButton
                    left_label=""
                    left_value=rec_left_value
                    left_click=rec_left_click
                    left_long_press=rec_long_press
                    left_class=rec_left_class
                    right_value=rec_right_value
                    right_class=rec_right_class
                    is_open=rec_is_open
                    toggle_menu=rec_toggle_menu
                    left_title="Record"
                    right_title="Record settings"
                    menu_direction="above"
                    panel_style="min-width: 280px;"
                >
                // ── Record mode ──
                <div class="layer-panel-title">"Record mode"</div>
                <button
                    class=move || {
                        let active = state.playback.record_mode().get() == RecordMode::ToFile;
                        if !state.is_tauri {
                            "layer-panel-opt disabled"
                        } else if active {
                            "layer-panel-opt sel"
                        } else {
                            "layer-panel-opt"
                        }
                    }
                    on:click=move |_| {
                        if state.is_tauri {
                            state.playback.record_mode().set(RecordMode::ToFile);
                        }
                    }
                >"To file"</button>
                <button class=move || layer_opt_class(state.playback.record_mode().get() == RecordMode::ToMemory)
                    on:click=move |_| {
                        state.playback.record_mode().set(RecordMode::ToMemory);
                    }
                >"To memory"</button>
                <button class=move || layer_opt_class(state.playback.record_mode().get() == RecordMode::ListenOnly)
                    on:click=move |_| {
                        // If currently recording, finish and switch to listening
                        if state.mic.recording().get_untracked() {
                            let st = state;
                            wasm_bindgen_futures::spawn_local(async move {
                                microphone::toggle_record(&st).await; // stops recording
                                microphone::toggle_listen(&st).await; // starts listening
                            });
                        }
                        state.playback.record_mode().set(RecordMode::ListenOnly);
                    }
                >"Listen only"</button>
                <hr />
                <div class="layer-panel-hint" style="padding: 4px 8px; font-size: 11px; opacity: 0.65;">
                    "Mic device + capture format live in the Mic button to the left."
                </div>
            </ComboButton>

                // ── Listen combo button (moved from hearing bar) ──
                <ListenButton/>
            </div>
        </div>
    }
}
