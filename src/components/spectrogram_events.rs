use leptos::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{HtmlCanvasElement, MouseEvent, PointerEvent};
use crate::canvas::coord::pointer_to_xtf;
use crate::canvas::hit_test::{hit_test_spec_handles, is_in_band_ff_drag_zone, hit_test_annotation_handles, hit_test_annotation_body, hit_test_band_ff_body};
use crate::canvas::spectrogram_renderer;
use crate::state::{ActiveFocus, AppState, CanvasTool, SpectrogramHandle, Selection, UndoEntry};
use crate::viewport;

pub const LABEL_AREA_WIDTH: f64 = 60.0;

/// Frequency-dependent snap increment for y-axis selection.
/// Below 20 kHz: 1 kHz (shift: 2 kHz). At/above 20 kHz: 5 kHz (shift: 10 kHz).
pub fn freq_snap(freq: f64, shift: bool) -> f64 {
    if freq < 20_000.0 {
        if shift { 2_000.0 } else { 1_000.0 }
    } else {
        if shift { 10_000.0 } else { 5_000.0 }
    }
}

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
    /// Double-tap detection: y-position of last tap (needed for x-axis zone check)
    pub last_tap_y: RwSignal<f64>,
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
    /// Saved BandFF range before corner drag, for restoration when switching to X-axis
    pub corner_drag_saved_ff: RwSignal<(f64, f64)>,
    /// Saved selection before corner drag, for restoration when switching to Y-axis
    pub corner_drag_saved_selection: RwSignal<Option<Selection>>,
    /// Pending annotation hit for Hand tool — deferred to mouseup so panning takes priority
    pub pending_annotation_hit: RwSignal<Option<(String, bool)>>, // (annotation_id, ctrl_held)
    /// Pending time-axis drag: defers actual drag until pointer moves >3px, allowing tap-to-clear.
    /// Stores (client_x, time, shift_held, anchor_time) when set.
    pub time_axis_pending: RwSignal<Option<(f64, f64, bool, f64)>>,
    /// Pending BandFF body click — deferred to mouseup so panning takes priority
    pub pending_band_ff_hit: RwSignal<bool>,
    /// Pending transient selection body click — deferred to mouseup so panning takes priority
    pub pending_selection_hit: RwSignal<bool>,
    /// Left-axis viewport pan state — None when no pan is active, otherwise
    /// (client_x, client_y, anchor_freq, start_min, start_max). The anchor
    /// is the frequency under the pointer at pointerdown; pan keeps it
    /// pinned under the pointer as it moves, so dragging the axis drags
    /// the visible frequency range with it. The stored client-x/y is used
    /// to decide whether pointerup was a tap (short distance = reset view
    /// to full range) or a real drag.
    pub freq_pan_start: RwSignal<Option<(f64, f64, f64, f64, f64)>>,
}

impl Default for SpectInteraction {
    fn default() -> Self {
        Self::new()
    }
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
            last_tap_y: RwSignal::new(0.0f64),
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
            time_axis_pending: RwSignal::new(None),
            pending_band_ff_hit: RwSignal::new(false),
            pending_selection_hit: RwSignal::new(false),
            freq_pan_start: RwSignal::new(None),
        }
    }
}

/// Read the file's Nyquist / max frequency — the ceiling for the freq
/// display range.
fn file_nyquist(state: AppState) -> f64 {
    let is_mic_active = state.mic_recording.get_untracked() || state.mic_listening.get_untracked();
    if is_mic_active && crate::canvas::live_waterfall::is_active() {
        crate::canvas::live_waterfall::max_freq()
    } else {
        let files = state.files.get_untracked();
        let idx = state.current_file_index.get_untracked();
        idx.and_then(|i| files.get(i))
            .map(|f| f.spectrogram.max_freq)
            .unwrap_or(96_000.0)
    }
}

/// Pan the frequency display range so that `anchor_freq` stays pinned to
/// the current pointer y. The span (start_max - start_min) is preserved,
/// and the new range is clamped to [0, nyquist]. Called on every
/// pointermove / touchmove during a left-axis viewport pan.
pub fn apply_freq_axis_pan(
    state: AppState,
    canvas_y: f64,
    canvas_h: f64,
    anchor_freq: f64,
    start_min: f64,
    start_max: f64,
) {
    if canvas_h <= 0.0 { return; }
    let span = (start_max - start_min).max(1.0);
    let nyquist = file_nyquist(state);
    // freq_to_y: y = h * (1 - (f - min) / span). Solve for min to pin
    // anchor_freq at canvas_y.
    let new_min = anchor_freq - span * (1.0 - (canvas_y / canvas_h));
    // Clamp so neither edge escapes the file's frequency range.
    let max_low = (nyquist - span).max(0.0);
    let clamped_min = new_min.clamp(0.0, max_low);
    let clamped_max = (clamped_min + span).min(nyquist);
    state.min_display_freq.set(Some(clamped_min));
    state.max_display_freq.set(Some(clamped_max));
}

/// Reset the frequency display range to "auto" (None) so the spectrogram
/// shows 0..Nyquist again. Used for tap-to-reset and double-click on the
/// left axis.
pub fn reset_freq_axis_view(state: AppState) {
    state.min_display_freq.set(None);
    state.max_display_freq.set(None);
}

