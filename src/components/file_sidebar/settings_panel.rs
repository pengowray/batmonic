use leptos::prelude::*;
use wasm_bindgen::JsCast;
use crate::audio::source::ChannelView;
use crate::state::{AppState, FftMode, FlowColorScheme, MainView, SpectrogramDisplay};
use crate::components::slider_row::SliderRow;
use crate::dsp::zero_crossing::zero_crossing_frequency;
use crate::annotations::{Annotation, AnnotationKind, AnnotationSet, Group, Region, generate_uuid, now_iso8601, build_annotation_tree, AnnotationNode, collect_descendants, renumber_children};

#[component]
pub(crate) fn SpectrogramSettingsPanel() -> impl IntoView {
    let state = expect_context::<AppState>();

    view! {
        <div class="sidebar-panel">
            // Gain/Range/Contrast — always shown (applies to all tile modes)
            <div class="setting-group">
                <div class="setting-group-title">"Intensity"</div>
                <SliderRow
                    label="Gain"
                    signal=state.spect_gain_db
                    min=-40.0
                    max=40.0
                    step=1.0
                    default=0.0
                    format_value=Callback::new(move |v: f32| {
                        if state.display_auto_gain.get() {
                            "auto".to_string()
                        } else {
                            format!("{:+.0} dB", v)
                        }
                    })
                    on_change=Callback::new(move |_: f32| {
                        state.display_auto_gain.set(false);
                    })
                />
                <SliderRow
                    label="Range"
                    signal=state.spect_range_db
                    min=20.0
                    max=120.0
                    step=5.0
                    default=120.0
                    format_value=Callback::new(|v: f32| format!("{:.0} dB", v))
                    on_change=Callback::new(move |v: f32| {
                        state.spect_floor_db.set(-v);
                    })
                />
                <SliderRow
                    label="Contrast"
                    signal=state.spect_gamma
                    min=0.2
                    max=3.0
                    step=0.05
                    default=1.0
                    format_value=Callback::new(|g: f32| {
                        if g == 1.0 { "linear".to_string() }
                        else { format!("{:.2}", g) }
                    })
                />
                <div class="setting-row">
                    <label class="setting-label" style="display:flex;align-items:center;gap:4px;cursor:pointer">
                        <input
                            type="checkbox"
                            prop:checked=move || state.display_auto_gain.get()
                            on:change=move |ev: web_sys::Event| {
                                let target = ev.target().unwrap();
                                let input: web_sys::HtmlInputElement = target.unchecked_into();
                                state.display_auto_gain.set(input.checked());
                            }
                        />
                        "Auto gain"
                    </label>
                </div>
                <div class="setting-row">
                    <label class="setting-label" style="display:flex;align-items:center;gap:4px;cursor:pointer">
                        <input
                            type="checkbox"
                            prop:checked=move || state.display_eq.get()
                            on:change=move |ev: web_sys::Event| {
                                let target = ev.target().unwrap();
                                let input: web_sys::HtmlInputElement = target.unchecked_into();
                                state.display_eq.set(input.checked());
                            }
                        />
                        "Show EQ"
                    </label>
                </div>
                <div class="setting-row">
                    <label class="setting-label" style="display:flex;align-items:center;gap:4px;cursor:pointer">
                        <input
                            type="checkbox"
                            prop:checked=move || state.display_noise_filter.get()
                            on:change=move |ev: web_sys::Event| {
                                let target = ev.target().unwrap();
                                let input: web_sys::HtmlInputElement = target.unchecked_into();
                                state.display_noise_filter.set(input.checked());
                            }
                        />
                        "Show noise filter"
                    </label>
                </div>
                <div class="setting-row">
                    <button
                        class="setting-button"
                        on:click=move |_| {
                            state.spect_gain_db.set(0.0);
                            state.spect_floor_db.set(-120.0);
                            state.spect_range_db.set(120.0);
                            state.spect_gamma.set(1.0);
                            state.display_auto_gain.set(false);
                            state.display_eq.set(false);
                            state.display_noise_filter.set(false);
                        }
                    >"Reset"</button>
                </div>
                <div class="setting-row">
                    <span class="setting-label">"FFT size"</span>
                    <select
                        class="setting-select"
                        on:change=move |ev: web_sys::Event| {
                            let target = ev.target().unwrap();
                            let select: web_sys::HtmlSelectElement = target.unchecked_into();
                            let val = select.value();
                            let mode = match val.as_str() {
                                "a512" => FftMode::Adaptive(512),
                                "a1024" => FftMode::Adaptive(1024),
                                "a2048" => FftMode::Adaptive(2048),
                                _ => {
                                    if let Ok(v) = val.parse::<usize>() {
                                        FftMode::Single(v)
                                    } else {
                                        return;
                                    }
                                }
                            };
                            state.spect_fft_mode.set(mode);
                        }
                    >
                        {move || {
                            let current = state.spect_fft_mode.get();
                            let options: [(&str, &str); 10] = [
                                ("128", "128"),
                                ("256", "256"),
                                ("512", "512"),
                                ("1024", "1024"),
                                ("2048", "2048"),
                                ("4096", "4096"),
                                ("8192", "8192"),
                                ("a512", "Adaptive 512"),
                                ("a1024", "Adaptive 1024"),
                                ("a2048", "Adaptive 2048"),
                            ];
                            options.into_iter().map(|(value, label)| {
                                let is_selected = match (value, current) {
                                    ("a512", FftMode::Adaptive(512)) => true,
                                    ("a1024", FftMode::Adaptive(1024)) => true,
                                    ("a2048", FftMode::Adaptive(2048)) => true,
                                    (v, FftMode::Single(sz)) => v.parse::<usize>().ok() == Some(sz),
                                    _ => false,
                                };
                                let v = value.to_string();
                                let l = label.to_string();
                                view! { <option value={v} selected=move || is_selected>{l}</option> }
                            }).collect::<Vec<_>>()
                        }}
                    </select>
                </div>
                <div class="setting-row">
                    <label class="setting-label" style="display:flex;align-items:center;gap:4px;cursor:pointer"
                        title="Sharpen time-frequency localization using the reassignment method (3x FFT cost)">
                        <input
                            type="checkbox"
                            prop:checked=move || state.reassign_enabled.get()
                            on:change=move |ev: web_sys::Event| {
                                let target = ev.target().unwrap();
                                let input: web_sys::HtmlInputElement = target.unchecked_into();
                                state.reassign_enabled.set(input.checked());
                            }
                        />
                        "Reassignment"
                    </label>
                </div>
                <div class="setting-row">
                    <label class="setting-label" style="display:flex;align-items:center;gap:4px;cursor:pointer">
                        <input
                            type="checkbox"
                            prop:checked=move || state.debug_tiles.get()
                            on:change=move |ev: web_sys::Event| {
                                let target = ev.target().unwrap();
                                let input: web_sys::HtmlInputElement = target.unchecked_into();
                                state.debug_tiles.set(input.checked());
                            }
                        />
                        "Debug tiles"
                    </label>
                </div>
            </div>

            // Flow-specific settings (shown only when Flow view is active)
            {move || {
                if state.main_view.get() == MainView::Flow {
                    let display = state.spectrogram_display.get();
                    let _ = display; // used for reactivity trigger above
                    view! {
                        <div class="setting-group">
                            <div class="setting-group-title">"Color"</div>
                            <div class="setting-row">
                                <span class="setting-label">"Algorithm"</span>
                                <select
                                    class="setting-select"
                                    on:change=move |ev: web_sys::Event| {
                                        let target = ev.target().unwrap();
                                        let select: web_sys::HtmlSelectElement = target.unchecked_into();
                                        let mode = match select.value().as_str() {
                                            "coherence" => SpectrogramDisplay::PhaseCoherence,
                                            "centroid" => SpectrogramDisplay::FlowCentroid,
                                            "gradient" => SpectrogramDisplay::FlowGradient,
                                            "phase" => SpectrogramDisplay::Phase,
                                            _ => SpectrogramDisplay::FlowOptical,
                                        };
                                        state.spectrogram_display.set(mode);
                                    }
                                    prop:value=move || match state.spectrogram_display.get() {
                                        SpectrogramDisplay::FlowOptical => "flow",
                                        SpectrogramDisplay::PhaseCoherence => "coherence",
                                        SpectrogramDisplay::FlowCentroid => "centroid",
                                        SpectrogramDisplay::FlowGradient => "gradient",
                                        SpectrogramDisplay::Phase => "phase",
                                    }
                                >
                                    <option value="flow">"Optical"</option>
                                    <option value="coherence">"Phase Coherence"</option>
                                    <option value="centroid">"Centroid"</option>
                                    <option value="gradient">"Gradient"</option>
                                    <option value="phase">"Phase"</option>
                                </select>
                            </div>
                            // Color scheme selector (only for flow algorithms, not phase)
                            {move || {
                                let display = state.spectrogram_display.get();
                                let is_flow_algo = matches!(display,
                                    SpectrogramDisplay::FlowOptical |
                                    SpectrogramDisplay::FlowCentroid |
                                    SpectrogramDisplay::FlowGradient
                                );
                                if is_flow_algo {
                                    view! {
                                        <div class="setting-row">
                                            <span class="setting-label">"Color scheme"</span>
                                            <select
                                                class="setting-select"
                                                on:change=move |ev: web_sys::Event| {
                                                    let target = ev.target().unwrap();
                                                    let select: web_sys::HtmlSelectElement = target.unchecked_into();
                                                    let scheme = match select.value().as_str() {
                                                        "coolwarm" => FlowColorScheme::CoolWarm,
                                                        "tealorange" => FlowColorScheme::TealOrange,
                                                        "purplegreen" => FlowColorScheme::PurpleGreen,
                                                        "spectral" => FlowColorScheme::Spectral,
                                                        _ => FlowColorScheme::RedBlue,
                                                    };
                                                    state.flow_color_scheme.set(scheme);
                                                }
                                                prop:value=move || match state.flow_color_scheme.get() {
                                                    FlowColorScheme::RedBlue => "redblue",
                                                    FlowColorScheme::CoolWarm => "coolwarm",
                                                    FlowColorScheme::TealOrange => "tealorange",
                                                    FlowColorScheme::PurpleGreen => "purplegreen",
                                                    FlowColorScheme::Spectral => "spectral",
                                                }
                                            >
                                                <option value="redblue">"Red-Blue"</option>
                                                <option value="coolwarm">"Cool-Warm"</option>
                                                <option value="tealorange">"Teal-Orange"</option>
                                                <option value="purplegreen">"Purple-Green"</option>
                                                <option value="spectral">"Spectral"</option>
                                            </select>
                                        </div>
                                    }.into_any()
                                } else {
                                    view! { <span></span> }.into_any()
                                }
                            }}
                            <div class="setting-row">
                                <span class="setting-label">"Intensity gate"</span>
                                <div class="setting-slider-row">
                                    <input
                                        type="range"
                                        class="setting-range"
                                        min="0"
                                        max="100"
                                        step="1"
                                        prop:value=move || (state.flow_intensity_gate.get() * 100.0).round().to_string()
                                        on:input=move |ev: web_sys::Event| {
                                            let target = ev.target().unwrap();
                                            let input: web_sys::HtmlInputElement = target.unchecked_into();
                                            if let Ok(val) = input.value().parse::<f32>() {
                                                state.flow_intensity_gate.set(val / 100.0);
                                            }
                                        }
                                    />
                                    <span class="setting-value">{move || format!("{}%", (state.flow_intensity_gate.get() * 100.0).round() as u32)}</span>
                                </div>
                            </div>
                            <div class="setting-row">
                                <span class="setting-label">"Color gain"</span>
                                <div class="setting-slider-row">
                                    <input
                                        type="range"
                                        class="setting-range"
                                        min="0.5"
                                        max="10.0"
                                        step="0.5"
                                        prop:value=move || state.flow_shift_gain.get().to_string()
                                        on:input=move |ev: web_sys::Event| {
                                            let target = ev.target().unwrap();
                                            let input: web_sys::HtmlInputElement = target.unchecked_into();
                                            if let Ok(val) = input.value().parse::<f32>() {
                                                state.flow_shift_gain.set(val);
                                            }
                                        }
                                    />
                                    <span class="setting-value">{move || format!("{:.1}x", state.flow_shift_gain.get())}</span>
                                </div>
                            </div>
                            <div class="setting-row">
                                <span class="setting-label">{move || {
                                    let g = state.flow_color_gamma.get();
                                    if g == 1.0 { "Color contrast: linear".to_string() }
                                    else { format!("Color contrast: {:.2}", g) }
                                }}</span>
                                <div class="setting-slider-row">
                                    <input
                                        type="range"
                                        class="setting-range"
                                        min="0.2"
                                        max="3.0"
                                        step="0.05"
                                        prop:value=move || state.flow_color_gamma.get().to_string()
                                        on:input=move |ev: web_sys::Event| {
                                            let target = ev.target().unwrap();
                                            let input: web_sys::HtmlInputElement = target.unchecked_into();
                                            if let Ok(val) = input.value().parse::<f32>() {
                                                state.flow_color_gamma.set(val);
                                            }
                                        }
                                    />
                                </div>
                            </div>
                            // Flow gate — threshold for minimum shift/deviation magnitude to show color
                            <div class="setting-row">
                                <span class="setting-label">"Flow gate"</span>
                                <div class="setting-slider-row">
                                    <input
                                        type="range"
                                        class="setting-range"
                                        min="0"
                                        max="100"
                                        step="1"
                                        prop:value=move || (state.flow_gate.get() * 100.0).round().to_string()
                                        on:input=move |ev: web_sys::Event| {
                                            let target = ev.target().unwrap();
                                            let input: web_sys::HtmlInputElement = target.unchecked_into();
                                            if let Ok(val) = input.value().parse::<f32>() {
                                                state.flow_gate.set(val / 100.0);
                                            }
                                        }
                                    />
                                    <span class="setting-value">{move || format!("{}%", (state.flow_gate.get() * 100.0).round() as u32)}</span>
                                </div>
                            </div>
                        </div>
                    }.into_any()
                } else {
                    view! { <span></span> }.into_any()
                }
            }}

            // Chromagram-specific settings (shown only when Chromagram view is active)
            {move || {
                if state.main_view.get() == MainView::Chromagram {
                    view! {
                        <div class="setting-group">
                            <div class="setting-group-title">"Chromagram"</div>
                            <div class="setting-row">
                                <span class="setting-label">{move || {
                                    let g = state.chroma_gain.get();
                                    if g == 1.0 { "Gain: default".to_string() }
                                    else { format!("Gain: {:.2}x", g) }
                                }}</span>
                                <input
                                    type="range"
                                    class="setting-range"
                                    min="0.25"
                                    max="4.0"
                                    step="0.05"
                                    prop:value=move || state.chroma_gain.get().to_string()
                                    on:input=move |ev: web_sys::Event| {
                                        let target = ev.target().unwrap();
                                        let input: web_sys::HtmlInputElement = target.unchecked_into();
                                        if let Ok(v) = input.value().parse::<f32>() {
                                            state.chroma_gain.set(v);
                                        }
                                    }
                                />
                            </div>
                            <div class="setting-row">
                                <span class="setting-label">{move || {
                                    let g = state.chroma_gamma.get();
                                    if g == 1.0 { "Contrast: linear".to_string() }
                                    else { format!("Contrast: {:.2}", g) }
                                }}</span>
                                <input
                                    type="range"
                                    class="setting-range"
                                    min="0.2"
                                    max="3.0"
                                    step="0.05"
                                    prop:value=move || state.chroma_gamma.get().to_string()
                                    on:input=move |ev: web_sys::Event| {
                                        let target = ev.target().unwrap();
                                        let input: web_sys::HtmlInputElement = target.unchecked_into();
                                        if let Ok(v) = input.value().parse::<f32>() {
                                            state.chroma_gamma.set(v);
                                        }
                                    }
                                />
                            </div>
                            <div class="setting-row">
                                <button
                                    class="setting-button"
                                    on:click=move |_| {
                                        state.chroma_gain.set(1.0);
                                        state.chroma_gamma.set(1.0);
                                    }
                                >"Reset"</button>
                            </div>
                        </div>
                    }.into_any()
                } else {
                    view! { <span></span> }.into_any()
                }
            }}
        </div>
    }
}

