use leptos::prelude::*;
use web_sys::{HtmlCanvasElement, MouseEvent};
use crate::canvas::coord::pointer_to_xtf;
use crate::canvas::hit_test::{hit_test_spec_handles, is_in_ff_drag_zone, hit_test_annotation_handles, hit_test_annotation_body};
use crate::canvas::spectrogram_renderer;
use crate::state::{AppState, CanvasTool, PlayStartMode, SpectrogramHandle, Selection, UndoEntry};
use crate::viewport;

pub const LABEL_AREA_WIDTH: f64 = 60.0;

/// Local interaction state for the spectrogram component.
/// These signals are only used by event handlers, not by rendering.
#[derive(Copy, Clone)]
pub struct SpectInteraction {
    /// Drag start for selection tool: (time, freq)
    pub drag_start: RwSignal<(f64, f64)>,
    /// Hand-tool drag state: (initial_client_x, initial_scroll_offset)
    pub hand_drag_start: RwSignal<(f64, f64)>,
    /// Pinch-to-zoom state (two-finger touch)
    pub pinch_state: RwSignal<Option<crate::components::pinch::PinchState>>,
    /// Raw (un-snapped) frequency where axis drag started
    pub axis_drag_raw_start: RwSignal<f64>,
    /// Label hover animation target (0.0 or 1.0)
    pub label_hover_target: RwSignal<f64>,
    /// Double-tap detection: timestamp of last tap
    pub last_tap_time: RwSignal<f64>,
    /// Double-tap detection: x-position of last tap
    pub last_tap_x: RwSignal<f64>,
    /// Time-axis tooltip: (x_px, tooltip_text) — None when not hovering the axis
    pub time_axis_tooltip: RwSignal<Option<(f64, String)>>,
    /// True when dragging along the bottom time axis to select a time segment
    pub time_axis_dragging: RwSignal<bool>,
    /// Raw (un-snapped) time where time-axis drag started
    pub time_axis_drag_raw_start: RwSignal<f64>,
    /// Velocity tracker for inertia scrolling
    pub velocity_tracker: StoredValue<crate::components::inertia::VelocityTracker>,
    /// Generation counter for cancelling inertia animations
    pub inertia_generation: StoredValue<u32>,
    /// True when drag started in the ambiguous bottom-left corner zone
    pub corner_drag_active: RwSignal<bool>,
    /// Client (x, y) at the start of a corner drag
    pub corner_drag_start_client: RwSignal<(f64, f64)>,
    /// Which axis is currently committed: None = undecided, true = Y (freq), false = X (time)
    pub corner_drag_axis: RwSignal<Option<bool>>,
    /// Saved FF range before corner drag, for restoration when switching to X-axis
    pub corner_drag_saved_ff: RwSignal<(f64, f64)>,
    /// Saved selection before corner drag, for restoration when switching to Y-axis
    pub corner_drag_saved_selection: RwSignal<Option<Selection>>,
    /// Pending annotation hit for Hand tool — deferred to mouseup so panning takes priority
    pub pending_annotation_hit: RwSignal<Option<(String, bool)>>, // (annotation_id, ctrl_held)
}

impl SpectInteraction {
    pub fn new() -> Self {
        Self {
            drag_start: RwSignal::new((0.0f64, 0.0f64)),
            hand_drag_start: RwSignal::new((0.0f64, 0.0f64)),
            pinch_state: RwSignal::new(None),
            axis_drag_raw_start: RwSignal::new(0.0f64),
            label_hover_target: RwSignal::new(0.0f64),
            last_tap_time: RwSignal::new(0.0f64),
            last_tap_x: RwSignal::new(0.0f64),
            time_axis_tooltip: RwSignal::new(None),
            time_axis_dragging: RwSignal::new(false),
            time_axis_drag_raw_start: RwSignal::new(0.0f64),
            velocity_tracker: StoredValue::new(crate::components::inertia::VelocityTracker::new()),
            inertia_generation: StoredValue::new(0u32),
            corner_drag_active: RwSignal::new(false),
            corner_drag_start_client: RwSignal::new((0.0, 0.0)),
            corner_drag_axis: RwSignal::new(None),
            corner_drag_saved_ff: RwSignal::new((0.0, 0.0)),
            corner_drag_saved_selection: RwSignal::new(None),
            pending_annotation_hit: RwSignal::new(None),
        }
    }
}

/// Apply a frequency handle drag (FF or HET). Shared by mouse and touch handlers.
pub fn apply_handle_drag(
    state: AppState,
    handle: SpectrogramHandle,
    freq_at_pointer: f64,
    file_max_freq: f64,
) {
    match handle {
        SpectrogramHandle::FfUpper => {
            let lo = state.ff_freq_lo.get_untracked();
            let clamped = freq_at_pointer.clamp(lo + 500.0, file_max_freq);
            state.set_ff_hi(clamped);
        }
        SpectrogramHandle::FfLower => {
            let hi = state.ff_freq_hi.get_untracked();
            let clamped = freq_at_pointer.clamp(0.0, hi - 500.0);
            state.set_ff_lo(clamped);
        }
        SpectrogramHandle::FfMiddle => {
            let lo = state.ff_freq_lo.get_untracked();
            let hi = state.ff_freq_hi.get_untracked();
            let bw = hi - lo;
            let mid = (lo + hi) / 2.0;
            let delta = freq_at_pointer - mid;
            let new_lo = (lo + delta).clamp(0.0, file_max_freq - bw);
            let new_hi = new_lo + bw;
            state.set_ff_range(new_lo, new_hi);
        }
        SpectrogramHandle::HetCenter => {
            state.het_freq_auto.set(false);
            let clamped = freq_at_pointer.clamp(1000.0, file_max_freq);
            state.het_frequency.set(clamped);
        }
        SpectrogramHandle::HetBandUpper => {
            state.het_cutoff_auto.set(false);
            let het_freq = state.het_frequency.get_untracked();
            let new_cutoff = (freq_at_pointer - het_freq).clamp(1000.0, 30000.0);
            state.het_cutoff.set(new_cutoff);
        }
        SpectrogramHandle::HetBandLower => {
            state.het_cutoff_auto.set(false);
            let het_freq = state.het_frequency.get_untracked();
            let new_cutoff = (het_freq - freq_at_pointer).clamp(1000.0, 30000.0);
            state.het_cutoff.set(new_cutoff);
        }
    }
}

/// Apply axis drag (frequency range selection on left axis).
/// Returns true if a meaningful range was updated.
pub fn apply_axis_drag(
    state: AppState,
    raw_start: f64,
    freq: f64,
    snap: f64,
) {
    let (snapped_start, snapped_end) = if freq > raw_start {
        // Dragging up: start floors down, end ceils up
        ((raw_start / snap).floor() * snap, (freq / snap).ceil() * snap)
    } else if freq < raw_start {
        // Dragging down: start ceils up, end floors down
        ((raw_start / snap).ceil() * snap, (freq / snap).floor() * snap)
    } else {
        let s = (raw_start / snap).round() * snap;
        (s, s)
    };
    state.axis_drag_start_freq.set(Some(snapped_start));
    state.axis_drag_current_freq.set(Some(snapped_end));
    // Live update FF range
    let lo = snapped_start.min(snapped_end);
    let hi = snapped_start.max(snapped_end);
    if hi - lo > 500.0 {
        state.set_ff_range(lo, hi);
    }
}

/// Resolve a pointer's pixel-Y into a frequency, given canvas and display state.
/// Shared helper for handle-drag in both mouse and touch handlers.
pub fn resolve_freq_at_pointer(
    px_y: f64,
    canvas_ref: &NodeRef<leptos::html::Canvas>,
    state: AppState,
) -> Option<(f64, f64)> {
    let canvas_el = canvas_ref.get()?;
    let canvas: &HtmlCanvasElement = canvas_el.as_ref();
    let ch = canvas.height() as f64;
    let files = state.files.get_untracked();
    let idx = state.current_file_index.get_untracked();
    let file = idx.and_then(|i| files.get(i));
    let file_max_freq = file.map(|f| f.spectrogram.max_freq).unwrap_or(96_000.0);
    let min_freq_val = state.min_display_freq.get_untracked().unwrap_or(0.0);
    let max_freq_val = state.max_display_freq.get_untracked().unwrap_or(file_max_freq);
    let freq = spectrogram_renderer::y_to_freq(px_y, min_freq_val, max_freq_val, ch);
    Some((freq, file_max_freq))
}

