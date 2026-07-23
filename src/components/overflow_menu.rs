use crate::annotations::{
    generate_default_label, now_iso8601, Annotation, AnnotationId, AnnotationKind, AnnotationSet,
    Marker, Region,
};
use crate::canvas::spectrogram_renderer::freq_to_y;
use crate::components::file_sidebar::settings_panel::{delete_annotation, toggle_annotation_lock};
use crate::state::store_fields::*;
use crate::state::{ActiveFocus, AppState, Selection};
use leptos::prelude::*;

// Icons for the expand/contract freq buttons.
// Expand = "remove frequency bounds" (treat as full range).
// Contract = "snap frequency to current view/focus".
const ICON_EXPAND: &str = "\u{26F6}"; // ⛶ square four corners — full range / remove bounds
const ICON_CONTRACT: &str = "\u{25AD}"; // ▭ white rectangle — snap to current view

/// Creates an annotation from the current transient selection and enters label-edit mode.
pub fn annotate_selection(state: &AppState) {
    let selection = state.interaction.selection().get_untracked();
    let file_idx = state.library.current_index().get_untracked();
    if let (Some(sel), Some(idx)) = (selection, file_idx) {
        let has_freq = sel.freq_low.is_some() && sel.freq_high.is_some();
        let Some(file_id) = state.file_id_at(idx) else {
            return;
        };
        let new_set = state.library.files().with_untracked(|files| {
            files.get(idx).map(|f| {
                let id = f.identity.clone().unwrap_or_else(|| {
                    crate::file_identity::identity_layer1(
                        &f.name,
                        f.audio.metadata.file_size as u64,
                    )
                });
                AnnotationSet::new_with_metadata(
                    id,
                    &f.audio,
                    f.cached_peak_db,
                    f.cached_full_peak_db,
                )
            })
        });
        let Some(new_set) = new_set else {
            return;
        };
        state.snapshot_annotations();
        let ann_id = AnnotationId::new();
        state.annotations.store().update(|store| {
            let set = store.entry_or_insert_with(file_id, || new_set);
            {
                let mut kind = AnnotationKind::Region(Region {
                    time_start: sel.time_start,
                    time_end: sel.time_end,
                    freq_low: sel.freq_low,
                    freq_high: sel.freq_high,
                    label: None,
                    color: None,
                    locked: None,
                });
                let default_label = generate_default_label(&set.annotations, &kind, None);
                if let AnnotationKind::Region(ref mut r) = kind {
                    r.label = Some(default_label);
                }
                set.annotations.push(Annotation {
                    id: ann_id.clone(),
                    kind,
                    created_at: now_iso8601(),
                    modified_at: now_iso8601(),
                    notes: None,
                    parent_id: None,
                    sort_order: None,
                    tags: Vec::new(),
                    label_default: Some(true),
                });
            }
        });
        state.annotations.dirty().set(true);
        state.annotations.visible().set(true);
        state.interaction.selection().set(None);
        state.annotations.selected_ids().set(vec![ann_id]);
        state
            .interaction
            .active_focus()
            .set(Some(ActiveFocus::Annotations));
        // Auto-enter label editing for the new annotation (floating editor)
        state.annotations.editing().set(true);
        state.annotations.is_new_edit().set(true);
        state.show_info_toast(if has_freq {
            "Region annotated"
        } else {
            "Segment annotated"
        });
    }
}