#[component]
pub(crate) fn SelectionPanel() -> impl IntoView {
    let state = expect_context::<AppState>();

    let analysis = move || {
        let selection = state.selection.get()?;
        let dragging = state.is_dragging.get();
        let files = state.files.get();
        let idx = state.current_file_index.get()?;
        let file = files.get(idx)?;

        let sr = file.audio.sample_rate;
        let total = file.audio.source.total_samples() as usize;
        let start = ((selection.time_start * sr as f64) as usize).min(total);
        let end = ((selection.time_end * sr as f64) as usize).min(total);

        if end <= start {
            return None;
        }

        let duration = selection.time_end - selection.time_start;
        let frames = end - start;

        let (crossing_count, estimated_freq) = if dragging {
            (None, None)
        } else {
            let slice = file.audio.source.read_region(ChannelView::MonoMix, start as u64, end - start);
            let zc = zero_crossing_frequency(&slice, sr);
            (Some(zc.crossing_count), Some(zc.estimated_frequency_hz))
        };

        Some((duration, frames, crossing_count, estimated_freq, selection.freq_low, selection.freq_high, selection))
    };

    let annotate_region = move |_| {
        let selection = state.selection.get_untracked();
        let file_idx = state.current_file_index.get_untracked();
        if let (Some(sel), Some(idx)) = (selection, file_idx) {
            let has_freq = sel.freq_low.is_some() && sel.freq_high.is_some();
            state.snapshot_annotations();
            let annotation = Annotation {
                id: generate_uuid(),
                kind: AnnotationKind::Region(Region {
                    time_start: sel.time_start,
                    time_end: sel.time_end,
                    freq_low: sel.freq_low,
                    freq_high: sel.freq_high,
                    label: None,
                    color: None,
                }),
                created_at: now_iso8601(),
                modified_at: now_iso8601(),
                notes: None,
                parent_id: None,
                sort_order: None,
            };
            state.annotation_store.update(|store| {
                store.ensure_len(idx + 1);
                if store.sets[idx].is_none() {
                    let new_set = state.files.with_untracked(|files| {
                        files.get(idx).map(|f| {
                            let id = f.identity.clone().unwrap_or_else(|| {
                                crate::file_identity::identity_layer1(&f.name, 0)
                            });
                            AnnotationSet::new_with_metadata(id, &f.audio)
                        })
                    });
                    if let Some(set) = new_set {
                        store.sets[idx] = Some(set);
                    }
                }
                if let Some(ref mut set) = store.sets[idx] {
                    set.annotations.push(annotation);
                }
            });
            state.annotations_dirty.set(true);
            state.show_info_toast(if has_freq { "Region annotated" } else { "Segment annotated" });
        }
    };

    view! {
        <div class="sidebar-panel">
            {move || {
                match analysis() {
                    Some((duration, frames, crossing_count, estimated_freq, freq_low, freq_high, _sel)) => {
                        let has_freq = freq_low.is_some() && freq_high.is_some();
                        let freq_range_str = match (freq_low, freq_high) {
                            (Some(fl), Some(fh)) => format!("{:.0} – {:.0} kHz", fl / 1000.0, fh / 1000.0),
                            _ => "—".to_string(),
                        };
                        let btn_label = if has_freq { "Annotate Region" } else { "Annotate Segment" };
                        view! {
                            <div class="setting-group">
                                <div class="setting-group-title">"Selection"</div>
                                <div class="setting-row">
                                    <span class="setting-label">"Duration"</span>
                                    <span class="setting-value">{crate::format_time::format_duration(duration, 3)}</span>
                                </div>
                                <div class="setting-row">
                                    <span class="setting-label">"Frames"</span>
                                    <span class="setting-value">{format!("{}", frames)}</span>
                                </div>
                                <div class="setting-row">
                                    <span class="setting-label">"Freq range"</span>
                                    <span class="setting-value">{freq_range_str}</span>
                                </div>
                                <div class="setting-row">
                                    <span class="setting-label">"ZC count"</span>
                                    <span class="setting-value">{match crossing_count { Some(c) => format!("{c}"), None => "...".into() }}</span>
                                </div>
                                <div class="setting-row">
                                    <span class="setting-label">"ZC est. freq"</span>
                                    <span class="setting-value">{match estimated_freq { Some(f) => format!("~{:.1} kHz", f / 1000.0), None => "...".into() }}</span>
                                </div>
                                <button class="sidebar-btn" on:click=annotate_region>{btn_label}</button>
                            </div>
                        }.into_any()
                    }
                    None => {
                        view! {
                            <div class="sidebar-panel-empty">"No selection"</div>
                        }.into_any()
                    }
                }
            }}
            <AnnotationsList />
        </div>
    }
}