/// Finalize axis drag — auto-enable HFR if a meaningful range was selected.
pub fn finalize_axis_drag(state: AppState) {
    let stack = state.focus_stack.get_untracked();
    let range = stack.effective_range_ignoring_hfr();
    if range.hi - range.lo > 500.0 && !stack.hfr_enabled() {
        state.toggle_hfr();
    }
    // Auto-combine: if there's a time-only segment, upgrade to region with FF range
    if let Some(sel) = state.selection.get_untracked() {
        if sel.freq_low.is_none() && sel.freq_high.is_none() && sel.time_end - sel.time_start > 0.0001 {
            let ff = state.focus_stack.get_untracked().effective_range_ignoring_hfr();
            if ff.is_active() {
                state.selection.set(Some(Selection {
                    freq_low: Some(ff.lo),
                    freq_high: Some(ff.hi),
                    ..sel
                }));
            }
        }
    }
    state.axis_drag_start_freq.set(None);
    state.axis_drag_current_freq.set(None);
    state.is_dragging.set(false);
}

/// Perform hand-tool panning given a delta from the drag start.
pub fn apply_hand_pan(
    state: AppState,
    client_x: f64,
    canvas_ref: &NodeRef<leptos::html::Canvas>,
    hand_drag_start: (f64, f64),
) {
    let (start_client_x, start_scroll) = hand_drag_start;
    let dx = client_x - start_client_x;
    let Some(canvas_el) = canvas_ref.get() else { return };
    let canvas: &HtmlCanvasElement = canvas_el.as_ref();
    let cw = canvas.width() as f64;
    if cw == 0.0 { return; }
    let files = state.files.get_untracked();
    let idx = state.current_file_index.get_untracked();
    let file = idx.and_then(|i| files.get(i));
    let timeline = state.active_timeline.get_untracked();
    let time_res = if let Some(ref tl) = timeline {
        tl.segments.first().and_then(|s| files.get(s.file_index))
            .map(|f| f.spectrogram.time_resolution).unwrap_or(1.0)
    } else {
        file.as_ref().map(|f| f.spectrogram.time_resolution).unwrap_or(1.0)
    };
    let zoom = state.zoom_level.get_untracked();
    let visible_time = viewport::visible_time(cw, zoom, time_res);
    let duration = if let Some(ref tl) = timeline {
        tl.total_duration_secs
    } else {
        file.as_ref().map(|f| f.audio.duration_secs).unwrap_or(f64::MAX)
    };
    let from_here_mode = state.play_start_mode.get_untracked() == PlayStartMode::FromHere;
    let dt = -(dx / cw) * visible_time;
    state.suspend_follow();
    state.scroll_offset.set(viewport::clamp_scroll_for_mode(start_scroll + dt, duration, visible_time, from_here_mode));
}

/// Apply annotation resize based on which handle is being dragged.
pub fn apply_annotation_resize(
    state: AppState,
    ann_id: String,
    handle: crate::state::ResizeHandlePosition,
    time: f64,
    freq: f64,
) {
    use crate::state::ResizeHandlePosition::*;

    let file_idx = state.current_file_index.get_untracked().unwrap_or(0);
    state.annotation_store.update(|store| {
        if let Some(Some(set)) = store.sets.get_mut(file_idx) {
            if let Some(ann) = set.annotations.iter_mut().find(|a| a.id == ann_id) {
                if let crate::annotations::AnnotationKind::Region(ref mut r) = ann.kind {
                    match handle {
                        Left => r.time_start = time.min(r.time_end - 0.0001),
                        Right => r.time_end = time.max(r.time_start + 0.0001),
                        Top => if r.freq_high.is_some() {
                            let lo = r.freq_low.unwrap_or(0.0);
                            r.freq_high = Some(freq.max(lo + 100.0));
                        },
                        Bottom => if r.freq_low.is_some() {
                            let hi = r.freq_high.unwrap_or(f64::MAX);
                            r.freq_low = Some(freq.min(hi - 100.0));
                        },
                        TopLeft => {
                            r.time_start = time.min(r.time_end - 0.0001);
                            if r.freq_high.is_some() {
                                let lo = r.freq_low.unwrap_or(0.0);
                                r.freq_high = Some(freq.max(lo + 100.0));
                            }
                        },
                        TopRight => {
                            r.time_end = time.max(r.time_start + 0.0001);
                            if r.freq_high.is_some() {
                                let lo = r.freq_low.unwrap_or(0.0);
                                r.freq_high = Some(freq.max(lo + 100.0));
                            }
                        },
                        BottomLeft => {
                            r.time_start = time.min(r.time_end - 0.0001);
                            if r.freq_low.is_some() {
                                let hi = r.freq_high.unwrap_or(f64::MAX);
                                r.freq_low = Some(freq.min(hi - 100.0));
                            }
                        },
                        BottomRight => {
                            r.time_end = time.max(r.time_start + 0.0001);
                            if r.freq_low.is_some() {
                                let hi = r.freq_high.unwrap_or(f64::MAX);
                                r.freq_low = Some(freq.min(hi - 100.0));
                            }
                        },
                    }
                }
            }
        }
    });
}

// ── Mouse event handlers ───────────────────────────────────────────────────

