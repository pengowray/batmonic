use leptos::prelude::*;
use wasm_bindgen::JsCast;
use crate::state::{AppState, LayerPanel};
use crate::annotations::{Annotation, AnnotationKind, AnnotationSet, Region, generate_uuid, now_iso8601};
use crate::components::combo_button::ComboButton;
use crate::components::file_sidebar::settings_panel::{
    toggle_annotation_lock, delete_annotation, update_annotation_label, update_annotation_tags,
};

fn toggle_panel(state: &AppState, panel: LayerPanel) {
    state.layer_panel_open.update(|p| {
        *p = if *p == Some(panel) { None } else { Some(panel) };
    });
}

/// Creates an annotation from the current transient selection.
fn annotate_selection(state: &AppState) {
    let selection = state.selection.get_untracked();
    let file_idx = state.current_file_index.get_untracked();
    if let (Some(sel), Some(idx)) = (selection, file_idx) {
        let has_freq = sel.freq_low.is_some() && sel.freq_high.is_some();
        state.snapshot_annotations();
        let ann_id = generate_uuid();
        let annotation = Annotation {
            id: ann_id.clone(),
            kind: AnnotationKind::Region(Region {
                time_start: sel.time_start,
                time_end: sel.time_end,
                freq_low: sel.freq_low,
                freq_high: sel.freq_high,
                label: None,
                color: None,
                locked: None,
            }),
            created_at: now_iso8601(),
            modified_at: now_iso8601(),
            notes: None,
            parent_id: None,
            sort_order: None,
            tags: Vec::new(),
        };
        state.annotation_store.update(|store| {
            store.ensure_len(idx + 1);
            if store.sets[idx].is_none() {
                let new_set = state.files.with_untracked(|files| {
                    files.get(idx).map(|f| {
                        let id = f.identity.clone().unwrap_or_else(|| {
                            crate::file_identity::identity_layer1(&f.name, f.audio.metadata.file_size as u64)
                        });
                        AnnotationSet::new_with_metadata(id, &f.audio, f.cached_peak_db, f.cached_full_peak_db)
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
        state.selection.set(None);
        state.selected_annotation_ids.set(vec![ann_id]);
        // Auto-enter label editing for the new annotation
        state.annotation_editing.set(true);
        state.annotation_is_new_edit.set(true);
        state.layer_panel_open.set(Some(LayerPanel::SelectionCombo));
        state.show_info_toast(if has_freq { "Region annotated" } else { "Segment annotated" });
    }
}

/// Toggle frequency bounds on transient selection or selected annotations (Q key logic).
fn toggle_region_segment(state: &AppState) {
    if let Some(sel) = state.selection.get_untracked() {
        // Transient selection
        if sel.freq_low.is_some() && sel.freq_high.is_some() {
            state.selection.set(Some(crate::state::Selection {
                freq_low: None,
                freq_high: None,
                ..sel
            }));
            state.show_info_toast("Region \u{2192} Segment");
        } else {
            let (lo, hi) = get_freq_bounds(state);
            state.selection.set(Some(crate::state::Selection {
                freq_low: Some(lo),
                freq_high: Some(hi),
                ..sel
            }));
            state.show_info_toast("Segment \u{2192} Region");
        }
    } else {
        // Selected annotations
        let sel_ids = state.selected_annotation_ids.get_untracked();
        let idx = match state.current_file_index.get_untracked() {
            Some(i) if !sel_ids.is_empty() => i,
            _ => return,
        };
        let store = state.annotation_store.get_untracked();
        let all_have_freq = if let Some(Some(ref set)) = store.sets.get(idx) {
            sel_ids.iter().all(|id| {
                set.annotations.iter().find(|a| &a.id == id).map_or(false, |a| {
                    matches!(&a.kind, AnnotationKind::Region(r) if r.freq_low.is_some() && r.freq_high.is_some())
                })
            })
        } else {
            false
        };
        drop(store);
        state.snapshot_annotations();
        if all_have_freq {
            state.annotation_store.update(|store| {
                if let Some(Some(ref mut set)) = store.sets.get_mut(idx) {
                    for ann in set.annotations.iter_mut() {
                        if sel_ids.contains(&ann.id) {
                            if let AnnotationKind::Region(ref mut r) = ann.kind {
                                r.freq_low = None;
                                r.freq_high = None;
                                ann.modified_at = now_iso8601();
                            }
                        }
                    }
                }
            });
            state.show_info_toast("Region \u{2192} Segment");
        } else {
            let (lo, hi) = get_freq_bounds(state);
            state.annotation_store.update(|store| {
                if let Some(Some(ref mut set)) = store.sets.get_mut(idx) {
                    for ann in set.annotations.iter_mut() {
                        if sel_ids.contains(&ann.id) {
                            if let AnnotationKind::Region(ref mut r) = ann.kind {
                                if r.freq_low.is_none() || r.freq_high.is_none() {
                                    r.freq_low = Some(lo);
                                    r.freq_high = Some(hi);
                                    ann.modified_at = now_iso8601();
                                }
                            }
                        }
                    }
                }
            });
            state.show_info_toast("Segment \u{2192} Region");
        }
        state.annotations_dirty.set(true);
    }
}

/// Get frequency bounds from focus stack or display range.
fn get_freq_bounds(state: &AppState) -> (f64, f64) {
    let ff = state.focus_stack.get_untracked().effective_range_ignoring_hfr();
    if ff.is_active() {
        (ff.lo, ff.hi)
    } else {
        let files = state.files.get_untracked();
        let idx = state.current_file_index.get_untracked().unwrap_or(0);
        let file_max = files.get(idx).map(|f| f.spectrogram.max_freq).unwrap_or(96_000.0);
        (
            state.min_display_freq.get_untracked().unwrap_or(0.0),
            state.max_display_freq.get_untracked().unwrap_or(file_max),
        )
    }
}

/// Get annotation info for a single selected annotation.
fn get_selected_annotation_info(state: &AppState) -> Option<AnnotationInfo> {
    let ids = state.selected_annotation_ids.get();
    if ids.len() != 1 { return None; }
    let idx = state.current_file_index.get()?;
    let store = state.annotation_store.get();
    let set = store.sets.get(idx)?.as_ref()?;
    let ann = set.annotations.iter().find(|a| a.id == ids[0])?;
    let (label, tags, is_locked, is_region) = match &ann.kind {
        AnnotationKind::Region(r) => (
            r.label.clone(),
            ann.tags.clone(),
            r.is_locked(),
            true,
        ),
        AnnotationKind::Marker(m) => (m.label.clone(), ann.tags.clone(), false, false),
        AnnotationKind::Group(g) => (g.label.clone(), ann.tags.clone(), false, false),
        AnnotationKind::Measurement(m) => (m.label.clone(), ann.tags.clone(), false, false),
    };
    let (dur, freq) = match &ann.kind {
        AnnotationKind::Region(r) => {
            let d = r.time_end - r.time_start;
            let dur = if d > 0.0001 { Some(crate::format_time::format_duration(d, 3)) } else { None };
            let freq = match (r.freq_low, r.freq_high) {
                (Some(fl), Some(fh)) => Some(format!("{:.0} \u{2013} {:.0} kHz", fl / 1000.0, fh / 1000.0)),
                _ => None,
            };
            (dur, freq)
        }
        _ => (None, None),
    };
    let has_freq = match &ann.kind {
        AnnotationKind::Region(r) => r.freq_low.is_some() && r.freq_high.is_some(),
        _ => false,
    };
    Some(AnnotationInfo {
        id: ann.id.clone(),
        label,
        tags,
        is_locked,
        is_region,
        has_freq,
        duration: dur,
        freq_range: freq,
    })
}

struct AnnotationInfo {
    id: String,
    label: Option<String>,
    tags: Vec<String>,
    is_locked: bool,
    is_region: bool,
    has_freq: bool,
    duration: Option<String>,
    freq_range: Option<String>,
}

#[component]
pub fn SelectionComboButton() -> impl IntoView {
    let state = expect_context::<AppState>();
    let is_open = Signal::derive(move || state.layer_panel_open.get() == Some(LayerPanel::SelectionCombo));
    let editing = state.annotation_editing;

    // Brief label for the left side value
    let left_value = Signal::derive(move || {
        if let Some(sel) = state.selection.get() {
            let d = sel.time_end - sel.time_start;
            if d > 0.0001 {
                return crate::format_time::format_duration(d, 2);
            }
        }
        let ids = state.selected_annotation_ids.get();
        if !ids.is_empty() {
            if ids.len() == 1 {
                return "\u{25AD}".to_string();
            } else {
                return format!("{}\u{25AD}", ids.len());
            }
        }
        "None".to_string()
    });

    let has_selection = move || state.selection.get().is_some();
    let has_last = move || state.last_selection.get().is_some();
    let has_selected_annotations = move || !state.selected_annotation_ids.get().is_empty();
    let has_anything = move || has_selection() || has_selected_annotations();

    let left_class = Signal::derive(move || {
        if is_open.get() {
            "layer-btn combo-btn-left open"
        } else if has_anything() {
            "layer-btn combo-btn-left active"
        } else {
            "layer-btn combo-btn-left"
        }
    });

    let right_value = Signal::derive(move || "Sel".to_string());
    let right_class = Signal::derive(move || {
        if is_open.get() { "layer-btn combo-btn-right dim open" } else { "layer-btn combo-btn-right dim" }
    });

    let toggle_menu = Callback::new(move |_: ()| {
        toggle_panel(&state, LayerPanel::SelectionCombo);
        editing.set(false);
        state.annotation_is_new_edit.set(false);
    });

    let left_click = Callback::new(move |_: web_sys::MouseEvent| {
        if state.selection.get_untracked().is_some() {
            state.last_selection.set(state.selection.get_untracked());
            state.selection.set(None);
            state.selected_annotation_ids.set(vec![]);
        } else if !state.selected_annotation_ids.get_untracked().is_empty() {
            state.selected_annotation_ids.set(vec![]);
        } else if let Some(last) = state.last_selection.get_untracked() {
            state.selection.set(Some(last));
        }
    });

    // Selection details for the dropdown
    let selection_details = move || {
        if let Some(sel) = state.selection.get() {
            let d = sel.time_end - sel.time_start;
            if d > 0.0001 {
                let dur = crate::format_time::format_duration(d, 3);
                let freq = match (sel.freq_low, sel.freq_high) {
                    (Some(fl), Some(fh)) => format!("{:.0} \u{2013} {:.0} kHz", fl / 1000.0, fh / 1000.0),
                    _ => "\u{2014}".to_string(),
                };
                let has_freq = sel.freq_low.is_some() && sel.freq_high.is_some();
                return Some((dur, freq, has_freq));
            }
        }
        None
    };

    view! {
        <div
            style=move || format!("position: absolute; top: 10px; right: 30px; pointer-events: none; z-index: 20; opacity: {}; transition: opacity 0.1s;",
                if state.mouse_in_label_area.get() { "0" } else { "1" })
            on:click=|ev: web_sys::MouseEvent| ev.stop_propagation()
            on:touchstart=|ev: web_sys::TouchEvent| ev.stop_propagation()
        >
            <ComboButton
                left_label="Sel"
                left_value=left_value
                left_click=left_click
                left_class=left_class
                right_value=right_value
                right_class=right_class
                is_open=is_open
                toggle_menu=toggle_menu
                left_title="Click to deselect / reselect"
                right_title="Selection options"
                menu_direction="below"
                panel_style="min-width: 230px; right: 0; left: auto;"
            >
                // ── Transient selection section ──
                {move || {
                    if let Some((dur, freq, has_freq)) = selection_details() {
                        let btn_label = if has_freq { "Annotate Region" } else { "Annotate Segment" };
                        view! {
                            <div class="layer-panel-title">"Selection"</div>
                            <div style="padding: 4px 8px; font-size: 11px; color: #aaa;">
                                <div>"Duration: " {dur}</div>
                                <div style="display: flex; align-items: center; gap: 4px;">
                                    "Freq range: " {freq}
                                    {if has_freq {
                                        view! {
                                            <button class="sel-combo-freq-btn remove"
                                                title="Remove frequency range (Q)"
                                                on:click=move |_| toggle_region_segment(&state)
                                            >{"\u{00D7}"}</button>
                                        }.into_any()
                                    } else {
                                        view! {
                                            <button class="sel-combo-freq-btn add"
                                                title="Add frequency range from current view (Q)"
                                                on:click=move |_| toggle_region_segment(&state)
                                            >{"+"}</button>
                                        }.into_any()
                                    }}
                                </div>
                            </div>
                            <div style="padding: 4px 8px;">
                                <button class="sel-combo-action-btn"
                                    on:click=move |_| annotate_selection(&state)
                                >{btn_label}</button>
                            </div>
                            <div class="layer-panel-divider"></div>
                        }.into_any()
                    } else {
                        view! { <span></span> }.into_any()
                    }
                }}
                // ── Selected annotation section ──
                {move || {
                    editing.get(); // subscribe so we re-render when editing toggles
                    if let Some(info) = get_selected_annotation_info(&state) {
                        let ann_id = info.id.clone();
                        let ann_id_del = info.id.clone();
                        let ann_id_lock = info.id.clone();
                        let ann_id_label = info.id.clone();
                        let ann_id_tags = info.id.clone();
                        let initial_label = info.label.clone().unwrap_or_default();
                        let initial_tags = info.tags.join(", ");

                        let lock_label = if info.is_locked { "\u{1F512} Unlock" } else { "\u{1F513} Lock" };
                        let new_locked = !info.is_locked;
                        let has_freq = info.has_freq;

                        if editing.get_untracked() {
                            // Editing mode: show label + tags inputs
                            let label_ref = NodeRef::<leptos::html::Input>::new();
                            let label_value = RwSignal::new(initial_label.clone());
                            let ann_id_cancel = ann_id.clone();
                            let ann_id_confirm = ann_id.clone();
                            let ann_id_focusout = ann_id.clone();
                            // Auto-focus the label input
                            Effect::new(move |_| {
                                if let Some(el) = label_ref.get() {
                                    let _ = el.focus();
                                }
                            });
                            view! {
                                <div class="layer-panel-title">"Edit Annotation"</div>
                                <div style="padding: 4px 8px;">
                                    <div style="font-size: 10px; color: #888; margin-bottom: 2px;">"Label"</div>
                                    <input
                                        class="sel-combo-input"
                                        type="text"
                                        node_ref=label_ref
                                        prop:value=initial_label.clone()
                                        on:input=move |ev: web_sys::Event| {
                                            let input = ev.target().unwrap().unchecked_into::<web_sys::HtmlInputElement>();
                                            label_value.set(input.value());
                                        }
                                        on:keydown=move |ev: web_sys::KeyboardEvent| {
                                            if ev.key() == "Escape" {
                                                ev.prevent_default();
                                                ev.stop_propagation();
                                                if state.annotation_is_new_edit.get_untracked() {
                                                    delete_annotation(state, &ann_id_label);
                                                }
                                                state.annotation_is_new_edit.set(false);
                                                editing.set(false);
                                            } else if ev.key() == "Enter" {
                                                ev.prevent_default();
                                                let input = ev.target().unwrap().unchecked_into::<web_sys::HtmlInputElement>();
                                                let val = input.value();
                                                let label = if val.trim().is_empty() { None } else { Some(val.trim().to_string()) };
                                                update_annotation_label(state, &ann_id_label, label);
                                                state.annotation_is_new_edit.set(false);
                                                editing.set(false);
                                            }
                                        }
                                        on:focusout={
                                            move |ev: web_sys::FocusEvent| {
                                                if !editing.get_untracked() { return; }
                                                // Check if focus moved to another element within the editing panel
                                                if let Some(related) = ev.related_target() {
                                                    if let Ok(el) = related.dyn_into::<web_sys::HtmlElement>() {
                                                        if el.closest(".sel-combo-edit-area").ok().flatten().is_some() {
                                                            return;
                                                        }
                                                    }
                                                }
                                                let input = ev.target().unwrap().unchecked_into::<web_sys::HtmlInputElement>();
                                                let val = input.value();
                                                let label = if val.trim().is_empty() { None } else { Some(val.trim().to_string()) };
                                                update_annotation_label(state, &ann_id_focusout, label);
                                                state.annotation_is_new_edit.set(false);
                                                editing.set(false);
                                            }
                                        }
                                    />
                                    <div style="font-size: 10px; color: #888; margin-top: 6px; margin-bottom: 2px;">"Tags (comma separated)"</div>
                                    <input
                                        class="sel-combo-input"
                                        type="text"
                                        prop:value=initial_tags
                                        on:keydown=move |ev: web_sys::KeyboardEvent| {
                                            if ev.key() == "Escape" {
                                                ev.prevent_default();
                                                ev.stop_propagation();
                                                if state.annotation_is_new_edit.get_untracked() {
                                                    delete_annotation(state, &ann_id_tags);
                                                }
                                                state.annotation_is_new_edit.set(false);
                                                editing.set(false);
                                            } else if ev.key() == "Enter" {
                                                ev.prevent_default();
                                                let input = ev.target().unwrap().unchecked_into::<web_sys::HtmlInputElement>();
                                                let val = input.value();
                                                let tags: Vec<String> = val.split(',')
                                                    .map(|s| s.trim().to_string())
                                                    .filter(|s| !s.is_empty())
                                                    .collect();
                                                update_annotation_tags(state, &ann_id_tags, tags);
                                                state.annotation_is_new_edit.set(false);
                                                editing.set(false);
                                            }
                                        }
                                    />
                                </div>
                                <div class="sel-combo-edit-area" style="padding: 4px 8px; display: flex; gap: 4px;">
                                    <button class="sel-combo-action-btn subtle"
                                        on:click=move |_| {
                                            let val = label_value.get_untracked();
                                            let label = if val.trim().is_empty() { None } else { Some(val.trim().to_string()) };
                                            update_annotation_label(state, &ann_id_confirm, label);
                                            state.annotation_is_new_edit.set(false);
                                            editing.set(false);
                                        }
                                    >{"\u{2713} Done"}</button>
                                    <button class="sel-combo-action-btn danger"
                                        on:click=move |_| {
                                            if state.annotation_is_new_edit.get_untracked() {
                                                delete_annotation(state, &ann_id_cancel);
                                            }
                                            state.annotation_is_new_edit.set(false);
                                            editing.set(false);
                                        }
                                    >{"\u{2717} Cancel"}</button>
                                </div>
                                <div class="layer-panel-divider"></div>
                            }.into_any()
                        } else {
                            // Normal view: show annotation details + action buttons
                            view! {
                                <div class="layer-panel-title">"Annotation"</div>
                                <div style="padding: 4px 8px; font-size: 11px; color: #aaa;">
                                    {info.label.as_ref().map(|l| view! {
                                        <div style="color: #ccc; font-weight: 600; margin-bottom: 2px;">{l.clone()}</div>
                                    })}
                                    {info.duration.as_ref().map(|d| view! {
                                        <div>"Duration: " {d.clone()}</div>
                                    })}
                                    <div style="display: flex; align-items: center; gap: 4px;">
                                        "Freq range: " {info.freq_range.as_deref().unwrap_or("\u{2014}").to_string()}
                                        {if has_freq {
                                            view! {
                                                <button class="sel-combo-freq-btn remove"
                                                    title="Remove frequency range (Q)"
                                                    on:click=move |_| toggle_region_segment(&state)
                                                >{"\u{00D7}"}</button>
                                            }.into_any()
                                        } else {
                                            view! {
                                                <button class="sel-combo-freq-btn add"
                                                    title="Add frequency range from current view (Q)"
                                                    on:click=move |_| toggle_region_segment(&state)
                                                >{"+"}</button>
                                            }.into_any()
                                        }}
                                    </div>
                                    {(!info.tags.is_empty()).then(|| view! {
                                        <div style="margin-top: 2px; color: #8cf; font-size: 10px;">{info.tags.join(", ")}</div>
                                    })}
                                </div>
                                <div style="padding: 4px 8px; display: flex; flex-direction: column; gap: 3px;">
                                    <button class="sel-combo-action-btn subtle"
                                        on:click=move |_| {
                                            state.annotation_is_new_edit.set(false);
                                            editing.set(true);
                                        }
                                    >{"\u{270E} Edit label & tags"}</button>
                                    {info.is_region.then(|| view! {
                                        <button class="sel-combo-action-btn subtle"
                                            on:click=move |_| {
                                                toggle_annotation_lock(state, &ann_id_lock, new_locked);
                                            }
                                        >{lock_label}</button>
                                    })}
                                    <button class="sel-combo-action-btn danger"
                                        on:click=move |_| {
                                            delete_annotation(state, &ann_id_del);
                                            state.layer_panel_open.set(None);
                                        }
                                    >{"\u{00D7} Delete"}</button>
                                </div>
                                <div class="layer-panel-divider"></div>
                            }.into_any()
                        }
                    } else if !has_selection() {
                        // No selection or annotation
                        if has_last() {
                            view! {
                                <div style="padding: 4px 8px; font-size: 11px; color: #666;">"No selection (click left to reselect)"</div>
                                <div class="layer-panel-divider"></div>
                            }.into_any()
                        } else {
                            view! {
                                <div style="padding: 4px 8px; font-size: 11px; color: #666;">"No selection"</div>
                                <div class="layer-panel-divider"></div>
                            }.into_any()
                        }
                    } else {
                        view! { <span></span> }.into_any()
                    }
                }}
                // ── Auto-focus settings ──
                <div class="layer-panel-title">"Auto-focus"</div>
                <label style="display: flex; align-items: center; gap: 6px; padding: 4px 8px; font-size: 11px; cursor: pointer;">
                    <input type="checkbox"
                        prop:checked=move || state.annotation_auto_focus.get()
                        on:change=move |ev| {
                            let checked = leptos::prelude::event_target_checked(&ev);
                            state.annotation_auto_focus.set(checked);
                        }
                    />
                    "Selection / Annotation"
                </label>
                <label style="display: flex; align-items: center; gap: 6px; padding: 4px 8px; font-size: 11px; cursor: pointer;">
                    <input type="checkbox"
                        prop:checked=move || state.bat_book_auto_focus.get()
                        on:change=move |ev| {
                            let checked = leptos::prelude::event_target_checked(&ev);
                            state.bat_book_auto_focus.set(checked);
                        }
                    />
                    "Bat Book"
                </label>
            </ComboButton>
        </div>
    }
}