/// Drop an annotation marker at the given time on the current file, open label
/// edit. Called from the keyboard shortcut (M) and the overflow menu.
pub fn add_marker_at_time(state: &AppState, time: f64) {
    let Some(idx) = state.library.current_index().get_untracked() else {
        return;
    };
    let duration = state
        .library
        .files()
        .with_untracked(|files| files.get(idx).map(|f| f.audio.duration_secs).unwrap_or(0.0));
    if duration <= 0.0 {
        return;
    }
    let time = time.clamp(0.0, duration);

    let Some(file_id) = state.file_id_at(idx) else {
        return;
    };
    let new_set = state.library.files().with_untracked(|files| {
        files.get(idx).map(|f| {
            let id = f.identity.clone().unwrap_or_else(|| {
                crate::file_identity::identity_layer1(&f.name, f.audio.metadata.file_size as u64)
            });
            AnnotationSet::new_with_metadata(id, &f.audio, f.cached_peak_db, f.cached_full_peak_db)
        })
    });
    let Some(new_set) = new_set else {
        return;
    };
    state.snapshot_annotations();
    let ann_id = AnnotationId::new();
    state.annotations.store().update(|store| {
        let set = store.entry_or_insert_with(file_id, || new_set);
        {
            let mut kind = AnnotationKind::Marker(Marker {
                time,
                label: None,
                color: None,
            });
            let default_label = generate_default_label(&set.annotations, &kind, None);
            if let AnnotationKind::Marker(ref mut m) = kind {
                m.label = Some(default_label);
            }
            set.annotations.push(Annotation {
                id: ann_id.clone(),
                kind,
                created_at: now_iso8601(),
                modified_at: now_iso8601(),
                notes: None,
                parent_id: None,
                sort_order: None,
                tags: Vec::new(),
                label_default: Some(true),
            });
        }
    });
    state.annotations.dirty().set(true);
    state.annotations.visible().set(true);
    state.annotations.selected_ids().set(vec![ann_id]);
    state
        .interaction
        .active_focus()
        .set(Some(ActiveFocus::Annotations));
    state.annotations.editing().set(true);
    state.annotations.is_new_edit().set(true);
    state.show_info_toast("Marker added");
}

/// Get frequency bounds from focus stack or display range.
fn get_freq_bounds(state: &AppState) -> (f64, f64) {
    let ff = state
        .viewmode
        .focus_stack()
        .get_untracked()
        .effective_range_ignoring_hfr();
    if ff.is_active() {
        (ff.lo, ff.hi)
    } else {
        let files = state.library.files().get_untracked();
        let idx = state.library.current_index().get_untracked().unwrap_or(0);
        let file_max = files
            .get(idx)
            .map(|f| f.spectrogram.max_freq)
            .unwrap_or(96_000.0);
        (
            state.view.min_display_freq().get_untracked().unwrap_or(0.0),
            state
                .view
                .max_display_freq()
                .get_untracked()
                .unwrap_or(file_max),
        )
    }
}

/// Format freq range for tooltip.
fn fmt_freq_range(lo: f64, hi: f64) -> String {
    format!("{:.0}\u{2013}{:.0} kHz", lo / 1000.0, hi / 1000.0)
}

/// Shared: pixel position of the TOP-LEFT corner of a
/// `[time_start, time_end] × freq_high` box, relative to the spectrogram canvas.
/// Returns `None` when the box is entirely off-screen (so a valid box sitting in
/// the top-left corner — x≈0, y≈0 — is not mistaken for "no selection").
fn corner_top_left(
    time_start: f64,
    time_end: f64,
    freq_high: Option<f64>,
    scroll_offset: f64,
    time_resolution: f64,
    zoom: f64,
    canvas_width: f64,
    canvas_height: f64,
    min_freq: f64,
    max_freq: f64,
) -> Option<(f64, f64)> {
    let visible_time = (canvas_width / zoom) * time_resolution;
    if visible_time <= 0.0 {
        return None;
    }
    let px_per_sec = canvas_width / visible_time;
    let x0 = (time_start - scroll_offset) * px_per_sec;
    let x1 = (time_end - scroll_offset) * px_per_sec;
    // Entirely off-screen: right edge left of the view, or left edge past the right.
    if x1 <= 0.0 || x0 >= canvas_width {
        return None;
    }
    let x = x0.clamp(0.0, canvas_width);
    let y = match freq_high {
        Some(fh) => freq_to_y(fh, min_freq, max_freq, canvas_height).max(0.0),
        None => 0.0,
    };
    Some((x, y))
}

/// Top-left corner of the transient selection.
fn selection_top_left(
    sel: &Selection,
    scroll_offset: f64,
    time_resolution: f64,
    zoom: f64,
    canvas_width: f64,
    canvas_height: f64,
    min_freq: f64,
    max_freq: f64,
) -> Option<(f64, f64)> {
    corner_top_left(
        sel.time_start,
        sel.time_end,
        sel.freq_high,
        scroll_offset,
        time_resolution,
        zoom,
        canvas_width,
        canvas_height,
        min_freq,
        max_freq,
    )
}