pub fn on_mousedown(
    ev: MouseEvent,
    ix: SpectInteraction,
    canvas_ref: &NodeRef<leptos::html::Canvas>,
    state: AppState,
) {
    if ev.button() != 0 { return; }

    // Check for spec handle drag first (FF or HET — takes priority over tool)
    // FF handles only start drag when clicking within the center handle zone.
    if let Some(handle) = state.spec_hover_handle.get_untracked() {
        let is_ff = matches!(handle, SpectrogramHandle::FfUpper | SpectrogramHandle::FfLower | SpectrogramHandle::FfMiddle);
        let allow_drag = if is_ff {
            if let Some((px_x, _, _, _)) = pointer_to_xtf(ev.client_x() as f64, ev.client_y() as f64, canvas_ref, &state) {
                if let Some(canvas_el) = canvas_ref.get() {
                    let canvas: &HtmlCanvasElement = canvas_el.as_ref();
                    is_in_ff_drag_zone(px_x, canvas.width() as f64)
                } else { false }
            } else { false }
        } else {
            true // HET handles drag from anywhere
        };
        if allow_drag {
            state.spec_drag_handle.set(Some(handle));
            state.is_dragging.set(true);
            ev.prevent_default();
            return;
        }
    }

    // Check for annotation resize handle drag (takes priority over axis/tool drags)
    if let Some((ref ann_id, handle_pos)) = state.annotation_hover_handle.get_untracked() {
        // Check if the annotation is locked
        let file_idx = state.current_file_index.get_untracked().unwrap_or(0);
        let store = state.annotation_store.get_untracked();
        let locked = store.sets.get(file_idx)
            .and_then(|s| s.as_ref())
            .and_then(|set| set.annotations.iter().find(|a| a.id == *ann_id))
            .and_then(|a| match &a.kind {
                crate::annotations::AnnotationKind::Region(r) => Some(r.is_locked()),
                _ => None,
            })
            .unwrap_or(false);

        if !locked {
            // Snapshot for undo
            let snapshot = store.sets.get(file_idx).and_then(|s| s.clone());
            state.undo_stack.update(|stack| {
                stack.push_undo(UndoEntry { file_idx, snapshot });
            });
            // Store original bounds
            if let Some(set) = store.sets.get(file_idx).and_then(|s| s.as_ref()) {
                if let Some(a) = set.annotations.iter().find(|a| a.id == *ann_id) {
                    if let crate::annotations::AnnotationKind::Region(ref r) = a.kind {
                        state.annotation_drag_original.set(Some((r.time_start, r.time_end, r.freq_low, r.freq_high)));
                    }
                }
            }
            state.annotation_drag_handle.set(Some((ann_id.clone(), handle_pos)));
            state.is_dragging.set(true);
            ev.prevent_default();
            return;
        }
    }

    // Check for ambiguous corner drag (bottom-left: both axis zones overlap)
    if let Some((px_x, px_y, t, freq)) = pointer_to_xtf(ev.client_x() as f64, ev.client_y() as f64, canvas_ref, &state) {
        if let Some(canvas_el) = canvas_ref.get() {
            let canvas: &HtmlCanvasElement = canvas_el.as_ref();
            let ch = canvas.get_bounding_client_rect().height();
            let in_left_axis = px_x < LABEL_AREA_WIDTH && !state.display_transform.get_untracked();
            let in_bottom_axis = px_y > ch - 16.0;

            if in_left_axis && in_bottom_axis {
                // Corner zone — defer axis choice until drag direction is clear.
                // Pre-initialize both axis drag states so we can commit to either.
                let ff_lo = state.ff_freq_lo.get_untracked();
                let ff_hi = state.ff_freq_hi.get_untracked();
                ix.corner_drag_saved_ff.set((ff_lo, ff_hi));
                ix.corner_drag_saved_selection.set(state.selection.get_untracked());

                // Y-axis (freq) init
                let _snap = if ev.shift_key() { 10_000.0 } else { 5_000.0 };
                let has_range = ff_hi > ff_lo;
                let raw_start_freq = if ev.shift_key() && has_range {
                    let anchor = if (freq - ff_lo).abs() < (freq - ff_hi).abs() { ff_hi } else { ff_lo };
                    anchor
                } else {
                    freq
                };
                ix.axis_drag_raw_start.set(raw_start_freq);

                // X-axis (time) init
                let anchor_time = if ev.shift_key() {
                    state.selection.get_untracked().and_then(|sel| {
                        if sel.time_end - sel.time_start > 0.0001 {
                            Some(if (t - sel.time_start).abs() < (t - sel.time_end).abs() {
                                sel.time_end
                            } else {
                                sel.time_start
                            })
                        } else { None }
                    })
                } else { None };
                ix.time_axis_drag_raw_start.set(anchor_time.unwrap_or(t));

                ix.corner_drag_active.set(true);
                ix.corner_drag_start_client.set((ev.client_x() as f64, ev.client_y() as f64));
                ix.corner_drag_axis.set(None);
                state.is_dragging.set(true);
                ev.prevent_default();
                return;
            }
        }
    }

    // Check for axis drag (left axis frequency range selection) — disabled in xform view
    if let Some((px_x, _, _, freq)) = pointer_to_xtf(ev.client_x() as f64, ev.client_y() as f64, canvas_ref, &state) {
        if px_x < LABEL_AREA_WIDTH && !state.display_transform.get_untracked() {
            // Single click (non-shift) with active HFR: toggle it off
            let ff_lo = state.ff_freq_lo.get_untracked();
            let ff_hi = state.ff_freq_hi.get_untracked();
            let has_range = ff_hi > ff_lo;
            if !ev.shift_key() && has_range && state.focus_stack.get_untracked().hfr_enabled() {
                state.toggle_hfr();
                ev.prevent_default();
                return;
            }
            let (raw_start, snap) = if ev.shift_key() && has_range {
                // Anchor at the edge farthest from the click
                let anchor = if (freq - ff_lo).abs() < (freq - ff_hi).abs() { ff_hi } else { ff_lo };
                (anchor, 5_000.0)
            } else {
                let snap = if ev.shift_key() { 10_000.0 } else { 5_000.0 };
                (freq, snap)
            };
            let snapped = (freq / snap).round() * snap;
            ix.axis_drag_raw_start.set(raw_start);
            state.axis_drag_start_freq.set(Some((raw_start / snap).round() * snap));
            state.axis_drag_current_freq.set(Some(snapped));
            // Live update FF range immediately for shift-extend
            if ev.shift_key() && has_range {
                let lo = raw_start.min(freq);
                let hi = raw_start.max(freq);
                if hi - lo > 500.0 {
                    state.set_ff_range(lo, hi);
                }
            }
            state.is_dragging.set(true);
            ev.prevent_default();
            return;
        }
    }

    // Check for time-axis drag (bottom axis time segment selection)
    if let Some((px_x, px_y, t, _)) = pointer_to_xtf(ev.client_x() as f64, ev.client_y() as f64, canvas_ref, &state) {
        if let Some(canvas_el) = canvas_ref.get() {
            let canvas: &HtmlCanvasElement = canvas_el.as_ref();
            let ch = canvas.get_bounding_client_rect().height();
            if px_y > ch - 16.0 && px_x > LABEL_AREA_WIDTH {
                // Shift+click: extend existing time selection (anchor at far edge)
                let anchor = if ev.shift_key() {
                    if let Some(sel) = state.selection.get_untracked() {
                        if sel.time_end - sel.time_start > 0.0001 {
                            Some(if (t - sel.time_start).abs() < (t - sel.time_end).abs() {
                                sel.time_end
                            } else {
                                sel.time_start
                            })
                        } else { None }
                    } else { None }
                } else { None };
                let start = anchor.unwrap_or(t);
                ix.time_axis_dragging.set(true);
                ix.time_axis_drag_raw_start.set(start);
                state.selection.set(Some(Selection {
                    time_start: start.min(t),
                    time_end: start.max(t),
                    freq_low: None,
                    freq_high: None,
                }));
                state.is_dragging.set(true);
                ev.prevent_default();
                return;
            }
        }
    }

    // Check for annotation body click-to-select only in Hand mode.
    // In Selection mode, allow drags to start on top of existing annotations.
    if state.canvas_tool.get_untracked() == CanvasTool::Hand {
        if let Some((px_x, px_y, _, _)) = pointer_to_xtf(ev.client_x() as f64, ev.client_y() as f64, canvas_ref, &state) {
        let file_idx = state.current_file_index.get_untracked().unwrap_or(0);
        let store = state.annotation_store.get_untracked();
        if let Some(Some(set)) = store.sets.get(file_idx) {
            if let Some(canvas_el) = canvas_ref.get() {
                let canvas: &HtmlCanvasElement = canvas_el.as_ref();
                let cw = canvas.width() as f64;
                let ch = canvas.height() as f64;
                let files = state.files.get_untracked();
                let file = files.get(file_idx);
                let file_max_freq = file.map(|f| f.spectrogram.max_freq).unwrap_or(96_000.0);
                let min_freq = state.min_display_freq.get_untracked().unwrap_or(0.0);
                let max_freq = state.max_display_freq.get_untracked().unwrap_or(file_max_freq);
                let scroll = state.scroll_offset.get_untracked();
                let time_res = file.map(|f| f.spectrogram.time_resolution).unwrap_or(1.0);
                let zoom = state.zoom_level.get_untracked();

                if let Some(hit_id) = hit_test_annotation_body(
                    set, px_x, px_y, min_freq, max_freq, scroll, time_res, zoom, cw, ch,
                ) {
                    let ctrl = ev.ctrl_key() || ev.meta_key();
                    // Defer annotation selection — panning takes priority.
                    ix.pending_annotation_hit.set(Some((hit_id, ctrl)));
                }
            }
        }
    }
    }

    // Click on empty area deselects annotations (unless modifier held)
    // For Hand tool: defer to mouseup so panning isn't blocked
    if state.canvas_tool.get_untracked() != CanvasTool::Hand {
        if !ev.ctrl_key() && !ev.meta_key() && !ev.shift_key() {
            let ids = state.selected_annotation_ids.get_untracked();
            if !ids.is_empty() {
                state.selected_annotation_ids.set(Vec::new());
            }
        }
    }

    match state.canvas_tool.get_untracked() {
        CanvasTool::Hand => {
            state.is_dragging.set(true);
            ix.hand_drag_start.set((ev.client_x() as f64, state.scroll_offset.get_untracked()));
        }
        CanvasTool::Selection => {
            if let Some((_, _, t, f)) = pointer_to_xtf(ev.client_x() as f64, ev.client_y() as f64, canvas_ref, &state) {
                state.is_dragging.set(true);
                ix.drag_start.set((t, f));
                state.selection.set(None);
            }
        }
    }
}