/// Get the display label for an annotation.
fn annotation_display(a: &Annotation) -> (String, Option<String>) {
    match &a.kind {
        AnnotationKind::Region(reg) => {
            let auto_label = match (reg.freq_low, reg.freq_high) {
                (Some(fl), Some(fh)) => format!("{}, {:.0}–{:.0} kHz",
                    crate::format_time::format_time_range(reg.time_start, reg.time_end, 3),
                    fl / 1000.0, fh / 1000.0),
                _ => crate::format_time::format_time_range(reg.time_start, reg.time_end, 3),
            };
            let display = reg.label.clone().unwrap_or_else(|| auto_label);
            (display, reg.label.clone())
        }
        AnnotationKind::Marker(m) => {
            let auto_label = crate::format_time::format_time_display(m.time, 3);
            let display = m.label.clone().unwrap_or_else(|| auto_label);
            (display, m.label.clone())
        }
        AnnotationKind::Group(g) => {
            let display = g.label.clone().unwrap_or_else(|| "Group".to_string());
            (display, g.label.clone())
        }
        AnnotationKind::Measurement(m) => {
            let display = m.label.clone().unwrap_or_else(|| {
                crate::format_time::format_time_range(m.start_time, m.end_time, 3)
            });
            (display, m.label.clone())
        }
    }
}

