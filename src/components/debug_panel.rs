use crate::audio::synthetic_mic::{self, SynthSignal};
use crate::state::store_fields::*;
use crate::state::AppState;
use leptos::prelude::*;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

#[component]
pub fn DebugPanel() -> impl IntoView {
    let state = expect_context::<AppState>();
    let container_ref = NodeRef::<leptos::html::Div>::new();

    // Synthetic live-waterfall test signal (no mic needed). Starting any test
    // collapses the right sidebar so the panel doesn't cover the view or skew
    // the render budget being measured.
    let synth_rate = RwSignal::new(256_000u32);
    let synth_opts_open = RwSignal::new(false);
    let close_panel = move || state.panels.right_collapsed().set(true);
    let start_synth = move |sig: SynthSignal| {
        close_panel();
        synthetic_mic::start(state, sig, synth_rate.get_untracked());
    };
    let stop_synth = move |_| {
        crate::audio::synth_bench::cancel();
        synthetic_mic::stop(&state);
    };
    let run_bench = move |_| {
        close_panel();
        crate::audio::synth_bench::run(state);
    };

    // Auto-scroll to bottom when entries change
    Effect::new(move |_| {
        let entries = state.status.debug_log().get();
        let _ = entries.len(); // subscribe
        if let Some(el) = container_ref.get() {
            let el: &web_sys::HtmlElement = &el;
            el.set_scroll_top(el.scroll_height());
        }
    });

    let on_copy = move |_| {
        let entries = state.status.debug_log().get_untracked();
        let text: String = entries
            .iter()
            .map(|(ts, level, msg)| {
                let secs = (ts / 1000.0) % 100000.0;
                format!("[{:.1}s] [{}] {}", secs, level, msg)
            })
            .collect::<Vec<_>>()
            .join("\n");
        if let Some(window) = web_sys::window() {
            if let Ok(nav) = js_sys::Reflect::get(&window, &JsValue::from_str("navigator")) {
                if let Ok(clip) = js_sys::Reflect::get(&nav, &JsValue::from_str("clipboard")) {
                    let _ = js_sys::Reflect::get(&clip, &JsValue::from_str("writeText"))
                        .ok()
                        .and_then(|f| f.dyn_ref::<js_sys::Function>().cloned())
                        .map(|f| f.call1(&clip, &JsValue::from_str(&text)));
                    state.show_info_toast("Debug log copied");
                }
            }
        }
    };

    let on_clear = move |_| {
        state.status.debug_log().update(|e| e.clear());
    };

    // Compute start_time from first entry (or now) for relative timestamps
    let start_time = js_sys::Date::now();

    view! {
        <div class="sidebar-panel debug-panel">
            // Debug tiles checkbox
            <div class="setting-row" style="padding: 4px 8px;">
                <label class="setting-label" style="display:flex;align-items:center;gap:4px;cursor:pointer">
                    <input
                        type="checkbox"
                        prop:checked=move || state.spect.debug_tiles().get()
                        on:change=move |ev: web_sys::Event| {
                            let target = ev.target().unwrap();
                            let input: web_sys::HtmlInputElement = target.unchecked_into();
                            state.spect.debug_tiles().set(input.checked());
                        }
                    />
                    "Debug tiles"
                </label>
            </div>
            <hr style="border-color: #444; margin: 4px 0;" />
            // Focus Stack visualization
            <div class="debug-focus-stack">
                <div class="debug-section-title">"Focus Stack"</div>
                {move || {
                    let stack = state.viewmode.focus_stack().get();
                    let layers = stack.debug_layers();
                    let hfr = stack.hfr_enabled();
                    let items: Vec<_> = layers.iter().map(|layer| {
                        let label = layer.source.label();
                        let range = if layer.range.is_active() {
                            format!("{:.1}\u{2013}{:.1} kHz", layer.range.lo / 1000.0, layer.range.hi / 1000.0)
                        } else {
                            "inactive".to_string()
                        };
                        let cls = if layer.is_effective && hfr {
                            "debug-focus-layer active"
                        } else {
                            "debug-focus-layer"
                        };
                        let adopted = if layer.adopted { " (adopted)" } else { "" };
                        view! {
                            <div class=cls>
                                <span class="debug-focus-source">{label}</span>
                                <span class="debug-focus-range">{range}</span>
                                <span class="debug-focus-adopted">{adopted}</span>
                            </div>
                        }
                    }).collect();
                    let hfr_label = if hfr { "HFR: ON" } else { "HFR: OFF" };
                    view! {
                        <div>
                            <div class="debug-focus-hfr">{hfr_label}</div>
                            {items}
                        </div>
                    }
                }}
            </div>
            <hr style="border-color: #444; margin: 4px 0;" />
            <div class="setting-row" style="padding: 4px 8px;">
                <button
                    class="setting-btn"
                    title="Run a SIMD-vs-scalar A/B benchmark on the resonator hot loop. Results are logged below."
                    on:click=move |_| crate::components::file_sidebar::run_resonator_bench(state)
                >"Bench Resonators (SIMD vs scalar)"</button>
            </div>
            <hr style="border-color: #444; margin: 4px 0;" />
            // ── Live-waterfall benchmark + manual test signals (collapsed) ──
            <div class="debug-synth" style="padding: 4px 8px;">
                <div class="debug-section-title">"Live Waterfall Benchmark"</div>
                <button
                    class="setting-btn"
                    style="width:100%;"
                    title="Run the full rate/view/signal benchmark unattended; downloads + copies a report tagged with the app version + device."
                    on:click=run_bench
                >"Run Benchmark"</button>
                <button
                    class="setting-btn"
                    style="width:100%;margin-top:4px;"
                    on:click=stop_synth
                >"Stop"</button>
                <button
                    class="setting-btn"
                    style="width:100%;margin-top:4px;font-size:9px;text-align:left;"
                    on:click=move |_| synth_opts_open.update(|v| *v = !*v)
                >
                    {move || if synth_opts_open.get() {
                        "\u{25BE} Manual test signals"
                    } else {
                        "\u{25B8} Manual test signals"
                    }}
                </button>
                <Show when=move || synth_opts_open.get()>
                    <div style="display:flex;gap:4px;flex-wrap:wrap;margin:4px 0;">
                        {[48_000u32, 192_000, 256_000, 384_000].into_iter().map(|r| {
                            let label = format!("{}k", r / 1000);
                            view! {
                                <button
                                    class=move || if synth_rate.get() == r { "setting-btn sel" } else { "setting-btn" }
                                    style="flex:1;min-width:42px;"
                                    on:click=move |_| synth_rate.set(r)
                                >{label}</button>
                            }
                        }).collect_view()}
                    </div>
                    <div style="display:flex;gap:4px;flex-wrap:wrap;">
                        {[SynthSignal::Noise, SynthSignal::Tone, SynthSignal::Chirp,
                          SynthSignal::MultiTone, SynthSignal::Pulses].into_iter().map(|sig| {
                            view! {
                                <button class="setting-btn" style="flex:1;min-width:60px;"
                                    on:click=move |_| start_synth(sig)
                                >{sig.label()}</button>
                            }
                        }).collect_view()}
                    </div>
                </Show>
            </div>
            <hr style="border-color: #444; margin: 4px 0;" />
            <div class="debug-panel-toolbar">
                <button class="setting-btn" on:click=on_copy>"Copy All"</button>
                <button class="setting-btn" on:click=on_clear>"Clear"</button>
            </div>
            <div class="debug-panel-log" node_ref=container_ref>
                {move || {
                    let entries = state.status.debug_log().get();
                    if entries.is_empty() {
                        return view! {
                            <div class="debug-panel-empty">"No log entries yet"</div>
                        }.into_any();
                    }
                    let items: Vec<_> = entries.iter().map(|(ts, level, msg)| {
                        let relative = (*ts - start_time) / 1000.0;
                        let time_str = format!("{:+.1}s", relative);
                        let level_cls = match level.as_str() {
                            "error" => "debug-level-error",
                            "warn" => "debug-level-warn",
                            _ => "debug-level-info",
                        };
                        view! {
                            <div class="debug-entry">
                                <span class="debug-time">{time_str}</span>
                                <span class=level_cls>{format!("[{}]", level)}</span>
                                <span class="debug-msg">{msg.clone()}</span>
                            </div>
                        }
                    }).collect();
                    view! { <div>{items}</div> }.into_any()
                }}
            </div>
        </div>
    }
}