pub fn on_mousemove(
    ev: MouseEvent,
    ix: SpectInteraction,
    canvas_ref: &NodeRef<leptos::html::Canvas>,
    state: AppState,
) {
    if let Some((px_x, px_y, t, f)) = pointer_to_xtf(ev.client_x() as f64, ev.client_y() as f64, canvas_ref, &state) {
        // Always track hover position
        state.mouse_freq.set(Some(f));
        state.mouse_canvas_x.set(px_x);
        state.cursor_time.set(Some(t));

        // Time-axis tooltip: show full datetime when hovering bottom 16px
        if let Some(canvas_el) = canvas_ref.get() {
            let canvas: &HtmlCanvasElement = canvas_el.as_ref();
            let ch = canvas.get_bounding_client_rect().height();
            if px_y > ch - 16.0 && px_x > LABEL_AREA_WIDTH {
                let tooltip = state.current_file()
                    .and_then(|f| f.recording_start_info())
                    .map(|(epoch, source)| {
                        crate::canvas::time_markers::format_clock_time_full(epoch, t, source)
                    });
                if let Some(text) = tooltip {
                    ix.time_axis_tooltip.set(Some((px_x, text)));
                } else {
                    ix.time_axis_tooltip.set(None);
                }
            } else {
                ix.time_axis_tooltip.set(None);
            }
        }

        // Update label hover target and in-label-area / time-axis state
        let in_label_area = px_x < LABEL_AREA_WIDTH;
        state.mouse_in_label_area.set(in_label_area);
        let in_time_axis = if let Some(canvas_el) = canvas_ref.get() {
            let canvas: &HtmlCanvasElement = canvas_el.as_ref();
            let ch = canvas.get_bounding_client_rect().height();
            px_y > ch - 16.0 && px_x > LABEL_AREA_WIDTH
        } else { false };
        state.mouse_in_time_axis.set(in_time_axis);
        let current_target = ix.label_hover_target.get_untracked();
        let new_target = if in_label_area { 1.0 } else { 0.0 };
        if current_target != new_target {
            ix.label_hover_target.set(new_target);
        }

        if state.is_dragging.get_untracked() {
            // Spec handle drag takes priority
            if let Some(handle) = state.spec_drag_handle.get_untracked() {
                if let Some((freq_at_mouse, file_max_freq)) = resolve_freq_at_pointer(px_y, canvas_ref, state) {
                    apply_handle_drag(state, handle, freq_at_mouse, file_max_freq);
                }
                return;
            }

            // Annotation resize handle drag takes second priority
            if let Some((ref ann_id, handle_pos)) = state.annotation_drag_handle.get_untracked() {
                apply_annotation_resize(state, ann_id.clone(), handle_pos, t, f);
                return;
            }

            // Corner drag: determine axis from drag direction, allow switching
            if ix.corner_drag_active.get_untracked() {
                let (sx, sy) = ix.corner_drag_start_client.get_untracked();
                let dx = (ev.client_x() as f64 - sx).abs();
                let dy = (ev.client_y() as f64 - sy).abs();
                // Need a minimum movement before committing
                if dx < 4.0 && dy < 4.0 {
                    return; // still undecided
                }
                let want_y_axis = dy >= dx; // true = freq axis, false = time axis
                let prev_axis = ix.corner_drag_axis.get_untracked();
                let axis_changed = prev_axis != Some(want_y_axis);
                if axis_changed {
                    if want_y_axis {
                        // Switching to Y-axis: restore saved selection, activate freq drag
                        let saved_sel = ix.corner_drag_saved_selection.get_untracked();
                        state.selection.set(saved_sel);
                        ix.time_axis_dragging.set(false);
                        // Set up freq axis drag signals
                        let raw_start = ix.axis_drag_raw_start.get_untracked();
                        let snap = if ev.shift_key() { 10_000.0 } else { 5_000.0 };
                        let snapped = (raw_start / snap).round() * snap;
                        state.axis_drag_start_freq.set(Some(snapped));
                        state.axis_drag_current_freq.set(Some(snapped));
                    } else {
                        // Switching to X-axis: restore saved FF range, activate time drag
                        let (saved_lo, saved_hi) = ix.corner_drag_saved_ff.get_untracked();
                        if saved_hi > saved_lo {
                            state.set_ff_range(saved_lo, saved_hi);
                        }
                        state.axis_drag_start_freq.set(None);
                        state.axis_drag_current_freq.set(None);
                        ix.time_axis_dragging.set(true);
                    }
                    ix.corner_drag_axis.set(Some(want_y_axis));
                }
                // Apply the committed axis's drag update
                if want_y_axis {
                    let raw_start = ix.axis_drag_raw_start.get_untracked();
                    let snap = if ev.shift_key() { 10_000.0 } else { 5_000.0 };
                    apply_axis_drag(state, raw_start, f, snap);
                } else {
                    let t0 = ix.time_axis_drag_raw_start.get_untracked();
                    state.selection.set(Some(Selection {
                        time_start: t0.min(t),
                        time_end: t0.max(t),
                        freq_low: None,
                        freq_high: None,
                    }));
                }
                return;
            }

            // Axis drag takes third priority
            if state.axis_drag_start_freq.get_untracked().is_some() {
                let raw_start = ix.axis_drag_raw_start.get_untracked();
                let snap = if ev.shift_key() { 10_000.0 } else { 5_000.0 };
                apply_axis_drag(state, raw_start, f, snap);
                return;
            }

            // Time-axis drag takes third priority
            if ix.time_axis_dragging.get_untracked() {
                let t0 = ix.time_axis_drag_raw_start.get_untracked();
                state.selection.set(Some(Selection {
                    time_start: t0.min(t),
                    time_end: t0.max(t),
                    freq_low: None,
                    freq_high: None,
                }));
                return;
            }

            match state.canvas_tool.get_untracked() {
                CanvasTool::Hand => {
                    apply_hand_pan(state, ev.client_x() as f64, canvas_ref, ix.hand_drag_start.get_untracked());
                }
                CanvasTool::Selection => {
                    let (t0, f0) = ix.drag_start.get_untracked();
                    state.selection.set(Some(Selection {
                        time_start: t0.min(t),
                        time_end: t0.max(t),
                        freq_low: Some(f0.min(f)),
                        freq_high: Some(f0.max(f)),
                    }));
                }
            }
        } else {
            // Not dragging — do spec handle hover detection (FF + HET)
            // Skip handle hover when in label area (to allow axis drag)
            if !in_label_area {
                if let Some((_, file_max_freq)) = resolve_freq_at_pointer(px_y, canvas_ref, state) {
                    let min_freq_val = state.min_display_freq.get_untracked().unwrap_or(0.0);
                    let max_freq_val = state.max_display_freq.get_untracked().unwrap_or(file_max_freq);
                    let canvas_el = canvas_ref.get();
                    if let Some(canvas_el) = canvas_el {
                        let canvas: &HtmlCanvasElement = canvas_el.as_ref();
                        let cw = canvas.width() as f64;
                        let ch = canvas.height() as f64;
                        let handle = hit_test_spec_handles(
                            &state, px_y, min_freq_val, max_freq_val, ch, 8.0,
                        );
                        state.spec_hover_handle.set(handle);

                        // Annotation resize handle hover detection
                        let selected_ids = state.selected_annotation_ids.get_untracked();
                        if !selected_ids.is_empty() {
                            let file_idx = state.current_file_index.get_untracked().unwrap_or(0);
                            let store = state.annotation_store.get_untracked();
                            if let Some(Some(set)) = store.sets.get(file_idx) {
                                let scroll = state.scroll_offset.get_untracked();
                                let files = state.files.get_untracked();
                                let time_res = files.get(file_idx)
                                    .map(|f| f.spectrogram.time_resolution).unwrap_or(1.0);
                                let zoom = state.zoom_level.get_untracked();
                                let ann_handle = hit_test_annotation_handles(
                                    set, &selected_ids,
                                    px_x, px_y,
                                    min_freq_val, max_freq_val,
                                    scroll, time_res, zoom, cw, ch,
                                    crate::canvas::hit_test::ANNOTATION_HANDLE_HIT_RADIUS,
                                );
                                state.annotation_hover_handle.set(ann_handle);
                            } else {
                                state.annotation_hover_handle.set(None);
                            }
                        } else {
                            state.annotation_hover_handle.set(None);
                        }
                    }
                }
            } else {
                state.spec_hover_handle.set(None);
                state.annotation_hover_handle.set(None);
            }
        }
    }
}