/// Icon prefix for annotation kind.
fn annotation_icon(kind: &AnnotationKind) -> &'static str {
    match kind {
        AnnotationKind::Region(r) if r.freq_low.is_some() => "\u{25AD} ",  // rectangle (region)
        AnnotationKind::Region(_) => "\u{2500} ",  // horizontal line (segment)
        AnnotationKind::Marker(_) => "\u{25C6} ",     // diamond
        AnnotationKind::Group(_) => "",                // handled by collapse toggle
        AnnotationKind::Measurement(_) => "\u{21D4} ", // double arrow
    }
}

#[component]
fn AnnotationsList() -> impl IntoView {
    let state = expect_context::<AppState>();

    let annotation_tree = move || {
        let idx = state.current_file_index.get()?;
        let store = state.annotation_store.get();
        let set = store.sets.get(idx)?.as_ref()?;
        if set.annotations.is_empty() { return None; }
        Some(build_annotation_tree(&set.annotations))
    };

    let on_export = move |_: web_sys::MouseEvent| {
        export_annotations(state);
    };

    let on_import = move |_: web_sys::MouseEvent| {
        import_annotations(state);
    };

    let on_group = move |_: web_sys::MouseEvent| {
        group_selected(state);
    };

    let on_ungroup = move |_: web_sys::MouseEvent| {
        ungroup_selected(state);
    };

    let has_annotations = move || {
        let idx = state.current_file_index.get()?;
        let store = state.annotation_store.get();
        let set = store.sets.get(idx)?.as_ref()?;
        if set.annotations.is_empty() { None } else { Some(true) }
    };

    let selected_is_group = move || {
        let sel_id = state.selected_annotation_id.get()?;
        let idx = state.current_file_index.get()?;
        let store = state.annotation_store.get();
        let set = store.sets.get(idx)?.as_ref()?;
        set.annotations.iter().find(|a| a.id == sel_id)
            .filter(|a| matches!(a.kind, AnnotationKind::Group(_)))
            .map(|_| true)
    };

    view! {
        {move || {
            if let Some(tree) = annotation_tree() {
                view! {
                    <div class="setting-group">
                        <div class="setting-group-title">"Annotations"</div>
                        <div class="annotation-tree"
                            on:dragover=move |ev: web_sys::DragEvent| {
                                ev.prevent_default();
                            }
                            on:drop=move |_ev: web_sys::DragEvent| {
                                perform_drop(state);
                            }
                        >
                            {render_tree_nodes(tree, state)}
                        </div>
                        <div class="setting-row" style="gap: 2px; padding: 2px 8px; justify-content: flex-end;">
                            <button class="sidebar-btn annotation-toolbar-btn"
                                title="Undo (Ctrl+Z)"
                                on:click=move |_| state.undo_annotations()
                                disabled=move || !state.can_undo()
                            >"\u{21B6}"</button>
                            <button class="sidebar-btn annotation-toolbar-btn"
                                title="Redo (Ctrl+Shift+Z)"
                                on:click=move |_| state.redo_annotations()
                                disabled=move || !state.can_redo()
                            >"\u{21B7}"</button>
                            <button class="sidebar-btn annotation-toolbar-btn"
                                title="Group selected"
                                on:click=on_group
                                disabled=move || state.selected_annotation_id.get().is_none()
                            >"Group"</button>
                            <button class="sidebar-btn annotation-toolbar-btn"
                                title="Ungroup"
                                on:click=on_ungroup
                                disabled=move || selected_is_group().is_none()
                            >"Ungroup"</button>
                        </div>
                    </div>
                }.into_any()
            } else {
                view! { <div></div> }.into_any()
            }
        }}
        <div class="setting-row" style="gap: 4px;">
            <button
                class="sidebar-btn"
                style="flex: 1;"
                on:click=on_export
                disabled=move || has_annotations().is_none()
            >
                "Export .batm"
            </button>
            <button
                class="sidebar-btn"
                style="flex: 1;"
                on:click=on_import
            >
                "Import .batm"
            </button>
        </div>
    }
}

