// Notch combo + NR (noise reduction) combo — both live in the Hearing
// Bar. Contents migrated from the Noise Filter right-sidebar tab, which
// was deleted: each combo is self-contained now.
//
// Profile management (save/load Tauri presets) was dropped for this
// pass; the Tauri-side commands are still there if it's reinstated.

use crate::state::store_fields::*;
use leptos::prelude::*;
use std::sync::Arc;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

use crate::audio::source::ChannelView;
use crate::components::combo_button::ComboButton;
use crate::dsp::notch::{self, DetectionConfig};
use crate::state::{AppState, LayerPanel};

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

async fn yield_to_browser() {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        if let Some(w) = web_sys::window() {
            let _ = w.set_timeout_with_callback(&resolve);
        }
    });
    let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
}

// ─── Notch combo ────────────────────────────────────────────────────────
//
// Left half toggles the notch filter on/off; left value shows the count
// of defined bands. Dropdown hosts detection (button + sensitivity),
// the per-band list (with bulk toggles), and harmonic suppression.

#[component]
pub fn NotchCombo() -> impl IntoView {
    let state = expect_context::<AppState>();

    // Sensitivity is a local UI value (slider 30..120 → prominence
    // threshold 12..3 inverted). Stored only inside the popup.
    let sensitivity = RwSignal::new(6.0f64);

    let is_open =
        Signal::derive(move || state.panels.layer_panel_open().get() == Some(LayerPanel::Notch));
    let no_file = move || {
        state.library.current_index().get().is_none() && state.timeline.active().get().is_none()
    };
    let band_count = Signal::derive(move || state.notch.bands().get().len());
    let enabled = Signal::derive(move || state.notch.enabled().get());

    let left_class = Signal::derive(move || {
        if no_file() {
            "layer-btn combo-btn-left disabled"
        } else if enabled.get() {
            "layer-btn combo-btn-left active"
        } else {
            "layer-btn combo-btn-left"
        }
    });
    let right_class = Signal::derive(move || {
        if no_file() {
            return "layer-btn combo-btn-right disabled";
        }
        let dim = if !enabled.get() { " dim" } else { "" };
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
        let n = band_count.get();
        if n == 0 {
            String::new()
        } else if n == 1 {
            "1 band".to_string()
        } else {
            format!("{} bands", n)
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
        toggle_panel(&state, LayerPanel::Notch);
    });

    // ── Detect noise bands ──
    let run_detect = move || {
        // While live, detect from the circular buffer so the bands reflect
        // current ambient noise rather than stale file content.
        let is_live =
            state.mic.listening().get_untracked() || state.mic.recording().get_untracked();
        if is_live {
            let is_tauri = state.is_tauri;
            let sample_rate = state.mic.sample_rate().get_untracked();
            let snapshot = crate::audio::mic_backend::with_live_samples(is_tauri, |s| s.to_vec());
            if sample_rate == 0 || snapshot.is_empty() {
                state.show_error_toast("Not enough live audio yet");
                return;
            }
            state.notch.detecting().set(true);
            let threshold = sensitivity.get_untracked();
            let samples = Arc::new(snapshot);
            spawn_local(async move {
                yield_to_browser().await;
                let duration = samples.len() as f64 / sample_rate as f64;
                let config = DetectionConfig {
                    analysis_duration_secs: duration.min(10.0),
                    prominence_threshold: threshold,
                    ..DetectionConfig::default()
                };
                let bands = notch::detect_noise_bands_async(
                    &samples,
                    sample_rate,
                    &config,
                    crate::canvas::tile_cache::yield_to_browser,
                )
                .await;
                let count = bands.len();
                state.notch.bands().set(bands);
                if count > 0 {
                    state.notch.enabled().set(true);
                    state.show_info_toast(format!(
                        "Found {} noise band{} from live",
                        count,
                        if count == 1 { "" } else { "s" }
                    ));
                } else {
                    state.show_info_toast("No persistent noise bands detected");
                }
                state.notch.detecting().set(false);
            });
            return;
        }

        let files = state.library.files().get_untracked();
        let idx = state.library.current_index().get_untracked();
        let Some(file) = idx.and_then(|i| files.get(i).cloned()) else {
            state.show_error_toast("No file loaded");
            return;
        };
        state.notch.detecting().set(true);
        let threshold = sensitivity.get_untracked();
        let total = file.audio.source.total_samples() as usize;
        let samples = Arc::new(
            file.audio
                .source
                .read_region(ChannelView::MonoMix, 0, total),
        );
        let sample_rate = file.audio.sample_rate;
        let duration = file.audio.duration_secs;

        spawn_local(async move {
            yield_to_browser().await;
            let config = DetectionConfig {
                analysis_duration_secs: if duration > 30.0 { 10.0 } else { duration },
                prominence_threshold: threshold,
                ..DetectionConfig::default()
            };
            let bands = notch::detect_noise_bands_async(
                &samples,
                sample_rate,
                &config,
                crate::canvas::tile_cache::yield_to_browser,
            )
            .await;
            let count = bands.len();
            state.notch.bands().set(bands);
            if count > 0 {
                state.notch.enabled().set(true);
                state.show_info_toast(format!(
                    "Found {} noise band{}",
                    count,
                    if count == 1 { "" } else { "s" }
                ));
            } else {
                state.show_info_toast("No persistent noise bands detected");
            }
            state.notch.detecting().set(false);
        });
    };

    // Pressing Notch ON with nothing defined yet auto-runs detection (live or
    // file, whichever applies) so the toggle "just works" without a second tap.
    let left_click = Callback::new(move |_: web_sys::MouseEvent| {
        if no_file() {
            return;
        }
        let turning_on = !state.notch.enabled().get_untracked();
        state.notch.enabled().set(turning_on);
        if turning_on
            && state.notch.bands().get_untracked().is_empty()
            && !state.notch.detecting().get_untracked()
        {
            run_detect();
        }
    });
    let on_detect = move |_: web_sys::MouseEvent| {
        run_detect();
    };

    let on_sensitivity_change = move |ev: web_sys::Event| {
        let target: web_sys::HtmlInputElement = ev.target().unwrap().unchecked_into();
        if let Ok(val) = target.value().parse::<f64>() {
            sensitivity.set((150.0 - val) / 10.0);
        }
    };

    let on_harmonic_change = move |ev: web_sys::Event| {
        let target: web_sys::HtmlInputElement = ev.target().unwrap().unchecked_into();
        if let Ok(val) = target.value().parse::<f64>() {
            state.notch.harmonic_suppression().set(val / 100.0);
        }
    };

    let toggle_band = move |index: usize| {
        state.notch.bands().update(|bands| {
            if let Some(band) = bands.get_mut(index) {
                band.enabled = !band.enabled;
            }
        });
    };
    let remove_band = move |index: usize| {
        state.notch.bands().update(|bands| {
            if index < bands.len() {
                bands.remove(index);
            }
        });
    };
    let set_all_enabled = move |enabled: bool| {
        state.notch.bands().update(|bands| {
            for band in bands.iter_mut() {
                band.enabled = enabled;
            }
        });
    };

    view! {
        <ComboButton
            left_label="Notch"
            left_value=left_value
            left_click=left_click
            left_class=left_class
            right_value=right_value
            right_class=right_class
            is_open=is_open
            toggle_menu=toggle_menu
            left_title="Toggle notch filter"
            right_title="Notch options"
            menu_direction="below"
            panel_align="right"
            panel_style="min-width: 260px;"
        >
            // ── Enable ──
            <div style="display: flex; gap: 2px; padding: 0 6px 4px;">
                <button class=move || layer_opt_class(state.notch.enabled().get())
                    on:click=move |_| state.notch.enabled().set(true)
                >"On"</button>
                <button class=move || layer_opt_class(!state.notch.enabled().get())
                    on:click=move |_| state.notch.enabled().set(false)
                >"Off"</button>
            </div>

            <hr />

            // ── Detection ──
            <div class="layer-panel-title">"Detection"</div>
            <div style="display: flex; gap: 4px; padding: 0 6px 4px;">
                <button
                    class="layer-panel-opt"
                    style="flex: 1;"
                    on:click=on_detect
                    disabled=move || {
                        if state.notch.detecting().get() { return true; }
                        let live = state.mic.listening().get() || state.mic.recording().get();
                        !live && state.library.current_index().get().is_none()
                    }
                >
                    {move || {
                        if state.notch.detecting().get() { return "Detecting\u{2026}".to_string(); }
                        let live = state.mic.listening().get() || state.mic.recording().get();
                        if live { "Detect from Live".to_string() } else { "Detect Noise".to_string() }
                    }}
                </button>
            </div>
            <div class="layer-panel-slider-row">
                <label>"Sensitivity"</label>
                <input type="range" min="30" max="120" step="5"
                    prop:value=move || (150.0 - sensitivity.get() * 10.0) as i32
                    on:input=on_sensitivity_change
                    title=move || format!("Threshold: {:.1}x ({:.0} dB)", sensitivity.get(), 20.0 * sensitivity.get().log10())
                />
            </div>

            <hr />

            // ── Bands ──
            <div class="layer-panel-title">{move || {
                let bands = state.notch.bands().get();
                let on = bands.iter().filter(|b| b.enabled).count();
                if bands.is_empty() { "Bands".to_string() }
                else { format!("Bands ({}/{})", on, bands.len()) }
            }}</div>
            {move || {
                let bands = state.notch.bands().get();
                if bands.is_empty() {
                    view! {
                        <div style="padding: 4px 8px; font-size: 11px; opacity: 0.55;">
                            "No bands detected yet"
                        </div>
                    }.into_any()
                } else {
                    let items: Vec<_> = bands.iter().enumerate().map(|(i, band)| {
                        let center = band.center_hz;
                        let strength = band.strength_db;
                        let band_enabled = band.enabled;
                        let bandwidth = band.bandwidth_hz;
                        view! {
                            <div
                                style="display: flex; align-items: center; gap: 6px; padding: 2px 8px; font-size: 11px;"
                                on:mouseenter=move |_| state.notch.hovering_band().set(Some(i))
                                on:mouseleave=move |_| state.notch.hovering_band().set(None)
                            >
                                <input type="checkbox"
                                    checked=band_enabled
                                    on:change=move |_| toggle_band(i)
                                    style="margin: 0;"
                                />
                                <span style="flex: 1; white-space: nowrap;">
                                    {if center >= 1000.0 { format!("{:.1} kHz", center / 1000.0) }
                                     else { format!("{:.0} Hz", center) }}
                                </span>
                                <span style="opacity: 0.6; font-size: 10px; white-space: nowrap;"
                                      title=format!("BW: {:.0} Hz", bandwidth)>
                                    {format!("+{:.0}dB", strength)}
                                </span>
                                <button
                                    style="background: none; border: none; color: inherit; opacity: 0.4; cursor: pointer; padding: 0 2px; font-size: 12px;"
                                    on:click=move |_: web_sys::MouseEvent| remove_band(i)
                                    title="Remove band"
                                >{"\u{00D7}"}</button>
                            </div>
                        }
                    }).collect();
                    view! {
                        <div style="max-height: 220px; overflow-y: auto;">{items}</div>
                        <div style="display: flex; gap: 4px; padding: 4px 6px 0;">
                            <button class="layer-panel-opt" style="flex: 1; font-size: 10px;"
                                on:click=move |_: web_sys::MouseEvent| set_all_enabled(true)
                            >"All On"</button>
                            <button class="layer-panel-opt" style="flex: 1; font-size: 10px;"
                                on:click=move |_: web_sys::MouseEvent| set_all_enabled(false)
                            >"All Off"</button>
                            <button class="layer-panel-opt" style="flex: 1; font-size: 10px;"
                                on:click=move |_: web_sys::MouseEvent| {
                                    state.notch.bands().set(Vec::new());
                                    state.notch.enabled().set(false);
                                }
                            >"Clear"</button>
                        </div>
                    }.into_any()
                }
            }}

            // ── Harmonic suppression (only when there are bands or a learned floor) ──
            {move || {
                let has_bands = !state.notch.bands().get().is_empty();
                let has_floor = state.noise_reduce.floor().get().is_some();
                if has_bands || has_floor {
                    view! {
                        <hr />
                        <div class="layer-panel-slider-row">
                            <label>"Harm. supp."</label>
                            <input type="range" min="0" max="100" step="5"
                                prop:value=move || (state.notch.harmonic_suppression().get() * 100.0) as i32
                                on:input=on_harmonic_change
                                title=move || {
                                    let v = state.notch.harmonic_suppression().get();
                                    if v == 0.0 { "Off".to_string() }
                                    else { format!("{:.0}% ({:.0} dB at 2\u{00D7} & 3\u{00D7})", v * 100.0, -48.0 * v) }
                                }
                            />
                            <span style="min-width: 30px; text-align: right; font-size: 10px; opacity: 0.7;">
                                {move || format!("{:.0}%", state.notch.harmonic_suppression().get() * 100.0)}
                            </span>
                        </div>
                    }.into_any()
                } else {
                    view! { <span></span> }.into_any()
                }
            }}
        </ComboButton>
    }
}

// ─── NR (Noise Reduction) combo ─────────────────────────────────────────
//
// Spectral-subtraction noise reduction. Left half toggles on/off;
// dropdown hosts the "learn" button, strength slider, and floor status.

#[component]
pub fn NrCombo() -> impl IntoView {
    let state = expect_context::<AppState>();

    let is_open = Signal::derive(move || {
        state.panels.layer_panel_open().get() == Some(LayerPanel::NoiseReduce)
    });
    let no_file = move || {
        state.library.current_index().get().is_none() && state.timeline.active().get().is_none()
    };
    let enabled = Signal::derive(move || state.noise_reduce.enabled().get());
    let has_floor = Signal::derive(move || state.noise_reduce.floor().get().is_some());

    let left_class = Signal::derive(move || {
        if no_file() {
            "layer-btn combo-btn-left disabled"
        } else if enabled.get() {
            "layer-btn combo-btn-left active"
        } else {
            "layer-btn combo-btn-left"
        }
    });
    let right_class = Signal::derive(move || {
        if no_file() {
            return "layer-btn combo-btn-right disabled";
        }
        let dim = if !enabled.get() { " dim" } else { "" };
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
        if !has_floor.get() {
            String::new()
        } else {
            format!("{:.0}%", state.noise_reduce.strength().get() * 100.0)
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
        toggle_panel(&state, LayerPanel::NoiseReduce);
    });

    let run_learn = move || {
        // While live, source samples from the circular capture buffer rather
        // than the file (which lags the buffer by the periodic snapshot
        // interval, and is short on history right after Listen starts).
        let is_live =
            state.mic.listening().get_untracked() || state.mic.recording().get_untracked();
        if is_live {
            let is_tauri = state.is_tauri;
            let sample_rate = state.mic.sample_rate().get_untracked();
            let snapshot = crate::audio::mic_backend::with_live_samples(is_tauri, |s| s.to_vec());
            if sample_rate == 0 || snapshot.is_empty() {
                state.show_error_toast("Not enough live audio yet");
                return;
            }
            let samples = Arc::new(snapshot);
            state.noise_reduce.learning().set(true);
            spawn_local(async move {
                yield_to_browser().await;
                let duration = samples.len() as f64 / sample_rate as f64;
                let analysis_secs = duration.min(10.0);
                let floor = crate::dsp::spectral_sub::learn_noise_floor_async(
                    &samples,
                    sample_rate,
                    analysis_secs,
                    crate::canvas::tile_cache::yield_to_browser,
                )
                .await;
                if let Some(f) = floor {
                    state.noise_reduce.floor().set(Some(f));
                    state.noise_reduce.enabled().set(true);
                    state.show_info_toast("Noise floor learned from live");
                } else {
                    state.show_error_toast("Not enough live audio to learn noise floor");
                }
                state.noise_reduce.learning().set(false);
            });
            return;
        }

        let files = state.library.files().get_untracked();
        let idx = state.library.current_index().get_untracked();
        let Some(file) = idx.and_then(|i| files.get(i).cloned()) else {
            state.show_error_toast("No file loaded");
            return;
        };
        state.noise_reduce.learning().set(true);
        let total = file.audio.source.total_samples() as usize;
        let samples = Arc::new(
            file.audio
                .source
                .read_region(ChannelView::MonoMix, 0, total),
        );
        let sample_rate = file.audio.sample_rate;
        let duration = file.audio.duration_secs;

        spawn_local(async move {
            yield_to_browser().await;
            let analysis_secs = if duration > 30.0 { 10.0 } else { duration };
            let floor = crate::dsp::spectral_sub::learn_noise_floor_async(
                &samples,
                sample_rate,
                analysis_secs,
                crate::canvas::tile_cache::yield_to_browser,
            )
            .await;
            if let Some(f) = floor {
                state.noise_reduce.floor().set(Some(f));
                state.noise_reduce.enabled().set(true);
                state.show_info_toast("Noise floor learned");
            } else {
                state.show_error_toast("Not enough audio to learn noise floor");
            }
            state.noise_reduce.learning().set(false);
        });
    };

    // Pressing NR ON with no floor learned yet auto-learns the noise floor
    // (live or file) so a single tap both learns and enables.
    let left_click = Callback::new(move |_: web_sys::MouseEvent| {
        if no_file() {
            return;
        }
        let turning_on = !state.noise_reduce.enabled().get_untracked();
        state.noise_reduce.enabled().set(turning_on);
        if turning_on
            && state.noise_reduce.floor().get_untracked().is_none()
            && !state.noise_reduce.learning().get_untracked()
        {
            run_learn();
        }
    });
    let on_learn_floor = move |_: web_sys::MouseEvent| {
        run_learn();
    };

    let on_strength_change = move |ev: web_sys::Event| {
        let target: web_sys::HtmlInputElement = ev.target().unwrap().unchecked_into();
        if let Ok(val) = target.value().parse::<f64>() {
            state.noise_reduce.strength().set(val / 100.0);
        }
    };

    view! {
        <ComboButton
            left_label="NR"
            left_value=left_value
            left_click=left_click
            left_class=left_class
            right_value=right_value
            right_class=right_class
            is_open=is_open
            toggle_menu=toggle_menu
            left_title="Toggle noise reduction (spectral subtraction)"
            right_title="Noise reduction options"
            menu_direction="below"
            panel_align="right"
            panel_style="min-width: 240px;"
        >
            // ── Enable ──
            <div style="display: flex; gap: 2px; padding: 0 6px 4px;">
                <button class=move || layer_opt_class(state.noise_reduce.enabled().get())
                    on:click=move |_| state.noise_reduce.enabled().set(true)
                >"On"</button>
                <button class=move || layer_opt_class(!state.noise_reduce.enabled().get())
                    on:click=move |_| state.noise_reduce.enabled().set(false)
                >"Off"</button>
            </div>

            <hr />

            // ── Learn ──
            <div style="display: flex; gap: 4px; padding: 0 6px 4px;">
                <button
                    class="layer-panel-opt"
                    style="flex: 1;"
                    on:click=on_learn_floor
                    disabled=move || {
                        if state.noise_reduce.learning().get() { return true; }
                        let live = state.mic.listening().get() || state.mic.recording().get();
                        !live && state.library.current_index().get().is_none()
                    }
                >
                    {move || {
                        if state.noise_reduce.learning().get() { return "Learning\u{2026}".to_string(); }
                        let live = state.mic.listening().get() || state.mic.recording().get();
                        if live { "Learn from Live".to_string() } else { "Learn Noise Floor".to_string() }
                    }}
                </button>
            </div>

            // ── Strength ──
            <div class="layer-panel-slider-row">
                <label>"Strength"</label>
                <input type="range" min="0" max="300" step="5"
                    prop:value=move || (state.noise_reduce.strength().get() * 100.0) as i32
                    on:input=on_strength_change
                    title=move || format!("{:.0}%", state.noise_reduce.strength().get() * 100.0)
                />
                <span style="min-width: 36px; text-align: right; font-size: 10px; opacity: 0.7;">
                    {move || format!("{:.0}%", state.noise_reduce.strength().get() * 100.0)}
                </span>
            </div>

            // ── Floor status ──
            {move || {
                let floor = state.noise_reduce.floor().get();
                if let Some(f) = floor {
                    let bins = f.bin_magnitudes.len();
                    let dur = f.analysis_duration_secs;
                    view! {
                        <hr />
                        <div style="padding: 2px 8px; font-size: 11px; opacity: 0.7;">
                            {format!("{} bins, {:.1}s analyzed", bins, dur)}
                        </div>
                        <div style="display: flex; gap: 4px; padding: 2px 6px 0;">
                            <button class="layer-panel-opt" style="flex: 1; font-size: 10px;"
                                on:click=move |_: web_sys::MouseEvent| {
                                    state.noise_reduce.floor().set(None);
                                    state.noise_reduce.enabled().set(false);
                                }
                            >"Clear Floor"</button>
                        </div>
                    }.into_any()
                } else {
                    view! {
                        <div style="padding: 2px 8px; font-size: 11px; opacity: 0.5;">
                            "No noise floor learned"
                        </div>
                    }.into_any()
                }
            }}
        </ComboButton>
    }
}