pub fn on_mouseleave(
    _ev: MouseEvent,
    ix: SpectInteraction,
    state: AppState,
) {
    state.mouse_freq.set(None);
    state.mouse_in_label_area.set(false);
    state.mouse_in_time_axis.set(false);
    state.cursor_time.set(None);
    ix.label_hover_target.set(0.0);
    state.is_dragging.set(false);
    state.spec_drag_handle.set(None);
    state.spec_hover_handle.set(None);
    state.annotation_drag_handle.set(None);
    state.annotation_drag_original.set(None);
    state.annotation_hover_handle.set(None);
    state.axis_drag_start_freq.set(None);
    state.axis_drag_current_freq.set(None);
    ix.time_axis_dragging.set(false);
    ix.time_axis_tooltip.set(None);
    ix.corner_drag_active.set(false);
    ix.corner_drag_axis.set(None);
    ix.pending_annotation_hit.set(None);
}

pub fn on_mouseup(
    ev: MouseEvent,
    ix: SpectInteraction,
    canvas_ref: &NodeRef<leptos::html::Canvas>,
    state: AppState,
) {
    if !state.is_dragging.get_untracked() { return; }

    // End HET/FF handle drag
    if state.spec_drag_handle.get_untracked().is_some() {
        state.spec_drag_handle.set(None);
        state.is_dragging.set(false);
        return;
    }

    // End annotation resize handle drag
    if let Some((ref ann_id, _)) = state.annotation_drag_handle.get_untracked() {
        let file_idx = state.current_file_index.get_untracked().unwrap_or(0);
        let now = js_sys::Date::new_0().to_iso_string().as_string().unwrap_or_default();
        state.annotation_store.update(|store| {
            if let Some(Some(set)) = store.sets.get_mut(file_idx) {
                if let Some(a) = set.annotations.iter_mut().find(|a| a.id == *ann_id) {
                    a.modified_at = now;
                }
            }
        });
        state.annotations_dirty.set(true);
        state.annotation_drag_handle.set(None);
        state.annotation_drag_original.set(None);
        state.is_dragging.set(false);
        return;
    }

    // End corner drag — clear corner state, then fall through to axis/time finalization
    let was_corner = ix.corner_drag_active.get_untracked();
    if was_corner {
        ix.corner_drag_active.set(false);
        let committed = ix.corner_drag_axis.get_untracked();
        ix.corner_drag_axis.set(None);
        // If never committed to an axis (tiny drag), just cancel
        if committed.is_none() {
            state.is_dragging.set(false);
            state.axis_drag_start_freq.set(None);
            state.axis_drag_current_freq.set(None);
            ix.time_axis_dragging.set(false);
            return;
        }
        // Otherwise fall through — the committed axis's signals are set
    }

    // End axis drag (FF range already updated live during drag)
    if state.axis_drag_start_freq.get_untracked().is_some() {
        finalize_axis_drag(state);
        return;
    }

    // End time-axis drag (selection already updated live during drag)
    if ix.time_axis_dragging.get_untracked() {
        ix.time_axis_dragging.set(false);
        state.is_dragging.set(false);
        // Clear tiny accidental drags
        if let Some(sel) = state.selection.get_untracked() {
            if sel.time_end - sel.time_start < 0.0001 {
                state.selection.set(None);
            } else if sel.freq_low.is_none() {
                // Auto-combine: if FF range is active, upgrade segment to region
                let ff = state.focus_stack.get_untracked().effective_range_ignoring_hfr();
                if ff.is_active() {
                    state.selection.set(Some(Selection {
                        freq_low: Some(ff.lo),
                        freq_high: Some(ff.hi),
                        ..sel
                    }));
                }
            }
        }
        return;
    }

    state.is_dragging.set(false);

    if state.canvas_tool.get_untracked() == CanvasTool::Hand {
        let (start_x, _) = ix.hand_drag_start.get_untracked();
        let dx = (ev.client_x() as f64 - start_x).abs();
        let was_click = dx < 3.0;

        if was_click {
            // Handle deferred annotation selection on click
            if let Some((hit_id, ctrl)) = ix.pending_annotation_hit.get_untracked() {
                if ctrl {
                    state.selected_annotation_ids.update(|ids| {
                        if let Some(pos) = ids.iter().position(|id| *id == hit_id) {
                            ids.remove(pos);
                        } else {
                            ids.push(hit_id.clone());
                        }
                    });
                } else {
                    state.selected_annotation_ids.set(vec![hit_id.clone()]);
                }
                state.last_clicked_annotation_id.set(Some(hit_id));
            } else {
                // Click on empty area deselects annotations
                if !ev.ctrl_key() && !ev.meta_key() && !ev.shift_key() {
                    let ids = state.selected_annotation_ids.get_untracked();
                    if !ids.is_empty() {
                        state.selected_annotation_ids.set(Vec::new());
                    }
                }
            }
            // Bookmark while playing
            if state.is_playing.get_untracked() {
                let t = state.playhead_time.get_untracked();
                state.bookmarks.update(|bm| bm.push(crate::state::Bookmark { time: t }));
            }
        }

        ix.pending_annotation_hit.set(None);
        return;
    }
    if state.canvas_tool.get_untracked() != CanvasTool::Selection { return; }
    if let Some((_, _, t, f)) = pointer_to_xtf(ev.client_x() as f64, ev.client_y() as f64, canvas_ref, &state) {
        let (t0, f0) = ix.drag_start.get_untracked();
        let sel = Selection {
            time_start: t0.min(t),
            time_end: t0.max(t),
            freq_low: Some(f0.min(f)),
            freq_high: Some(f0.max(f)),
        };
        if sel.time_end - sel.time_start > 0.0001 {
            state.selection.set(Some(sel));
            if state.annotation_auto_focus.get_untracked() {
                if let (Some(lo), Some(hi)) = (sel.freq_low, sel.freq_high) {
                    if hi - lo > 100.0 {
                        state.set_ff_range(lo, hi);
                    }
                }
            }
        } else {
            state.selection.set(None);
        }
    }
}

pub fn on_dblclick(
    ev: MouseEvent,
    canvas_ref: &NodeRef<leptos::html::Canvas>,
    state: AppState,
) {
    let has_range = state.ff_freq_hi.get_untracked() > state.ff_freq_lo.get_untracked();
    if !has_range { return; }

    if let Some((px_x, _, _, _)) = pointer_to_xtf(ev.client_x() as f64, ev.client_y() as f64, canvas_ref, &state) {
        let in_label = px_x < LABEL_AREA_WIDTH;
        let on_handle = matches!(
            state.spec_hover_handle.get_untracked(),
            Some(SpectrogramHandle::FfUpper | SpectrogramHandle::FfLower | SpectrogramHandle::FfMiddle)
        );
        if in_label || on_handle {
            state.toggle_hfr();
            ev.prevent_default();
        }
    }
}

// ── Touch event handlers ───────────────────────────────────────────────────