fn render_tree_nodes(nodes: Vec<AnnotationNode>, state: AppState) -> impl IntoView {
    nodes.into_iter().map(move |node| {
        let id = node.annotation.id.clone();
        let (display, existing_label) = annotation_display(&node.annotation);
        let icon = annotation_icon(&node.annotation.kind);
        let is_group = matches!(node.annotation.kind, AnnotationKind::Group(_));
        let is_collapsed = match &node.annotation.kind {
            AnnotationKind::Group(g) => g.collapsed.unwrap_or(false),
            _ => false,
        };
        let depth = node.depth;
        let children = node.children;

        let id_click = id.clone();
        let id_delete = id.clone();
        let id_edit = id.clone();
        let id_drag = id.clone();
        let id_dragover = id.clone();
        let id_dragover2 = id.clone();

        let editing = RwSignal::new(false);
        let edit_value = RwSignal::new(existing_label.unwrap_or_default());

        let indent_px = depth * 16;

        view! {
            <div
                class="annotation-tree-item"
                class:annotation-selected=move || state.selected_annotation_id.get().as_deref() == Some(id_click.as_str())
                class:annotation-drop-target=move || {
                    state.drop_target.get().as_ref().map(|(tid, _)| tid.as_str()) == Some(id_dragover.as_str())
                }
                class:annotation-group-item=is_group
                style:padding-left=format!("{}px", 8 + indent_px)
                draggable="true"
                on:click=move |_| {
                    let click_id = id.clone();
                    if is_group {
                        toggle_group_collapsed(state, &click_id);
                    } else {
                        restore_selection(state, &click_id);
                    }
                    state.selected_annotation_id.set(Some(id.clone()));
                }
                on:dragstart=move |ev: web_sys::DragEvent| {
                    state.dragging_annotation_id.set(Some(id_drag.clone()));
                    if let Some(dt) = ev.data_transfer() {
                        let _ = dt.set_data("text/plain", &id_drag);
                        dt.set_effect_allowed("move");
                    }
                }
                on:dragover=move |ev: web_sys::DragEvent| {
                    ev.prevent_default();
                    if let Some(dt) = ev.data_transfer() {
                        dt.set_drop_effect("move");
                    }
                    // Determine drop position based on mouse Y within element
                    let target: web_sys::HtmlElement = ev.current_target().unwrap().unchecked_into();
                    let rect = target.get_bounding_client_rect();
                    let y = ev.client_y() as f64 - rect.top();
                    let h = rect.height();
                    let position = if is_group && y > h * 0.25 && y < h * 0.75 {
                        "inside".to_string()
                    } else if y < h * 0.5 {
                        "before".to_string()
                    } else {
                        "after".to_string()
                    };
                    state.drop_target.set(Some((id_dragover2.clone(), position)));
                }
                on:dragleave=move |_: web_sys::DragEvent| {
                    // Only clear if we're leaving the tree item
                    state.drop_target.set(None);
                }
                on:drop=move |ev: web_sys::DragEvent| {
                    ev.prevent_default();
                    ev.stop_propagation();
                    perform_drop(state);
                }
            >
                {if is_group {
                    let collapse_char = if is_collapsed { "\u{25B6}" } else { "\u{25BC}" };
                    view! { <span class="annotation-collapse-toggle">{collapse_char}" "</span> }.into_any()
                } else {
                    view! { <span class="annotation-icon">{icon}</span> }.into_any()
                }}
                {move || {
                    if editing.get() {
                        let id_save = id_edit.clone();
                        let id_save2 = id_edit.clone();
                        let input_ref = NodeRef::<leptos::html::Input>::new();
                        Effect::new(move |_| {
                            if let Some(el) = input_ref.get() {
                                let _ = el.focus();
                            }
                        });
                        view! {
                            <input
                                class="annotation-label-input"
                                type="text"
                                prop:value=move || edit_value.get()
                                placeholder="Label..."
                                node_ref=input_ref
                                on:input=move |ev| {
                                    edit_value.set(leptos::prelude::event_target_value(&ev));
                                }
                                on:keydown=move |ev| {
                                    if ev.key() == "Enter" {
                                        let val = edit_value.get_untracked();
                                        let label = if val.trim().is_empty() { None } else { Some(val) };
                                        update_annotation_label(state, &id_save, label);
                                        editing.set(false);
                                    } else if ev.key() == "Escape" {
                                        editing.set(false);
                                    }
                                }
                                on:blur=move |_| {
                                    let val = edit_value.get_untracked();
                                    let label = if val.trim().is_empty() { None } else { Some(val) };
                                    update_annotation_label(state, &id_save2, label);
                                    editing.set(false);
                                }
                                on:click=move |e| { e.stop_propagation(); }
                            />
                        }.into_any()
                    } else {
                        view! {
                            <span class="annotation-label">{display.clone()}</span>
                        }.into_any()
                    }
                }}
                <button class="annotation-edit"
                    title="Edit label"
                    on:click=move |e| {
                        e.stop_propagation();
                        editing.set(true);
                    }
                >"\u{270E}"</button>
                <button class="annotation-delete"
                    on:click=move |e| {
                        e.stop_propagation();
                        delete_annotation(state, &id_delete);
                    }
                >"\u{00d7}"</button>
            </div>
            {if is_group && !is_collapsed && !children.is_empty() {
                view! { <div class="annotation-group-children">{render_tree_nodes(children, state)}</div> }.into_any()
            } else {
                view! { <span></span> }.into_any()
            }}
        }
    }).collect_view()
}

