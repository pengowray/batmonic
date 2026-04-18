use leptos::prelude::*;
use wasm_bindgen::JsCast;
use crate::state::{AppState, FlowColorScheme, MainView, SpectrogramDisplay};
use crate::annotations::{Annotation, AnnotationKind, AnnotationSet, Group, generate_uuid, now_iso8601, build_annotation_tree, AnnotationNode, collect_descendants, renumber_children};

#[component]
pub(crate) fn SpectrogramSettingsPanel() -> impl IntoView {
    let state = expect_context::<AppState>();

    view! {
        <div class="sidebar-panel">
            // Intensity sliders moved to DSP panel (floating combo button)
            <div class="setting-group">
                <div class="setting-group-title">"Spectrogram"</div>
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
                                    let db = state.chroma_gain.get();
                                    if db == 0.0 { "Gain: 0 dB".to_string() }
                                    else { format!("Gain: {:+.0} dB", db) }
                                }}</span>
                                <input
                                    type="range"
                                    class="setting-range"
                                    min="-20"
                                    max="60"
                                    step="1"
                                    prop:value=move || state.chroma_gain.get().round().to_string()
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
                                        state.chroma_gain.set(0.0);
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

    let has_annotations = move || {
        let idx = state.current_file_index.get()?;
        let store = state.annotation_store.get();
        let set = store.sets.get(idx)?.as_ref()?;
        if set.annotations.is_empty() { None } else { Some(()) }
    };

    let has_wav_markers = move || {
        let idx = state.current_file_index.get()?;
        let files = state.files.get();
        let file = files.get(idx)?;
        if file.wav_markers.is_empty() { None } else { Some(()) }
    };

    let has_nothing = move || has_annotations().is_none() && has_wav_markers().is_none();

    view! {
        <div class="sidebar-panel">
            {move || {
                if has_nothing() {
                    view! {
                        <div class="sidebar-panel-empty">"No annotations"</div>
                    }.into_any()
                } else {
                    view! { <span></span> }.into_any()
                }
            }}
            <WavMarkersList />
            <AnnotationsList />
        </div>
    }
}

/// Read-only list of file-embedded time markers — WAV cue points or
/// M4A/M4B chapters — parsed from the current file.
#[component]
fn WavMarkersList() -> impl IntoView {
    let state = expect_context::<AppState>();

    let markers = move || {
        let idx = state.current_file_index.get()?;
        let files = state.files.get();
        let file = files.get(idx)?;
        if file.wav_markers.is_empty() { return None; }
        let sr = file.audio.sample_rate;
        // Heading adapts to what kind of markers these are based on the
        // file format. Same underlying field, different source semantics.
        let heading = match file.audio.metadata.format {
            "M4A" => "Chapters",
            _ => "Cue Points",
        };
        Some((
            heading,
            file.wav_markers.iter().map(|m| {
                let time_secs = m.position as f64 / sr as f64;
                let time_str = crate::format_time::format_time_display(time_secs, 3);
                let label = m.label.clone().unwrap_or_else(|| format!("Marker {}", m.id));
                (m.position, time_str, label)
            }).collect::<Vec<_>>()
        ))
    };

    view! {
        {move || {
            if let Some((heading, items)) = markers() {
                view! {
                    <div class="setting-group">
                        <div class="setting-group-title">{heading}</div>
                        <div class="annotation-tree">
                            {items.into_iter().map(|(pos, time_str, label)| {
                                let state2 = state;
                                view! {
                                    <div
                                        class="annotation-tree-item wav-marker-item"
                                        title=format!("Sample {}", pos)
                                        on:click=move |_| {
                                            let sr = state2.files.with_untracked(|files| {
                                                state2.current_file_index.get_untracked()
                                                    .and_then(|i| files.get(i))
                                                    .map(|f| f.audio.sample_rate)
                                                    .unwrap_or(1)
                                            });
                                            let time = pos as f64 / sr as f64;
                                            // 1) Remember this as the "play from here" position
                                            //    so the play button picks up from the marker
                                            //    instead of the previous scroll position.
                                            state2.play_from_here_time.set(time);
                                            // 2) Update the visual playhead.
                                            state2.playhead_time.set(time);
                                            // 3) Scroll viewport to center on the marker.
                                            jump_to_time(state2, time);
                                            // 4) If playback is active, restart from here so
                                            //    the audio actually jumps.
                                            if state2.is_playing.get_untracked() {
                                                crate::audio::playback::play_from_time(&state2, time);
                                            }
                                        }
                                    >
                                        <span class="annotation-icon">{"\u{25C6} "}</span>
                                        <span class="annotation-label">{label}</span>
                                        <span class="annotation-time">{time_str}</span>
                                    </div>
                                }
                            }).collect::<Vec<_>>()}
                        </div>
                    </div>
                }.into_any()
            } else {
                view! { <div></div> }.into_any()
            }
        }}
    }
}