pub fn on_touchstart(
    ev: web_sys::TouchEvent,
    ix: SpectInteraction,
    canvas_ref: &NodeRef<leptos::html::Canvas>,
    state: AppState,
) {
    // Cancel any ongoing inertia animation immediately
    crate::components::inertia::cancel_inertia(ix.inertia_generation);
    ix.velocity_tracker.update_value(|t| t.reset());

    let touches = ev.touches();
    let n = touches.length();

    // Two-finger: initialize pinch-to-zoom
    if n == 2 {
        ev.prevent_default();
        use crate::components::pinch::{two_finger_geometry, PinchState};
        if let Some((mid_x, dist)) = two_finger_geometry(&touches) {
            let files = state.files.get_untracked();
            let idx = state.current_file_index.get_untracked();
            let file = idx.and_then(|i| files.get(i));
            let time_res = file.as_ref().map(|f| f.spectrogram.time_resolution).unwrap_or(1.0);
            let duration = file.as_ref().map(|f| f.audio.duration_secs).unwrap_or(f64::MAX);
            ix.pinch_state.set(Some(PinchState {
                initial_dist: dist,
                initial_zoom: state.zoom_level.get_untracked(),
                initial_scroll: state.scroll_offset.get_untracked(),
                initial_mid_client_x: mid_x,
                time_res,
                duration,
                from_here_mode: state.play_start_mode.get_untracked() == PlayStartMode::FromHere,
            }));
        }
        // End any in-progress single-touch gesture
        state.is_dragging.set(false);
        state.spec_drag_handle.set(None);
        state.axis_drag_start_freq.set(None);
        state.axis_drag_current_freq.set(None);
        ix.time_axis_dragging.set(false);
        ix.corner_drag_active.set(false);
        ix.corner_drag_axis.set(None);
        return;
    }

    if n != 1 { return; }
    // Transitioning from 2 to 1 finger — re-anchor pan position
    if ix.pinch_state.get_untracked().is_some() {
        ix.pinch_state.set(None);
        if let Some(touch) = touches.get(0) {
            ix.hand_drag_start.set((touch.client_x() as f64, state.scroll_offset.get_untracked()));
            if state.canvas_tool.get_untracked() == CanvasTool::Hand {
                state.is_dragging.set(true);
            }
        }
        return;
    }

    let touch = touches.get(0).unwrap();

    // Check for spec handle drag first — hit-test at touch position
    if let Some((px_x, px_y, _, _)) = pointer_to_xtf(touch.client_x() as f64, touch.client_y() as f64, canvas_ref, &state) {
        if let Some(canvas_el) = canvas_ref.get() {
            let canvas: &HtmlCanvasElement = canvas_el.as_ref();
            let cw = canvas.width() as f64;
            let ch = canvas.height() as f64;
            let files = state.files.get_untracked();
            let idx = state.current_file_index.get_untracked();
            let file = idx.and_then(|i| files.get(i));
            let file_max_freq = file.map(|f| f.spectrogram.max_freq).unwrap_or(96_000.0);
            let min_freq_val = state.min_display_freq.get_untracked().unwrap_or(0.0);
            let max_freq_val = state.max_display_freq.get_untracked().unwrap_or(file_max_freq);
            let handle = hit_test_spec_handles(
                &state, px_y, min_freq_val, max_freq_val, ch, 16.0, // wider touch target
            );
            if let Some(handle) = handle {
                let is_ff = matches!(handle, SpectrogramHandle::FfUpper | SpectrogramHandle::FfLower | SpectrogramHandle::FfMiddle);
                if !is_ff || is_in_ff_drag_zone(px_x, cw) {
                    state.spec_drag_handle.set(Some(handle));
                    state.is_dragging.set(true);
                    ev.prevent_default();
                    return;
                }
            }
        }
    }

    // Check for annotation resize handle drag (touch)
    if let Some((px_x, px_y, _, _)) = pointer_to_xtf(touch.client_x() as f64, touch.client_y() as f64, canvas_ref, &state) {
        if let Some(canvas_el) = canvas_ref.get() {
            let canvas: &HtmlCanvasElement = canvas_el.as_ref();
            let cw = canvas.width() as f64;
            let ch = canvas.height() as f64;
            let selected_ids = state.selected_annotation_ids.get_untracked();
            if !selected_ids.is_empty() {
                let file_idx = state.current_file_index.get_untracked().unwrap_or(0);
                let store = state.annotation_store.get_untracked();
                let files = state.files.get_untracked();
                let file_max_freq = files.get(file_idx).map(|f| f.spectrogram.max_freq).unwrap_or(96_000.0);
                let min_freq_val = state.min_display_freq.get_untracked().unwrap_or(0.0);
                let max_freq_val = state.max_display_freq.get_untracked().unwrap_or(file_max_freq);
                let scroll = state.scroll_offset.get_untracked();
                let time_res = files.get(file_idx).map(|f| f.spectrogram.time_resolution).unwrap_or(1.0);
                let zoom = state.zoom_level.get_untracked();
                if let Some(Some(set)) = store.sets.get(file_idx) {
                    let ann_handle = hit_test_annotation_handles(
                        set, &selected_ids,
                        px_x, px_y,
                        min_freq_val, max_freq_val,
                        scroll, time_res, zoom, cw, ch,
                        crate::canvas::hit_test::ANNOTATION_HANDLE_HIT_RADIUS_TOUCH,
                    );
                    if let Some((ref ann_id, handle_pos)) = ann_handle {
                        // Check if locked
                        let locked = set.annotations.iter().find(|a| a.id == *ann_id)
                            .and_then(|a| match &a.kind {
                                crate::annotations::AnnotationKind::Region(r) => Some(r.is_locked()),
                                _ => None,
                            })
                            .unwrap_or(false);
                        if !locked {
                            // Snapshot for undo
                            let snapshot = store.sets.get(file_idx).and_then(|s| s.clone());
                            state.undo_stack.update(|stack| {
                                stack.push_undo(UndoEntry { file_idx, snapshot });
                            });
                            // Store original bounds
                            if let Some(a) = set.annotations.iter().find(|a| a.id == *ann_id) {
                                if let crate::annotations::AnnotationKind::Region(ref r) = a.kind {
                                    state.annotation_drag_original.set(Some((r.time_start, r.time_end, r.freq_low, r.freq_high)));
                                }
                            }
                            state.annotation_drag_handle.set(Some((ann_id.clone(), handle_pos)));
                            state.is_dragging.set(true);
                            ev.prevent_default();
                            return;
                        }
                    }
                }
            }
        }
    }

    // Check for ambiguous corner drag (bottom-left: both axis zones overlap)
    if let Some((px_x, px_y, t, freq)) = pointer_to_xtf(touch.client_x() as f64, touch.client_y() as f64, canvas_ref, &state) {
        if let Some(canvas_el) = canvas_ref.get() {
            let canvas: &HtmlCanvasElement = canvas_el.as_ref();
            let ch = canvas.get_bounding_client_rect().height();
            let in_left_axis = px_x < LABEL_AREA_WIDTH;
            let in_bottom_axis = px_y > ch - 16.0;

            if in_left_axis && in_bottom_axis {
                let ff_lo = state.ff_freq_lo.get_untracked();
                let ff_hi = state.ff_freq_hi.get_untracked();
                ix.corner_drag_saved_ff.set((ff_lo, ff_hi));
                ix.corner_drag_saved_selection.set(state.selection.get_untracked());
                ix.axis_drag_raw_start.set(freq);
                ix.time_axis_drag_raw_start.set(t);
                ix.corner_drag_active.set(true);
                ix.corner_drag_start_client.set((touch.client_x() as f64, touch.client_y() as f64));
                ix.corner_drag_axis.set(None);
                state.is_dragging.set(true);
                ev.prevent_default();
                return;
            }
        }
    }

    // Check for axis drag
    if let Some((px_x, _, _, freq)) = pointer_to_xtf(touch.client_x() as f64, touch.client_y() as f64, canvas_ref, &state) {
        if px_x < LABEL_AREA_WIDTH {
            // Single tap with active HFR: toggle it off
            let ff_lo = state.ff_freq_lo.get_untracked();
            let ff_hi = state.ff_freq_hi.get_untracked();
            if ff_hi > ff_lo && state.focus_stack.get_untracked().hfr_enabled() {
                state.toggle_hfr();
                ev.prevent_default();
                return;
            }
            let snap = 5_000.0;
            let snapped = (freq / snap).round() * snap;
            ix.axis_drag_raw_start.set(freq);
            state.axis_drag_start_freq.set(Some(snapped));
            state.axis_drag_current_freq.set(Some(snapped));
            state.is_dragging.set(true);
            ev.prevent_default();
            return;
        }
    }

    // Check for time-axis drag (bottom axis time segment selection)
    if let Some((px_x, px_y, t, _)) = pointer_to_xtf(touch.client_x() as f64, touch.client_y() as f64, canvas_ref, &state) {
        if let Some(canvas_el) = canvas_ref.get() {
            let canvas: &HtmlCanvasElement = canvas_el.as_ref();
            let ch = canvas.get_bounding_client_rect().height();
            if px_y > ch - 16.0 && px_x > LABEL_AREA_WIDTH {
                ix.time_axis_dragging.set(true);
                ix.time_axis_drag_raw_start.set(t);
                state.selection.set(Some(Selection {
                    time_start: t,
                    time_end: t,
                    freq_low: None,
                    freq_high: None,
                }));
                state.is_dragging.set(true);
                ev.prevent_default();
                return;
            }
        }
    }

    match state.canvas_tool.get_untracked() {
        CanvasTool::Hand => {
            ev.prevent_default();
            state.is_dragging.set(true);
            ix.hand_drag_start.set((touch.client_x() as f64, state.scroll_offset.get_untracked()));
        }
        CanvasTool::Selection => {
            ev.prevent_default();
        }
    }
}