/// Top-left corner of an annotation region.
fn annotation_top_left(
    time_start: f64,
    time_end: f64,
    freq_high: Option<f64>,
    scroll_offset: f64,
    time_resolution: f64,
    zoom: f64,
    canvas_width: f64,
    canvas_height: f64,
    min_freq: f64,
    max_freq: f64,
) -> Option<(f64, f64)> {
    corner_top_left(
        time_start,
        time_end,
        freq_high,
        scroll_offset,
        time_resolution,
        zoom,
        canvas_width,
        canvas_height,
        min_freq,
        max_freq,
    )
}

/// Renders the "..." overflow menus for the transient selection and annotations.
/// Mounted inside `main-overlays` in app.rs.
#[component]
pub fn CanvasOverflowMenus() -> impl IntoView {
    let state = expect_context::<AppState>();

    view! {
        {move || {
            let focus = state.interaction.active_focus().get();
            if focus != Some(ActiveFocus::TransientSelection) {
                state.interaction.selection_overflow_open().set(false);
            }
            if focus != Some(ActiveFocus::Annotations) {
                state.interaction.annotation_overflow_open().set(false);
            }
            match focus {
                Some(ActiveFocus::TransientSelection) => {
                    if state.interaction.selection().get().is_some() {
                        Some(view! { <SelectionOverflowMenu /> }.into_any())
                    } else {
                        None
                    }
                }
                Some(ActiveFocus::Annotations) => {
                    let ids = state.annotations.selected_ids().get();
                    if !ids.is_empty() {
                        Some(view! { <AnnotationOverflowMenu /> }.into_any())
                    } else {
                        None
                    }
                }
                _ => None,
            }
        }}
    }
}

const BTN_SIZE: f64 = 22.0;
const BTN_MARGIN: f64 = 4.0;

/// Frequency row: "Freq: 20–50 kHz" on the left, [⇅] [⇵] buttons on the right.
/// Buttons are always shown; disabled when they wouldn't change anything.
#[component]
fn FreqRow(
    /// Displayed freq range (or em-dash).
    #[prop(into)]
    freq_text: Signal<String>,
    /// Whether the target currently has a freq range.
    #[prop(into)]
    has_freq: Signal<bool>,
    /// Whether the current freq range already matches the snap-to-view target.
    #[prop(into)]
    matches_view: Signal<bool>,
    /// Tooltip extension for the contract button (e.g. "Snap to 20–50 kHz").
    #[prop(into)]
    contract_target: Signal<String>,
    /// Remove freq bounds (expand to full).
    on_expand: Callback<()>,
    /// Snap freq to current view/focus.
    on_contract: Callback<()>,
) -> impl IntoView {
    let expand_disabled = Signal::derive(move || !has_freq.get());
    let contract_disabled = Signal::derive(move || matches_view.get());

    view! {
        <div class="canvas-overflow-freq-row">
            <span class="canvas-overflow-freq-text">{"Freq: "}{move || freq_text.get()}</span>
            <div class="canvas-overflow-btn-group">
                <button
                    class="canvas-overflow-action-btn"
                    disabled=move || expand_disabled.get()
                    title="Full frequency range (remove bounds)"
                    on:click=move |_| { if !expand_disabled.get_untracked() { on_expand.run(()); } }
                >
                    {ICON_EXPAND}
                </button>
                <button
                    class="canvas-overflow-action-btn"
                    disabled=move || contract_disabled.get()
                    title=move || {
                        let t = contract_target.get();
                        if t.is_empty() {
                            "Snap to band".to_string()
                        } else {
                            format!("Snap to band ({})", t)
                        }
                    }
                    on:click=move |_| { if !contract_disabled.get_untracked() { on_contract.run(()); } }
                >
                    {ICON_CONTRACT}
                </button>
            </div>
        </div>
    }
}