// --- Helper functions ---

fn restore_selection(state: AppState, annotation_id: &str) {
    let idx = match state.current_file_index.get_untracked() {
        Some(i) => i,
        None => return,
    };
    let store = state.annotation_store.get_untracked();
    let set = match store.sets.get(idx).and_then(|s| s.as_ref()) {
        Some(s) => s,
        None => return,
    };
    for a in &set.annotations {
        if a.id == annotation_id {
            let jump_time = match &a.kind {
                AnnotationKind::Region(reg) => {
                    state.selection.set(Some(crate::state::Selection {
                        time_start: reg.time_start,
                        time_end: reg.time_end,
                        freq_low: reg.freq_low,
                        freq_high: reg.freq_high,
                    }));
                    Some((reg.time_start + reg.time_end) / 2.0)
                }
                AnnotationKind::Marker(m) => Some(m.time),
                AnnotationKind::Measurement(m) => Some((m.start_time + m.end_time) / 2.0),
                _ => None,
            };
            if let Some(t) = jump_time {
                jump_to_time(state, t);
            }
            return;
        }
    }
}

/// Push nav history, then scroll so that `time` is centered in the spectrogram view.
fn jump_to_time(state: AppState, time: f64) {
    state.push_nav();
    state.suspend_follow();

    let files = state.files.get_untracked();
    let idx = state.current_file_index.get_untracked();
    if let Some(file) = idx.and_then(|i| files.get(i)) {
        let zoom = state.zoom_level.get_untracked();
        let canvas_w = state.spectrogram_canvas_width.get_untracked();
        let half_visible = (canvas_w / zoom) * file.spectrogram.time_resolution / 2.0;
        let visible = half_visible * 2.0;
        let max_scroll = (file.audio.duration_secs - visible).max(0.0);
        let centered = (time - half_visible).clamp(0.0, max_scroll);
        state.scroll_offset.set(centered);
    }
}