pub fn on_touchmove(
    ev: web_sys::TouchEvent,
    ix: SpectInteraction,
    canvas_ref: &NodeRef<leptos::html::Canvas>,
    state: AppState,
) {
    let touches = ev.touches();
    let n = touches.length();

    // Two-finger pinch/pan
    if n == 2 {
        if let Some(ps) = ix.pinch_state.get_untracked() {
            ev.prevent_default();
            use crate::components::pinch::{two_finger_geometry, apply_pinch};
            if let Some((mid_x, dist)) = two_finger_geometry(&touches) {
                let Some(canvas_el) = canvas_ref.get() else { return };
                let canvas: &HtmlCanvasElement = canvas_el.as_ref();
                let rect = canvas.get_bounding_client_rect();
                let cw = canvas.width() as f64;
                let (new_zoom, new_scroll) = apply_pinch(&ps, dist, mid_x, rect.left(), cw);
                state.suspend_follow();
                state.zoom_level.set(new_zoom);
                state.scroll_offset.set(new_scroll);
            }
        }
        return;
    }

    if n != 1 { return; }
    let touch = touches.get(0).unwrap();

    if !state.is_dragging.get_untracked() { return; }
    ev.prevent_default();

    // Spec handle drag takes priority
    if let Some(handle) = state.spec_drag_handle.get_untracked() {
        if let Some((_, px_y, _, _)) = pointer_to_xtf(touch.client_x() as f64, touch.client_y() as f64, canvas_ref, &state) {
            if let Some((freq_at_touch, file_max_freq)) = resolve_freq_at_pointer(px_y, canvas_ref, state) {
                apply_handle_drag(state, handle, freq_at_touch, file_max_freq);
            }
        }
        return;
    }

    // Annotation resize handle drag
    if let Some((ref ann_id, handle_pos)) = state.annotation_drag_handle.get_untracked() {
        if let Some((_, _, t, f)) = pointer_to_xtf(touch.client_x() as f64, touch.client_y() as f64, canvas_ref, &state) {
            apply_annotation_resize(state, ann_id.clone(), handle_pos, t, f);
        }
        return;
    }

    // Corner drag: determine axis from drag direction, allow switching
    if ix.corner_drag_active.get_untracked() {
        if let Some((_, _, t, f)) = pointer_to_xtf(touch.client_x() as f64, touch.client_y() as f64, canvas_ref, &state) {
            let (sx, sy) = ix.corner_drag_start_client.get_untracked();
            let dx = (touch.client_x() as f64 - sx).abs();
            let dy = (touch.client_y() as f64 - sy).abs();
            if dx < 4.0 && dy < 4.0 {
                return;
            }
            let want_y_axis = dy >= dx;
            let prev_axis = ix.corner_drag_axis.get_untracked();
            let axis_changed = prev_axis != Some(want_y_axis);
            if axis_changed {
                if want_y_axis {
                    let saved_sel = ix.corner_drag_saved_selection.get_untracked();
                    state.selection.set(saved_sel);
                    ix.time_axis_dragging.set(false);
                    let raw_start = ix.axis_drag_raw_start.get_untracked();
                    let snap = 5_000.0;
                    let snapped = (raw_start / snap).round() * snap;
                    state.axis_drag_start_freq.set(Some(snapped));
                    state.axis_drag_current_freq.set(Some(snapped));
                } else {
                    let (saved_lo, saved_hi) = ix.corner_drag_saved_ff.get_untracked();
                    if saved_hi > saved_lo {
                        state.set_ff_range(saved_lo, saved_hi);
                    }
                    state.axis_drag_start_freq.set(None);
                    state.axis_drag_current_freq.set(None);
                    ix.time_axis_dragging.set(true);
                }
                ix.corner_drag_axis.set(Some(want_y_axis));
            }
            if want_y_axis {
                let raw_start = ix.axis_drag_raw_start.get_untracked();
                let snap = 5_000.0;
                apply_axis_drag(state, raw_start, f, snap);
            } else {
                let t0 = ix.time_axis_drag_raw_start.get_untracked();
                state.selection.set(Some(Selection {
                    time_start: t0.min(t),
                    time_end: t0.max(t),
                    freq_low: None,
                    freq_high: None,
                }));
            }
        }
        return;
    }

    // Axis drag takes second priority
    if state.axis_drag_start_freq.get_untracked().is_some() {
        if let Some((_, _, _, f)) = pointer_to_xtf(touch.client_x() as f64, touch.client_y() as f64, canvas_ref, &state) {
            let raw_start = ix.axis_drag_raw_start.get_untracked();
            let snap = 5_000.0;
            apply_axis_drag(state, raw_start, f, snap);
        }
        return;
    }

    // Time-axis drag takes third priority
    if ix.time_axis_dragging.get_untracked() {
        if let Some((_, _, t, _)) = pointer_to_xtf(touch.client_x() as f64, touch.client_y() as f64, canvas_ref, &state) {
            let t0 = ix.time_axis_drag_raw_start.get_untracked();
            state.selection.set(Some(Selection {
                time_start: t0.min(t),
                time_end: t0.max(t),
                freq_low: None,
                freq_high: None,
            }));
        }
        return;
    }

    match state.canvas_tool.get_untracked() {
        CanvasTool::Hand => {
            apply_hand_pan(state, touch.client_x() as f64, canvas_ref, ix.hand_drag_start.get_untracked());
            // Record velocity sample for inertia
            let now = web_sys::window().unwrap().performance().unwrap().now();
            ix.velocity_tracker.update_value(|t| t.push(now, touch.client_x() as f64));
        }
        CanvasTool::Selection => {}
    }
}