/// "..." overflow button + dropdown for transient selection.
#[component]
fn SelectionOverflowMenu() -> impl IntoView {
    let state = expect_context::<AppState>();
    let is_open = state.interaction.selection_overflow_open();

    // Reactive position: top-right corner of selection
    let pos = Signal::derive(move || {
        let sel = state.interaction.selection().get()?;
        let scroll = state.view.scroll_offset().get();
        let zoom = state.view.zoom_level().get();
        let canvas_w = state.viewmode.spectrogram_canvas_width().get();

        let files = state.library.files().get();
        let idx = state.library.current_index().get()?;
        let file = files.get(idx)?;
        let time_res = file.spectrogram.time_resolution;
        let file_max_freq = file.spectrogram.max_freq;
        let min_freq = state.view.min_display_freq().get().unwrap_or(0.0);
        let max_freq = state.view.max_display_freq().get().unwrap_or(file_max_freq);

        let canvas_h = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| {
                d.query_selector(".spectrogram-container canvas")
                    .ok()
                    .flatten()
            })
            .map(|el| el.get_bounding_client_rect().height())
            .unwrap_or(400.0);

        selection_top_left(
            &sel, scroll, time_res, zoom, canvas_w, canvas_h, min_freq, max_freq,
        )
    });

    let sel_details = Signal::derive(move || {
        let sel = state.interaction.selection().get()?;
        let d = sel.time_end - sel.time_start;
        if d < 0.0001 {
            return None;
        }
        let dur = crate::format_time::format_duration(d, 3);
        let freq_text = match (sel.freq_low, sel.freq_high) {
            (Some(fl), Some(fh)) => format!("{:.0} \u{2013} {:.0} kHz", fl / 1000.0, fh / 1000.0),
            _ => "\u{2014}".to_string(),
        };
        let has_freq = sel.freq_low.is_some() && sel.freq_high.is_some();
        Some((dur, freq_text, has_freq))
    });

    let has_freq_sig = Signal::derive(move || {
        state
            .interaction
            .selection()
            .get()
            .is_some_and(|s| s.freq_low.is_some() && s.freq_high.is_some())
    });

    let freq_text_sig = Signal::derive(move || {
        state
            .interaction
            .selection()
            .get()
            .and_then(|s| match (s.freq_low, s.freq_high) {
                (Some(fl), Some(fh)) => Some(format!(
                    "{:.0} \u{2013} {:.0} kHz",
                    fl / 1000.0,
                    fh / 1000.0
                )),
                _ => None,
            })
            .unwrap_or_else(|| "\u{2014}".to_string())
    });

    let matches_view_sig = Signal::derive(move || {
        let sel = match state.interaction.selection().get() {
            Some(s) => s,
            None => return false,
        };
        let (tlo, thi) = get_freq_bounds(&state);
        if thi <= tlo {
            return false;
        }
        match (sel.freq_low, sel.freq_high) {
            (Some(sl), Some(sh)) => (sl - tlo).abs() < 1.0 && (sh - thi).abs() < 1.0,
            _ => false,
        }
    });

    let contract_target_sig = Signal::derive(move || {
        let (lo, hi) = get_freq_bounds(&state);
        if hi > lo {
            fmt_freq_range(lo, hi)
        } else {
            String::new()
        }
    });

    let on_expand = Callback::new(move |_: ()| {
        if let Some(sel) = state.interaction.selection().get_untracked() {
            state.interaction.selection().set(Some(Selection {
                freq_low: None,
                freq_high: None,
                ..sel
            }));
        }
    });

    let on_contract = Callback::new(move |_: ()| {
        if let Some(sel) = state.interaction.selection().get_untracked() {
            let (lo, hi) = get_freq_bounds(&state);
            if hi > lo {
                state.interaction.selection().set(Some(Selection {
                    freq_low: Some(lo),
                    freq_high: Some(hi),
                    ..sel
                }));
            }
        }
    });

    view! {
        {move || {
            let (x, y) = match pos.get() { Some(p) => p, None => return None };
            let canvas_w = state.viewmode.spectrogram_canvas_width().get();

            // Anchor the "..." at the TOP-LEFT corner of the selection (matching
            // annotated regions), clamped so it never spills off either edge.
            let btn_left = (x + BTN_MARGIN).min((canvas_w - BTN_SIZE - BTN_MARGIN).max(0.0)).max(0.0);
            let btn_top = (y + BTN_MARGIN).max(0.0);

            Some(view! {
                <div
                    class="canvas-overflow-anchor"
                    style=format!(
                        "position: absolute; left: {:.0}px; top: {:.0}px; pointer-events: auto; z-index: 25;",
                        btn_left, btn_top
                    )
                >
                    <button
                        class="canvas-overflow-btn"
                        title="Selection options"
                        on:click=move |ev| {
                            ev.stop_propagation();
                            is_open.update(|v| *v = !*v);
                        }
                    >
                        "\u{22EF}"
                    </button>

                    {move || is_open.get().then(|| {
                        view! {
                            <div
                                class="canvas-overflow-backdrop"
                                on:click=move |_| is_open.set(false)
                            ></div>
                            <div class="canvas-overflow-menu">
                                {move || {
                                    if let Some((dur, _freq_text, has_freq)) = sel_details.get() {
                                        let btn_label = if has_freq { "Annotate Region" } else { "Annotate Segment" };
                                        view! {
                                            <div class="canvas-overflow-info">
                                                <div>"Duration: " {dur}</div>
                                            </div>
                                            <FreqRow
                                                freq_text=freq_text_sig
                                                has_freq=has_freq_sig
                                                matches_view=matches_view_sig
                                                contract_target=contract_target_sig
                                                on_expand=on_expand
                                                on_contract=on_contract
                                            />
                                            <div class="canvas-overflow-separator"></div>
                                            <button
                                                class="canvas-overflow-item"
                                                on:click=move |_| {
                                                    annotate_selection(&state);
                                                    is_open.set(false);
                                                }
                                            >
                                                {btn_label}
                                            </button>
                                        }.into_any()
                                    } else {
                                        view! { <span></span> }.into_any()
                                    }
                                }}
                            </div>
                        }
                    })}
                </div>
            })
        }}
    }
}

