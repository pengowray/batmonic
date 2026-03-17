use leptos::prelude::*;
use wasm_bindgen::prelude::*;
use crate::state::{AppState, CanvasTool, GainMode, LayerPanel, PlaybackMode, PlayStartMode, RecordMode};
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
    let has_file = move || state.current_file_index.get().is_some();
    let is_mobile = state.is_mobile.get_untracked();

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
        } else {
            if let Some(id) = interval_id.get_value() {
                if let Some(window) = web_sys::window() {
                    window.clear_interval_with_handle(id);
                }
                interval_id.set_value(None);
            }
        }
    });

    // ── Play ComboButton setup ──
    let play_is_open = Signal::derive(move || state.layer_panel_open.get() == Some(LayerPanel::PlayMode));

    let play_left_class = Signal::derive(move || {
        let playing = state.is_playing.get();
        let open = play_is_open.get();
        match (playing, open) {
            (true, true) => "layer-btn combo-btn-left active open",
            (true, false) => "layer-btn combo-btn-left active",
            (false, true) => "layer-btn combo-btn-left open",
            (false, false) => "layer-btn combo-btn-left",
        }
    });
    let play_right_class = Signal::derive(move || {
        if play_is_open.get() { "layer-btn combo-btn-right open" } else { "layer-btn combo-btn-right" }
    });

    let play_left_value = Signal::derive(move || "\u{25B6}".to_string()); // ▶
    let play_right_value = Signal::derive(move || {
        match state.play_start_mode.get() {
            PlayStartMode::All => "All".to_string(),
            PlayStartMode::FromHere => "Here".to_string(),
            PlayStartMode::Selected => "Sel".to_string(),
        }
    });

    let play_left_click = Callback::new(move |_: web_sys::MouseEvent| {
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
            }
        }
    });
    let play_toggle_menu = Callback::new(move |()| {
        toggle_panel(&state, LayerPanel::PlayMode);
    });

    // ── Record ComboButton setup ──
    let rec_is_open = Signal::derive(move || state.layer_panel_open.get() == Some(LayerPanel::RecordMode));

    let rec_left_class = Signal::derive(move || {
        let recording = state.mic_recording.get();
        let listen_only = state.record_mode.get() == RecordMode::ListenOnly;
        let open = rec_is_open.get();
        if recording {
            if open { "layer-btn combo-btn-left mic-recording open" } else { "layer-btn combo-btn-left mic-recording" }
        } else if listen_only {
            if open { "layer-btn combo-btn-left disabled open" } else { "layer-btn combo-btn-left disabled" }
        } else {
            if open { "layer-btn combo-btn-left open" } else { "layer-btn combo-btn-left" }
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
            format!("Rec {:.1}s", secs)
        } else {
            "\u{23FA}".to_string() // ⏺
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
        if state.record_mode.get_untracked() == RecordMode::ListenOnly {
            return; // greyed out
        }
        let st = state;
        wasm_bindgen_futures::spawn_local(async move {
            microphone::toggle_record(&st).await;
        });
    });
    let rec_toggle_menu = Callback::new(move |()| {
        toggle_panel(&state, LayerPanel::RecordMode);
    });

    view! {
        <div class=if is_mobile { "bottom-toolbar mobile" } else { "bottom-toolbar" }
            on:click=|ev: web_sys::MouseEvent| ev.stop_propagation()
            on:touchstart=|ev: web_sys::TouchEvent| ev.stop_propagation()
        >
            // ── HFR combo button ──
            <HfrButton />

            <div class="bottom-toolbar-sep"></div>

            // ── Play combo button ──
            {move || has_file().then(|| view! {
                <ComboButton
                    left_label=""
                    left_value=play_left_value
                    left_click=play_left_click
                    left_class=play_left_class
                    right_value=play_right_value
                    right_class=play_right_class
                    is_open=play_is_open
                    toggle_menu=play_toggle_menu
                    left_title="Play / Stop"
                    right_title="Play mode"
                    menu_direction="above"
                    panel_style="min-width: 180px;"
                >
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
                    let mode = state.gain_mode.get();
                    let open = gain_is_open.get();
                    let active = mode != GainMode::Off;
                    match (active, open) {
                        (true, true) => "layer-btn combo-btn-left active open",
                        (true, false) => "layer-btn combo-btn-left active",
                        (false, true) => "layer-btn combo-btn-left no-annotation open",
                        (false, false) => "layer-btn combo-btn-left no-annotation",
                    }
                });
                let gain_right_class = Signal::derive(move || {
                    let dim = if state.gain_mode.get() == GainMode::Off { " dim" } else { "" };
                    if gain_is_open.get() {
                        if dim.is_empty() { "layer-btn combo-btn-right open" } else { "layer-btn combo-btn-right dim open" }
                    } else {
                        if dim.is_empty() { "layer-btn combo-btn-right" } else { "layer-btn combo-btn-right dim" }
                    }
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
                        >"Adaptive \u{2014} Per-chunk compression"</button>
                        <div class="layer-panel-slider-row" style="margin-top: 6px;">
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
                left_class=rec_left_class
                right_value=rec_right_value
                right_class=rec_right_class
                is_open=rec_is_open
                toggle_menu=rec_toggle_menu
                left_title="Record"
                right_title="Record mode"
                menu_direction="above"
                panel_style="min-width: 160px;"
            >
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
                            state.layer_panel_open.set(None);
                        }
                    }
                >"To file"</button>
                <button class=move || layer_opt_class(state.record_mode.get() == RecordMode::ToMemory)
                    on:click=move |_| {
                        state.record_mode.set(RecordMode::ToMemory);
                        state.layer_panel_open.set(None);
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
                        state.layer_panel_open.set(None);
                    }
                >"Listen only"</button>
            </ComboButton>

            // ── Listen button ──
            <button
                class=move || if state.mic_listening.get() { "layer-btn mic-armed" } else { "layer-btn" }
                on:click=move |_| {
                    let st = state;
                    wasm_bindgen_futures::spawn_local(async move {
                        microphone::toggle_listen(&st).await;
                    });
                }
                title=move || if state.mic_needs_permission.get() && state.is_tauri {
                    "Grant USB mic permission to start listening"
                } else {
                    "Toggle live listening (L)"
                }
            >
                <span class="layer-btn-category">"Mic"</span>
                <span class="layer-btn-value">{move || if state.mic_needs_permission.get() && state.is_tauri && !state.mic_listening.get() {
                    "USB mic"
                } else {
                    "Listen"
                }}</span>
            </button>

            <div class="bottom-toolbar-sep"></div>

            // ── Tool button (Hand / Selection) ──
            <ToolButtonInline />
        </div>
    }
}

/// Tool button adapted for inline use in the bottom toolbar (no absolute positioning).
#[component]
fn ToolButtonInline() -> impl IntoView {
    let state = expect_context::<AppState>();
    let is_open = move || state.layer_panel_open.get() == Some(LayerPanel::Tool);

    view! {
        <div style="position: relative;">
            <button
                class=move || if is_open() { "layer-btn open" } else { "layer-btn" }
                on:click=move |_| toggle_panel(&state, LayerPanel::Tool)
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