pub fn on_touchend(
    ev: web_sys::TouchEvent,
    ix: SpectInteraction,
    canvas_ref: &NodeRef<leptos::html::Canvas>,
    state: AppState,
) {
    let remaining = ev.touches().length();

    if remaining < 2 {
        ix.pinch_state.set(None);
    }

    // One finger remains after pinch — re-anchor pan to avoid jump
    if remaining == 1 {
        if let Some(touch) = ev.touches().get(0) {
            ix.hand_drag_start.set((touch.client_x() as f64, state.scroll_offset.get_untracked()));
            if state.canvas_tool.get_untracked() == CanvasTool::Hand {
                state.is_dragging.set(true);
            }
        }
        return;
    }

    if remaining == 0 {
        if state.spec_drag_handle.get_untracked().is_some() {
            state.spec_drag_handle.set(None);
            state.is_dragging.set(false);
            return;
        }
        // End annotation resize handle drag
        if let Some((ref ann_id, _)) = state.annotation_drag_handle.get_untracked() {
            let file_idx = state.current_file_index.get_untracked().unwrap_or(0);
            let now = js_sys::Date::new_0().to_iso_string().as_string().unwrap_or_default();
            state.annotation_store.update(|store| {
                if let Some(Some(set)) = store.sets.get_mut(file_idx) {
                    if let Some(a) = set.annotations.iter_mut().find(|a| a.id == *ann_id) {
                        a.modified_at = now;
                    }
                }
            });
            state.annotations_dirty.set(true);
            state.annotation_drag_handle.set(None);
            state.annotation_drag_original.set(None);
            state.is_dragging.set(false);
            return;
        }
        // End corner drag — clear corner state, then fall through
        let was_corner = ix.corner_drag_active.get_untracked();
        if was_corner {
            ix.corner_drag_active.set(false);
            let committed = ix.corner_drag_axis.get_untracked();
            ix.corner_drag_axis.set(None);
            if committed.is_none() {
                state.is_dragging.set(false);
                state.axis_drag_start_freq.set(None);
                state.axis_drag_current_freq.set(None);
                ix.time_axis_dragging.set(false);
                return;
            }
        }
        // Finalize axis drag
        if state.axis_drag_start_freq.get_untracked().is_some() {
            finalize_axis_drag(state);
            return;
        }
        // Finalize time-axis drag
        if ix.time_axis_dragging.get_untracked() {
            ix.time_axis_dragging.set(false);
            state.is_dragging.set(false);
            if let Some(sel) = state.selection.get_untracked() {
                if sel.time_end - sel.time_start < 0.0001 {
                    state.selection.set(None);
                } else if sel.freq_low.is_none() {
                    // Auto-combine: if FF range is active, upgrade segment to region
                    let ff = state.focus_stack.get_untracked().effective_range_ignoring_hfr();
                    if ff.is_active() {
                        state.selection.set(Some(Selection {
                            freq_low: Some(ff.lo),
                            freq_high: Some(ff.hi),
                            ..sel
                        }));
                    }
                }
            }
            return;
        }
        state.is_dragging.set(false);

        // Hand tool: bookmark on tap (no significant drag) while playing, or launch inertia
        if state.canvas_tool.get_untracked() == CanvasTool::Hand {
            if let Some(touch) = ev.changed_touches().get(0) {
                let (start_x, _) = ix.hand_drag_start.get_untracked();
                let dx = (touch.client_x() as f64 - start_x).abs();
                if dx < 5.0 && state.is_playing.get_untracked() {
                    let t = state.playhead_time.get_untracked();
                    state.bookmarks.update(|bm| bm.push(crate::state::Bookmark { time: t }));
                } else if dx >= 5.0 {
                    // Flick → launch inertia
                    let velocity = ix.velocity_tracker.with_value(|t| t.velocity_px_per_sec());
                    if let Some(canvas_el) = canvas_ref.get() {
                        let canvas: &HtmlCanvasElement = canvas_el.as_ref();
                        let cw = canvas.width() as f64;
                        let files = state.files.get_untracked();
                        let idx = state.current_file_index.get_untracked();
                        let file = idx.and_then(|i| files.get(i));
                        let timeline = state.active_timeline.get_untracked();
                        let time_res = if let Some(ref tl) = timeline {
                            tl.segments.first().and_then(|s| files.get(s.file_index))
                                .map(|f| f.spectrogram.time_resolution).unwrap_or(1.0)
                        } else {
                            file.as_ref().map(|f| f.spectrogram.time_resolution).unwrap_or(1.0)
                        };
                        let duration = if let Some(ref tl) = timeline {
                            tl.total_duration_secs
                        } else {
                            file.as_ref().map(|f| f.audio.duration_secs).unwrap_or(f64::MAX)
                        };
                        let from_here_mode = state.play_start_mode.get_untracked() == PlayStartMode::FromHere;
                        crate::components::inertia::start_inertia(
                            state, velocity, cw, time_res, duration, from_here_mode, ix.inertia_generation,
                        );
                    }
                }
            }
        }

        // Update frequency focus from selection (if auto-focus enabled)
        if state.canvas_tool.get_untracked() == CanvasTool::Selection && state.annotation_auto_focus.get_untracked() {
            if let Some(sel) = state.selection.get_untracked() {
                if let (Some(lo), Some(hi)) = (sel.freq_low, sel.freq_high) {
                    if hi - lo > 100.0 {
                        state.set_ff_range(lo, hi);
                    }
                }
            }
        }

        // Double-tap detection: if two taps within 400ms in label area → toggle HFR
        if let Some(touch) = ev.changed_touches().get(0) {
            if let Some((px_x, _, _, _)) = pointer_to_xtf(touch.client_x() as f64, touch.client_y() as f64, canvas_ref, &state) {
                let now = js_sys::Date::now();
                let prev_time = ix.last_tap_time.get_untracked();
                let prev_x = ix.last_tap_x.get_untracked();
                ix.last_tap_time.set(now);
                ix.last_tap_x.set(px_x);
                let in_label = px_x < LABEL_AREA_WIDTH;
                let prev_in_label = prev_x < LABEL_AREA_WIDTH;
                if now - prev_time < 400.0 && in_label && prev_in_label {
                    let has_range = state.ff_freq_hi.get_untracked() > state.ff_freq_lo.get_untracked();
                    if has_range {
                        state.toggle_hfr();
                    }
                }
            }
        }
    }
}

pub fn on_wheel(
    ev: web_sys::WheelEvent,
    state: AppState,
) {
    ev.prevent_default();
    if ev.shift_key() {
        // Shift+scroll: vertical freq zoom around mouse position
        let files = state.files.get_untracked();
        let idx = state.current_file_index.get_untracked();
        let file_max_freq = idx
            .and_then(|i| files.get(i))
            .map(|f| f.spectrogram.max_freq)
            .unwrap_or(96_000.0);
        let cur_max = state.max_display_freq.get_untracked().unwrap_or(file_max_freq);
        let cur_min = state.min_display_freq.get_untracked().unwrap_or(0.0);
        let range = cur_max - cur_min;
        if range < 1.0 { return; }

        let anchor_frac = if let Some(mf) = state.mouse_freq.get_untracked() {
            ((mf - cur_min) / range).clamp(0.0, 1.0)
        } else {
            0.5
        };

        let factor = if ev.delta_y() > 0.0 { 1.15 } else { 1.0 / 1.15 };
        let new_range = (range * factor).clamp(500.0, file_max_freq);
        let anchor_freq = cur_min + anchor_frac * range;
        let new_min = (anchor_freq - anchor_frac * new_range).max(0.0);
        let new_max = (new_min + new_range).min(file_max_freq);
        let new_min = (new_max - new_range).max(0.0);

        state.min_display_freq.set(Some(new_min));
        state.max_display_freq.set(Some(new_max));
    } else if ev.ctrl_key() {
        let delta = if ev.delta_y() > 0.0 { 0.9 } else { 1.1 };
        state.zoom_level.update(|z| {
            *z = (*z * delta).max(0.1).min(400.0);
        });
    } else {
        let raw_delta = ev.delta_y() + ev.delta_x();
        let files = state.files.get_untracked();
        let timeline = state.active_timeline.get_untracked();
        let (time_res, duration) = if let Some(ref tl) = timeline {
            let tr = tl.segments.first().and_then(|s| files.get(s.file_index))
                .map(|f| f.spectrogram.time_resolution).unwrap_or(1.0);
            (tr, tl.total_duration_secs)
        } else {
            let idx = state.current_file_index.get_untracked().unwrap_or(0);
            match files.get(idx) {
                Some(file) => (file.spectrogram.time_resolution, file.audio.duration_secs),
                None => return,
            }
        };
        {
            let zoom = state.zoom_level.get_untracked();
            let canvas_w = state.spectrogram_canvas_width.get_untracked();
            let visible_time = viewport::visible_time(canvas_w, zoom, time_res);
            let from_here_mode = state.play_start_mode.get_untracked() == PlayStartMode::FromHere;
            // Scroll proportional to visible time (like arrow keys),
            // normalized so a typical wheel tick (~100px) scrolls ~10% of the view
            let delta = raw_delta.signum() * visible_time * 0.1 * (raw_delta.abs() / 100.0).min(3.0);
            state.suspend_follow();
            state.scroll_offset.update(|s| {
                *s = viewport::clamp_scroll_for_mode(*s + delta, duration, visible_time, from_here_mode);
            });
        }
    }
}