/// "..." overflow button + dropdown for selected annotations.
#[component]
fn AnnotationOverflowMenu() -> impl IntoView {
    let state = expect_context::<AppState>();
    let is_open = state.interaction.annotation_overflow_open();

    // Reactive position: top-right corner of first selected annotation
    let pos = Signal::derive(move || {
        let ids = state.annotations.selected_ids().get();
        if ids.is_empty() {
            return None;
        }
        let idx = state.library.current_index().get()?;
        let file_id = state.current_file_id_tracked()?;
        let store = state.annotations.store().get();
        let set = store.get(file_id)?;
        let ann = set.annotations.iter().find(|a| ids.contains(&a.id))?;

        let region = match &ann.kind {
            AnnotationKind::Region(r) => r,
            _ => return None,
        };

        let scroll = state.view.scroll_offset().get();
        let zoom = state.view.zoom_level().get();
        let canvas_w = state.viewmode.spectrogram_canvas_width().get();

        let files = state.library.files().get();
        let file = files.get(idx)?;
        let time_res = file.spectrogram.time_resolution;
        let file_max_freq = file.spectrogram.max_freq;
        let min_freq = state.view.min_display_freq().get().unwrap_or(0.0);
        let max_freq = state.view.max_display_freq().get().unwrap_or(file_max_freq);

        let canvas_h = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| {
                d.query_selector(".spectrogram-container canvas")
                    .ok()
                    .flatten()
            })
            .map(|el| el.get_bounding_client_rect().height())
            .unwrap_or(400.0);

        annotation_top_left(
            region.time_start,
            region.time_end,
            region.freq_high,
            scroll,
            time_res,
            zoom,
            canvas_w,
            canvas_h,
            min_freq,
            max_freq,
        )
    });

    // Get annotation info for display
    let ann_info = Signal::derive(move || {
        let ids = state.annotations.selected_ids().get();
        if ids.len() != 1 {
            return None;
        }
        let file_id = state.current_file_id_tracked()?;
        let store = state.annotations.store().get();
        let set = store.get(file_id)?;
        let ann = set.annotations.iter().find(|a| a.id == ids[0])?;
        let is_default = ann.label_default.unwrap_or(false);
        match &ann.kind {
            AnnotationKind::Region(r) => {
                let d = r.time_end - r.time_start;
                let dur = if d > 0.0001 {
                    Some(crate::format_time::format_duration(d, 3))
                } else {
                    None
                };
                let freq_text = match (r.freq_low, r.freq_high) {
                    (Some(fl), Some(fh)) => {
                        format!("{:.0} \u{2013} {:.0} kHz", fl / 1000.0, fh / 1000.0)
                    }
                    _ => "\u{2014}".to_string(),
                };
                let has_freq = r.freq_low.is_some() && r.freq_high.is_some();
                Some((
                    ann.id.clone(),
                    r.label.clone(),
                    r.is_locked(),
                    true, // is_region
                    is_default,
                    ann.tags.clone(),
                    dur,
                    freq_text,
                    has_freq,
                ))
            }
            _ => Some((
                ann.id.clone(),
                None,
                false,
                false,
                is_default,
                ann.tags.clone(),
                None,
                "\u{2014}".to_string(),
                false,
            )),
        }
    });

    let has_freq_sig = Signal::derive(move || ann_info.get().is_some_and(|info| info.8));
    let freq_text_sig = Signal::derive(move || {
        ann_info
            .get()
            .map(|info| info.7)
            .unwrap_or_else(|| "\u{2014}".to_string())
    });

    let matches_view_sig = Signal::derive(move || {
        let info = match ann_info.get() {
            Some(i) => i,
            None => return false,
        };
        if !info.8 {
            return false;
        }
        let (tlo, thi) = get_freq_bounds(&state);
        if thi <= tlo {
            return false;
        }
        // Re-read annotation freq
        let file_id = match state.current_file_id() {
            Some(i) => i,
            None => return false,
        };
        let store = state.annotations.store().get_untracked();
        let set = match store.get(file_id) {
            Some(s) => s,
            None => return false,
        };
        let ann = match set.annotations.iter().find(|a| a.id == info.0) {
            Some(a) => a,
            None => return false,
        };
        if let AnnotationKind::Region(r) = &ann.kind {
            if let (Some(fl), Some(fh)) = (r.freq_low, r.freq_high) {
                return (fl - tlo).abs() < 1.0 && (fh - thi).abs() < 1.0;
            }
        }
        false
    });

    let contract_target_sig = Signal::derive(move || {
        let (lo, hi) = get_freq_bounds(&state);
        if hi > lo {
            fmt_freq_range(lo, hi)
        } else {
            String::new()
        }
    });

    let on_expand = Callback::new(move |_: ()| {
        let ids = state.annotations.selected_ids().get_untracked();
        let file_id = match state.current_file_id() {
            Some(i) => i,
            None => return,
        };
        if ids.is_empty() {
            return;
        }
        state.snapshot_annotations();
        state.annotations.store().update(|store| {
            if let Some(set) = store.get_mut(file_id) {
                for ann in set.annotations.iter_mut() {
                    if ids.contains(&ann.id) {
                        if let AnnotationKind::Region(ref mut r) = ann.kind {
                            r.freq_low = None;
                            r.freq_high = None;
                            ann.modified_at = now_iso8601();
                        }
                    }
                }
            }
        });
        state.annotations.dirty().set(true);
    });

    let on_contract = Callback::new(move |_: ()| {
        let ids = state.annotations.selected_ids().get_untracked();
        let file_id = match state.current_file_id() {
            Some(i) => i,
            None => return,
        };
        if ids.is_empty() {
            return;
        }
        let (lo, hi) = get_freq_bounds(&state);
        if hi <= lo {
            return;
        }
        state.snapshot_annotations();
        state.annotations.store().update(|store| {
            if let Some(set) = store.get_mut(file_id) {
                for ann in set.annotations.iter_mut() {
                    if ids.contains(&ann.id) {
                        if let AnnotationKind::Region(ref mut r) = ann.kind {
                            r.freq_low = Some(lo);
                            r.freq_high = Some(hi);
                            ann.modified_at = now_iso8601();
                        }
                    }
                }
            }
        });
        state.annotations.dirty().set(true);
    });

    view! {
        {move || {
            let (x, y) = match pos.get() { Some(p) => p, None => return None };
            let canvas_w = state.viewmode.spectrogram_canvas_width().get();

            // Anchor the "..." at the TOP-LEFT corner of the annotation region,
            // clamped so it never spills off either edge.
            let btn_left = (x + BTN_MARGIN).min((canvas_w - BTN_SIZE - BTN_MARGIN).max(0.0)).max(0.0);
            let btn_top = (y + BTN_MARGIN).max(0.0);

            Some(view! {
                <div
                    class="canvas-overflow-anchor"
                    style=format!(
                        "position: absolute; left: {:.0}px; top: {:.0}px; pointer-events: auto; z-index: 25;",
                        btn_left, btn_top
                    )
                >
                    <button
                        class="canvas-overflow-btn"
                        title="Annotation options"
                        on:click=move |ev| {
                            ev.stop_propagation();
                            is_open.update(|v| *v = !*v);
                        }
                    >
                        "\u{22EF}"
                    </button>

                    {move || is_open.get().then(|| {
                        view! {
                            <div
                                class="canvas-overflow-backdrop"
                                on:click=move |_| is_open.set(false)
                            ></div>
                            <div class="canvas-overflow-menu">
                                {move || {
                                    let info = ann_info.get();
                                    if let Some((id, label, is_locked, is_region, label_is_default, tags, duration, _freq_text, _has_freq)) = info {
                                        let id_lock = id.clone();
                                        let id_del = id.clone();
                                        let lock_label = if is_locked { "\u{1F512} Unlock" } else { "\u{1F513} Lock" };
                                        let new_locked = !is_locked;

                                        view! {
                                            {label.map(|l| {
                                                let style = if label_is_default {
                                                    "font-weight: 600; color: #999; font-style: italic;"
                                                } else {
                                                    "font-weight: 600; color: #ccc;"
                                                };
                                                view! {
                                                    <div class="canvas-overflow-info">
                                                        <div style=style>{l}</div>
                                                    </div>
                                                }
                                            })}
                                            {duration.map(|d| view! {
                                                <div class="canvas-overflow-info"><div>"Duration: " {d}</div></div>
                                            })}
                                            <FreqRow
                                                freq_text=freq_text_sig
                                                has_freq=has_freq_sig
                                                matches_view=matches_view_sig
                                                contract_target=contract_target_sig
                                                on_expand=on_expand
                                                on_contract=on_contract
                                            />
                                            {(!tags.is_empty()).then(move || view! {
                                                <div class="canvas-overflow-info" style="color: #8cf; font-size: 10px;">
                                                    {tags.join(", ")}
                                                </div>
                                            })}
                                            <div class="canvas-overflow-separator"></div>
                                            <button
                                                class="canvas-overflow-item"
                                                on:click=move |_| {
                                                    state.annotations.is_new_edit().set(false);
                                                    state.annotations.editing().set(true);
                                                    is_open.set(false);
                                                }
                                            >
                                                "\u{270E} Edit label & tags"
                                            </button>
                                            {is_region.then(move || {
                                                view! {
                                                    <button
                                                        class="canvas-overflow-item"
                                                        on:click=move |_| {
                                                            toggle_annotation_lock(state, &id_lock, new_locked);
                                                            is_open.set(false);
                                                        }
                                                    >
                                                        {lock_label}
                                                    </button>
                                                }
                                            })}
                                            <div class="canvas-overflow-separator"></div>
                                            <button
                                                class="canvas-overflow-item danger"
                                                on:click=move |_| {
                                                    delete_annotation(state, &id_del);
                                                    is_open.set(false);
                                                }
                                            >
                                                "\u{00D7} Delete"
                                            </button>
                                        }.into_any()
                                    } else {
                                        view! { <span></span> }.into_any()
                                    }
                                }}
                            </div>
                        }
                    })}
                </div>
            })
        }}
    }
}