/// Get the display label for an annotation.
/// Returns (display_text, stored_label, is_default).
/// is_default is true when the label was auto-generated (render italic).
fn annotation_display(a: &Annotation) -> (String, Option<String>, bool) {
    let is_default = a.label_default.unwrap_or(false);
    match &a.kind {
        AnnotationKind::Region(reg) => {
            let auto_label = match (reg.freq_low, reg.freq_high) {
                (Some(fl), Some(fh)) => format!("{}, {:.0}–{:.0} kHz",
                    crate::format_time::format_time_range(reg.time_start, reg.time_end, 3),
                    fl / 1000.0, fh / 1000.0),
                _ => crate::format_time::format_time_range(reg.time_start, reg.time_end, 3),
            };
            let (display, italic) = match &reg.label {
                Some(l) => (l.clone(), is_default),
                None => (auto_label, true),
            };
            (display, reg.label.clone(), italic)
        }
        AnnotationKind::Marker(m) => {
            let auto_label = crate::format_time::format_time_display(m.time, 3);
            let (display, italic) = match &m.label {
                Some(l) => (l.clone(), is_default),
                None => (auto_label, true),
            };
            (display, m.label.clone(), italic)
        }
        AnnotationKind::Group(g) => {
            let (display, italic) = match &g.label {
                Some(l) => (l.clone(), is_default),
                None => ("Group".to_string(), true),
            };
            (display, g.label.clone(), italic)
        }
        AnnotationKind::Measurement(m) => {
            let auto_label = crate::format_time::format_time_range(m.start_time, m.end_time, 3);
            let (display, italic) = match &m.label {
                Some(l) => (l.clone(), is_default),
                None => (auto_label, true),
            };
            (display, m.label.clone(), italic)
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

    let has_file_path = move || -> Option<bool> {
        let idx = state.current_file_index.get()?;
        let files = state.files.get();
        let file = files.get(idx)?;
        file.identity.as_ref()?.file_path.as_ref().map(|_| true)
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
        let sel_id = state.selected_annotation_id()?;
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
                                title="Group selected"
                                on:click=on_group
                                disabled=move || state.selected_annotation_ids.get().is_empty()
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
        <super::export_section::ExportSection
            on_export_batm=Callback::new(move |()| export_annotations(state))
            on_save_sidecar=Callback::new(move |()| {
                if let Some(idx) = state.current_file_index.get_untracked() {
                    crate::opfs::save_sidecar_explicit(state, idx);
                }
            })
            on_import_batm=Callback::new(move |()| import_annotations(state))
            has_annotations=Signal::derive(has_annotations)
            has_file_path=Signal::derive(has_file_path)
        />
    }
}

fn render_tree_nodes(nodes: Vec<AnnotationNode>, state: AppState) -> impl IntoView {
    nodes.into_iter().map(move |node| {
        let id = node.annotation.id.clone();
        let (display, existing_label, label_is_default) = annotation_display(&node.annotation);
        let icon = annotation_icon(&node.annotation.kind);
        let is_group = matches!(node.annotation.kind, AnnotationKind::Group(_));
        let is_collapsed = match &node.annotation.kind {
            AnnotationKind::Group(g) => g.collapsed.unwrap_or(false),
            _ => false,
        };
        let existing_tags = node.annotation.tags.clone();
        let depth = node.depth;
        let children = node.children;

        let is_region = matches!(node.annotation.kind, AnnotationKind::Region(_));
        let initial_locked = match &node.annotation.kind {
            AnnotationKind::Region(r) => r.is_locked(),
            _ => false,
        };
        let locked_signal = RwSignal::new(initial_locked);

        let id_click = id.clone();
        let id_delete = id.clone();
        let id_edit = id.clone();
        let id_tags = id.clone();
        let id_lock = id.clone();
        let id_drag = id.clone();
        let id_dragover = id.clone();
        let id_dragover2 = id.clone();

        let editing = RwSignal::new(false);
        // Start the input blank when the current label is auto-generated, so hitting
        // Enter keeps it as a default (with a freshly generated number). If the user
        // starts typing, their input becomes the user-authored label.
        let initial_edit = if label_is_default { String::new() } else { existing_label.unwrap_or_default() };
        let edit_value = RwSignal::new(initial_edit);
        let tags_value = RwSignal::new(existing_tags.join(", "));

        let indent_px = depth * 16;

        // Tags displayed as pills (non-editing mode)
        let tags_display = existing_tags.clone();

        view! {
            <div
                class="annotation-tree-item"
                class:annotation-selected=move || state.selected_annotation_ids.get().contains(&id_click)
                class:annotation-drop-target=move || {
                    state.drop_target.get().as_ref().map(|(tid, _)| tid.as_str()) == Some(id_dragover.as_str())
                }
                class:annotation-group-item=is_group
                style:padding-left=format!("{}px", 8 + indent_px)
                draggable="true"
                on:click=move |ev: web_sys::MouseEvent| {
                    let click_id = id.clone();
                    let ctrl = ev.ctrl_key() || ev.meta_key();
                    let shift = ev.shift_key();

                    if is_group && !ctrl && !shift {
                        toggle_group_collapsed(state, &click_id);
                    }

                    if ctrl {
                        // Toggle this annotation in/out of selection
                        state.selected_annotation_ids.update(|ids| {
                            if let Some(pos) = ids.iter().position(|x| x == &click_id) {
                                ids.remove(pos);
                            } else {
                                ids.push(click_id.clone());
                            }
                        });
                    } else if shift {
                        // Range select from last-clicked to this one
                        if let Some(anchor) = state.last_clicked_annotation_id.get_untracked() {
                            if let Some(idx) = state.current_file_index.get_untracked() {
                                let store = state.annotation_store.get_untracked();
                                if let Some(Some(set)) = store.sets.get(idx) {
                                    let flat_ids: Vec<String> = set.annotations.iter().map(|a| a.id.clone()).collect();
                                    let anchor_pos = flat_ids.iter().position(|x| x == &anchor);
                                    let click_pos = flat_ids.iter().position(|x| x == &click_id);
                                    if let (Some(a), Some(b)) = (anchor_pos, click_pos) {
                                        let lo = a.min(b);
                                        let hi = a.max(b);
                                        let range_ids: Vec<String> = flat_ids[lo..=hi].to_vec();
                                        if ev.ctrl_key() || ev.meta_key() {
                                            // Add range to existing selection
                                            state.selected_annotation_ids.update(|ids| {
                                                for rid in &range_ids {
                                                    if !ids.contains(rid) {
                                                        ids.push(rid.clone());
                                                    }
                                                }
                                            });
                                        } else {
                                            state.selected_annotation_ids.set(range_ids);
                                        }
                                    }
                                }
                            }
                        }
                    } else {
                        // Plain click: select only this one
                        state.selected_annotation_ids.set(vec![click_id.clone()]);
                    }

                    state.last_clicked_annotation_id.set(Some(click_id.clone()));

                    // Restore selection for last-clicked non-group annotation
                    if !is_group {
                        restore_selection(state, &click_id);
                    }
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
                    if editing.try_get().unwrap_or(false) {
                        let id_save = id_edit.clone();
                        let id_save2 = id_edit.clone();
                        let id_tags_save = id_tags.clone();
                        let id_tags_save2 = id_tags.clone();
                        let input_ref = NodeRef::<leptos::html::Input>::new();
                        Effect::new(move |_| {
                            if let Some(el) = input_ref.get() {
                                let _ = el.focus();
                            }
                        });
                        let save_all = move |id_l: &str, id_t: &str| {
                            let val = match edit_value.try_get_untracked() {
                                Some(v) => v,
                                None => return,
                            };
                            let label = if val.trim().is_empty() { None } else { Some(val) };
                            update_annotation_label(state, id_l, label);
                            let tags_str = match tags_value.try_get_untracked() {
                                Some(v) => v,
                                None => return,
                            };
                            let tags: Vec<String> = tags_str.split(',')
                                .map(|s| s.trim().to_string())
                                .filter(|s| !s.is_empty())
                                .collect();
                            update_annotation_tags(state, id_t, tags);
                        };
                        let id_l_enter = id_save.clone();
                        let id_t_enter = id_tags_save.clone();
                        let id_l_blur = id_save2.clone();
                        let id_t_blur = id_tags_save2.clone();
                        let id_l_focusout = id_save.clone();
                        let id_t_focusout = id_tags_save.clone();
                        view! {
                            <div class="annotation-edit-fields"
                                on:click=move |e| { e.stop_propagation(); }
                                on:focusout=move |ev: web_sys::FocusEvent| {
                                    // Save when focus leaves the editing area entirely
                                    let target = ev.current_target().unwrap();
                                    let related = ev.related_target();
                                    let container: web_sys::HtmlElement = target.unchecked_into();
                                    let still_inside = related.map(|r| {
                                        let node: web_sys::Node = r.unchecked_into();
                                        container.contains(Some(&node))
                                    }).unwrap_or(false);
                                    if !still_inside {
                                        save_all(&id_l_focusout, &id_t_focusout);
                                        let _ = editing.try_set(false);
                                    }
                                }
                            >
                                <input
                                    class="annotation-label-input"
                                    type="text"
                                    prop:value=move || edit_value.try_get().unwrap_or_default()
                                    placeholder="Label..."
                                    node_ref=input_ref
                                    on:input=move |ev| {
                                        let _ = edit_value.try_set(leptos::prelude::event_target_value(&ev));
                                    }
                                    on:keydown=move |ev| {
                                        if ev.key() == "Enter" {
                                            save_all(&id_l_enter, &id_t_enter);
                                            let _ = editing.try_set(false);
                                        } else if ev.key() == "Escape" {
                                            let _ = editing.try_set(false);
                                        }
                                    }
                                />
                                <input
                                    class="annotation-label-input annotation-tags-input"
                                    type="text"
                                    prop:value=move || tags_value.try_get().unwrap_or_default()
                                    placeholder="Tags (comma separated)..."
                                    on:input=move |ev| {
                                        let _ = tags_value.try_set(leptos::prelude::event_target_value(&ev));
                                    }
                                    on:keydown=move |ev| {
                                        if ev.key() == "Enter" {
                                            save_all(&id_l_blur, &id_t_blur);
                                            let _ = editing.try_set(false);
                                        } else if ev.key() == "Escape" {
                                            let _ = editing.try_set(false);
                                        }
                                    }
                                />
                            </div>
                        }.into_any()
                    } else {
                        let tags_pills = tags_display.clone();
                        let label_class = if label_is_default { "annotation-label default" } else { "annotation-label" };
                        view! {
                            <span class=label_class>
                                {display.clone()}
                                {if !tags_pills.is_empty() {
                                    view! {
                                        <span class="annotation-tags">
                                            {tags_pills.into_iter().map(|tag| {
                                                let tag_click = tag.clone();
                                                let tag_title = tag.clone();
                                                let tag_display = tag.clone();
                                                view! {
                                                    <span class="annotation-tag"
                                                        title=format!("Select all with tag '{}'", tag_title)
                                                        on:click=move |ev: web_sys::MouseEvent| {
                                                            ev.stop_propagation();
                                                            let target_tag = tag_click.clone();
                                                            if let Some(idx) = state.current_file_index.get_untracked() {
                                                                let store = state.annotation_store.get_untracked();
                                                                if let Some(Some(set)) = store.sets.get(idx) {
                                                                    let matching: Vec<String> = set.annotations.iter()
                                                                        .filter(|a| a.tags.contains(&target_tag))
                                                                        .map(|a| a.id.clone())
                                                                        .collect();
                                                                    state.selected_annotation_ids.set(matching);
                                                                }
                                                            }
                                                        }
                                                    >{tag_display}</span>
                                                }
                                            }).collect_view()}
                                        </span>
                                    }.into_any()
                                } else {
                                    view! { <span></span> }.into_any()
                                }}
                            </span>
                        }.into_any()
                    }
                }}
                <button class="annotation-edit"
                    title="Edit label & tags"
                    on:click=move |e| {
                        e.stop_propagation();
                        let _ = editing.try_set(true);
                    }
                >"\u{270E}"</button>
                {if is_region {
                    view! {
                        <button
                            class=move || if locked_signal.get() { "annotation-lock locked" } else { "annotation-lock unlocked" }
                            title=move || if locked_signal.get() { "Unlock (allow resize)" } else { "Lock (prevent resize)" }
                            on:click=move |e| {
                                e.stop_propagation();
                                let new_locked = !locked_signal.get_untracked();
                                locked_signal.set(new_locked);
                                toggle_annotation_lock(state, &id_lock, new_locked);
                            }
                        >
                            {move || if locked_signal.get() { "\u{1F512}" } else { "\u{1F513}" }}
                        </button>
                    }.into_any()
                } else {
                    view! { <span></span> }.into_any()
                }}
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
                AnnotationKind::Measurement(m) => {
                    Some((m.start_time + m.end_time) / 2.0)
                }
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

pub(crate) fn toggle_annotation_lock(state: AppState, annotation_id: &str, locked: bool) {
    let idx = match state.current_file_index.get_untracked() {
        Some(i) => i,
        None => return,
    };
    state.annotation_store.update(|store| {
        if let Some(Some(ref mut set)) = store.sets.get_mut(idx) {
            if let Some(ann) = set.annotations.iter_mut().find(|a| a.id == annotation_id) {
                if let AnnotationKind::Region(ref mut r) = ann.kind {
                    r.locked = if locked { Some(true) } else { None };
                    ann.modified_at = js_sys::Date::new_0().to_iso_string().as_string().unwrap_or_default();
                }
            }
        }
    });
    state.annotations_dirty.set(true);
}

pub(crate) fn delete_annotation(state: AppState, annotation_id: &str) {
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
    let was_selected = state.selected_annotation_ids.get_untracked().iter().any(|x| x == annotation_id);
    if was_selected {
        state.selected_annotation_ids.update(|ids| ids.retain(|x| x != annotation_id));
    }
    state.annotations_dirty.set(true);
}

pub(crate) fn update_annotation_label(state: AppState, annotation_id: &str, label: Option<String>) {
    let idx = match state.current_file_index.get_untracked() {
        Some(i) => i,
        None => return,
    };
    state.snapshot_annotations();
    state.annotation_store.update(|store| {
        if let Some(Some(ref mut set)) = store.sets.get_mut(idx) {
            // If user cleared the label, regenerate a default ("Region 4" etc.)
            // and mark it so the UI renders it in italic.
            let (new_label, is_default) = if label.is_none() {
                let kind = match set.annotations.iter().find(|a| a.id == annotation_id) {
                    Some(a) => a.kind.clone(),
                    None => return,
                };
                let default_label = crate::annotations::generate_default_label(
                    &set.annotations, &kind, Some(annotation_id),
                );
                (Some(default_label), true)
            } else {
                (label, false)
            };
            if let Some(a) = set.annotations.iter_mut().find(|a| a.id == annotation_id) {
                match a.kind {
                    AnnotationKind::Region(ref mut reg) => { reg.label = new_label; }
                    AnnotationKind::Marker(ref mut m) => { m.label = new_label; }
                    AnnotationKind::Group(ref mut g) => { g.label = new_label; }
                    AnnotationKind::Measurement(ref mut m) => { m.label = new_label; }
                }
                a.label_default = if is_default { Some(true) } else { None };
                a.modified_at = now_iso8601();
            }
        }
    });
    state.annotations_dirty.set(true);
}

pub(crate) fn update_annotation_tags(state: AppState, annotation_id: &str, tags: Vec<String>) {
    let idx = match state.current_file_index.get_untracked() {
        Some(i) => i,
        None => return,
    };
    state.annotation_store.update(|store| {
        if let Some(Some(ref mut set)) = store.sets.get_mut(idx) {
            if let Some(a) = set.annotations.iter_mut().find(|a| a.id == annotation_id) {
                a.tags = tags;
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
    let sel_ids = state.selected_annotation_ids.get_untracked();
    if sel_ids.is_empty() { return; }
    let idx = match state.current_file_index.get_untracked() {
        Some(i) => i,
        None => return,
    };

    state.snapshot_annotations();

    // Create a new group and move all selected annotations into it
    let group_id = generate_uuid();
    let now = now_iso8601();

    state.annotation_store.update(|store| {
        if let Some(Some(ref mut set)) = store.sets.get_mut(idx) {
            // Use the first selected annotation's parent and sort_order for the group
            let (parent, order) = set.annotations.iter()
                .find(|a| sel_ids.contains(&a.id))
                .map(|a| (a.parent_id.clone(), a.sort_order))
                .unwrap_or((None, None));

            // Create group at the same level as the first selected item
            let kind = AnnotationKind::Group(Group {
                label: Some(crate::annotations::generate_default_label(
                    &set.annotations,
                    &AnnotationKind::Group(Group { label: None, color: None, collapsed: None }),
                    None,
                )),
                color: None,
                collapsed: Some(false),
            });
            let group = Annotation {
                id: group_id.clone(),
                kind,
                created_at: now.clone(),
                modified_at: now,
                notes: None,
                parent_id: parent,
                sort_order: order,
                tags: Vec::new(),
                label_default: Some(true),
            };
            set.annotations.push(group);

            // Move all selected annotations into the group
            for (i, a) in set.annotations.iter_mut().enumerate() {
                if sel_ids.contains(&a.id) {
                    a.parent_id = Some(group_id.clone());
                    a.sort_order = Some(i as f64);
                }
            }

            // Renumber siblings at the old level
            let parent_key = set.annotations.iter()
                .find(|a| a.id == group_id)
                .and_then(|a| a.parent_id.clone());
            renumber_children(&mut set.annotations, parent_key.as_deref());
        }
    });
    state.selected_annotation_ids.set(vec![group_id]);
    state.annotations_dirty.set(true);
    state.annotations_visible.set(true);
}

fn ungroup_selected(state: AppState) {
    let group_id = match state.selected_annotation_id() {
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
    state.selected_annotation_ids.set(Vec::new());
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
                _ => {
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

    let filename = format!("{}.batm", set.file_identity.filename);

    if state.is_tauri {
        // Tauri: export via IPC command to app data exports directory
        wasm_bindgen_futures::spawn_local(async move {
            let args = js_sys::Object::new();
            let _ = js_sys::Reflect::set(&args, &wasm_bindgen::JsValue::from_str("filename"), &wasm_bindgen::JsValue::from_str(&filename));
            let _ = js_sys::Reflect::set(&args, &wasm_bindgen::JsValue::from_str("yaml"), &wasm_bindgen::JsValue::from_str(&yaml));
            match crate::tauri_bridge::tauri_invoke("export_annotations_file", &args.into()).await {
                Ok(path) => {
                    let path_str = path.as_string().unwrap_or_default();
                    if !path_str.is_empty() {
                        state.show_info_toast(format!("Exported to {path_str}"));
                    }
                    // empty = user cancelled, no toast needed
                }
                Err(e) => state.show_error_toast(format!("Export failed: {e}")),
            }
        });
    } else {
        // Browser: download via blob URL
        let arr = js_sys::Array::of1(&wasm_bindgen::JsValue::from_str(&yaml));
        let Ok(blob) = web_sys::Blob::new_with_str_sequence(&arr) else { return };
        let Ok(url) = web_sys::Url::create_object_url_with_blob(&blob) else { return };

        let doc = web_sys::window().unwrap().document().unwrap();
        let a: web_sys::HtmlAnchorElement = doc.create_element("a").unwrap().unchecked_into();
        a.set_href(&url);
        a.set_download(&filename);
        a.click();
        let _ = web_sys::Url::revoke_object_url(&url);

        state.show_info_toast("Annotations exported");
    }
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
