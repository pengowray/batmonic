use leptos::prelude::*;
use wasm_bindgen::prelude::*;
use crate::state::{ActiveFocus, AppState, CanvasTool, ChannelMode, GainMode, LayerPanel, ListenMode, MicAcquisitionState, MicStrategy, PeakSource, PlaybackMode, PlayStartMode, RecordMode, RecordReadyState};
use crate::audio::{microphone, playback};
use crate::audio::streaming_playback::PV_MODE_BOOST_DB;
use crate::audio::source::ChannelView;
use crate::components::hfr_button::HfrButton;
use crate::components::combo_button::ComboButton;

fn layer_opt_class(active: bool) -> &'static str {
    if active { "layer-panel-opt sel" } else { "layer-panel-opt" }
}

fn toggle_panel(state: &AppState, panel: LayerPanel) {
    state.layer_panel_open.update(|p| {
        *p = if *p == Some(panel) { None } else { Some(panel) };
    });
}

#[component]
pub fn BottomToolbar() -> impl IntoView {
    let state = expect_context::<AppState>();
    // Always show all buttons; use has_file/is_file_disabled for enable/disable logic
    let has_file = move || true;
    let is_file_disabled = move || {
        state.current_file_index.get().is_none() && state.active_timeline.get().is_none()
    };

    // ── Recording timer ──
    let interval_id: StoredValue<Option<i32>> = StoredValue::new(None);
    Effect::new(move |_| {
        let recording = state.mic_recording.get();
        if recording {
            let cb = Closure::<dyn FnMut()>::new(move || {
                state.mic_timer_tick.update(|n| *n = n.wrapping_add(1));
            });
            if let Some(window) = web_sys::window() {
                if let Ok(id) = window.set_interval_with_callback_and_timeout_and_arguments_0(
                    cb.as_ref().unchecked_ref(), 100,
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
    let play_is_open = Signal::derive(move || state.layer_panel_open.get() == Some(LayerPanel::PlayMode));

    let play_left_class = Signal::derive(move || {
        let no_file = state.current_file_index.get().is_none() && state.active_timeline.get().is_none();
        let recording_and_listening = state.mic_recording.get() && state.mic_listening.get();
        if no_file || recording_and_listening {
            "layer-btn combo-btn-left disabled"
        } else if state.is_playing.get() {
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

    let play_left_value = Signal::derive(move || "\u{25B6}".to_string()); // ▶
    let play_right_label = Signal::derive(move || {
        match state.play_start_mode.get() {
            PlayStartMode::Auto => "Auto".to_string(),
            _ => String::new(),
        }
    });
    let play_right_frozen: StoredValue<Option<String>> = StoredValue::new(None);
    let play_right_value = Signal::derive(move || {
        // Freeze the label while playing so scrolling doesn't flicker it
        if state.is_playing.get() {
            if let Some(frozen) = play_right_frozen.get_value() {
                return frozen;
            }
        }
        let val = match state.play_start_mode.get() {
            PlayStartMode::All => "All".to_string(),
            PlayStartMode::FromHere => "Here".to_string(),
            PlayStartMode::Selected => "Sel".to_string(),
            PlayStartMode::Auto => {
                // Subscribe to signals that affect auto-play mode for reactivity
                let _sel = state.selection.get();
                let _ann = state.selected_annotation_ids.get();
                let _scroll = state.scroll_offset.get();
                let _zoom = state.zoom_level.get();
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

    let play_left_click = Callback::new(move |_: web_sys::MouseEvent| {
        let no_file = state.current_file_index.get_untracked().is_none() && state.active_timeline.get_untracked().is_none();
        let recording_and_listening = state.mic_recording.get_untracked() && state.mic_listening.get_untracked();
        if no_file || recording_and_listening { return; }
        if state.is_playing.get_untracked() {
            playback::stop(&state);
        } else {
            match state.play_start_mode.get_untracked() {
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
                        } else if state.scroll_offset.get_untracked() <= 0.0 {
                            playback::play_from_start(&state);
                        } else {
                            playback::play_from_here(&state);
                        }
                    } else if state.scroll_offset.get_untracked() <= 0.0 {
                        playback::play_from_start(&state);
                    } else {
                        playback::play_from_here(&state);
                    }
                }
            }
        }
    });
    let play_toggle_menu = Callback::new(move |()| {
        toggle_panel(&state, LayerPanel::PlayMode);
    });

    // ── Record ComboButton setup ──
    let rec_is_open = Signal::derive(move || state.layer_panel_open.get() == Some(LayerPanel::RecordMode));

    let rec_left_class = Signal::derive(move || {
        if state.mic_recording.get() {
            "layer-btn combo-btn-left rec-btn mic-recording"
        } else if state.record_mode.get() == RecordMode::ListenOnly || state.mic_strategy.get() == MicStrategy::None {
            "layer-btn combo-btn-left rec-btn disabled"
        } else if state.mic_strategy.get() == MicStrategy::Ask && state.mic_backend.get().is_none() {
            "layer-btn combo-btn-left rec-btn mic-ask"
        } else {
            "layer-btn combo-btn-left rec-btn"
        }
    });
    let rec_right_class = Signal::derive(move || {
        if rec_is_open.get() { "layer-btn combo-btn-right open" } else { "layer-btn combo-btn-right" }
    });

    let rec_left_value = Signal::derive(move || {
        if state.mic_recording.get() {
            let _ = state.mic_timer_tick.get();
            let start = state.mic_recording_start_time.get_untracked().unwrap_or(0.0);
            let now = js_sys::Date::now();
            let secs = (now - start) / 1000.0;
            format!("Rec {}", crate::format_time::format_duration_compact(secs))
        } else if state.mic_strategy.get() == MicStrategy::Ask && state.mic_backend.get().is_none() {
            "?".to_string() // ? — pressing record will prompt for mic selection
        } else {
            String::new() // CSS ::before renders the red dot
        }
    });
    let rec_right_value = Signal::derive(move || {
        match state.record_mode.get() {
            RecordMode::ToFile => "File".to_string(),
            RecordMode::ToMemory => "Mem".to_string(),
            RecordMode::ListenOnly => "Listen".to_string(),
        }
    });

    let rec_left_click = Callback::new(move |_: web_sys::MouseEvent| {
        if state.record_mode.get_untracked() == RecordMode::ListenOnly
            || state.mic_strategy.get_untracked() == MicStrategy::None {
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
        if state.mic_strategy.get_untracked() == MicStrategy::None {
            return;
        }
        // Only meaningful when currently listening
        if !state.mic_listening.get_untracked() {
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
        if !drag_active.get_value() { return; }
        ev.prevent_default();
        let delta = drag_start_y.get_value() - ev.client_y() as f64; // dragging up = positive
        let new_height = (drag_start_height.get_value() + delta).clamp(48.0, 400.0);
        max_height.set(Some(new_height));
    };

    let on_handle_pointerup = move |ev: web_sys::PointerEvent| {
        if !drag_active.get_value() { return; }
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
        <div class=move || if state.is_mobile.get() { "bottom-toolbar mobile" } else { "bottom-toolbar" }
            class:panel-open=move || state.layer_panel_open.get().is_some()
            node_ref=toolbar_node
            style=move || {
                // Inline z-index is the load-bearing lift: keeps combo dropdowns
                // above .main-overlays (z-index: 20) immediately and across all
                // views, regardless of :has() invalidation quirks or Leptos
                // class-diff timing. Matches the CSS-backup rule at line ~1793.
                let mut s = if state.layer_panel_open.get().is_some() {
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

            // ── Play combo button ──
            {move || has_file().then(|| view! {
                <ComboButton
                    left_label=""
                    left_value=play_left_value
                    left_click=play_left_click
                    left_class=play_left_class
                    right_value=play_right_value
                    right_class=play_right_class
                    right_label=play_right_label
                    is_open=play_is_open
                    toggle_menu=play_toggle_menu
                    left_title="Play / Stop"
                    right_title="Play mode"
                    menu_direction="above"
                    panel_style="min-width: 180px;"
                >
                    <button class=move || layer_opt_class(state.play_start_mode.get() == PlayStartMode::Auto)
                        on:click=move |_| {
                            state.play_start_mode.set(PlayStartMode::Auto);
                            state.layer_panel_open.set(None);
                        }
                    >"Auto \u{2014} Selected / Here / All"</button>
                    <button class=move || layer_opt_class(state.play_start_mode.get() == PlayStartMode::All)
                        on:click=move |_| {
                            state.play_start_mode.set(PlayStartMode::All);
                            state.layer_panel_open.set(None);
                        }
                    >"All \u{2014} Play from start"</button>
                    <button class=move || layer_opt_class(state.play_start_mode.get() == PlayStartMode::FromHere)
                        on:click=move |_| {
                            state.play_start_mode.set(PlayStartMode::FromHere);
                            state.layer_panel_open.set(None);
                        }
                    >"From here \u{2014} Current position"</button>
                    <button
                        class=move || {
                            let active = state.play_start_mode.get() == PlayStartMode::Selected;
                            let _sel = state.selection.get();
                            let _ann = state.selected_annotation_ids.get();
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
                                state.play_start_mode.set(PlayStartMode::Selected);
                                state.layer_panel_open.set(None);
                            }
                        }
                    >"Selected \u{2014} Play selection"</button>
                </ComboButton>
            })}

            // ── Gain combo button ──
            {move || has_file().then(|| {
                let gain_is_open = Signal::derive(move || state.layer_panel_open.get() == Some(LayerPanel::Gain));

                let gain_left_class = Signal::derive(move || {
                    if is_file_disabled() {
                        "layer-btn combo-btn-left disabled"
                    } else if state.gain_mode.get() != GainMode::Off {
                        "layer-btn combo-btn-left active"
                    } else {
                        "layer-btn combo-btn-left no-annotation"
                    }
                });
                let gain_right_class = Signal::derive(move || {
                    if is_file_disabled() { return "layer-btn combo-btn-right disabled"; }
                    let dim = if state.gain_mode.get() == GainMode::Off { " dim" } else { "" };
                    if gain_is_open.get() {
                        if dim.is_empty() { "layer-btn combo-btn-right open" } else { "layer-btn combo-btn-right dim open" }
                    } else if dim.is_empty() { "layer-btn combo-btn-right" } else { "layer-btn combo-btn-right dim" }
                });

                let gain_left_value = Signal::derive(move || {
                    let mode = state.gain_mode.get();
                    let manual_db = state.gain_db.get();
                    let pv_boost = if state.playback_mode.get() == PlaybackMode::PhaseVocoder { PV_MODE_BOOST_DB } else { 0.0 };
                    match mode {
                        GainMode::Off => {
                            if pv_boost > 0.0 { format!("+{:.0}dB", pv_boost) }
                            else { String::new() }
                        }
                        GainMode::Manual => {
                            let total = manual_db + pv_boost;
                            if total > 0.0 { format!("+{:.0}dB", total) }
                            else { format!("{:.0}dB", total) }
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
                let gain_right_value = Signal::derive(move || {
                    match state.gain_mode.get() {
                        GainMode::Off => "OFF".to_string(),
                        mode => mode.label().to_string(),
                    }
                });

                let gain_left_click = Callback::new(move |_: web_sys::MouseEvent| {
                    let mode = state.gain_mode.get_untracked();
                    if mode == GainMode::Off {
                        // Turn on: restore last auto mode
                        let last = state.gain_mode_last_auto.get_untracked();
                        state.gain_mode.set(last);
                        state.auto_gain.set(last.is_auto());
                    } else {
                        // Turn off: remember current mode
                        if mode.is_auto() {
                            state.gain_mode_last_auto.set(mode);
                        }
                        state.gain_mode.set(GainMode::Off);
                        state.auto_gain.set(false);
                    }
                });
                let gain_toggle_menu = Callback::new(move |()| {
                    toggle_panel(&state, LayerPanel::Gain);
                });

                view! {
                    <ComboButton
                        left_label="Gain"
                        left_value=gain_left_value
                        left_click=gain_left_click
                        left_class=gain_left_class
                        right_value=gain_right_value
                        right_class=gain_right_class
                        is_open=gain_is_open
                        toggle_menu=gain_toggle_menu
                        left_title="Toggle gain"
                        right_title="Gain mode"
                        menu_direction="above"
                        panel_style="min-width: 210px;"
                    >
                        <button class=move || layer_opt_class(state.gain_mode.get() == GainMode::Off)
                            on:click=move |_| {
                                state.gain_mode.set(GainMode::Off);
                                state.auto_gain.set(false);
                                state.layer_panel_open.set(None);
                            }
                        >"Off"</button>
                        <button class=move || layer_opt_class(state.gain_mode.get() == GainMode::Manual)
                            on:click=move |_| {
                                state.gain_mode.set(GainMode::Manual);
                                state.auto_gain.set(false);
                                state.layer_panel_open.set(None);
                            }
                        >"Manual \u{2014} Slider boost only"</button>
                        <button class=move || layer_opt_class(state.gain_mode.get() == GainMode::AutoPeak)
                            on:click=move |_| {
                                state.gain_mode.set(GainMode::AutoPeak);
                                state.gain_mode_last_auto.set(GainMode::AutoPeak);
                                state.auto_gain.set(true);
                                state.layer_panel_open.set(None);
                            }
                        >"Peak \u{2014} Normalize to peak"</button>
                        <button class=move || layer_opt_class(state.gain_mode.get() == GainMode::Adaptive)
                            on:click=move |_| {
                                state.gain_mode.set(GainMode::Adaptive);
                                state.gain_mode_last_auto.set(GainMode::Adaptive);
                                state.auto_gain.set(true);
                                state.layer_panel_open.set(None);
                            }
                        >"AGC \u{2014} Automatic gain control"</button>
                        <Show when=move || state.gain_mode.get() == GainMode::AutoPeak>
                            <div class="peak-source-row">
                                <span class="peak-source-label">"Peak from:"</span>
                                <button class=move || if state.peak_source.get() == PeakSource::First30s { "peak-src-btn sel" } else { "peak-src-btn" }
                                    on:click=move |_| state.peak_source.set(PeakSource::First30s)
                                    title="Peak from first 30 seconds"
                                >"30s"</button>
                                <button class=move || if state.peak_source.get() == PeakSource::FullWave { "peak-src-btn sel" } else { "peak-src-btn" }
                                    on:click=move |_| state.peak_source.set(PeakSource::FullWave)
                                    title="Peak from entire file"
                                >"Full"</button>
                                <button class=move || {
                                    let base = if state.peak_source.get() == PeakSource::Selection { "peak-src-btn sel" } else { "peak-src-btn" };
                                    if state.selection.get().is_none() { format!("{} disabled", base) } else { base.to_string() }
                                }
                                    on:click=move |_| {
                                        if state.selection.get_untracked().is_some() {
                                            state.peak_source.set(PeakSource::Selection);
                                        }
                                    }
                                    title="Peak from current selection"
                                >"Sel"</button>
                                <button class=move || if state.peak_source.get() == PeakSource::Processed { "peak-src-btn sel" } else { "peak-src-btn" }
                                    on:click=move |_| state.peak_source.set(PeakSource::Processed)
                                    title="Peak after DSP processing"
                                >"DSP"</button>
                            </div>
                        </Show>
                        <div class="layer-panel-slider-row" style="margin-top: 6px;">
                            <span class="slider-label">"Boost"</span>
                            <label>{move || {
                                let db = state.gain_db.get();
                                let pv = if state.playback_mode.get() == PlaybackMode::PhaseVocoder { PV_MODE_BOOST_DB } else { 0.0 };
                                let total = db + pv;
                                if total > 0.0 { format!("+{:.0}dB", total) }
                                else { format!("{:.0}dB", total) }
                            }}</label>
                            <input type="range" min="-12" max="60" step="1"
                                prop:value=move || state.gain_db.get().to_string()
                                on:input=move |ev| {
                                    let val: f64 = event_target_value(&ev).parse().unwrap_or(0.0);
                                    state.gain_db.set(val);
                                    // If currently Off, switch to Manual when slider is adjusted
                                    if state.gain_mode.get_untracked() == GainMode::Off && val > 0.0 {
                                        state.gain_mode.set(GainMode::Manual);
                                    }
                                }
                                on:dblclick=move |_| {
                                    state.gain_db.set(0.0);
                                }
                            />
                        </div>
                    </ComboButton>
                }
            })}

            // ── Channel / Track selector (stereo+ or timeline multitracks) ──
            <Show when=move || {
                let files = state.files.get();
                let idx = state.current_file_index.get();
                let has_stereo = idx.and_then(|i| files.get(i)).map(|f| f.audio.channels).unwrap_or(1) > 1;
                let has_mt = state.active_timeline.with(|t| {
                    t.as_ref().map(|tv| !tv.multitrack_groups.is_empty()).unwrap_or(false)
                });
                has_stereo || has_mt
            }>
                <div style="position:relative">
                    <button
                        class=move || if state.layer_panel_open.get() == Some(LayerPanel::Channel) { "layer-btn open" } else { "layer-btn" }
                        on:click=move |_| toggle_panel(&state, LayerPanel::Channel)
                        title="Channel / Track view"
                    >
                        <span class="layer-btn-category">"Ch"</span>
                        <span class="layer-btn-value">{move || {
                            // Show active track label if in timeline mode with multitrack
                            if let Some(ref track) = state.active_timeline_track.get() {
                                return format!("Trk {}", track);
                            }
                            match state.channel_view.get() {
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
                    <Show when=move || state.layer_panel_open.get() == Some(LayerPanel::Channel)>
                        {
                            let set_ch = move |cv: ChannelView| {
                                move |_: web_sys::MouseEvent| {
                                    state.channel_view.set(cv);
                                    state.active_timeline_track.set(None); // Clear track when switching channel
                                    crate::canvas::tile_cache::clear_all_caches();
                                    state.tile_ready_signal.update(|n| *n = n.wrapping_add(1));
                                    state.layer_panel_open.set(None);
                                }
                            };

                            // Check if current file is stereo
                            let files = state.files.get_untracked();
                            let idx = state.current_file_index.get_untracked();
                            let is_stereo = idx.and_then(|i| files.get(i)).map(|f| f.audio.channels).unwrap_or(1) > 1;

                            // Get multitrack options from active timeline
                            let mt_groups: Vec<crate::timeline::MultitrackOption> = state.active_timeline.with_untracked(|t| {
                                t.as_ref().map(|tv| tv.multitrack_groups.clone()).unwrap_or_default()
                            });

                            view! {
                                <div class="layer-panel" style="bottom: calc(100% + 4px); left: 0; min-width:100px;">
                                    <div class="layer-panel-title">"Channel"</div>
                                    {if is_stereo {
                                        Some(view! {
                                            <button
                                                class=move || layer_opt_class(state.channel_view.get() == ChannelView::Stereo && state.active_timeline_track.with(|t| t.is_none()))
                                                on:click=set_ch(ChannelView::Stereo)
                                            >"Stereo"</button>
                                            <button
                                                class=move || layer_opt_class(state.channel_view.get() == ChannelView::MonoMix && state.active_timeline_track.with(|t| t.is_none()))
                                                on:click=set_ch(ChannelView::MonoMix)
                                            >"Mono (L+R)"</button>
                                            <button
                                                class=move || layer_opt_class(state.channel_view.get() == ChannelView::Channel(0) && state.active_timeline_track.with(|t| t.is_none()))
                                                on:click=set_ch(ChannelView::Channel(0))
                                            >"Left"</button>
                                            <button
                                                class=move || layer_opt_class(state.channel_view.get() == ChannelView::Channel(1) && state.active_timeline_track.with(|t| t.is_none()))
                                                on:click=set_ch(ChannelView::Channel(1))
                                            >"Right"</button>
                                            <button
                                                class=move || layer_opt_class(state.channel_view.get() == ChannelView::Difference && state.active_timeline_track.with(|t| t.is_none()))
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
                                                        state.active_timeline_track.with(|t| t.as_deref() == Some(&label3))
                                                    )
                                                    on:click=move |_: web_sys::MouseEvent| {
                                                        state.active_timeline_track.set(Some(label2.clone()));
                                                        crate::canvas::tile_cache::clear_all_caches();
                                                        state.tile_ready_signal.update(|n| *n = n.wrapping_add(1));
                                                        state.layer_panel_open.set(None);
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
            </Show>

            <div class="bottom-toolbar-sep"></div>

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
                        let active = state.record_mode.get() == RecordMode::ToFile;
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
                            state.record_mode.set(RecordMode::ToFile);
                        }
                    }
                >"To file"</button>
                <button class=move || layer_opt_class(state.record_mode.get() == RecordMode::ToMemory)
                    on:click=move |_| {
                        state.record_mode.set(RecordMode::ToMemory);
                    }
                >"To memory"</button>
                <button class=move || layer_opt_class(state.record_mode.get() == RecordMode::ListenOnly)
                    on:click=move |_| {
                        // If currently recording, finish and switch to listening
                        if state.mic_recording.get_untracked() {
                            let st = state;
                            wasm_bindgen_futures::spawn_local(async move {
                                microphone::toggle_record(&st).await; // stops recording
                                microphone::toggle_listen(&st).await; // starts listening
                            });
                        }
                        state.record_mode.set(RecordMode::ListenOnly);
                    }
                >"Listen only"</button>

                // ── Microphone ──
                <hr />
                <div class="layer-panel-title">"Microphone"</div>
                <div style="display: flex; gap: 2px; padding: 0 6px 4px;">
                    <button class=move || layer_opt_class(state.mic_strategy.get() == MicStrategy::Ask)
                        on:click=move |_| {
                            state.mic_strategy.set(MicStrategy::Ask);
                            // Clear previous selection so user gets asked again
                            state.mic_backend.set(None);
                            state.mic_device_info.set(None);
                            state.mic_selected_device.set(None);
                        }
                    >"Ask"</button>
                    <button class=move || {
                        if state.mic_strategy.get() == MicStrategy::Selected {
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
                            layer_opt_class(state.mic_strategy.get() == MicStrategy::Browser)
                        }
                    }
                        on:click=move |_| {
                            if !state.is_tauri {
                                state.mic_strategy.set(MicStrategy::Browser);
                            }
                        }
                        title=move || if state.is_tauri { "Not available on desktop/mobile" } else { "Use browser Web Audio API" }
                    >"Browser"</button>
                    <button class=move || layer_opt_class(state.mic_strategy.get() == MicStrategy::None)
                        on:click=move |_| state.mic_strategy.set(MicStrategy::None)
                    >"None"</button>
                </div>

                // Selected device info + change button
                <Show when=move || matches!(state.mic_strategy.get(), MicStrategy::Ask | MicStrategy::Selected)>
                    <div style="padding: 2px 8px;">
                        {move || {
                            let info = state.mic_device_info.get();
                            match info {
                                Some(info) => {
                                    let rate_str = if !info.supported_rates.is_empty() {
                                        let max = info.supported_rates.iter().max().copied().unwrap_or(0);
                                        if max >= 1000 { format!("{}k", max / 1000) } else { format!("{}", max) }
                                    } else {
                                        "?".to_string()
                                    };
                                    let bits_str = if !info.supported_bit_depths.is_empty() {
                                        let max = info.supported_bit_depths.iter().max().copied().unwrap_or(0);
                                        format!("{}-bit", max)
                                    } else {
                                        String::new()
                                    };
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
                        <button class="layer-panel-opt"
                            on:click=move |_| {
                                state.show_mic_chooser.set(true);
                            }
                        >{move || if state.mic_device_info.get().is_some() { "Change\u{2026}" } else { "Select mic\u{2026}" }}</button>
                    </div>
                </Show>

                // ── Settings ──
                <hr />
                <div class="layer-panel-title">"Settings"</div>
                <div style="padding: 2px 8px;">
                    // Max sample rate
                    <div class="layer-panel-slider-row het-text-row">
                        <label style="font-size: 11px;">"Max sample rate"</label>
                        <select style="font-size: 11px; background: #333; color: #ccc; border: 1px solid #555; padding: 1px 2px;"
                            on:change=move |ev| {
                                if let Ok(val) = leptos::prelude::event_target_value(&ev).parse::<u32>() {
                                    state.mic_max_sample_rate.set(val);
                                }
                            }
                        >
                            <option value="0" selected=move || state.mic_max_sample_rate.get() == 0>"Auto"</option>
                            <option value="44100" selected=move || state.mic_max_sample_rate.get() == 44100>"44.1k"</option>
                            <option value="48000" selected=move || state.mic_max_sample_rate.get() == 48000>"48k"</option>
                            <option value="96000" selected=move || state.mic_max_sample_rate.get() == 96000>"96k"</option>
                            <option value="192000" selected=move || state.mic_max_sample_rate.get() == 192000>"192k"</option>
                            <option value="256000" selected=move || state.mic_max_sample_rate.get() == 256000>"256k"</option>
                            <option value="384000" selected=move || state.mic_max_sample_rate.get() == 384000>"384k"</option>
                            <option value="500000" selected=move || state.mic_max_sample_rate.get() == 500000>"500k"</option>
                        </select>
                    </div>
                    // Max bit depth
                    <div class="layer-panel-slider-row het-text-row">
                        <label style="font-size: 11px;">"Max bit depth"</label>
                        <select style="font-size: 11px; background: #333; color: #ccc; border: 1px solid #555; padding: 1px 2px;"
                            on:change=move |ev| {
                                if let Ok(val) = leptos::prelude::event_target_value(&ev).parse::<u16>() {
                                    state.mic_max_bit_depth.set(val);
                                }
                            }
                        >
                            <option value="0" selected=move || state.mic_max_bit_depth.get() == 0>"Auto"</option>
                            <option value="16" selected=move || state.mic_max_bit_depth.get() == 16>"16-bit"</option>
                            <option value="24" selected=move || state.mic_max_bit_depth.get() == 24>"24-bit"</option>
                            <option value="32" selected=move || state.mic_max_bit_depth.get() == 32>"32-bit"</option>
                        </select>
                    </div>
                    // Channels
                    <div class="layer-panel-slider-row het-text-row">
                        <label style="font-size: 11px;">"Channels"</label>
                        <select style="font-size: 11px; background: #333; color: #ccc; border: 1px solid #555; padding: 1px 2px;"
                            on:change=move |ev| {
                                let val = leptos::prelude::event_target_value(&ev);
                                state.mic_channel_mode.set(if val == "stereo" { ChannelMode::Stereo } else { ChannelMode::Mono });
                            }
                        >
                            <option value="mono" selected=move || state.mic_channel_mode.get() == ChannelMode::Mono>"Mono"</option>
                            <option value="stereo" selected=move || state.mic_channel_mode.get() == ChannelMode::Stereo>"Stereo"</option>
                        </select>
                    </div>
                    // Pre-roll buffer
                    <div class="layer-panel-slider-row het-text-row">
                        <label style="font-size: 11px;">"Pre-roll buffer"</label>
                        <select style="font-size: 11px; background: #333; color: #ccc; border: 1px solid #555; padding: 1px 2px;"
                            on:change=move |ev| {
                                if let Ok(val) = leptos::prelude::event_target_value(&ev).parse::<u32>() {
                                    state.mic_preroll_buffer_secs.set(val);
                                }
                            }
                        >
                            <option value="2" selected=move || state.mic_preroll_buffer_secs.get() == 2>"2s"</option>
                            <option value="5" selected=move || state.mic_preroll_buffer_secs.get() == 5>"5s"</option>
                            <option value="10" selected=move || state.mic_preroll_buffer_secs.get() == 10>"10s"</option>
                            <option value="15" selected=move || state.mic_preroll_buffer_secs.get() == 15>"15s"</option>
                            <option value="20" selected=move || state.mic_preroll_buffer_secs.get() == 20>"20s"</option>
                            <option value="30" selected=move || state.mic_preroll_buffer_secs.get() == 30>"30s"</option>
                        </select>
                    </div>
                </div>
            </ComboButton>

            // ── Listen combo button ──
            {
                let listen_is_open = Signal::derive(move || state.layer_panel_open.get() == Some(LayerPanel::ListenMode));

                let listen_left_class = Signal::derive(move || {
                    if state.mic_strategy.get() == MicStrategy::None {
                        return "layer-btn combo-btn-left disabled";
                    }
                    let is_listening_ready = state.mic_listening.get()
                        && state.mic_acquisition_state.get() == MicAcquisitionState::Ready;
                    let is_rec_ready = state.record_ready_state.get() == RecordReadyState::AwaitingConfirmation;
                    if is_listening_ready || is_rec_ready {
                        "layer-btn combo-btn-left mic-armed"
                    } else {
                        "layer-btn combo-btn-left"
                    }
                });
                let listen_right_class = Signal::derive(move || {
                    if listen_is_open.get() { "layer-btn combo-btn-right open" } else { "layer-btn combo-btn-right" }
                });

                let listen_left_value = Signal::derive(move || {
                    if state.record_ready_state.get() == RecordReadyState::AwaitingConfirmation {
                        "\u{23F8} Rec ready\u{2026}".to_string() // ⏸ Rec ready…
                    } else if state.mic_acquisition_state.get() == MicAcquisitionState::Acquiring {
                        "Readying\u{2026}".to_string()
                    } else if state.mic_listening.get() && state.listen_mode.get() == ListenMode::ReadyMic {
                        if state.mic_acquisition_state.get() == MicAcquisitionState::Ready {
                            "\u{23F8} Ready".to_string() // ⏸ Ready
                        } else {
                            "Readying\u{2026}".to_string()
                        }
                    } else if state.mic_usb_connected.get() && state.mic_backend.get().is_none() && state.is_tauri && !state.mic_listening.get() {
                        "USB mic".to_string()
                    } else {
                        "\u{1F3A4} Listen".to_string() // 🎤 Listen
                    }
                });
                let listen_right_value = Signal::derive(move || {
                    match state.listen_mode.get() {
                        ListenMode::Heterodyne => "HET".to_string(),
                        ListenMode::PitchShift => "PS".to_string(),
                        ListenMode::PhaseVocoder => "PV".to_string(),
                        ListenMode::ZeroCrossing => "ZC".to_string(),
                        ListenMode::Normal => "1:1".to_string(),
                        ListenMode::ReadyMic => "RDY".to_string(),
                    }
                });

                let listen_left_click = Callback::new(move |_: web_sys::MouseEvent| {
                    if state.mic_strategy.get_untracked() == MicStrategy::None {
                        return; // greyed out
                    }
                    let st = state;
                    wasm_bindgen_futures::spawn_local(async move {
                        microphone::toggle_listen(&st).await;
                    });
                });
                let listen_toggle_menu = Callback::new(move |()| {
                    toggle_panel(&state, LayerPanel::ListenMode);
                });

                view! {
                    <ComboButton
                        left_label="Mic"
                        left_value=listen_left_value
                        left_click=listen_left_click
                        left_class=listen_left_class
                        right_value=listen_right_value
                        right_class=listen_right_class
                        is_open=listen_is_open
                        toggle_menu=listen_toggle_menu
                        left_title="Toggle live listening (L)"
                        right_title="Listen mode settings"
                        menu_direction="above"
                        panel_style="min-width: 220px;"
                    >
                        // ── Mode selection ──
                        <div class="layer-panel-title">"Mode"</div>
                        <button class=move || layer_opt_class(state.listen_mode.get() == ListenMode::Heterodyne)
                            on:click=move |_| state.listen_mode.set(ListenMode::Heterodyne)
                        >"HET \u{2014} Heterodyne"</button>
                        <button class=move || layer_opt_class(state.listen_mode.get() == ListenMode::PitchShift)
                            on:click=move |_| state.listen_mode.set(ListenMode::PitchShift)
                        >"PS \u{2014} Pitch Shift"</button>
                        <button class=move || layer_opt_class(state.listen_mode.get() == ListenMode::PhaseVocoder)
                            on:click=move |_| state.listen_mode.set(ListenMode::PhaseVocoder)
                        >"PV \u{2014} Phase Vocoder"</button>
                        <button class=move || layer_opt_class(state.listen_mode.get() == ListenMode::ZeroCrossing)
                            on:click=move |_| state.listen_mode.set(ListenMode::ZeroCrossing)
                        >"ZC \u{2014} Zero Crossing"</button>
                        <button class=move || layer_opt_class(state.listen_mode.get() == ListenMode::Normal)
                            on:click=move |_| state.listen_mode.set(ListenMode::Normal)
                        >"1:1 \u{2014} Normal (passthrough)"</button>
                        <button class=move || layer_opt_class(state.listen_mode.get() == ListenMode::ReadyMic)
                            on:click=move |_| state.listen_mode.set(ListenMode::ReadyMic)
                        >"Ready \u{2014} Mic warm-up (no audio)"</button>

                        // ── Heterodyne settings ──
                        <Show when=move || state.listen_mode.get() == ListenMode::Heterodyne>
                            <hr />
                            <div class="layer-panel-title">"Heterodyne"</div>
                            <div class="layer-panel-slider-row het-text-row">
                                <label>"Freq"</label>
                                <span class="het-value">{move || format!("{:.1} kHz", state.listen_het_frequency.get() / 1000.0)}</span>
                            </div>
                            <div class="layer-panel-slider-row">
                                <input type="range" min="10000" max="200000" step="1000"
                                    prop:value=move || state.listen_het_frequency.get().to_string()
                                    on:input=move |ev| {
                                        if let Ok(val) = event_target_value(&ev).parse::<f64>() {
                                            state.listen_het_frequency.set(val);
                                        }
                                    }
                                />
                            </div>
                            <div class="layer-panel-slider-row het-text-row">
                                <label>"LP cutoff"</label>
                                <span class="het-value">{move || format!("{:.1} kHz", state.listen_het_cutoff.get() / 1000.0)}</span>
                            </div>
                            <div class="layer-panel-slider-row">
                                <input type="range" min="1000" max="20000" step="500"
                                    prop:value=move || state.listen_het_cutoff.get().to_string()
                                    on:input=move |ev| {
                                        if let Ok(val) = event_target_value(&ev).parse::<f64>() {
                                            state.listen_het_cutoff.set(val);
                                        }
                                    }
                                />
                            </div>
                        </Show>

                        // ── Pitch Shift settings ──
                        <Show when=move || state.listen_mode.get() == ListenMode::PitchShift>
                            <hr />
                            <div class="layer-panel-title">"Pitch Shift"</div>
                            <div class="layer-panel-slider-row het-text-row">
                                <label>"Factor"</label>
                                <span class="het-value">{move || format!("\u{00f7}{:.0}", state.ps_factor.get())}</span>
                            </div>
                            <div class="layer-panel-slider-row">
                                <input type="range" min="2" max="40" step="1"
                                    prop:value=move || state.ps_factor.get().to_string()
                                    on:input=move |ev| {
                                        if let Ok(val) = event_target_value(&ev).parse::<f64>() {
                                            state.ps_factor.set(val);
                                        }
                                    }
                                />
                            </div>
                        </Show>

                        // ── Phase Vocoder settings ──
                        <Show when=move || state.listen_mode.get() == ListenMode::PhaseVocoder>
                            <hr />
                            <div class="layer-panel-title">"Phase Vocoder"</div>
                            <div class="layer-panel-slider-row het-text-row">
                                <label>"Factor"</label>
                                <span class="het-value">{move || format!("\u{00f7}{:.0}", state.pv_factor.get())}</span>
                            </div>
                            <div class="layer-panel-slider-row">
                                <input type="range" min="2" max="40" step="1"
                                    prop:value=move || state.pv_factor.get().to_string()
                                    on:input=move |ev| {
                                        if let Ok(val) = event_target_value(&ev).parse::<f64>() {
                                            state.pv_factor.set(val);
                                        }
                                    }
                                />
                            </div>
                        </Show>

                        // ── Zero Crossing settings ──
                        <Show when=move || state.listen_mode.get() == ListenMode::ZeroCrossing>
                            <hr />
                            <div class="layer-panel-title">"Zero Crossing"</div>
                            <div class="layer-panel-slider-row het-text-row">
                                <label>"Division"</label>
                                <span class="het-value">{move || format!("\u{00f7}{:.0}", state.zc_factor.get())}</span>
                            </div>
                            <div class="layer-panel-slider-row">
                                <input type="range" min="2" max="32" step="1"
                                    prop:value=move || state.zc_factor.get().to_string()
                                    on:input=move |ev| {
                                        if let Ok(val) = event_target_value(&ev).parse::<f64>() {
                                            state.zc_factor.set(val);
                                        }
                                    }
                                />
                            </div>
                        </Show>

                        // ── Buffer size (PS/PV only) ──
                        <Show when=move || matches!(state.listen_mode.get(), ListenMode::PitchShift | ListenMode::PhaseVocoder)>
                            <hr />
                            <div class="layer-panel-title">"Buffer"</div>
                            <div style="display: flex; gap: 2px; padding: 0 6px 4px;">
                                <button class=move || layer_opt_class(state.listen_context_samples.get() == 4096)
                                    on:click=move |_| state.listen_context_samples.set(4096)
                                    title="4096 samples — minimum context (more artifacts, lowest latency)"
                                >"4K"</button>
                                <button class=move || layer_opt_class(state.listen_context_samples.get() == 8192)
                                    on:click=move |_| state.listen_context_samples.set(8192)
                                    title="8192 samples"
                                >"8K"</button>
                                <button class=move || layer_opt_class(state.listen_context_samples.get() == 16384)
                                    on:click=move |_| state.listen_context_samples.set(16384)
                                    title="16384 samples (default)"
                                >"16K"</button>
                                <button class=move || layer_opt_class(state.listen_context_samples.get() == 32768)
                                    on:click=move |_| state.listen_context_samples.set(32768)
                                    title="32768 samples (smoother, more CPU)"
                                >"32K"</button>
                                <button class=move || layer_opt_class(state.listen_context_samples.get() == 65536)
                                    on:click=move |_| state.listen_context_samples.set(65536)
                                    title="65536 samples (smoothest, most CPU)"
                                >"64K"</button>
                            </div>
                        </Show>

                        // ── Bandpass filter ──
                        <hr />
                        <div class="layer-panel-title">"Bandpass Filter"</div>
                        <div style="display: flex; gap: 2px; padding: 0 6px 4px;">
                            <button class=move || layer_opt_class(!state.listen_bandpass_enabled.get())
                                on:click=move |_| state.listen_bandpass_enabled.set(false)
                            >"OFF"</button>
                            <button class=move || layer_opt_class(state.listen_bandpass_enabled.get())
                                on:click=move |_| state.listen_bandpass_enabled.set(true)
                            >"ON"</button>
                        </div>
                        <Show when=move || state.listen_bandpass_enabled.get()>
                            <div class="layer-panel-slider-row het-text-row">
                                <label>"Low"</label>
                                <span class="het-value">{move || format!("{:.1} kHz", state.listen_bandpass_lo.get() / 1000.0)}</span>
                            </div>
                            <div class="layer-panel-slider-row">
                                <input type="range" min="1000" max="200000" step="1000"
                                    prop:value=move || state.listen_bandpass_lo.get().to_string()
                                    on:input=move |ev| {
                                        if let Ok(val) = event_target_value(&ev).parse::<f64>() {
                                            state.listen_bandpass_lo.set(val);
                                        }
                                    }
                                />
                            </div>
                            <div class="layer-panel-slider-row het-text-row">
                                <label>"High"</label>
                                <span class="het-value">{move || format!("{:.1} kHz", state.listen_bandpass_hi.get() / 1000.0)}</span>
                            </div>
                            <div class="layer-panel-slider-row">
                                <input type="range" min="1000" max="200000" step="1000"
                                    prop:value=move || state.listen_bandpass_hi.get().to_string()
                                    on:input=move |ev| {
                                        if let Ok(val) = event_target_value(&ev).parse::<f64>() {
                                            state.listen_bandpass_hi.set(val);
                                        }
                                    }
                                />
                            </div>
                        </Show>
                    </ComboButton>
                }
            }

            // ── Annotations visibility toggle ──
            // Single-word button — on/off state is carried by the `active`
            // highlight. An NBSP placeholder fills the category slot so this
            // button's baseline lines up with the two-line buttons next to it.
            {move || has_file().then(|| view! {
                <button
                    class=move || if state.annotations_visible.get() { "layer-btn active" } else { "layer-btn" }
                    on:click=move |_| {
                        let new_visible = !state.annotations_visible.get_untracked();
                        state.annotations_visible.set(new_visible);
                        if !new_visible {
                            // Drop annotation focus/selection and clear interaction state.
                            if state.active_focus.get_untracked() == Some(ActiveFocus::Annotations) {
                                state.active_focus.set(None);
                            }
                            if !state.selected_annotation_ids.get_untracked().is_empty() {
                                state.selected_annotation_ids.set(Vec::new());
                            }
                            state.annotation_hover_handle.set(None);
                            state.annotation_drag_handle.set(None);
                            state.annotation_editing.set(false);
                            state.annotation_is_new_edit.set(false);
                        }
                    }
                    title=move || if state.annotations_visible.get() { "Hide annotations" } else { "Show annotations" }
                >
                    <span class="layer-btn-category">"\u{00A0}"</span>
                    <span class="layer-btn-value">"Anno"</span>
                </button>
            })}

            // ── Bat book strip toggle ──
            // Replaces the old edge-tab handle on the right side of the main
            // view — the strip now lives behind a discoverable toolbar button
            // next to the other view toggles. Label is always "Bat / Book";
            // on/off state rides on the `active` highlight.
            <button
                class=move || if state.bat_book_open.get() { "layer-btn active" } else { "layer-btn" }
                on:click=move |_| { state.bat_book_open.update(|v| *v = !*v); }
                title=move || if state.bat_book_open.get() { "Hide bat book" } else { "Show bat book" }
            >
                <span class="layer-btn-category">"Bat"</span>
                <span class="layer-btn-value">"Book"</span>
            </button>

            // ── Tool button (Hand / Selection, only when file is open; hidden on mobile) ──
            {move || (!state.is_mobile.get() && has_file()).then(|| view! {
                <div class="bottom-toolbar-sep"></div>
                <ToolButtonInline />
            })}

            // ── HFR combo button — pinned to the right end so it sits under
            // the right gutter it controls. On mobile it occupies the top-right
            // grid cell via `.hfr-slot` rules in style.css. ──
            {move || has_file().then(|| view! {
                <div class="hfr-slot"><HfrButton /></div>
            })}
        </div>
    }
}

/// Tool button adapted for inline use in the bottom toolbar (no absolute positioning).
#[component]
fn ToolButtonInline() -> impl IntoView {
    let state = expect_context::<AppState>();
    let is_open = move || state.layer_panel_open.get() == Some(LayerPanel::Tool);
    let no_file = move || state.current_file_index.get().is_none() && state.active_timeline.get().is_none();

    view! {
        <div style="position: relative;">
            <button
                class=move || {
                    if no_file() { "layer-btn disabled" }
                    else if is_open() { "layer-btn open" }
                    else { "layer-btn" }
                }
                on:click=move |_| { if !no_file() { toggle_panel(&state, LayerPanel::Tool); } }
                title="Tool"
            >
                <span class="layer-btn-category">"Tool"</span>
                <span class="layer-btn-value">{move || match state.canvas_tool.get() {
                    CanvasTool::Hand => "Hand",
                    CanvasTool::Selection => "Select",
                }}</span>
            </button>
            <Show when=move || is_open()>
                <div class="layer-panel" style="bottom: calc(100% + 4px); right: 0;">
                    <div class="layer-panel-title">"Tool"</div>
                    <button
                        class=move || layer_opt_class(state.canvas_tool.get() == CanvasTool::Hand)
                        on:click=move |_| {
                            state.canvas_tool.set(CanvasTool::Hand);
                            state.layer_panel_open.set(None);
                        }
                    >"Hand (pan)"</button>
                    <button
                        class=move || layer_opt_class(state.canvas_tool.get() == CanvasTool::Selection)
                        on:click=move |_| {
                            state.canvas_tool.set(CanvasTool::Selection);
                            state.layer_panel_open.set(None);
                        }
                    >"Selection"</button>
                </div>
            </Show>
        </div>
    }
}