/// Apply a frequency handle drag (BandFF or HET). Shared by mouse and touch handlers.
pub fn apply_handle_drag(
    state: AppState,
    handle: SpectrogramHandle,
    freq_at_pointer: f64,
    file_max_freq: f64,
) {
    match handle {
        SpectrogramHandle::BandFfUpper => {
            let lo = state.band_ff_freq_lo.get_untracked();
            let clamped = freq_at_pointer.clamp(lo + 500.0, file_max_freq);
            state.set_band_ff_hi(clamped);
        }
        SpectrogramHandle::BandFfLower => {
            let hi = state.band_ff_freq_hi.get_untracked();
            let clamped = freq_at_pointer.clamp(0.0, hi - 500.0);
            state.set_band_ff_lo(clamped);
        }
        SpectrogramHandle::BandFfMiddle => {
            let lo = state.band_ff_freq_lo.get_untracked();
            let hi = state.band_ff_freq_hi.get_untracked();
            let bw = hi - lo;
            let mid = (lo + hi) / 2.0;
            let delta = freq_at_pointer - mid;
            let new_lo = (lo + delta).clamp(0.0, file_max_freq - bw);
            let new_hi = new_lo + bw;
            state.set_band_ff_range(new_lo, new_hi);
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
    shift: bool,
) {
    // Clamp to non-negative frequencies
    let raw_start = raw_start.max(0.0);
    let freq = freq.max(0.0);
    // Snap each endpoint independently based on its own frequency zone
    let snap_start = freq_snap(raw_start, shift);
    let snap_end = freq_snap(freq, shift);
    let (snapped_start, snapped_end) = if freq > raw_start {
        // Dragging up: start floors down, end ceils up
        ((raw_start / snap_start).floor() * snap_start, (freq / snap_end).ceil() * snap_end)
    } else if freq < raw_start {
        // Dragging down: start ceils up, end floors down
        ((raw_start / snap_start).ceil() * snap_start, (freq / snap_end).floor() * snap_end)
    } else {
        let s = (raw_start / snap_start).round() * snap_start;
        (s, s)
    };
    // Ensure snapped values don't go below 0
    let snapped_start = snapped_start.max(0.0);
    let snapped_end = snapped_end.max(0.0);
    state.axis_drag_start_freq.set(Some(snapped_start));
    state.axis_drag_current_freq.set(Some(snapped_end));
    // Live update BandFF range
    let lo = snapped_start.min(snapped_end);
    let hi = snapped_start.max(snapped_end);
    if hi - lo > 500.0 {
        state.set_band_ff_range(lo, hi);
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
    let is_mic_active = state.mic_recording.get_untracked() || state.mic_listening.get_untracked();
    let wf_active = is_mic_active && crate::canvas::live_waterfall::is_active();
    let file_max_freq = if wf_active {
        crate::canvas::live_waterfall::max_freq()
    } else {
        let files = state.files.get_untracked();
        let idx = state.current_file_index.get_untracked();
        idx.and_then(|i| files.get(i))
            .map(|f| f.spectrogram.max_freq)
            .unwrap_or(96_000.0)
    };
    let min_freq_val = state.min_display_freq.get_untracked().unwrap_or(0.0);
    let max_freq_val = state.max_display_freq.get_untracked().unwrap_or(file_max_freq);
    let freq = spectrogram_renderer::y_to_freq(px_y, min_freq_val, max_freq_val, ch);
    Some((freq, file_max_freq))
}

/// Select all frequencies: set BandFF range to 0..Nyquist and enable HFR.
/// Used by double-click / double-tap on the y-axis.
pub fn select_all_frequencies(state: AppState) {
    let is_mic_active = state.mic_recording.get_untracked() || state.mic_listening.get_untracked();
    let file_max_freq = if is_mic_active && crate::canvas::live_waterfall::is_active() {
        crate::canvas::live_waterfall::max_freq()
    } else {
        let files = state.files.get_untracked();
        let idx = state.current_file_index.get_untracked();
        idx.and_then(|i| files.get(i))
            .map(|f| f.spectrogram.max_freq)
            .unwrap_or(96_000.0)
    };
    state.set_band_ff_range(0.0, file_max_freq);
    let stack = state.focus_stack.get_untracked();
    if !stack.hfr_enabled() {
        state.toggle_hfr();
    }
}

/// Select all time: create a full-duration selection on the x-axis.
/// If HFR is active, the selection includes frequency bounds (region);
/// otherwise it's time-only (segment).
/// Used by double-click / double-tap on the x-axis.
pub fn select_all_time(state: AppState) {
    let duration = if let Some(ref tl) = state.active_timeline.get_untracked() {
        tl.total_duration_secs
    } else {
        let files = state.files.get_untracked();
        let idx = state.current_file_index.get_untracked();
        idx.and_then(|i| files.get(i))
            .map(|f| f.audio.duration_secs)
            .unwrap_or(0.0)
    };
    if duration <= 0.0 { return; }

    let ff = state.focus_stack.get_untracked().effective_range();
    let (fl, fh) = if ff.is_active() { (Some(ff.lo), Some(ff.hi)) } else { (None, None) };

    state.selection.set(Some(Selection {
        time_start: 0.0,
        time_end: duration,
        freq_low: fl,
        freq_high: fh,
    }));
    state.active_focus.set(Some(ActiveFocus::TransientSelection));
}

/// Finalize axis drag — auto-enable HFR if a meaningful range was selected,
/// or toggle HFR off if it was just a tap (no meaningful drag).
pub fn finalize_axis_drag(state: AppState) {
    let start = state.axis_drag_start_freq.get_untracked();
    let current = state.axis_drag_current_freq.get_untracked();
    let was_tap = match (start, current) {
        (Some(s), Some(c)) => (s - c).abs() < 1.0, // start == end means no drag movement
        _ => true,
    };

    if was_tap {
        // Tap on y-axis: toggle HFR off (if on)
        let stack = state.focus_stack.get_untracked();
        if stack.hfr_enabled() {
            state.toggle_hfr();
        }
    } else {
        // Drag completed: enable HFR if a meaningful range was selected
        let stack = state.focus_stack.get_untracked();
        let range = stack.effective_range_ignoring_hfr();
        if range.hi - range.lo > 500.0 && !stack.hfr_enabled() {
            state.toggle_hfr();
        }
    }
    // Auto-combine: if there's a time-only segment, upgrade to region — only when HFR is on
    if let Some(sel) = state.selection.get_untracked() {
        if sel.freq_low.is_none() && sel.freq_high.is_none() && sel.time_end - sel.time_start > 0.0001 {
            let ff = state.focus_stack.get_untracked().effective_range();
            if ff.is_active() {
                state.selection.set(Some(Selection {
                    freq_low: Some(ff.lo),
                    freq_high: Some(ff.hi),
                    ..sel
                }));
            }
        }
    }
    // Set focus to BandFF when axis drag creates/modifies BandFF
    if !was_tap {
        state.active_focus.set(Some(ActiveFocus::FrequencyFocus));
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
    let waterfall_active = (state.mic_recording.get_untracked()
        || state.mic_listening.get_untracked())
        && crate::canvas::live_waterfall::is_active();
    let time_res = if waterfall_active {
        crate::canvas::live_waterfall::time_resolution()
    } else if let Some(ref tl) = timeline {
        tl.segments.first().and_then(|s| files.get(s.file_index))
            .map(|f| f.spectrogram.time_resolution).unwrap_or(1.0)
    } else {
        file.as_ref().map(|f| f.spectrogram.time_resolution).unwrap_or(1.0)
    };
    let zoom = state.zoom_level.get_untracked();
    let visible_time = viewport::visible_time(cw, zoom, time_res);
    let dt = -(dx / cw) * visible_time;
    state.suspend_follow();
    // During live listen/record, push the waterfall snap-back 2s into the
    // future so a release between gestures doesn't yank the view to "now".
    state.suspend_waterfall_follow(2000.0);

    let new_scroll = if waterfall_active {
        // Bound panning to what the circular buffer actually holds: can't go
        // earlier than the oldest retained column, can't go past the live edge.
        let total_time = crate::canvas::live_waterfall::total_time();
        let oldest = crate::canvas::live_waterfall::oldest_time();
        let max_scroll = (total_time - visible_time).max(oldest);
        (start_scroll + dt).clamp(oldest, max_scroll)
    } else {
        let duration = if let Some(ref tl) = timeline {
            tl.total_duration_secs
        } else {
            file.as_ref().map(|f| f.audio.duration_secs).unwrap_or(f64::MAX)
        };
        let from_here_mode = state.play_start_mode.get_untracked().uses_from_here();
        viewport::clamp_scroll_for_mode(start_scroll + dt, duration, visible_time, from_here_mode)
    };
    state.scroll_offset.set(new_scroll);
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

// ── Pointer capture helper ────────────────────────────────────────────────

/// Call setPointerCapture on the event target so that pointermove/pointerup
/// continue to fire even when the cursor leaves the canvas (e.g. into the
/// toolbar, sidebar, or off-window).
fn capture_pointer(ev: &PointerEvent) {
    if let Some(target) = ev.target() {
        if let Ok(el) = target.dyn_into::<web_sys::Element>() {
            let _ = el.set_pointer_capture(ev.pointer_id());
        }
    }
}

// ── Pointer event handlers ───────────────────────────────────────────────────

pub fn on_pointerdown(
    ev: PointerEvent,
    ix: SpectInteraction,
    canvas_ref: &NodeRef<leptos::html::Canvas>,
    state: AppState,
) {
    if ev.button() != 0 { return; }
    // When viewport is pinch-zoomed, let the browser handle all gestures so
    // the user can zoom back out via native pinch.
    if state.viewport_zoomed.get_untracked() { return; }

    state.pointer_is_down.set(true);

    // Check for annotation resize handle drag first (selected annotations take
    // priority over BandFF/HET handles when they overlap). Skipped when annotations are hidden.
    if state.annotations_visible.get_untracked() {
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
            capture_pointer(&ev);
            ev.prevent_default();
            return;
        }
    }
    }

    // Check for spec handle drag (BandFF or HET — takes priority over axis/tool drags)
    // BandFF handles only start drag when clicking within the center handle zone.
    if let Some(handle) = state.spec_hover_handle.get_untracked() {
        let is_ff = matches!(handle, SpectrogramHandle::BandFfUpper | SpectrogramHandle::BandFfLower | SpectrogramHandle::BandFfMiddle);
        let allow_drag = if is_ff {
            if let Some((px_x, _, _, _)) = pointer_to_xtf(ev.client_x() as f64, ev.client_y() as f64, canvas_ref, &state) {
                if let Some(canvas_el) = canvas_ref.get() {
                    let canvas: &HtmlCanvasElement = canvas_el.as_ref();
                    is_in_band_ff_drag_zone(px_x, canvas.width() as f64)
                } else { false }
            } else { false }
        } else {
            true // HET handles drag from anywhere
        };
        if allow_drag {
            state.spec_drag_handle.set(Some(handle));
            state.is_dragging.set(true);
            capture_pointer(&ev);
            ev.prevent_default();
            return;
        }
    }

    // Corner drag is gone: with the time axis moved out to the TimeGutter
    // below the canvas, the bottom-left corner is no longer an ambiguous
    // dual-axis zone — the left edge routes to freq-axis viewport pan,
    // and time selection happens in the gutter strip.

    // Left-axis viewport pan. Band selection lives on the right-side band
    // gutter now; the left axis is for navigating what you *look at*
    // (drag to pan the visible frequency range, tap to reset to full).
    if let Some((px_x, _, _, freq)) = pointer_to_xtf(ev.client_x() as f64, ev.client_y() as f64, canvas_ref, &state) {
        if px_x < LABEL_AREA_WIDTH && !state.display_transform.get_untracked() {
            let nyquist = file_nyquist(state);
            let start_min = state.min_display_freq.get_untracked().unwrap_or(0.0);
            let start_max = state.max_display_freq.get_untracked().unwrap_or(nyquist);
            ix.freq_pan_start.set(Some((
                ev.client_x() as f64, ev.client_y() as f64,
                freq, start_min, start_max,
            )));
            state.is_dragging.set(true);
            capture_pointer(&ev);
            ev.prevent_default();
            return;
        }
    }

    // Time-axis selection moved out to the <TimeGutter/> strip; the
    // bottom pixels of the spectrogram canvas no longer have special
    // gesture meaning.

    // Check for annotation body, transient selection body, and BandFF body clicks.
    // Priority: annotation > selection > BandFF. All deferred to pointer-up so panning takes priority.
    ix.pending_band_ff_hit.set(false);
    ix.pending_selection_hit.set(false);
    if state.canvas_tool.get_untracked() == CanvasTool::Hand {
        if let Some((px_x, px_y, t, freq)) = pointer_to_xtf(ev.client_x() as f64, ev.client_y() as f64, canvas_ref, &state) {
        let file_idx = state.current_file_index.get_untracked().unwrap_or(0);
        let files = state.files.get_untracked();
        let file = files.get(file_idx);
        let file_max_freq = file.map(|f| f.spectrogram.max_freq).unwrap_or(96_000.0);
        let min_freq = state.min_display_freq.get_untracked().unwrap_or(0.0);
        let max_freq = state.max_display_freq.get_untracked().unwrap_or(file_max_freq);

        // Check annotation body first (highest priority; skipped when annotations are hidden)
        let mut hit_annotation = false;
        if state.annotations_visible.get_untracked() {
            let store = state.annotation_store.get_untracked();
            if let Some(Some(set)) = store.sets.get(file_idx) {
                if let Some(canvas_el) = canvas_ref.get() {
                    let canvas: &HtmlCanvasElement = canvas_el.as_ref();
                    let cw = canvas.width() as f64;
                    let ch = canvas.height() as f64;
                    let scroll = state.scroll_offset.get_untracked();
                    let time_res = file.map(|f| f.spectrogram.time_resolution).unwrap_or(1.0);
                    let zoom = state.zoom_level.get_untracked();

                    if let Some(hit_id) = hit_test_annotation_body(
                        set, px_x, px_y, min_freq, max_freq, scroll, time_res, zoom, cw, ch,
                    ) {
                        let ctrl = ev.ctrl_key() || ev.meta_key();
                        ix.pending_annotation_hit.set(Some((hit_id, ctrl)));
                        hit_annotation = true;
                    }
                }
            }
        }

        if !hit_annotation {
            // Check transient selection body (priority over BandFF)
            if let Some(sel) = state.selection.get_untracked() {
                if point_in_selection(&sel, t, freq) {
                    ix.pending_selection_hit.set(true);
                } else {
                    // Check BandFF body click
                    if let Some(canvas_el) = canvas_ref.get() {
                        let canvas: &HtmlCanvasElement = canvas_el.as_ref();
                        let ch = canvas.height() as f64;
                        let band_ff_lo = state.band_ff_freq_lo.get_untracked();
                        let band_ff_hi = state.band_ff_freq_hi.get_untracked();
                        if hit_test_band_ff_body(px_y, band_ff_lo, band_ff_hi, min_freq, max_freq, ch) {
                            ix.pending_band_ff_hit.set(true);
                        }
                    }
                }
            } else {
                // No selection — check BandFF body click
                if let Some(canvas_el) = canvas_ref.get() {
                    let canvas: &HtmlCanvasElement = canvas_el.as_ref();
                    let ch = canvas.height() as f64;
                    let band_ff_lo = state.band_ff_freq_lo.get_untracked();
                    let band_ff_hi = state.band_ff_freq_hi.get_untracked();
                    if hit_test_band_ff_body(px_y, band_ff_lo, band_ff_hi, min_freq, max_freq, ch) {
                        ix.pending_band_ff_hit.set(true);
                    }
                }
            }
        }
    }
    }

    // Click on empty area deselects annotations and clears focus (unless modifier held)
    // For Hand tool: defer to mouseup so panning isn't blocked
    if state.canvas_tool.get_untracked() != CanvasTool::Hand
        && !ev.ctrl_key() && !ev.meta_key() && !ev.shift_key() {
            let ids = state.selected_annotation_ids.get_untracked();
            if !ids.is_empty() {
                state.selected_annotation_ids.set(Vec::new());
            }
            state.active_focus.set(None);
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

    // Capture pointer so drag continues even when cursor leaves the canvas
    if state.is_dragging.get_untracked() {
        capture_pointer(&ev);
    }
}

pub fn on_pointermove(
    ev: PointerEvent,
    ix: SpectInteraction,
    canvas_ref: &NodeRef<leptos::html::Canvas>,
    state: AppState,
) {
    if let Some((px_x, px_y, t, f)) = pointer_to_xtf(ev.client_x() as f64, ev.client_y() as f64, canvas_ref, &state) {
        // Always track hover position
        state.mouse_freq.set(Some(f));
        state.mouse_canvas_x.set(px_x);
        state.cursor_time.set(Some(t));

        // Canvas height for time-axis zone detection (reuse single rect query)
        let canvas_height = canvas_ref.get()
            .map(|el| {
                let canvas: &HtmlCanvasElement = el.as_ref();
                canvas.get_bounding_client_rect().height()
            });

        // Time-axis tooltip + hover tracking lives on the <TimeGutter/>
        // strip now — the spectrogram's bottom pixels are just spectrogram.
        ix.time_axis_tooltip.set(None);

        // Update label hover target and in-label-area state
        let in_label_area = px_x < LABEL_AREA_WIDTH;
        state.mouse_in_label_area.set(in_label_area);
        state.mouse_in_time_axis.set(false);
        let current_target = ix.label_hover_target.get_untracked();
        let new_target = if in_label_area { 1.0 } else { 0.0 };
        if current_target != new_target {
            ix.label_hover_target.set(new_target);
        }

        if state.is_dragging.get_untracked() {
            // Left-axis viewport pan takes priority over every other drag —
            // once the user grabs the axis, they're navigating the display
            // window, not selecting or panning the main canvas.
            if let Some((_cx, _cy, anchor_freq, start_min, start_max)) = ix.freq_pan_start.get_untracked() {
                if let Some(ch) = canvas_height {
                    apply_freq_axis_pan(state, px_y, ch, anchor_freq, start_min, start_max);
                }
                return;
            }

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
                        // Switching to Y-axis: restore saved selection, activate
                        // left-axis viewport pan (the freq-axis is a display
                        // control now, not a band-selection surface).
                        let saved_sel = ix.corner_drag_saved_selection.get_untracked();
                        state.selection.set(saved_sel);
                        ix.time_axis_dragging.set(false);
                        let anchor_freq = ix.axis_drag_raw_start.get_untracked();
                        let nyquist = file_nyquist(state);
                        let start_min = state.min_display_freq.get_untracked().unwrap_or(0.0);
                        let start_max = state.max_display_freq.get_untracked().unwrap_or(nyquist);
                        ix.freq_pan_start.set(Some((
                            ev.client_x() as f64, ev.client_y() as f64,
                            anchor_freq, start_min, start_max,
                        )));
                    } else {
                        // Switching to X-axis: restore saved BandFF range, activate time drag
                        let (saved_lo, saved_hi) = ix.corner_drag_saved_ff.get_untracked();
                        if saved_hi > saved_lo {
                            state.set_band_ff_range(saved_lo, saved_hi);
                        }
                        ix.freq_pan_start.set(None);
                        ix.time_axis_dragging.set(true);
                    }
                    ix.corner_drag_axis.set(Some(want_y_axis));
                }
                // Apply the committed axis's drag update
                if want_y_axis {
                    if let Some((_cx, _cy, anchor_freq, start_min, start_max)) = ix.freq_pan_start.get_untracked() {
                        if let Some(ch) = canvas_height {
                            apply_freq_axis_pan(state, px_y, ch, anchor_freq, start_min, start_max);
                        }
                    }
                } else {
                    let t0 = ix.time_axis_drag_raw_start.get_untracked();
                    let ff = state.focus_stack.get_untracked().effective_range();
                    let (fl, fh) = if ff.is_active() { (Some(ff.lo), Some(ff.hi)) } else { (None, None) };
                    state.selection.set(Some(Selection {
                        time_start: t0.min(t),
                        time_end: t0.max(t),
                        freq_low: fl,
                        freq_high: fh,
                    }));
                }
                return;
            }

            // Axis drag takes third priority
            if state.axis_drag_start_freq.get_untracked().is_some() {
                let raw_start = ix.axis_drag_raw_start.get_untracked();
                apply_axis_drag(state, raw_start, f, ev.shift_key());
                return;
            }

            // Pending time-axis: commit to drag once pointer moves >3px
            if let Some((start_cx, _start_t, _shift, anchor_t)) = ix.time_axis_pending.get_untracked() {
                let dx = (ev.client_x() as f64 - start_cx).abs();
                if dx > 3.0 {
                    // Commit to drag
                    ix.time_axis_pending.set(None);
                    ix.time_axis_dragging.set(true);
                    ix.time_axis_drag_raw_start.set(anchor_t);
                    let ff = state.focus_stack.get_untracked().effective_range();
                    let (fl, fh) = if ff.is_active() { (Some(ff.lo), Some(ff.hi)) } else { (None, None) };
                    state.selection.set(Some(Selection {
                        time_start: anchor_t.min(t),
                        time_end: anchor_t.max(t),
                        freq_low: fl,
                        freq_high: fh,
                    }));
                }
                return;
            }

            // Time-axis drag takes third priority
            if ix.time_axis_dragging.get_untracked() {
                let t0 = ix.time_axis_drag_raw_start.get_untracked();
                let ff = state.focus_stack.get_untracked().effective_range();
                let (fl, fh) = if ff.is_active() { (Some(ff.lo), Some(ff.hi)) } else { (None, None) };
                state.selection.set(Some(Selection {
                    time_start: t0.min(t),
                    time_end: t0.max(t),
                    freq_low: fl,
                    freq_high: fh,
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
            // Not dragging — do spec handle hover detection (BandFF + HET)
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
                        let band_ff_focused = state.active_focus.get_untracked() == Some(ActiveFocus::FrequencyFocus);
                        let handle = hit_test_spec_handles(
                            &state, px_y, min_freq_val, max_freq_val, ch, 8.0, band_ff_focused,
                        );
                        state.spec_hover_handle.set(handle);

                        // Annotation resize handle hover detection (only when annotations have focus and are visible)
                        let annotations_focused = state.active_focus.get_untracked() == Some(ActiveFocus::Annotations)
                            && state.annotations_visible.get_untracked();
                        let selected_ids = state.selected_annotation_ids.get_untracked();
                        if annotations_focused && !selected_ids.is_empty() {
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

pub fn on_pointerleave(
    _ev: PointerEvent,
    ix: SpectInteraction,
    state: AppState,
) {
    // When pointer is captured (during a drag), pointerleave won't normally fire.
    // But if it does somehow, preserve drag state so the gesture isn't interrupted.
    if state.is_dragging.get_untracked() {
        return;
    }

    state.pointer_is_down.set(false);
    state.mouse_freq.set(None);
    state.mouse_in_label_area.set(false);
    state.mouse_in_time_axis.set(false);
    state.cursor_time.set(None);
    ix.label_hover_target.set(0.0);
    state.spec_hover_handle.set(None);
    state.annotation_hover_handle.set(None);
    ix.time_axis_tooltip.set(None);
    ix.pending_annotation_hit.set(None);
}

pub fn on_pointerup(
    ev: PointerEvent,
    ix: SpectInteraction,
    canvas_ref: &NodeRef<leptos::html::Canvas>,
    state: AppState,
) {
    state.pointer_is_down.set(false);
    if !state.is_dragging.get_untracked() { return; }

    // End HET/BandFF handle drag
    if state.spec_drag_handle.get_untracked().is_some() {
        state.spec_drag_handle.set(None);
        state.is_dragging.set(false);
        return;
    }

    // End left-axis viewport pan. If the pointer barely moved, treat as
    // a tap and reset the display range to full (0..Nyquist). Otherwise
    // the pan has already been applied live, so just clear the state.
    if let Some((cx, cy, _anchor, _smin, _smax)) = ix.freq_pan_start.get_untracked() {
        let dx = (ev.client_x() as f64 - cx).abs();
        let dy = (ev.client_y() as f64 - cy).abs();
        let was_tap = dx < 3.0 && dy < 3.0;
        ix.freq_pan_start.set(None);
        state.is_dragging.set(false);
        if was_tap {
            reset_freq_axis_view(state);
        }
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

    // End axis drag (BandFF range already updated live during drag)
    if state.axis_drag_start_freq.get_untracked().is_some() {
        finalize_axis_drag(state);
        return;
    }

    // End pending time-axis tap (pointer didn't move enough — treat as tap-to-clear)
    if ix.time_axis_pending.get_untracked().is_some() {
        ix.time_axis_pending.set(None);
        state.is_dragging.set(false);
        // Tap on x-axis: clear any existing selection
        if state.selection.get_untracked().is_some() {
            state.selection.set(None);
        }
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
            } else {
                if sel.freq_low.is_none() {
                    // Auto-combine: only upgrade segment to region when HFR is on
                    let ff = state.focus_stack.get_untracked().effective_range();
                    if ff.is_active() {
                        state.selection.set(Some(Selection {
                            freq_low: Some(ff.lo),
                            freq_high: Some(ff.hi),
                            ..sel
                        }));
                    }
                }
                // Mutual exclusion: clear annotation selection when time selection is created
                state.selected_annotation_ids.set(Vec::new());
                // Focus the new transient selection
                state.active_focus.set(Some(ActiveFocus::TransientSelection));
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
                state.active_focus.set(Some(ActiveFocus::Annotations));
            } else if ix.pending_selection_hit.get_untracked() {
                // Deferred transient selection body click-to-refocus
                state.active_focus.set(Some(ActiveFocus::TransientSelection));
            } else if ix.pending_band_ff_hit.get_untracked() {
                // Deferred BandFF body click-to-select
                state.active_focus.set(Some(ActiveFocus::FrequencyFocus));
            } else {
                // Click on empty area deselects annotations and clears focus
                if !ev.ctrl_key() && !ev.meta_key() && !ev.shift_key() {
                    let ids = state.selected_annotation_ids.get_untracked();
                    if !ids.is_empty() {
                        state.selected_annotation_ids.set(Vec::new());
                    }
                    state.active_focus.set(None);
                }
            }
            // Bookmark while playing
            if state.is_playing.get_untracked() {
                let t = state.playhead_time.get_untracked();
                state.bookmarks.update(|bm| bm.push(crate::state::Bookmark { time: t }));
            }
        }

        ix.pending_annotation_hit.set(None);
        ix.pending_band_ff_hit.set(false);
        ix.pending_selection_hit.set(false);
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
            state.active_focus.set(Some(ActiveFocus::TransientSelection));
            if state.selection_auto_focus.get_untracked() {
                if let (Some(lo), Some(hi)) = (sel.freq_low, sel.freq_high) {
                    if hi - lo > 100.0 {
                        state.set_band_ff_range(lo, hi);
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
    // Double-click on y-axis: reset the frequency display range to
    // 0..Nyquist (same as a tap). The band gutter owns "select all
    // frequencies" (HFR) now; this axis is a viewport control.
    if let Some((px_x, px_y, _, _)) = pointer_to_xtf(ev.client_x() as f64, ev.client_y() as f64, canvas_ref, &state) {
        if px_x < LABEL_AREA_WIDTH && !state.display_transform.get_untracked() {
            reset_freq_axis_view(state);
            ev.prevent_default();
            return;
        }
        // Double-click time-axis-select-all now lives on the TimeGutter.
        let _ = px_y;
    }

    // Double-click inside a transient selection: promote it to an annotation and open label edit.
    if let Some(sel) = state.selection.get_untracked() {
        if let Some((_, _, t, freq)) = pointer_to_xtf(ev.client_x() as f64, ev.client_y() as f64, canvas_ref, &state) {
            if point_in_selection(&sel, t, freq) {
                crate::components::overflow_menu::annotate_selection(&state);
                ev.prevent_default();
                return;
            }
            // Outside selection: clear it (existing behavior)
            state.last_selection.set(Some(sel));
            state.selection.set(None);
            if state.active_focus.get_untracked() == Some(ActiveFocus::TransientSelection) {
                state.active_focus.set(None);
            }
            ev.prevent_default();
            return;
        }
    }

    // Double-click inside an annotation body: enter label edit for that annotation.
    if state.annotations_visible.get_untracked() {
        if let Some((px_x, px_y, _, _)) = pointer_to_xtf(ev.client_x() as f64, ev.client_y() as f64, canvas_ref, &state) {
            if let Some(canvas_el) = canvas_ref.get() {
                let canvas: &HtmlCanvasElement = canvas_el.as_ref();
                let cw = canvas.width() as f64;
                let ch = canvas.height() as f64;
                let file_idx = state.current_file_index.get_untracked().unwrap_or(0);
                let files = state.files.get_untracked();
                let file = files.get(file_idx);
                let file_max_freq = file.map(|f| f.spectrogram.max_freq).unwrap_or(96_000.0);
                let min_freq = state.min_display_freq.get_untracked().unwrap_or(0.0);
                let max_freq = state.max_display_freq.get_untracked().unwrap_or(file_max_freq);
                let scroll = state.scroll_offset.get_untracked();
                let time_res = file.map(|f| f.spectrogram.time_resolution).unwrap_or(1.0);
                let zoom = state.zoom_level.get_untracked();
                let store = state.annotation_store.get_untracked();
                if let Some(Some(set)) = store.sets.get(file_idx) {
                    if let Some(hit_id) = hit_test_annotation_body(
                        set, px_x, px_y, min_freq, max_freq, scroll, time_res, zoom, cw, ch,
                    ) {
                        let is_locked = set.annotations.iter()
                            .find(|a| a.id == hit_id)
                            .and_then(|a| match &a.kind {
                                crate::annotations::AnnotationKind::Region(r) => Some(r.is_locked()),
                                _ => None,
                            })
                            .unwrap_or(false);
                        state.selected_annotation_ids.set(vec![hit_id]);
                        state.active_focus.set(Some(ActiveFocus::Annotations));
                        if is_locked {
                            state.show_info_toast("Annotation is locked \u{2014} unlock to edit label by double-click");
                        } else {
                            state.annotation_is_new_edit.set(false);
                            state.annotation_editing.set(true);
                        }
                        ev.prevent_default();
                        return;
                    }
                }
            }
        }
    }

    // Double-click on BandFF handle toggles HFR (label area tap handled by finalize_axis_drag)
    let has_range = state.band_ff_freq_hi.get_untracked() > state.band_ff_freq_lo.get_untracked();
    if !has_range { return; }

    let on_handle = matches!(
        state.spec_hover_handle.get_untracked(),
        Some(SpectrogramHandle::BandFfUpper | SpectrogramHandle::BandFfLower | SpectrogramHandle::BandFfMiddle)
    );
    if on_handle {
        state.toggle_hfr();
        ev.prevent_default();
    }
}

/// Check whether a point (time, freq) falls inside a selection.
fn point_in_selection(sel: &Selection, t: f64, freq: f64) -> bool {
    if t < sel.time_start || t > sel.time_end {
        return false;
    }
    match (sel.freq_low, sel.freq_high) {
        (Some(lo), Some(hi)) => freq >= lo && freq <= hi,
        _ => true, // time-only segment: any freq is inside
    }
}

// ── Touch event handlers ───────────────────────────────────────────────────

pub fn on_touchstart(
    ev: web_sys::TouchEvent,
    ix: SpectInteraction,
    canvas_ref: &NodeRef<leptos::html::Canvas>,
    state: AppState,
) {
    // When viewport is pinch-zoomed, let the browser handle all touch gestures
    // so the user can zoom back out via native pinch.
    if state.viewport_zoomed.get_untracked() { return; }

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
                from_here_mode: state.play_start_mode.get_untracked() .uses_from_here(),
            }));
        }
        // End any in-progress single-touch gesture
        state.is_dragging.set(false);
        state.spec_drag_handle.set(None);
        state.axis_drag_start_freq.set(None);
        state.axis_drag_current_freq.set(None);
        ix.time_axis_dragging.set(false);
        ix.time_axis_pending.set(None);
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

    // Check for annotation resize handle drag first (touch) — selected annotations
    // take priority over BandFF/HET handles when they overlap
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

    // Check for spec handle drag — hit-test at touch position
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
            let band_ff_focused = state.active_focus.get_untracked() == Some(ActiveFocus::FrequencyFocus);
            let handle = hit_test_spec_handles(
                &state, px_y, min_freq_val, max_freq_val, ch, 16.0, band_ff_focused, // wider touch target
            );
            if let Some(handle) = handle {
                let is_ff = matches!(handle, SpectrogramHandle::BandFfUpper | SpectrogramHandle::BandFfLower | SpectrogramHandle::BandFfMiddle);
                if !is_ff || is_in_band_ff_drag_zone(px_x, cw) {
                    state.spec_drag_handle.set(Some(handle));
                    state.is_dragging.set(true);
                    ev.prevent_default();
                    return;
                }
            }
        }
    }

    // Corner drag gone — see pointer variant above.

    // Left-axis viewport pan (touch). Band selection lives on the right-
    // side band gutter. Double-tap resets the view; single-tap after
    // touchend also resets (handled in on_touchend).
    if let Some((px_x, _, _, freq)) = pointer_to_xtf(touch.client_x() as f64, touch.client_y() as f64, canvas_ref, &state) {
        if px_x < LABEL_AREA_WIDTH && !state.display_transform.get_untracked() {
            let now = js_sys::Date::now();
            let last_time = ix.last_tap_time.get_untracked();
            let last_x = ix.last_tap_x.get_untracked();
            if now - last_time < 400.0 && last_x < LABEL_AREA_WIDTH {
                // Double-tap: reset display range.
                reset_freq_axis_view(state);
                ix.last_tap_time.set(0.0);
                ev.prevent_default();
                return;
            }
            let nyquist = file_nyquist(state);
            let start_min = state.min_display_freq.get_untracked().unwrap_or(0.0);
            let start_max = state.max_display_freq.get_untracked().unwrap_or(nyquist);
            ix.freq_pan_start.set(Some((
                touch.client_x() as f64, touch.client_y() as f64,
                freq, start_min, start_max,
            )));
            state.is_dragging.set(true);
            ev.prevent_default();
            return;
        }
    }

    // Time-axis touch interactions (drag-to-select, double-tap) moved to
    // the <TimeGutter/> strip.

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
    // When viewport is pinch-zoomed, let the browser handle all touch gestures.
    if state.viewport_zoomed.get_untracked() { return; }

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

    // Left-axis viewport pan (touch). Takes priority over every other
    // drag so grabbing the axis always navigates the display window.
    if let Some((_cx, _cy, anchor_freq, start_min, start_max)) = ix.freq_pan_start.get_untracked() {
        if let Some((_, px_y, _, _)) = pointer_to_xtf(touch.client_x() as f64, touch.client_y() as f64, canvas_ref, &state) {
            let Some(canvas_el) = canvas_ref.get() else { return };
            let canvas: &HtmlCanvasElement = canvas_el.as_ref();
            let ch = canvas.get_bounding_client_rect().height();
            apply_freq_axis_pan(state, px_y, ch, anchor_freq, start_min, start_max);
        }
        return;
    }

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
        if let Some((_, _, t, _f)) = pointer_to_xtf(touch.client_x() as f64, touch.client_y() as f64, canvas_ref, &state) {
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
                    // Corner → Y commits to left-axis viewport pan (the axis
                    // no longer does band selection).
                    let saved_sel = ix.corner_drag_saved_selection.get_untracked();
                    state.selection.set(saved_sel);
                    ix.time_axis_dragging.set(false);
                    let anchor_freq = ix.axis_drag_raw_start.get_untracked();
                    let nyquist = file_nyquist(state);
                    let start_min = state.min_display_freq.get_untracked().unwrap_or(0.0);
                    let start_max = state.max_display_freq.get_untracked().unwrap_or(nyquist);
                    ix.freq_pan_start.set(Some((
                        touch.client_x() as f64, touch.client_y() as f64,
                        anchor_freq, start_min, start_max,
                    )));
                } else {
                    let (saved_lo, saved_hi) = ix.corner_drag_saved_ff.get_untracked();
                    if saved_hi > saved_lo {
                        state.set_band_ff_range(saved_lo, saved_hi);
                    }
                    ix.freq_pan_start.set(None);
                    ix.time_axis_dragging.set(true);
                }
                ix.corner_drag_axis.set(Some(want_y_axis));
            }
            if want_y_axis {
                if let Some((_cx, _cy, anchor_freq, start_min, start_max)) = ix.freq_pan_start.get_untracked() {
                    if let Some((_, px_y, _, _)) = pointer_to_xtf(touch.client_x() as f64, touch.client_y() as f64, canvas_ref, &state) {
                        let Some(canvas_el) = canvas_ref.get() else { return };
                        let canvas: &HtmlCanvasElement = canvas_el.as_ref();
                        let ch = canvas.get_bounding_client_rect().height();
                        apply_freq_axis_pan(state, px_y, ch, anchor_freq, start_min, start_max);
                    }
                }
            } else {
                let t0 = ix.time_axis_drag_raw_start.get_untracked();
                let ff = state.focus_stack.get_untracked().effective_range();
                let (fl, fh) = if ff.is_active() { (Some(ff.lo), Some(ff.hi)) } else { (None, None) };
                state.selection.set(Some(Selection {
                    time_start: t0.min(t),
                    time_end: t0.max(t),
                    freq_low: fl,
                    freq_high: fh,
                }));
            }
        }
        return;
    }

    // Axis drag takes second priority
    if state.axis_drag_start_freq.get_untracked().is_some() {
        if let Some((_, _, _, f)) = pointer_to_xtf(touch.client_x() as f64, touch.client_y() as f64, canvas_ref, &state) {
            let raw_start = ix.axis_drag_raw_start.get_untracked();
            apply_axis_drag(state, raw_start, f, false);
        }
        return;
    }

    // Pending time-axis: commit to drag once finger moves >5px
    if let Some((start_cx, _start_t, _shift, anchor_t)) = ix.time_axis_pending.get_untracked() {
        let dx = (touch.client_x() as f64 - start_cx).abs();
        if dx > 5.0 {
            ix.time_axis_pending.set(None);
            ix.time_axis_dragging.set(true);
            ix.time_axis_drag_raw_start.set(anchor_t);
            if let Some((_, _, t, _)) = pointer_to_xtf(touch.client_x() as f64, touch.client_y() as f64, canvas_ref, &state) {
                let ff = state.focus_stack.get_untracked().effective_range();
                let (fl, fh) = if ff.is_active() { (Some(ff.lo), Some(ff.hi)) } else { (None, None) };
                state.selection.set(Some(Selection {
                    time_start: anchor_t.min(t),
                    time_end: anchor_t.max(t),
                    freq_low: fl,
                    freq_high: fh,
                }));
            }
        }
        return;
    }

    // Time-axis drag takes third priority
    if ix.time_axis_dragging.get_untracked() {
        if let Some((_, _, t, _)) = pointer_to_xtf(touch.client_x() as f64, touch.client_y() as f64, canvas_ref, &state) {
            let t0 = ix.time_axis_drag_raw_start.get_untracked();
            let ff = state.focus_stack.get_untracked().effective_range();
            let (fl, fh) = if ff.is_active() { (Some(ff.lo), Some(ff.hi)) } else { (None, None) };
            state.selection.set(Some(Selection {
                time_start: t0.min(t),
                time_end: t0.max(t),
                freq_low: fl,
                freq_high: fh,
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
        // End left-axis viewport pan (touch). Tap with no movement resets
        // the display range; an actual drag has already been applied live.
        if let Some((cx, cy, _anchor, _smin, _smax)) = ix.freq_pan_start.get_untracked() {
            let end_touch = ev.changed_touches().get(0);
            let (dx, dy) = match end_touch.as_ref() {
                Some(t) => (
                    (t.client_x() as f64 - cx).abs(),
                    (t.client_y() as f64 - cy).abs(),
                ),
                None => (0.0, 0.0),
            };
            let was_tap = dx < 3.0 && dy < 3.0;
            ix.freq_pan_start.set(None);
            state.is_dragging.set(false);
            if was_tap {
                reset_freq_axis_view(state);
                // Track tap for double-tap detection so a second tap still
                // registers (even though single-tap already reset the view).
                if let Some(touch) = end_touch {
                    if let Some((px_x, px_y, _, _)) = pointer_to_xtf(touch.client_x() as f64, touch.client_y() as f64, canvas_ref, &state) {
                        ix.last_tap_time.set(js_sys::Date::now());
                        ix.last_tap_x.set(px_x);
                        ix.last_tap_y.set(px_y);
                    }
                }
            }
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
            // Track tap for double-tap detection (y-axis taps return early here)
            if let Some(touch) = ev.changed_touches().get(0) {
                if let Some((px_x, px_y, _, _)) = pointer_to_xtf(touch.client_x() as f64, touch.client_y() as f64, canvas_ref, &state) {
                    ix.last_tap_time.set(js_sys::Date::now());
                    ix.last_tap_x.set(px_x);
                    ix.last_tap_y.set(px_y);
                }
            }
            return;
        }
        // End pending time-axis tap (finger didn't move enough — treat as tap-to-clear)
        if ix.time_axis_pending.get_untracked().is_some() {
            ix.time_axis_pending.set(None);
            state.is_dragging.set(false);
            if state.selection.get_untracked().is_some() {
                state.selection.set(None);
            }
            // Track tap for double-tap detection (x-axis taps return early here)
            if let Some(touch) = ev.changed_touches().get(0) {
                if let Some((px_x, px_y, _, _)) = pointer_to_xtf(touch.client_x() as f64, touch.client_y() as f64, canvas_ref, &state) {
                    ix.last_tap_time.set(js_sys::Date::now());
                    ix.last_tap_x.set(px_x);
                    ix.last_tap_y.set(px_y);
                }
            }
            return;
        }
        // Finalize time-axis drag
        if ix.time_axis_dragging.get_untracked() {
            ix.time_axis_dragging.set(false);
            state.is_dragging.set(false);
            if let Some(sel) = state.selection.get_untracked() {
                if sel.time_end - sel.time_start < 0.0001 {
                    state.selection.set(None);
                } else {
                    if sel.freq_low.is_none() {
                        // Auto-combine: only upgrade segment to region when HFR is on
                        let ff = state.focus_stack.get_untracked().effective_range();
                        if ff.is_active() {
                            state.selection.set(Some(Selection {
                                freq_low: Some(ff.lo),
                                freq_high: Some(ff.hi),
                                ..sel
                            }));
                        }
                    }
                    state.active_focus.set(Some(ActiveFocus::TransientSelection));
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
                        let from_here_mode = state.play_start_mode.get_untracked() .uses_from_here();
                        crate::components::inertia::start_inertia(
                            state, velocity, cw, time_res, duration, from_here_mode, ix.inertia_generation,
                        );
                    }
                }
            }
        }

        // Update frequency focus from selection (if auto-focus enabled)
        if state.canvas_tool.get_untracked() == CanvasTool::Selection && state.selection_auto_focus.get_untracked() {
            if let Some(sel) = state.selection.get_untracked() {
                if let (Some(lo), Some(hi)) = (sel.freq_low, sel.freq_high) {
                    if hi - lo > 100.0 {
                        state.set_band_ff_range(lo, hi);
                    }
                }
            }
        }

        // Track last tap time/position (used for double-tap detection on axes)
        if let Some(touch) = ev.changed_touches().get(0) {
            if let Some((px_x, px_y, _, _)) = pointer_to_xtf(touch.client_x() as f64, touch.client_y() as f64, canvas_ref, &state) {
                ix.last_tap_time.set(js_sys::Date::now());
                ix.last_tap_x.set(px_x);
                ix.last_tap_y.set(px_y);
            }
        }
    }
}

pub fn on_wheel(
    ev: web_sys::WheelEvent,
    state: AppState,
) {
    ev.prevent_default();

    // Resolve file_max_freq / time_res: prefer waterfall params when active,
    // so scroll/zoom work during listening/recording without a file.
    let is_mic_active = state.mic_recording.get_untracked() || state.mic_listening.get_untracked();
    let wf_active = is_mic_active && crate::canvas::live_waterfall::is_active();

    if ev.shift_key() {
        // Shift+scroll: vertical freq zoom around mouse position
        let file_max_freq = if wf_active {
            crate::canvas::live_waterfall::max_freq()
        } else {
            let files = state.files.get_untracked();
            let idx = state.current_file_index.get_untracked();
            idx.and_then(|i| files.get(i))
                .map(|f| f.spectrogram.max_freq)
                .unwrap_or(96_000.0)
        };
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
            *z = (*z * delta).clamp(viewport::MIN_ZOOM, viewport::MAX_ZOOM);
        });
    } else {
        let raw_delta = ev.delta_y() + ev.delta_x();
        let files = state.files.get_untracked();
        let timeline = state.active_timeline.get_untracked();
        let (time_res, duration) = if wf_active {
            let tr = crate::canvas::live_waterfall::time_resolution();
            let dur = crate::canvas::live_waterfall::total_columns() as f64 * tr;
            (tr, dur)
        } else if let Some(ref tl) = timeline {
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
            let from_here_mode = state.play_start_mode.get_untracked() .uses_from_here();
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