fn delete_annotation(state: AppState, annotation_id: &str) {
    let idx = match state.current_file_index.get_untracked() {
        Some(i) => i,
        None => return,
    };
    state.snapshot_annotations();
    // Also delete all descendants
    let descendants = {
        let store = state.annotation_store.get_untracked();
        let set = match store.sets.get(idx).and_then(|s| s.as_ref()) {
            Some(s) => s,
            None => return,
        };
        collect_descendants(&set.annotations, annotation_id)
    };
    state.annotation_store.update(|store| {
        if let Some(Some(ref mut set)) = store.sets.get_mut(idx) {
            set.annotations.retain(|a| a.id != annotation_id && !descendants.contains(&a.id));
        }
    });
    if state.selected_annotation_id.get_untracked().as_deref() == Some(annotation_id) {
        state.selected_annotation_id.set(None);
    }
    state.annotations_dirty.set(true);
}

fn update_annotation_label(state: AppState, annotation_id: &str, label: Option<String>) {
    let idx = match state.current_file_index.get_untracked() {
        Some(i) => i,
        None => return,
    };
    state.snapshot_annotations();
    state.annotation_store.update(|store| {
        if let Some(Some(ref mut set)) = store.sets.get_mut(idx) {
            if let Some(a) = set.annotations.iter_mut().find(|a| a.id == annotation_id) {
                match a.kind {
                    AnnotationKind::Region(ref mut reg) => { reg.label = label; }
                    AnnotationKind::Marker(ref mut m) => { m.label = label; }
                    AnnotationKind::Group(ref mut g) => { g.label = label; }
                    AnnotationKind::Measurement(ref mut m) => { m.label = label; }
                }
                a.modified_at = now_iso8601();
            }
        }
    });
    state.annotations_dirty.set(true);
}

fn toggle_group_collapsed(state: AppState, annotation_id: &str) {
    let idx = match state.current_file_index.get_untracked() {
        Some(i) => i,
        None => return,
    };
    state.annotation_store.update(|store| {
        if let Some(Some(ref mut set)) = store.sets.get_mut(idx) {
            if let Some(a) = set.annotations.iter_mut().find(|a| a.id == annotation_id) {
                if let AnnotationKind::Group(ref mut g) = a.kind {
                    let cur = g.collapsed.unwrap_or(false);
                    g.collapsed = Some(!cur);
                }
            }
        }
    });
    state.annotations_dirty.set(true);
}

fn group_selected(state: AppState) {
    let sel_id = match state.selected_annotation_id.get_untracked() {
        Some(id) => id,
        None => return,
    };
    let idx = match state.current_file_index.get_untracked() {
        Some(i) => i,
        None => return,
    };

    state.snapshot_annotations();

    // Create a new group and move the selected annotation into it
    let group_id = generate_uuid();
    let now = now_iso8601();

    state.annotation_store.update(|store| {
        if let Some(Some(ref mut set)) = store.sets.get_mut(idx) {
            // Get the selected annotation's parent and sort_order
            let (parent, order) = set.annotations.iter()
                .find(|a| a.id == sel_id)
                .map(|a| (a.parent_id.clone(), a.sort_order))
                .unwrap_or((None, None));

            // Create group at the same level as the selected item
            let group = Annotation {
                id: group_id.clone(),
                kind: AnnotationKind::Group(Group {
                    label: None,
                    color: None,
                    collapsed: Some(false),
                }),
                created_at: now.clone(),
                modified_at: now,
                notes: None,
                parent_id: parent,
                sort_order: order,
            };
            set.annotations.push(group);

            // Move the selected annotation into the group
            if let Some(a) = set.annotations.iter_mut().find(|a| a.id == sel_id) {
                a.parent_id = Some(group_id.clone());
                a.sort_order = Some(0.0);
            }

            // Renumber siblings at the old level
            let parent_key = set.annotations.iter()
                .find(|a| a.id == group_id)
                .and_then(|a| a.parent_id.clone());
            renumber_children(&mut set.annotations, parent_key.as_deref());
        }
    });
    state.selected_annotation_id.set(Some(group_id));
    state.annotations_dirty.set(true);
}

fn ungroup_selected(state: AppState) {
    let group_id = match state.selected_annotation_id.get_untracked() {
        Some(id) => id,
        None => return,
    };
    let idx = match state.current_file_index.get_untracked() {
        Some(i) => i,
        None => return,
    };

    state.snapshot_annotations();
    state.annotation_store.update(|store| {
        if let Some(Some(ref mut set)) = store.sets.get_mut(idx) {
            // Verify it's a group
            let group_parent = match set.annotations.iter().find(|a| a.id == group_id) {
                Some(a) if matches!(a.kind, AnnotationKind::Group(_)) => a.parent_id.clone(),
                _ => return,
            };

            // Move all direct children to the group's parent level
            for a in set.annotations.iter_mut() {
                if a.parent_id.as_deref() == Some(group_id.as_str()) {
                    a.parent_id = group_parent.clone();
                }
            }

            // Remove the group itself
            set.annotations.retain(|a| a.id != group_id);

            // Renumber at the parent level
            renumber_children(&mut set.annotations, group_parent.as_deref());
        }
    });
    state.selected_annotation_id.set(None);
    state.annotations_dirty.set(true);
}

