use crate::dsp::pulse_detect::{self, DetectedPulse, PulseDetectionParams};
use crate::state::store_fields::*;
use crate::state::{AppState, RightSidebarTab};
use leptos::prelude::*;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::spawn_local;

#[component]
pub(crate) fn PulsePanel() -> impl IntoView {
    let state = expect_context::<AppState>();

    // Local detection parameters
    let threshold_db = RwSignal::new(6.0f64);
    let min_duration_ms = RwSignal::new(0.3f64);
    let max_duration_ms = RwSignal::new(50.0f64);
    let min_gap_ms = RwSignal::new(3.0f64);

    // Generation counter for cancellation
    let compute_gen = RwSignal::new(0u32);
    let last_computed_idx: RwSignal<Option<usize>> = RwSignal::new(None);
    let last_computed_ff: RwSignal<(f64, f64)> = RwSignal::new((0.0, 0.0));
    // Bumped by Re-detect to force the Effect to re-run without remounting the component
    let redetect_trigger = RwSignal::new(0u32);

    // Trigger pulse detection when tab is active and file changes
    Effect::new(move || {
        let tab = state.panels.right_tab().get();
        let files = state.library.files().get();
        let idx = state.library.current_index().get();
        let band_ff_lo = state.filter.band_ff_freq_lo().get();
        let band_ff_hi = state.filter.band_ff_freq_hi().get();
        let _trigger = redetect_trigger.get(); // subscribe so Re-detect re-runs this Effect

        if tab != RightSidebarTab::Pulses {
            return;
        }

        let band_ff_pair = (band_ff_lo, band_ff_hi);
        // Already computed for this file + same BandFF range (trigger bypass: cache was cleared)
        if idx == last_computed_idx.get_untracked()
            && band_ff_pair == last_computed_ff.get_untracked()
            && !state.pulse.detected().get_untracked().is_empty()
        {
            return;
        }

        let file = idx.and_then(|i| files.get(i).cloned());
        let Some(file) = file else {
            state.pulse.detected().set(Vec::new());
            state.pulse.detecting().set(false);
            last_computed_idx.set(None);
            return;
        };

        // Start detection
        state.pulse.detected().set(Vec::new());
        state.pulse.selected_index().set(None);
        state.pulse.detecting().set(true);
        last_computed_idx.set(idx);
        last_computed_ff.set(band_ff_pair);
        compute_gen.update(|g| *g += 1);
        let generation = compute_gen.get_untracked();

        let audio = file.audio.clone();
        let spectrogram = file.spectrogram.clone();
        let thresh = threshold_db.get_untracked();
        let min_dur = min_duration_ms.get_untracked();
        let max_dur = max_duration_ms.get_untracked();
        let gap = min_gap_ms.get_untracked();

        spawn_local(async move {
            yield_to_browser().await;
            if compute_gen.try_get_untracked() != Some(generation) {
                return;
            }

            let params = PulseDetectionParams {
                min_pulse_duration_ms: min_dur,
                max_pulse_duration_ms: max_dur,
                min_gap_ms: gap,
                threshold_db: thresh,
                bandpass_low_hz: band_ff_lo,
                bandpass_high_hz: if band_ff_hi > band_ff_lo {
                    band_ff_hi
                } else {
                    0.0
                },
            };

            let pulses = pulse_detect::detect_pulses(&audio, &spectrogram, &params);

            if compute_gen.try_get_untracked() != Some(generation) {
                return;
            }
            state.pulse.detected().set(pulses);
            state.pulse.detecting().set(false);
        });
    });

    // Re-detect handler
    let on_redetect = move |_: web_sys::MouseEvent| {
        // Force re-detection by clearing cache and bumping the trigger signal.
        // We do NOT set right_sidebar_tab here — that would remount the component
        // and reset all local slider values back to defaults.
        last_computed_idx.set(None);
        last_computed_ff.set((0.0, 0.0));
        redetect_trigger.update(|t| *t += 1);
    };

    // Click a pulse to navigate
    let on_pulse_click = move |pulse: DetectedPulse| {
        state.pulse.selected_index().set(Some(pulse.index));

        // Center spectrogram on this pulse
        let files = state.library.files().get_untracked();
        let idx = state.library.current_index().get_untracked();
        if let Some(file) = idx.and_then(|i| files.get(i)) {
            let canvas_w = state.viewmode.spectrogram_canvas_width().get_untracked();
            let zoom = state.view.zoom_level().get_untracked();
            let time_res = file.spectrogram.time_resolution;
            let visible_time = (canvas_w / zoom) * time_res;
            let target_scroll = (pulse.peak_time - visible_time / 2.0).max(0.0);
            state.view.scroll_offset().set(target_scroll);
        }
    };

    view! {
        <div class="sidebar-panel">
            // Settings
            <div class="setting-group">
                <div class="setting-group-title">"Detection Settings"</div>
                <div class="setting-row">
                    <span class="setting-label">"Threshold"</span>
                    <span class="setting-value">{move || format!("{:.0} dB", threshold_db.get())}</span>
                </div>
                <div class="setting-row">
                    <input
                        type="range"
                        class="setting-range"
                        min="3" max="20" step="1"
                        prop:value=move || threshold_db.get().to_string()
                        on:input=move |ev| {
                            if let Ok(v) = event_target_value(&ev).parse::<f64>() {
                                threshold_db.set(v);
                            }
                        }
                    />
                </div>
                <div class="setting-row">
                    <span class="setting-label">"Min duration"</span>
                    <span class="setting-value">{move || format!("{:.1} ms", min_duration_ms.get())}</span>
                </div>
                <div class="setting-row">
                    <input
                        type="range"
                        class="setting-range"
                        min="0.1" max="5.0" step="0.1"
                        prop:value=move || min_duration_ms.get().to_string()
                        on:input=move |ev| {
                            if let Ok(v) = event_target_value(&ev).parse::<f64>() {
                                min_duration_ms.set(v);
                            }
                        }
                    />
                </div>
                <div class="setting-row">
                    <span class="setting-label">"Max duration"</span>
                    <span class="setting-value">{move || format!("{:.0} ms", max_duration_ms.get())}</span>
                </div>
                <div class="setting-row">
                    <input
                        type="range"
                        class="setting-range"
                        min="5" max="200" step="5"
                        prop:value=move || max_duration_ms.get().to_string()
                        on:input=move |ev| {
                            if let Ok(v) = event_target_value(&ev).parse::<f64>() {
                                max_duration_ms.set(v);
                            }
                        }
                    />
                </div>
                <div class="setting-row">
                    <label class="setting-label">
                        <input
                            type="checkbox"
                            prop:checked=move || state.pulse.overlay_enabled().get()
                            on:change=move |ev| {
                                let checked = event_target_checked(&ev);
                                state.pulse.overlay_enabled().set(checked);
                            }
                        />
                        " Show overlay"
                    </label>
                </div>
                <div class="setting-row">
                    <button class="setting-button" on:click=on_redetect>"Re-detect"</button>
                </div>
            </div>
            // Status / Results
            {move || {
                let files = state.library.files().get();
                let idx = state.library.current_index().get();
                let has_file = idx.and_then(|i| files.get(i)).is_some();

                if !has_file {
                    return view! {
                        <div class="sidebar-panel-empty">"No file selected"</div>
                    }.into_any();
                }

                if state.pulse.detecting().get() {
                    return view! {
                        <div class="sidebar-panel-empty">"Detecting pulses\u{2026}"</div>
                    }.into_any();
                }

                let pulses = state.pulse.detected().get();
                let selected = state.pulse.selected_index().get();

                if pulses.is_empty() {
                    return view! {
                        <div class="sidebar-panel-empty">"No pulses detected"</div>
                    }.into_any();
                }

                let count = pulses.len();
                let pulse_items: Vec<_> = pulses.iter().map(|p| {
                    let pulse = p.clone();
                    let pulse2 = p.clone();
                    let is_selected = selected == Some(p.index);
                    let item_class = if is_selected { "pulse-item selected" } else { "pulse-item" };
                    let dur_ms = p.duration_ms();
                    let freq_khz = p.peak_freq / 1000.0;
                    let time_text = crate::format_time::format_time_display(p.start_time, 3);
                    let dur_text = format!("{:.1}ms", dur_ms);
                    let freq_text = format!("{:.1}kHz", freq_khz);
                    let snr_text = format!("{:.0}dB", p.snr_db);
                    let tooltip = format!(
                        "Pulse #{}: {:.4}s \u{2013} {:.4}s ({:.2}ms)\nPeak freq: {:.1} kHz\nSNR: {:.1} dB",
                        p.index, p.start_time, p.end_time, dur_ms, freq_khz, p.snr_db
                    );

                    view! {
                        <div
                            class=item_class
                            title=tooltip
                            on:click=move |_| on_pulse_click(pulse.clone())
                        >
                            <span class="pulse-index">{format!("#{}", pulse2.index)}</span>
                            <span class="pulse-time">{time_text}</span>
                            <span class="pulse-dur">{dur_text}</span>
                            <span class="pulse-freq">{freq_text}</span>
                            <span class="pulse-snr">{snr_text}</span>
                        </div>
                    }
                }).collect();

                view! {
                    <div class="setting-group">
                        <div class="setting-group-title">{format!("Pulses ({})", count)}</div>
                        <div class="pulse-list">
                            {pulse_items}
                        </div>
                    </div>
                }.into_any()
            }}
        </div>
    }
}

fn event_target_value(ev: &web_sys::Event) -> String {
    use wasm_bindgen::JsCast;
    ev.target()
        .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
        .map(|el| el.value())
        .unwrap_or_default()
}

fn event_target_checked(ev: &web_sys::Event) -> bool {
    use wasm_bindgen::JsCast;
    ev.target()
        .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
        .map(|el| el.checked())
        .unwrap_or(false)
}

/// Yield once to the browser event loop via a zero-duration setTimeout.
async fn yield_to_browser() {
    let promise = js_sys::Promise::new(&mut |resolve, _reject| {
        let win = web_sys::window().unwrap();
        let cb = wasm_bindgen::closure::Closure::once_into_js(move || {
            let _ = resolve.call0(&wasm_bindgen::JsValue::NULL);
        });
        let _ = win.set_timeout_with_callback_and_timeout_and_arguments_0(cb.unchecked_ref(), 0);
    });
    let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
}