fn perform_drop(state: AppState) {
    let dragged_id = match state.dragging_annotation_id.get_untracked() {
        Some(id) => id,
        None => return,
    };
    let (target_id, position) = match state.drop_target.get_untracked() {
        Some(t) => t,
        None => { state.dragging_annotation_id.set(None); return; }
    };

    // Clear drag state
    state.dragging_annotation_id.set(None);
    state.drop_target.set(None);

    if dragged_id == target_id { return; }

    let idx = match state.current_file_index.get_untracked() {
        Some(i) => i,
        None => return,
    };

    state.snapshot_annotations();
    state.annotation_store.update(|store| {
        if let Some(Some(ref mut set)) = store.sets.get_mut(idx) {
            // Don't allow dropping into own descendants
            let descendants = collect_descendants(&set.annotations, &dragged_id);
            if descendants.contains(&target_id) { return; }

            // Find target's parent and sort_order
            let target_info = set.annotations.iter()
                .find(|a| a.id == target_id)
                .map(|a| (a.parent_id.clone(), a.sort_order.unwrap_or(0.0), matches!(a.kind, AnnotationKind::Group(_))));
            let (target_parent, target_order, target_is_group) = match target_info {
                Some(info) => info,
                None => return,
            };

            match position.as_str() {
                "inside" if target_is_group => {
                    // Drop inside a group
                    if let Some(a) = set.annotations.iter_mut().find(|a| a.id == dragged_id) {
                        a.parent_id = Some(target_id.clone());
                        a.sort_order = Some(f64::MAX); // append to end
                    }
                    renumber_children(&mut set.annotations, Some(target_id.as_str()));
                }
                "before" => {
                    if let Some(a) = set.annotations.iter_mut().find(|a| a.id == dragged_id) {
                        a.parent_id = target_parent.clone();
                        a.sort_order = Some(target_order - 0.5);
                    }
                    renumber_children(&mut set.annotations, target_parent.as_deref());
                }
                "after" | _ => {
                    if let Some(a) = set.annotations.iter_mut().find(|a| a.id == dragged_id) {
                        a.parent_id = target_parent.clone();
                        a.sort_order = Some(target_order + 0.5);
                    }
                    renumber_children(&mut set.annotations, target_parent.as_deref());
                }
            }
        }
    });
    state.annotations_dirty.set(true);
}

fn export_annotations(state: AppState) {
    let idx = match state.current_file_index.get_untracked() {
        Some(i) => i,
        None => { state.show_error_toast("No file selected"); return; }
    };
    let store = state.annotation_store.get_untracked();
    let set = match store.sets.get(idx).and_then(|s| s.as_ref()) {
        Some(s) => s,
        None => { state.show_error_toast("No annotations to export"); return; }
    };

    let yaml = match yaml_serde::to_string(set) {
        Ok(y) => y,
        Err(e) => { state.show_error_toast(format!("Serialize error: {e}")); return; }
    };

    let arr = js_sys::Array::of1(&wasm_bindgen::JsValue::from_str(&yaml));
    let Ok(blob) = web_sys::Blob::new_with_str_sequence(&arr) else { return };
    let Ok(url) = web_sys::Url::create_object_url_with_blob(&blob) else { return };

    let doc = web_sys::window().unwrap().document().unwrap();
    let a: web_sys::HtmlAnchorElement = doc.create_element("a").unwrap().unchecked_into();
    a.set_href(&url);
    let filename = format!("{}.batm", set.file_identity.filename);
    a.set_download(&filename);
    a.click();
    let _ = web_sys::Url::revoke_object_url(&url);

    state.show_info_toast("Annotations exported");
}

fn import_annotations(state: AppState) {
    let doc = web_sys::window().unwrap().document().unwrap();
    let input: web_sys::HtmlInputElement = doc.create_element("input").unwrap().unchecked_into();
    input.set_type("file");
    input.set_attribute("accept", ".batm,.yaml,.yml").unwrap();

    let on_change = wasm_bindgen::closure::Closure::<dyn FnMut(web_sys::Event)>::new(move |ev: web_sys::Event| {
        let target: web_sys::HtmlInputElement = ev.target().unwrap().unchecked_into();
        let Some(file_list) = target.files() else { return };
        let Some(file) = file_list.get(0) else { return };

        let reader = web_sys::FileReader::new().unwrap();
        let reader_clone = reader.clone();
        let on_load = wasm_bindgen::closure::Closure::<dyn FnMut(web_sys::Event)>::new(move |_: web_sys::Event| {
            let result = reader_clone.result().unwrap();
            let text = result.as_string().unwrap_or_default();
            match yaml_serde::from_str::<AnnotationSet>(&text) {
                Ok(imported) => {
                    let idx = state.current_file_index.get_untracked().unwrap_or(0);
                    state.snapshot_annotations();
                    state.annotation_store.update(|store| {
                        store.ensure_len(idx + 1);
                        if let Some(Some(ref mut existing)) = store.sets.get_mut(idx) {
                            existing.annotations.extend(imported.annotations);
                        } else {
                            store.sets[idx] = Some(imported);
                        }
                    });
                    state.annotations_dirty.set(true);
                    state.show_info_toast("Annotations imported");
                }
                Err(e) => {
                    state.show_error_toast(format!("Import error: {e}"));
                }
            }
        });
        reader.set_onload(Some(on_load.as_ref().unchecked_ref()));
        on_load.forget();
        reader.read_as_text(&file).unwrap();
    });
    input.set_onchange(Some(on_change.as_ref().unchecked_ref()));
    on_change.forget();
    input.click();
}
