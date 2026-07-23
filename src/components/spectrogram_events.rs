use crate::annotations::AnnotationId;
use crate::canvas::coord::pointer_to_xtf;
use crate::canvas::hit_test::{
    hit_test_annotation_body, hit_test_annotation_handles, hit_test_band_ff_body,
    hit_test_spec_handles, is_in_band_ff_drag_zone,
};
use crate::canvas::spectrogram_renderer;
use crate::state::store_fields::*;
use crate::state::{ActiveFocus, AppState, CanvasTool, Selection, SpectrogramHandle, UndoEntry};
use crate::viewport;
use leptos::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{HtmlCanvasElement, MouseEvent, PointerEvent};

pub const LABEL_AREA_WIDTH: f64 = 60.0;

/// Auto-scroll the viewport to follow the playhead during playback, with
/// manual-scroll suspension support (resume once the playhead is back on-screen
/// and the user has stopped scrolling for 200ms). Call from a single reactive
/// Effect in each canvas component — the `.get()`s here establish its deps.
///
/// Extracted from the previously copy-pasted (and drift-prone) follow Effects
/// in `spectrogram.rs` + `waveform.rs`; a fix now lands in both at once.
pub fn follow_playhead(state: &AppState, canvas_ref: NodeRef<leptos::html::Canvas>) {
    let playhead = state.playback.playhead_time().get();
    let is_playing = state.playback.is_playing().get();
    let follow = state.view.follow_cursor().get();
    // get_untracked to avoid recursive Effect invocation — this already re-runs
    // via playhead_time / is_playing / follow_cursor changes.
    let suspended = state.view.follow_suspended().get_untracked();

    if !follow {
        return;
    }
    if !is_playing {
        // Reset suspension when playback stops so the next play starts fresh.
        if suspended {
            state.view.follow_suspended().set(false);
            state.view.follow_visible_since().set(None);
        }
        return;
    }

    let Some(canvas_el) = canvas_ref.get() else {
        return;
    };
    let canvas: &HtmlCanvasElement = canvas_el.as_ref();
    let display_w = canvas.width() as f64;
    if display_w == 0.0 {
        return;
    }

    let files = state.library.files().get_untracked();
    let idx = state.library.current_index().get_untracked();
    let (time_res, duration) = idx
        .and_then(|i| files.get(i))
        .map(|f| (f.spectrogram.time_resolution, f.audio.duration_secs))
        .unwrap_or((1.0, 0.0));
    let zoom = state.view.zoom_level().get_untracked();
    let scroll = state.view.scroll_offset().get_untracked();
    let from_here_mode = state.playback.start_mode().get_untracked().uses_from_here();

    let visible_time = viewport::visible_time(display_w, zoom, time_res);
    let playhead_rel = playhead - scroll;

    if suspended {
        // Resume once the playhead is on-screen and the user stopped scrolling.
        let playhead_visible = playhead_rel >= 0.0 && playhead_rel <= visible_time;
        if playhead_visible {
            let resume = match state.view.follow_visible_since().get_untracked() {
                Some(since) => js_sys::Date::now() - since >= 200.0,
                None => true, // no recorded scroll time — safe to resume
            };
            if resume {
                state.view.follow_suspended().set(false);
                state.view.follow_visible_since().set(None);
            }
        }
        return;
    }

    // When very zoomed in (viewport < 0.5s), follow every frame to keep the
    // playhead pinned at FOLLOW_CURSOR_FRACTION — the edge trigger would fire
    // too frequently and look jarring.
    if visible_time < viewport::FOLLOW_EXACT_THRESHOLD_SECS {
        let target_scroll = playhead - visible_time * viewport::FOLLOW_CURSOR_FRACTION;
        state
            .view
            .scroll_offset()
            .set(viewport::clamp_scroll_for_mode(
                target_scroll,
                duration,
                visible_time,
                from_here_mode,
            ));
    }
    // Normal follow: scroll when the playhead nears the edge.
    else if playhead_rel > visible_time * viewport::FOLLOW_CURSOR_EDGE_FRACTION
        || playhead_rel < 0.0
    {
        let target_scroll = playhead - visible_time * viewport::FOLLOW_CURSOR_FRACTION;
        state
            .view
            .scroll_offset()
            .set(viewport::clamp_scroll_for_mode(
                target_scroll,
                duration,
                visible_time,
                from_here_mode,
            ));
    }
}

/// A deferred annotation-body click (Hand tool): resolved on mouseup so a pan
/// gesture takes priority over selecting the annotation under the cursor.
#[derive(Clone)]
pub struct PendingAnnotationHit {
    pub id: AnnotationId,
    /// Whether Ctrl/Cmd was held (toggle add/remove from the selection).
    pub ctrl: bool,
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
    /// Locked pinch axis (time vs frequency) — set once per gesture from the
    /// dominant spread direction so zoom snaps to one axis at a time.
    pub pinch_axis: RwSignal<Option<crate::components::pinch::PinchAxis>>,
    /// Label hover animation target (0.0 or 1.0)
    pub label_hover_target: RwSignal<f64>,
    /// Double-tap detection: timestamp of last tap
    pub last_tap_time: RwSignal<f64>,
    /// Double-tap detection: x-position of last tap
    pub last_tap_x: RwSignal<f64>,
    /// Double-tap detection: y-position of last tap
    pub last_tap_y: RwSignal<f64>,
    /// Velocity tracker for inertia scrolling
    pub velocity_tracker: StoredValue<crate::components::inertia::VelocityTracker>,
    /// Generation counter for cancelling inertia animations
    pub inertia_generation: StoredValue<u32>,
    /// Pending annotation hit for Hand tool — deferred to mouseup so panning takes priority
    pub pending_annotation_hit: RwSignal<Option<PendingAnnotationHit>>,
    /// Pending BandFF body click — deferred to mouseup so panning takes priority
    pub pending_band_ff_hit: RwSignal<bool>,
    /// Pending transient selection body click — deferred to mouseup so panning takes priority
    pub pending_selection_hit: RwSignal<bool>,
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
            pinch_axis: RwSignal::new(None),
            label_hover_target: RwSignal::new(0.0f64),
            last_tap_time: RwSignal::new(0.0f64),
            last_tap_x: RwSignal::new(0.0f64),
            last_tap_y: RwSignal::new(0.0f64),
            velocity_tracker: StoredValue::new(crate::components::inertia::VelocityTracker::new()),
            inertia_generation: StoredValue::new(0u32),
            pending_annotation_hit: RwSignal::new(None),
            pending_band_ff_hit: RwSignal::new(false),
            pending_selection_hit: RwSignal::new(false),
        }
    }
}

/// Read the file's Nyquist / max frequency — the ceiling for the freq
/// display range.
fn file_nyquist(state: AppState) -> f64 {
    let is_mic_active =
        state.mic.recording().get_untracked() || state.mic.listening().get_untracked();
    if is_mic_active && crate::canvas::live_waterfall::is_active() {
        crate::canvas::live_waterfall::max_freq()
    } else {
        let files = state.library.files().get_untracked();
        let idx = state.library.current_index().get_untracked();
        idx.and_then(|i| files.get(i))
            .map(|f| f.spectrogram.max_freq)
            .unwrap_or(96_000.0)
    }
}

/// Reset the frequency display range to "auto" (None) so the spectrogram
/// shows 0..Nyquist again. Used for tap-to-reset and double-click on the
/// left axis.
pub fn reset_freq_axis_view(state: AppState) {
    state.view.min_display_freq().set(None);
    state.view.max_display_freq().set(None);
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
            let lo = state.filter.band_ff_freq_lo().get_untracked();
            let clamped = freq_at_pointer.clamp(lo + 500.0, file_max_freq);
            state.set_band_ff_hi(clamped);
        }
        SpectrogramHandle::BandFfLower => {
            let hi = state.filter.band_ff_freq_hi().get_untracked();
            let clamped = freq_at_pointer.clamp(0.0, hi - 500.0);
            state.set_band_ff_lo(clamped);
        }
        SpectrogramHandle::BandFfMiddle => {
            let lo = state.filter.band_ff_freq_lo().get_untracked();
            let hi = state.filter.band_ff_freq_hi().get_untracked();
            let bw = hi - lo;
            let mid = (lo + hi) / 2.0;
            let delta = freq_at_pointer - mid;
            let new_lo = (lo + delta).clamp(0.0, file_max_freq - bw);
            let new_hi = new_lo + bw;
            state.set_band_ff_range(new_lo, new_hi);
        }
        SpectrogramHandle::HetCenter => {
            state.transform.het_freq_auto().set(false);
            let clamped = freq_at_pointer.clamp(1000.0, file_max_freq);
            state.transform.het_frequency().set(clamped);
        }
        SpectrogramHandle::HetBandUpper => {
            state.transform.het_cutoff_auto().set(false);
            let het_freq = state.transform.het_frequency().get_untracked();
            let new_cutoff = (freq_at_pointer - het_freq).clamp(1000.0, 30000.0);
            state.transform.het_cutoff().set(new_cutoff);
        }
        SpectrogramHandle::HetBandLower => {
            state.transform.het_cutoff_auto().set(false);
            let het_freq = state.transform.het_frequency().get_untracked();
            let new_cutoff = (het_freq - freq_at_pointer).clamp(1000.0, 30000.0);
            state.transform.het_cutoff().set(new_cutoff);
        }
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
    let is_mic_active =
        state.mic.recording().get_untracked() || state.mic.listening().get_untracked();
    let wf_active = is_mic_active && crate::canvas::live_waterfall::is_active();
    let file_max_freq = if wf_active {
        crate::canvas::live_waterfall::max_freq()
    } else {
        let files = state.library.files().get_untracked();
        let idx = state.library.current_index().get_untracked();
        idx.and_then(|i| files.get(i))
            .map(|f| f.spectrogram.max_freq)
            .unwrap_or(96_000.0)
    };
    let min_freq_val = state.view.min_display_freq().get_untracked().unwrap_or(0.0);
    let max_freq_val = state
        .view
        .max_display_freq()
        .get_untracked()
        .unwrap_or(file_max_freq);
    let freq = spectrogram_renderer::y_to_freq(px_y, min_freq_val, max_freq_val, ch);
    Some((freq, file_max_freq))
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
    let Some(canvas_el) = canvas_ref.get() else {
        return;
    };
    let canvas: &HtmlCanvasElement = canvas_el.as_ref();
    let cw = canvas.width() as f64;
    if cw == 0.0 {
        return;
    }
    let files = state.library.files().get_untracked();
    let idx = state.library.current_index().get_untracked();
    let file = idx.and_then(|i| files.get(i));
    let timeline = state.timeline.active().get_untracked();
    let waterfall_active = (state.mic.recording().get_untracked()
        || state.mic.listening().get_untracked())
        && crate::canvas::live_waterfall::is_active();
    let time_res = if waterfall_active {
        crate::canvas::live_waterfall::time_resolution()
    } else if let Some(ref tl) = timeline {
        tl.segments
            .first()
            .and_then(|s| files.get(s.file_index))
            .map(|f| f.spectrogram.time_resolution)
            .unwrap_or(1.0)
    } else {
        file.as_ref()
            .map(|f| f.spectrogram.time_resolution)
            .unwrap_or(1.0)
    };
    let zoom = state.view.zoom_level().get_untracked();
    let visible_time = viewport::visible_time(cw, zoom, time_res);
    let dt = -(dx / cw) * visible_time;
    state.suspend_follow();

    let new_scroll = if waterfall_active {
        // Bound panning to what the circular buffer actually holds: can't go
        // earlier than the oldest retained column, can't go past the live edge.
        let total_time = crate::canvas::live_waterfall::total_time();
        let oldest = crate::canvas::live_waterfall::oldest_time();
        let max_scroll = (total_time - visible_time).max(oldest);
        let s = (start_scroll + dt).clamp(oldest, max_scroll);
        // Panning to (or near) the live edge re-engages auto-follow immediately
        // so the view keeps scrolling with incoming audio. Panning back into
        // history pushes the snap-back 2s out so a release between gestures
        // doesn't yank the view to "now".
        if s >= max_scroll - visible_time * 0.05 {
            state.resume_waterfall_follow();
        } else {
            state.suspend_waterfall_follow(2000.0);
        }
        s
    } else {
        let duration = if let Some(ref tl) = timeline {
            tl.total_duration_secs
        } else {
            file.as_ref()
                .map(|f| f.audio.duration_secs)
                .unwrap_or(f64::MAX)
        };
        let from_here_mode = state.playback.start_mode().get_untracked().uses_from_here();
        viewport::clamp_scroll_for_mode(start_scroll + dt, duration, visible_time, from_here_mode)
    };
    state.view.scroll_offset().set(new_scroll);
}

/// Apply annotation resize based on which handle is being dragged.
pub fn apply_annotation_resize(
    state: AppState,
    ann_id: AnnotationId,
    handle: crate::state::ResizeHandlePosition,
    time: f64,
    freq: f64,
) {
    use crate::state::ResizeHandlePosition::*;

    let Some(file_id) = state.current_file_id() else {
        return;
    };
    state.annotations.store().update(|store| {
        if let Some(set) = store.get_mut(file_id) {
            if let Some(ann) = set.annotations.iter_mut().find(|a| a.id == ann_id) {
                if let crate::annotations::AnnotationKind::Region(ref mut r) = ann.kind {
                    match handle {
                        Left => r.time_start = time.min(r.time_end - 0.0001),
                        Right => r.time_end = time.max(r.time_start + 0.0001),
                        Top => {
                            if r.freq_high.is_some() {
                                let lo = r.freq_low.unwrap_or(0.0);
                                r.freq_high = Some(freq.max(lo + 100.0));
                            }
                        }
                        Bottom => {
                            if r.freq_low.is_some() {
                                let hi = r.freq_high.unwrap_or(f64::MAX);
                                r.freq_low = Some(freq.min(hi - 100.0));
                            }
                        }
                        TopLeft => {
                            r.time_start = time.min(r.time_end - 0.0001);
                            if r.freq_high.is_some() {
                                let lo = r.freq_low.unwrap_or(0.0);
                                r.freq_high = Some(freq.max(lo + 100.0));
                            }
                        }
                        TopRight => {
                            r.time_end = time.max(r.time_start + 0.0001);
                            if r.freq_high.is_some() {
                                let lo = r.freq_low.unwrap_or(0.0);
                                r.freq_high = Some(freq.max(lo + 100.0));
                            }
                        }
                        BottomLeft => {
                            r.time_start = time.min(r.time_end - 0.0001);
                            if r.freq_low.is_some() {
                                let hi = r.freq_high.unwrap_or(f64::MAX);
                                r.freq_low = Some(freq.min(hi - 100.0));
                            }
                        }
                        BottomRight => {
                            r.time_end = time.max(r.time_start + 0.0001);
                            if r.freq_low.is_some() {
                                let hi = r.freq_high.unwrap_or(f64::MAX);
                                r.freq_low = Some(freq.min(hi - 100.0));
                            }
                        }
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
    if ev.button() != 0 {
        return;
    }
    // When viewport is pinch-zoomed, let the browser handle all gestures so
    // the user can zoom back out via native pinch.
    if state.status.viewport_zoomed().get_untracked() {
        return;
    }

    state.interaction.pointer_is_down().set(true);

    // Check for annotation resize handle drag first (selected annotations take
    // priority over BandFF/HET handles when they overlap). Skipped when annotations are hidden.
    if state.annotations.visible().get_untracked() {
        if let Some((ref ann_id, handle_pos)) = state.annotations.hover_handle().get_untracked() {
            // Check if the annotation is locked
            let Some(file_id) = state.current_file_id() else {
                return;
            };
            let store = state.annotations.store().get_untracked();
            let locked = store
                .get(file_id)
                .and_then(|set| set.annotations.iter().find(|a| a.id == *ann_id))
                .and_then(|a| match &a.kind {
                    crate::annotations::AnnotationKind::Region(r) => Some(r.is_locked()),
                    _ => None,
                })
                .unwrap_or(false);

            if !locked {
                // Snapshot for undo
                let snapshot = store.get(file_id).cloned();
                state.annotations.undo_stack().update(|stack| {
                    stack.push_undo(UndoEntry { file_id, snapshot });
                });
                // Store original bounds
                if let Some(set) = store.get(file_id) {
                    if let Some(a) = set.annotations.iter().find(|a| a.id == *ann_id) {
                        if let crate::annotations::AnnotationKind::Region(ref r) = a.kind {
                            state.annotations.drag_original().set(Some((
                                r.time_start,
                                r.time_end,
                                r.freq_low,
                                r.freq_high,
                            )));
                        }
                    }
                }
                state
                    .annotations
                    .drag_handle()
                    .set(Some((ann_id.clone(), handle_pos)));
                state.interaction.is_dragging().set(true);
                capture_pointer(&ev);
                ev.prevent_default();
                return;
            }
        }
    }

    // Check for spec handle drag (BandFF or HET — takes priority over axis/tool drags)
    // BandFF handles only start drag when clicking within the center handle zone.
    if let Some(handle) = state.interaction.spec_hover_handle().get_untracked() {
        let is_ff = matches!(
            handle,
            SpectrogramHandle::BandFfUpper
                | SpectrogramHandle::BandFfLower
                | SpectrogramHandle::BandFfMiddle
        );
        let allow_drag = if is_ff {
            if let Some((px_x, _, _, _)) = pointer_to_xtf(
                ev.client_x() as f64,
                ev.client_y() as f64,
                canvas_ref,
                &state,
            ) {
                if let Some(canvas_el) = canvas_ref.get() {
                    let canvas: &HtmlCanvasElement = canvas_el.as_ref();
                    is_in_band_ff_drag_zone(px_x, canvas.width() as f64)
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            true // HET handles drag from anywhere
        };
        if allow_drag {
            state.interaction.spec_drag_handle().set(Some(handle));
            state.interaction.is_dragging().set(true);
            capture_pointer(&ev);
            ev.prevent_default();
            return;
        }
    }

    // The left edge used to be a freq-axis viewport-pan zone, cued by the now-
    // removed hover markers. That zone is retired with them: the left edge falls
    // through to the normal pan/drag below like the rest of the spectrogram.
    // Frequency navigation now lives on the sibling BandGutter.

    // Time-axis selection moved out to the <TimeGutter/> strip; the
    // bottom pixels of the spectrogram canvas no longer have special
    // gesture meaning.

    // Check for annotation body, transient selection body, and BandFF body clicks.
    // Priority: annotation > selection > BandFF. All deferred to pointer-up so panning takes priority.
    ix.pending_band_ff_hit.set(false);
    ix.pending_selection_hit.set(false);
    if state.interaction.canvas_tool().get_untracked() == CanvasTool::Hand {
        if let Some((px_x, px_y, t, freq)) = pointer_to_xtf(
            ev.client_x() as f64,
            ev.client_y() as f64,
            canvas_ref,
            &state,
        ) {
            let file_idx = state.library.current_index().get_untracked().unwrap_or(0);
            let files = state.library.files().get_untracked();
            let file = files.get(file_idx);
            let file_max_freq = file.map(|f| f.spectrogram.max_freq).unwrap_or(96_000.0);
            let min_freq = state.view.min_display_freq().get_untracked().unwrap_or(0.0);
            let max_freq = state
                .view
                .max_display_freq()
                .get_untracked()
                .unwrap_or(file_max_freq);

            // Check annotation body first (highest priority; skipped when annotations are hidden)
            let mut hit_annotation = false;
            if state.annotations.visible().get_untracked() {
                let store = state.annotations.store().get_untracked();
                if let Some(set) = state.file_id_at(file_idx).and_then(|id| store.get(id)) {
                    if let Some(canvas_el) = canvas_ref.get() {
                        let canvas: &HtmlCanvasElement = canvas_el.as_ref();
                        let cw = canvas.width() as f64;
                        let ch = canvas.height() as f64;
                        let scroll = state.view.scroll_offset().get_untracked();
                        let time_res = file.map(|f| f.spectrogram.time_resolution).unwrap_or(1.0);
                        let zoom = state.view.zoom_level().get_untracked();

                        if let Some(hit_id) = hit_test_annotation_body(
                            set, px_x, px_y, min_freq, max_freq, scroll, time_res, zoom, cw, ch,
                        ) {
                            let ctrl = ev.ctrl_key() || ev.meta_key();
                            ix.pending_annotation_hit
                                .set(Some(PendingAnnotationHit { id: hit_id, ctrl }));
                            hit_annotation = true;
                        }
                    }
                }
            }

            if !hit_annotation {
                // Check transient selection body (priority over BandFF)
                if let Some(sel) = state.interaction.selection().get_untracked() {
                    if point_in_selection(&sel, t, freq) {
                        ix.pending_selection_hit.set(true);
                    } else {
                        // Check BandFF body click
                        if let Some(canvas_el) = canvas_ref.get() {
                            let canvas: &HtmlCanvasElement = canvas_el.as_ref();
                            let ch = canvas.height() as f64;
                            let band_ff_lo = state.filter.band_ff_freq_lo().get_untracked();
                            let band_ff_hi = state.filter.band_ff_freq_hi().get_untracked();
                            if hit_test_band_ff_body(
                                px_y, band_ff_lo, band_ff_hi, min_freq, max_freq, ch,
                            ) {
                                ix.pending_band_ff_hit.set(true);
                            }
                        }
                    }
                } else {
                    // No selection — check BandFF body click
                    if let Some(canvas_el) = canvas_ref.get() {
                        let canvas: &HtmlCanvasElement = canvas_el.as_ref();
                        let ch = canvas.height() as f64;
                        let band_ff_lo = state.filter.band_ff_freq_lo().get_untracked();
                        let band_ff_hi = state.filter.band_ff_freq_hi().get_untracked();
                        if hit_test_band_ff_body(
                            px_y, band_ff_lo, band_ff_hi, min_freq, max_freq, ch,
                        ) {
                            ix.pending_band_ff_hit.set(true);
                        }
                    }
                }
            }
        }
    }

    // Click on empty area deselects annotations and clears focus (unless modifier held)
    // For Hand tool: defer to mouseup so panning isn't blocked
    if state.interaction.canvas_tool().get_untracked() != CanvasTool::Hand
        && !ev.ctrl_key()
        && !ev.meta_key()
        && !ev.shift_key()
    {
        let ids = state.annotations.selected_ids().get_untracked();
        if !ids.is_empty() {
            state.annotations.selected_ids().set(Vec::new());
        }
        state.interaction.active_focus().set(None);
    }

    match state.interaction.canvas_tool().get_untracked() {
        CanvasTool::Hand => {
            state.interaction.is_dragging().set(true);
            ix.hand_drag_start.set((
                ev.client_x() as f64,
                state.view.scroll_offset().get_untracked(),
            ));
        }
        CanvasTool::Selection => {
            if let Some((_, _, t, f)) = pointer_to_xtf(
                ev.client_x() as f64,
                ev.client_y() as f64,
                canvas_ref,
                &state,
            ) {
                state.interaction.is_dragging().set(true);
                ix.drag_start.set((t, f));
                state.interaction.selection().set(None);
            }
        }
    }

    // Capture pointer so drag continues even when cursor leaves the canvas
    if state.interaction.is_dragging().get_untracked() {
        capture_pointer(&ev);
    }
}

pub fn on_pointermove(
    ev: PointerEvent,
    ix: SpectInteraction,
    canvas_ref: &NodeRef<leptos::html::Canvas>,
    state: AppState,
) {
    if let Some((px_x, px_y, t, f)) = pointer_to_xtf(
        ev.client_x() as f64,
        ev.client_y() as f64,
        canvas_ref,
        &state,
    ) {
        // Always track hover position
        state.interaction.mouse_freq().set(Some(f));
        state.interaction.mouse_canvas_x().set(px_x);
        state.interaction.cursor_time().set(Some(t));

        // Update label hover target + in-label-area state. (Time-axis
        // interactions live on the sibling <TimeGutter/> now.)
        let in_label_area = px_x < LABEL_AREA_WIDTH;
        state.interaction.mouse_in_label_area().set(in_label_area);
        let current_target = ix.label_hover_target.get_untracked();
        let new_target = if in_label_area { 1.0 } else { 0.0 };
        if current_target != new_target {
            ix.label_hover_target.set(new_target);
        }

        if state.interaction.is_dragging().get_untracked() {
            // Spec handle drag takes priority
            if let Some(handle) = state.interaction.spec_drag_handle().get_untracked() {
                if let Some((freq_at_mouse, file_max_freq)) =
                    resolve_freq_at_pointer(px_y, canvas_ref, state)
                {
                    apply_handle_drag(state, handle, freq_at_mouse, file_max_freq);
                }
                return;
            }

            // Annotation resize handle drag takes second priority
            if let Some((ref ann_id, handle_pos)) = state.annotations.drag_handle().get_untracked()
            {
                apply_annotation_resize(state, ann_id.clone(), handle_pos, t, f);
                return;
            }

            match state.interaction.canvas_tool().get_untracked() {
                CanvasTool::Hand => {
                    apply_hand_pan(
                        state,
                        ev.client_x() as f64,
                        canvas_ref,
                        ix.hand_drag_start.get_untracked(),
                    );
                }
                CanvasTool::Selection => {
                    let (t0, f0) = ix.drag_start.get_untracked();
                    state.interaction.selection().set(Some(Selection {
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
                    let min_freq_val = state.view.min_display_freq().get_untracked().unwrap_or(0.0);
                    let max_freq_val = state
                        .view
                        .max_display_freq()
                        .get_untracked()
                        .unwrap_or(file_max_freq);
                    let canvas_el = canvas_ref.get();
                    if let Some(canvas_el) = canvas_el {
                        let canvas: &HtmlCanvasElement = canvas_el.as_ref();
                        let cw = canvas.width() as f64;
                        let ch = canvas.height() as f64;
                        let band_ff_focused = state.interaction.active_focus().get_untracked()
                            == Some(ActiveFocus::FrequencyFocus);
                        let handle = hit_test_spec_handles(
                            &state,
                            px_y,
                            min_freq_val,
                            max_freq_val,
                            ch,
                            8.0,
                            band_ff_focused,
                        );
                        state.interaction.spec_hover_handle().set(handle);

                        // Annotation resize handle hover detection (only when annotations have focus and are visible)
                        let annotations_focused = state.interaction.active_focus().get_untracked()
                            == Some(ActiveFocus::Annotations)
                            && state.annotations.visible().get_untracked();
                        let selected_ids = state.annotations.selected_ids().get_untracked();
                        if annotations_focused && !selected_ids.is_empty() {
                            let file_idx =
                                state.library.current_index().get_untracked().unwrap_or(0);
                            let store = state.annotations.store().get_untracked();
                            if let Some(set) =
                                state.file_id_at(file_idx).and_then(|id| store.get(id))
                            {
                                let scroll = state.view.scroll_offset().get_untracked();
                                let files = state.library.files().get_untracked();
                                let time_res = files
                                    .get(file_idx)
                                    .map(|f| f.spectrogram.time_resolution)
                                    .unwrap_or(1.0);
                                let zoom = state.view.zoom_level().get_untracked();
                                let ann_handle = hit_test_annotation_handles(
                                    set,
                                    &selected_ids,
                                    px_x,
                                    px_y,
                                    min_freq_val,
                                    max_freq_val,
                                    scroll,
                                    time_res,
                                    zoom,
                                    cw,
                                    ch,
                                    crate::canvas::hit_test::ANNOTATION_HANDLE_HIT_RADIUS,
                                );
                                state.annotations.hover_handle().set(ann_handle);
                            } else {
                                state.annotations.hover_handle().set(None);
                            }
                        } else {
                            state.annotations.hover_handle().set(None);
                        }
                    }
                }
            } else {
                state.interaction.spec_hover_handle().set(None);
                state.annotations.hover_handle().set(None);
            }
        }
    }
}

pub fn on_pointerleave(_ev: PointerEvent, ix: SpectInteraction, state: AppState) {
    // When pointer is captured (during a drag), pointerleave won't normally fire.
    // But if it does somehow, preserve drag state so the gesture isn't interrupted.
    if state.interaction.is_dragging().get_untracked() {
        return;
    }

    state.interaction.pointer_is_down().set(false);
    state.interaction.mouse_freq().set(None);
    state.interaction.mouse_in_label_area().set(false);
    state.interaction.cursor_time().set(None);
    ix.label_hover_target.set(0.0);
    state.interaction.spec_hover_handle().set(None);
    state.annotations.hover_handle().set(None);
    ix.pending_annotation_hit.set(None);
}

pub fn on_pointerup(
    ev: PointerEvent,
    ix: SpectInteraction,
    canvas_ref: &NodeRef<leptos::html::Canvas>,
    state: AppState,
) {
    state.interaction.pointer_is_down().set(false);
    if !state.interaction.is_dragging().get_untracked() {
        return;
    }

    // End HET/BandFF handle drag
    if state
        .interaction
        .spec_drag_handle()
        .get_untracked()
        .is_some()
    {
        state.interaction.spec_drag_handle().set(None);
        state.interaction.is_dragging().set(false);
        return;
    }

    // End annotation resize handle drag
    if let Some((ref ann_id, _)) = state.annotations.drag_handle().get_untracked() {
        if let Some(file_id) = state.current_file_id() {
            let now = js_sys::Date::new_0()
                .to_iso_string()
                .as_string()
                .unwrap_or_default();
            state.annotations.store().update(|store| {
                if let Some(set) = store.get_mut(file_id) {
                    if let Some(a) = set.annotations.iter_mut().find(|a| a.id == *ann_id) {
                        a.modified_at = now;
                    }
                }
            });
        }
        state.annotations.dirty().set(true);
        state.annotations.drag_handle().set(None);
        state.annotations.drag_original().set(None);
        state.interaction.is_dragging().set(false);
        return;
    }

    state.interaction.is_dragging().set(false);

    if state.interaction.canvas_tool().get_untracked() == CanvasTool::Hand {
        let (start_x, _) = ix.hand_drag_start.get_untracked();
        let dx = (ev.client_x() as f64 - start_x).abs();
        let was_click = dx < 3.0;

        if was_click {
            // Handle deferred annotation selection on click
            if let Some(PendingAnnotationHit { id: hit_id, ctrl }) =
                ix.pending_annotation_hit.get_untracked()
            {
                if ctrl {
                    state.annotations.selected_ids().update(|ids| {
                        if let Some(pos) = ids.iter().position(|id| *id == hit_id) {
                            ids.remove(pos);
                        } else {
                            ids.push(hit_id.clone());
                        }
                    });
                } else {
                    state.annotations.selected_ids().set(vec![hit_id.clone()]);
                }
                state.annotations.last_clicked_id().set(Some(hit_id));
                state
                    .interaction
                    .active_focus()
                    .set(Some(ActiveFocus::Annotations));
            } else if ix.pending_selection_hit.get_untracked() {
                // Deferred transient selection body click-to-refocus
                state
                    .interaction
                    .active_focus()
                    .set(Some(ActiveFocus::TransientSelection));
            } else if ix.pending_band_ff_hit.get_untracked() {
                // Deferred BandFF body click-to-select
                state
                    .interaction
                    .active_focus()
                    .set(Some(ActiveFocus::FrequencyFocus));
            } else {
                // Click on empty area deselects annotations and clears focus
                if !ev.ctrl_key() && !ev.meta_key() && !ev.shift_key() {
                    let ids = state.annotations.selected_ids().get_untracked();
                    if !ids.is_empty() {
                        state.annotations.selected_ids().set(Vec::new());
                    }
                    state.interaction.active_focus().set(None);
                }
            }
            // Bookmark while playing
            if state.playback.is_playing().get_untracked() {
                let t = state.playback.playhead_time().get_untracked();
                state
                    .viewmode
                    .bookmarks()
                    .update(|bm| bm.push(crate::state::Bookmark { time: t }));
            }
        }

        ix.pending_annotation_hit.set(None);
        ix.pending_band_ff_hit.set(false);
        ix.pending_selection_hit.set(false);
        return;
    }
    if state.interaction.canvas_tool().get_untracked() != CanvasTool::Selection {
        return;
    }
    if let Some((_, _, t, f)) = pointer_to_xtf(
        ev.client_x() as f64,
        ev.client_y() as f64,
        canvas_ref,
        &state,
    ) {
        let (t0, f0) = ix.drag_start.get_untracked();
        let sel = Selection {
            time_start: t0.min(t),
            time_end: t0.max(t),
            freq_low: Some(f0.min(f)),
            freq_high: Some(f0.max(f)),
        };
        if sel.time_end - sel.time_start > 0.0001 {
            state.interaction.selection().set(Some(sel));
            state
                .interaction
                .active_focus()
                .set(Some(ActiveFocus::TransientSelection));
            if state.annotations.selection_auto_focus().get_untracked() {
                if let (Some(lo), Some(hi)) = (sel.freq_low, sel.freq_high) {
                    if hi - lo > 100.0 {
                        state.set_band_ff_range(lo, hi);
                    }
                }
            }
        } else {
            state.interaction.selection().set(None);
        }
    }
}

pub fn on_dblclick(ev: MouseEvent, canvas_ref: &NodeRef<leptos::html::Canvas>, state: AppState) {
    // Double-click on y-axis: reset the frequency display range to
    // 0..Nyquist (same as a tap). The band gutter owns "select all
    // frequencies" (HFR) now; this axis is a viewport control.
    if let Some((px_x, px_y, _, _)) = pointer_to_xtf(
        ev.client_x() as f64,
        ev.client_y() as f64,
        canvas_ref,
        &state,
    ) {
        if px_x < LABEL_AREA_WIDTH && !state.display.transform().get_untracked() {
            reset_freq_axis_view(state);
            ev.prevent_default();
            return;
        }
        // Double-click time-axis-select-all now lives on the TimeGutter.
        let _ = px_y;
    }

    // Double-click inside a transient selection: promote it to an annotation and open label edit.
    if let Some(sel) = state.interaction.selection().get_untracked() {
        if let Some((_, _, t, freq)) = pointer_to_xtf(
            ev.client_x() as f64,
            ev.client_y() as f64,
            canvas_ref,
            &state,
        ) {
            if point_in_selection(&sel, t, freq) {
                crate::components::overflow_menu::annotate_selection(&state);
                ev.prevent_default();
                return;
            }
            // Outside selection: clear it (existing behavior)
            state.interaction.last_selection().set(Some(sel));
            state.interaction.selection().set(None);
            if state.interaction.active_focus().get_untracked()
                == Some(ActiveFocus::TransientSelection)
            {
                state.interaction.active_focus().set(None);
            }
            ev.prevent_default();
            return;
        }
    }

    // Double-click inside an annotation body: enter label edit for that annotation.
    if state.annotations.visible().get_untracked() {
        if let Some((px_x, px_y, _, _)) = pointer_to_xtf(
            ev.client_x() as f64,
            ev.client_y() as f64,
            canvas_ref,
            &state,
        ) {
            if let Some(canvas_el) = canvas_ref.get() {
                let canvas: &HtmlCanvasElement = canvas_el.as_ref();
                let cw = canvas.width() as f64;
                let ch = canvas.height() as f64;
                let file_idx = state.library.current_index().get_untracked().unwrap_or(0);
                let files = state.library.files().get_untracked();
                let file = files.get(file_idx);
                let file_max_freq = file.map(|f| f.spectrogram.max_freq).unwrap_or(96_000.0);
                let min_freq = state.view.min_display_freq().get_untracked().unwrap_or(0.0);
                let max_freq = state
                    .view
                    .max_display_freq()
                    .get_untracked()
                    .unwrap_or(file_max_freq);
                let scroll = state.view.scroll_offset().get_untracked();
                let time_res = file.map(|f| f.spectrogram.time_resolution).unwrap_or(1.0);
                let zoom = state.view.zoom_level().get_untracked();
                let store = state.annotations.store().get_untracked();
                if let Some(set) = state.file_id_at(file_idx).and_then(|id| store.get(id)) {
                    if let Some(hit_id) = hit_test_annotation_body(
                        set, px_x, px_y, min_freq, max_freq, scroll, time_res, zoom, cw, ch,
                    ) {
                        let is_locked = set
                            .annotations
                            .iter()
                            .find(|a| a.id == hit_id)
                            .and_then(|a| match &a.kind {
                                crate::annotations::AnnotationKind::Region(r) => {
                                    Some(r.is_locked())
                                }
                                _ => None,
                            })
                            .unwrap_or(false);
                        state.annotations.selected_ids().set(vec![hit_id]);
                        state
                            .interaction
                            .active_focus()
                            .set(Some(ActiveFocus::Annotations));
                        if is_locked {
                            state.show_info_toast("Annotation is locked \u{2014} unlock to edit label by double-click");
                        } else {
                            state.annotations.is_new_edit().set(false);
                            state.annotations.editing().set(true);
                        }
                        ev.prevent_default();
                        return;
                    }
                }
            }
        }
    }

    // Double-click on BandFF handle toggles HFR (label area tap handled by finalize_axis_drag)
    if state.band_ff_range_untracked().is_none() {
        return;
    }

    let on_handle = matches!(
        state.interaction.spec_hover_handle().get_untracked(),
        Some(
            SpectrogramHandle::BandFfUpper
                | SpectrogramHandle::BandFfLower
                | SpectrogramHandle::BandFfMiddle
        )
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
    if state.status.viewport_zoomed().get_untracked() {
        return;
    }

    // Cancel any ongoing inertia animation immediately
    crate::components::inertia::cancel_inertia(ix.inertia_generation);
    ix.velocity_tracker.update_value(|t| t.reset());

    let touches = ev.touches();
    let n = touches.length();

    // Two-finger: initialize pinch-to-zoom
    if n == 2 {
        ev.prevent_default();
        use crate::components::pinch::{two_finger_axes, PinchState};
        if let Some((mid_x, mid_y, dist_x, dist_y)) = two_finger_axes(&touches) {
            let files = state.library.files().get_untracked();
            let idx = state.library.current_index().get_untracked();
            let file = idx.and_then(|i| files.get(i));
            let wf_active = (state.mic.recording().get_untracked()
                || state.mic.listening().get_untracked())
                && crate::canvas::live_waterfall::is_active();
            let (time_res, duration) = if wf_active {
                let tr = crate::canvas::live_waterfall::time_resolution();
                (
                    tr,
                    crate::canvas::live_waterfall::total_columns() as f64 * tr,
                )
            } else {
                (
                    file.as_ref()
                        .map(|f| f.spectrogram.time_resolution)
                        .unwrap_or(1.0),
                    file.as_ref()
                        .map(|f| f.audio.duration_secs)
                        .unwrap_or(f64::MAX),
                )
            };
            let nyquist = file_nyquist(state);
            let initial_min_freq = state.view.min_display_freq().get_untracked().unwrap_or(0.0);
            let initial_max_freq = state
                .view
                .max_display_freq()
                .get_untracked()
                .unwrap_or(nyquist);
            // Was the view following the live edge when the pinch began? (No pan
            // grace window active.) Drives whether we keep butted to the edge or
            // anchor-zoom in place.
            let was_following = wf_active
                && state.mic.scroll_user_pan_until().get_untracked() <= js_sys::Date::now();
            ix.pinch_state.set(Some(PinchState {
                initial_dist_x: dist_x.max(1.0),
                initial_dist_y: dist_y.max(1.0),
                initial_zoom: state.view.zoom_level().get_untracked(),
                initial_scroll: state.view.scroll_offset().get_untracked(),
                initial_mid_client_x: mid_x,
                initial_mid_client_y: mid_y,
                initial_min_freq,
                initial_max_freq,
                nyquist,
                time_res,
                duration,
                from_here_mode: state.playback.start_mode().get_untracked().uses_from_here(),
                was_following,
            }));
            // New gesture — axis is decided on first significant move.
            ix.pinch_axis.set(None);
        }
        // End any in-progress single-touch gesture
        state.interaction.is_dragging().set(false);
        state.interaction.spec_drag_handle().set(None);
        state.interaction.axis_drag_start_freq().set(None);
        state.interaction.axis_drag_current_freq().set(None);
        return;
    }

    if n != 1 {
        return;
    }
    // Transitioning from 2 to 1 finger — re-anchor pan position
    if ix.pinch_state.get_untracked().is_some() {
        ix.pinch_state.set(None);
        ix.pinch_axis.set(None);
        if let Some(touch) = touches.get(0) {
            ix.hand_drag_start.set((
                touch.client_x() as f64,
                state.view.scroll_offset().get_untracked(),
            ));
            if state.interaction.canvas_tool().get_untracked() == CanvasTool::Hand {
                state.interaction.is_dragging().set(true);
            }
        }
        return;
    }

    let touch = touches.get(0).unwrap();

    // Check for annotation resize handle drag first (touch) — selected annotations
    // take priority over BandFF/HET handles when they overlap
    if let Some((px_x, px_y, _, _)) = pointer_to_xtf(
        touch.client_x() as f64,
        touch.client_y() as f64,
        canvas_ref,
        &state,
    ) {
        if let Some(canvas_el) = canvas_ref.get() {
            let canvas: &HtmlCanvasElement = canvas_el.as_ref();
            let cw = canvas.width() as f64;
            let ch = canvas.height() as f64;
            let selected_ids = state.annotations.selected_ids().get_untracked();
            if !selected_ids.is_empty() {
                let file_idx = state.library.current_index().get_untracked().unwrap_or(0);
                let store = state.annotations.store().get_untracked();
                let files = state.library.files().get_untracked();
                let file_max_freq = files
                    .get(file_idx)
                    .map(|f| f.spectrogram.max_freq)
                    .unwrap_or(96_000.0);
                let min_freq_val = state.view.min_display_freq().get_untracked().unwrap_or(0.0);
                let max_freq_val = state
                    .view
                    .max_display_freq()
                    .get_untracked()
                    .unwrap_or(file_max_freq);
                let scroll = state.view.scroll_offset().get_untracked();
                let time_res = files
                    .get(file_idx)
                    .map(|f| f.spectrogram.time_resolution)
                    .unwrap_or(1.0);
                let zoom = state.view.zoom_level().get_untracked();
                let file_id = state.file_id_at(file_idx);
                if let Some(set) = file_id.and_then(|id| store.get(id)) {
                    let ann_handle = hit_test_annotation_handles(
                        set,
                        &selected_ids,
                        px_x,
                        px_y,
                        min_freq_val,
                        max_freq_val,
                        scroll,
                        time_res,
                        zoom,
                        cw,
                        ch,
                        crate::canvas::hit_test::ANNOTATION_HANDLE_HIT_RADIUS_TOUCH,
                    );
                    if let Some((ref ann_id, handle_pos)) = ann_handle {
                        // Check if locked
                        let locked = set
                            .annotations
                            .iter()
                            .find(|a| a.id == *ann_id)
                            .and_then(|a| match &a.kind {
                                crate::annotations::AnnotationKind::Region(r) => {
                                    Some(r.is_locked())
                                }
                                _ => None,
                            })
                            .unwrap_or(false);
                        if !locked {
                            // Snapshot for undo. file_id is Some here (the set was
                            // found via it above), so unwrap_or(0) never triggers.
                            let file_id = file_id.unwrap_or(0);
                            let snapshot = store.get(file_id).cloned();
                            state.annotations.undo_stack().update(|stack| {
                                stack.push_undo(UndoEntry { file_id, snapshot });
                            });
                            // Store original bounds
                            if let Some(a) = set.annotations.iter().find(|a| a.id == *ann_id) {
                                if let crate::annotations::AnnotationKind::Region(ref r) = a.kind {
                                    state.annotations.drag_original().set(Some((
                                        r.time_start,
                                        r.time_end,
                                        r.freq_low,
                                        r.freq_high,
                                    )));
                                }
                            }
                            state
                                .annotations
                                .drag_handle()
                                .set(Some((ann_id.clone(), handle_pos)));
                            state.interaction.is_dragging().set(true);
                            ev.prevent_default();
                            return;
                        }
                    }
                }
            }
        }
    }

    // Check for spec handle drag — hit-test at touch position
    if let Some((px_x, px_y, _, _)) = pointer_to_xtf(
        touch.client_x() as f64,
        touch.client_y() as f64,
        canvas_ref,
        &state,
    ) {
        if let Some(canvas_el) = canvas_ref.get() {
            let canvas: &HtmlCanvasElement = canvas_el.as_ref();
            let cw = canvas.width() as f64;
            let ch = canvas.height() as f64;
            let files = state.library.files().get_untracked();
            let idx = state.library.current_index().get_untracked();
            let file = idx.and_then(|i| files.get(i));
            let file_max_freq = file.map(|f| f.spectrogram.max_freq).unwrap_or(96_000.0);
            let min_freq_val = state.view.min_display_freq().get_untracked().unwrap_or(0.0);
            let max_freq_val = state
                .view
                .max_display_freq()
                .get_untracked()
                .unwrap_or(file_max_freq);
            let band_ff_focused = state.interaction.active_focus().get_untracked()
                == Some(ActiveFocus::FrequencyFocus);
            let handle = hit_test_spec_handles(
                &state,
                px_y,
                min_freq_val,
                max_freq_val,
                ch,
                16.0,
                band_ff_focused, // wider touch target
            );
            if let Some(handle) = handle {
                let is_ff = matches!(
                    handle,
                    SpectrogramHandle::BandFfUpper
                        | SpectrogramHandle::BandFfLower
                        | SpectrogramHandle::BandFfMiddle
                );
                if !is_ff || is_in_band_ff_drag_zone(px_x, cw) {
                    state.interaction.spec_drag_handle().set(Some(handle));
                    state.interaction.is_dragging().set(true);
                    ev.prevent_default();
                    return;
                }
            }
        }
    }

    // Corner drag gone — see pointer variant above.

    // Left-axis freq-pan zone retired (see on_pointerdown) — touch on the left
    // edge falls through to the normal pan/drag below.

    // Time-axis touch interactions (drag-to-select, double-tap) moved to
    // the <TimeGutter/> strip.

    match state.interaction.canvas_tool().get_untracked() {
        CanvasTool::Hand => {
            ev.prevent_default();
            state.interaction.is_dragging().set(true);
            ix.hand_drag_start.set((
                touch.client_x() as f64,
                state.view.scroll_offset().get_untracked(),
            ));
        }
        CanvasTool::Selection => {
            ev.prevent_default();
            // Begin a marquee selection with a single finger (mirrors the
            // pointer path so touch devices can select regions too).
            if let Some((_, _, t, f)) = pointer_to_xtf(
                touch.client_x() as f64,
                touch.client_y() as f64,
                canvas_ref,
                &state,
            ) {
                state.interaction.is_dragging().set(true);
                ix.drag_start.set((t, f));
                state.interaction.selection().set(None);
            }
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
    if state.status.viewport_zoomed().get_untracked() {
        return;
    }

    let touches = ev.touches();
    let n = touches.length();

    // Two-finger pinch — zoom time OR frequency, snapped to the dominant axis.
    if n == 2 {
        if let Some(ps) = ix.pinch_state.get_untracked() {
            ev.prevent_default();
            use crate::components::pinch::{
                apply_freq_pinch, apply_pinch, two_finger_axes, FreqPinchState, PinchAxis,
            };
            if let Some((mid_x, mid_y, dist_x, dist_y)) = two_finger_axes(&touches) {
                let Some(canvas_el) = canvas_ref.get() else {
                    return;
                };
                let canvas: &HtmlCanvasElement = canvas_el.as_ref();
                let rect = canvas.get_bounding_client_rect();
                let cw = canvas.width() as f64;
                let ch = canvas.height() as f64;

                // Decide & lock the zoom axis on the first significant spread —
                // whichever direction the fingers moved apart more wins, then
                // stays locked for the rest of the gesture.
                let axis = match ix.pinch_axis.get_untracked() {
                    Some(a) => Some(a),
                    None => {
                        const LOCK_PX: f64 = 10.0;
                        let decided = crate::components::pinch::decide_pinch_axis(
                            ps.initial_dist_x,
                            ps.initial_dist_y,
                            dist_x,
                            dist_y,
                            LOCK_PX,
                        );
                        if let Some(a) = decided {
                            ix.pinch_axis.set(Some(a));
                        }
                        decided
                    }
                };

                let waterfall_active = (state.mic.recording().get_untracked()
                    || state.mic.listening().get_untracked())
                    && crate::canvas::live_waterfall::is_active();

                match axis {
                    Some(PinchAxis::Horizontal) => {
                        let (new_zoom, new_scroll) =
                            apply_pinch(&ps, dist_x, mid_x, rect.left(), cw);
                        if waterfall_active {
                            // Live: zoom, but never jump to the start or slip into
                            // empty space. If we were following the live edge,
                            // stay butted against it and keep scrolling; if we
                            // were parked in history, anchor-zoom in place — and
                            // in both cases clamp inside the retained buffer.
                            state.view.zoom_level().set(new_zoom);
                            let now = crate::canvas::live_waterfall::total_time();
                            let oldest = crate::canvas::live_waterfall::oldest_time();
                            let visible_time = viewport::visible_time(cw, new_zoom, ps.time_res);
                            let edge = (now - visible_time).max(oldest).max(0.0);
                            if ps.was_following {
                                state.mic.recording_target_scroll().set(edge);
                                state.view.scroll_offset().set(edge);
                                state.resume_waterfall_follow();
                            } else {
                                let lo = oldest.min(edge);
                                state.view.scroll_offset().set(new_scroll.clamp(lo, edge));
                                state.suspend_waterfall_follow(2000.0);
                            }
                        } else {
                            state.suspend_follow();
                            state.view.zoom_level().set(new_zoom);
                            state.view.scroll_offset().set(new_scroll);
                        }
                    }
                    Some(PinchAxis::Vertical) => {
                        // Vertical pinch → frequency zoom (same anchor-zoom +
                        // two-finger pan as the band gutter). Doesn't touch
                        // scroll, so live follow just needs to be kept in the
                        // state it was already in (don't yank a parked view back
                        // to the edge).
                        let fps = FreqPinchState {
                            initial_dist_y: ps.initial_dist_y,
                            initial_min_freq: ps.initial_min_freq,
                            initial_max_freq: ps.initial_max_freq,
                            initial_mid_canvas_y: ps.initial_mid_client_y - rect.top(),
                            nyquist: ps.nyquist,
                        };
                        let current_mid_canvas_y = mid_y - rect.top();
                        let (new_min, new_max) =
                            apply_freq_pinch(&fps, dist_y, current_mid_canvas_y, ch);
                        state.view.min_display_freq().set(Some(new_min));
                        state.view.max_display_freq().set(Some(new_max));
                        if waterfall_active && !ps.was_following {
                            state.suspend_waterfall_follow(2000.0);
                        }
                    }
                    None => {}
                }
            }
        }
        return;
    }

    if n != 1 {
        return;
    }
    let touch = touches.get(0).unwrap();

    if !state.interaction.is_dragging().get_untracked() {
        return;
    }
    ev.prevent_default();

    // Spec handle drag takes priority
    if let Some(handle) = state.interaction.spec_drag_handle().get_untracked() {
        if let Some((_, px_y, _, _)) = pointer_to_xtf(
            touch.client_x() as f64,
            touch.client_y() as f64,
            canvas_ref,
            &state,
        ) {
            if let Some((freq_at_touch, file_max_freq)) =
                resolve_freq_at_pointer(px_y, canvas_ref, state)
            {
                apply_handle_drag(state, handle, freq_at_touch, file_max_freq);
            }
        }
        return;
    }

    // Annotation resize handle drag
    if let Some((ref ann_id, handle_pos)) = state.annotations.drag_handle().get_untracked() {
        if let Some((_, _, t, f)) = pointer_to_xtf(
            touch.client_x() as f64,
            touch.client_y() as f64,
            canvas_ref,
            &state,
        ) {
            apply_annotation_resize(state, ann_id.clone(), handle_pos, t, f);
        }
        return;
    }

    match state.interaction.canvas_tool().get_untracked() {
        CanvasTool::Hand => {
            apply_hand_pan(
                state,
                touch.client_x() as f64,
                canvas_ref,
                ix.hand_drag_start.get_untracked(),
            );
            // Record velocity sample for inertia
            let now = web_sys::window().unwrap().performance().unwrap().now();
            ix.velocity_tracker
                .update_value(|t| t.push(now, touch.client_x() as f64));
        }
        CanvasTool::Selection => {
            // Grow the marquee from the touch-start anchor (mirrors pointer).
            if let Some((_, _, t, f)) = pointer_to_xtf(
                touch.client_x() as f64,
                touch.client_y() as f64,
                canvas_ref,
                &state,
            ) {
                let (t0, f0) = ix.drag_start.get_untracked();
                state.interaction.selection().set(Some(Selection {
                    time_start: t0.min(t),
                    time_end: t0.max(t),
                    freq_low: Some(f0.min(f)),
                    freq_high: Some(f0.max(f)),
                }));
            }
        }
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
        ix.pinch_axis.set(None);
    }

    // One finger remains after pinch — re-anchor pan to avoid jump
    if remaining == 1 {
        if let Some(touch) = ev.touches().get(0) {
            ix.hand_drag_start.set((
                touch.client_x() as f64,
                state.view.scroll_offset().get_untracked(),
            ));
            if state.interaction.canvas_tool().get_untracked() == CanvasTool::Hand {
                state.interaction.is_dragging().set(true);
            }
        }
        return;
    }

    if remaining == 0 {
        if state
            .interaction
            .spec_drag_handle()
            .get_untracked()
            .is_some()
        {
            state.interaction.spec_drag_handle().set(None);
            state.interaction.is_dragging().set(false);
            return;
        }
        // End annotation resize handle drag
        if let Some((ref ann_id, _)) = state.annotations.drag_handle().get_untracked() {
            if let Some(file_id) = state.current_file_id() {
                let now = js_sys::Date::new_0()
                    .to_iso_string()
                    .as_string()
                    .unwrap_or_default();
                state.annotations.store().update(|store| {
                    if let Some(set) = store.get_mut(file_id) {
                        if let Some(a) = set.annotations.iter_mut().find(|a| a.id == *ann_id) {
                            a.modified_at = now;
                        }
                    }
                });
            }
            state.annotations.dirty().set(true);
            state.annotations.drag_handle().set(None);
            state.annotations.drag_original().set(None);
            state.interaction.is_dragging().set(false);
            return;
        }
        state.interaction.is_dragging().set(false);

        // Hand tool: bookmark on tap (no significant drag) while playing, or launch inertia
        if state.interaction.canvas_tool().get_untracked() == CanvasTool::Hand {
            if let Some(touch) = ev.changed_touches().get(0) {
                let (start_x, _) = ix.hand_drag_start.get_untracked();
                let dx = (touch.client_x() as f64 - start_x).abs();
                if dx < 5.0 && state.playback.is_playing().get_untracked() {
                    let t = state.playback.playhead_time().get_untracked();
                    state
                        .viewmode
                        .bookmarks()
                        .update(|bm| bm.push(crate::state::Bookmark { time: t }));
                } else if dx >= 5.0 {
                    // Flick → launch inertia
                    let velocity = ix.velocity_tracker.with_value(|t| t.velocity_px_per_sec());
                    if let Some(canvas_el) = canvas_ref.get() {
                        let canvas: &HtmlCanvasElement = canvas_el.as_ref();
                        let cw = canvas.width() as f64;
                        let files = state.library.files().get_untracked();
                        let idx = state.library.current_index().get_untracked();
                        let file = idx.and_then(|i| files.get(i));
                        let timeline = state.timeline.active().get_untracked();
                        let time_res = if let Some(ref tl) = timeline {
                            tl.segments
                                .first()
                                .and_then(|s| files.get(s.file_index))
                                .map(|f| f.spectrogram.time_resolution)
                                .unwrap_or(1.0)
                        } else {
                            file.as_ref()
                                .map(|f| f.spectrogram.time_resolution)
                                .unwrap_or(1.0)
                        };
                        let duration = if let Some(ref tl) = timeline {
                            tl.total_duration_secs
                        } else {
                            file.as_ref()
                                .map(|f| f.audio.duration_secs)
                                .unwrap_or(f64::MAX)
                        };
                        let from_here_mode =
                            state.playback.start_mode().get_untracked().uses_from_here();
                        crate::components::inertia::start_inertia(
                            state,
                            velocity,
                            cw,
                            time_res,
                            duration,
                            from_here_mode,
                            ix.inertia_generation,
                        );
                    }
                }
            }
        }

        // Finalize a marquee selection: keep it only if it has real width,
        // focus it (so the "..." menu appears), and optionally drive the
        // BandFF range from it. Mirrors on_pointerup.
        if state.interaction.canvas_tool().get_untracked() == CanvasTool::Selection {
            if let Some(sel) = state.interaction.selection().get_untracked() {
                if sel.time_end - sel.time_start > 0.0001 {
                    state
                        .interaction
                        .active_focus()
                        .set(Some(ActiveFocus::TransientSelection));
                    if state.annotations.selection_auto_focus().get_untracked() {
                        if let (Some(lo), Some(hi)) = (sel.freq_low, sel.freq_high) {
                            if hi - lo > 100.0 {
                                state.set_band_ff_range(lo, hi);
                            }
                        }
                    }
                } else {
                    state.interaction.selection().set(None);
                }
            }
        }

        // Track last tap time/position (used for double-tap detection on axes)
        if let Some(touch) = ev.changed_touches().get(0) {
            if let Some((px_x, px_y, _, _)) = pointer_to_xtf(
                touch.client_x() as f64,
                touch.client_y() as f64,
                canvas_ref,
                &state,
            ) {
                ix.last_tap_time.set(js_sys::Date::now());
                ix.last_tap_x.set(px_x);
                ix.last_tap_y.set(px_y);
            }
        }
    }
}

pub fn on_wheel(ev: web_sys::WheelEvent, state: AppState) {
    ev.prevent_default();

    // Resolve file_max_freq / time_res: prefer waterfall params when active,
    // so scroll/zoom work during listening/recording without a file.
    let is_mic_active =
        state.mic.recording().get_untracked() || state.mic.listening().get_untracked();
    let wf_active = is_mic_active && crate::canvas::live_waterfall::is_active();

    if ev.shift_key() {
        // Shift+scroll: vertical freq zoom around mouse position
        let file_max_freq = if wf_active {
            crate::canvas::live_waterfall::max_freq()
        } else {
            let files = state.library.files().get_untracked();
            let idx = state.library.current_index().get_untracked();
            idx.and_then(|i| files.get(i))
                .map(|f| f.spectrogram.max_freq)
                .unwrap_or(96_000.0)
        };
        let cur_max = state
            .view
            .max_display_freq()
            .get_untracked()
            .unwrap_or(file_max_freq);
        let cur_min = state.view.min_display_freq().get_untracked().unwrap_or(0.0);
        let range = cur_max - cur_min;
        if range < 1.0 {
            return;
        }

        let anchor_frac = if let Some(mf) = state.interaction.mouse_freq().get_untracked() {
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

        state.view.min_display_freq().set(Some(new_min));
        state.view.max_display_freq().set(Some(new_max));
    } else if ev.ctrl_key() {
        let delta = if ev.delta_y() > 0.0 { 0.9 } else { 1.1 };
        let new_zoom = (state.view.zoom_level().get_untracked() * delta)
            .clamp(viewport::MIN_ZOOM, viewport::MAX_ZOOM);
        state.view.zoom_level().set(new_zoom);
        // Live: if we were already following the live edge, stay butted against
        // it as we zoom (clamped inside the retained buffer) instead of drifting
        // off the right or showing empty space.
        let following = state.mic.scroll_user_pan_until().get_untracked() <= js_sys::Date::now();
        if wf_active && following {
            let canvas_w = state.viewmode.spectrogram_canvas_width().get_untracked();
            let time_res = crate::canvas::live_waterfall::time_resolution();
            let now = crate::canvas::live_waterfall::total_time();
            let oldest = crate::canvas::live_waterfall::oldest_time();
            let visible_time = viewport::visible_time(canvas_w, new_zoom, time_res);
            let edge = (now - visible_time).max(oldest).max(0.0);
            state.mic.recording_target_scroll().set(edge);
            state.view.scroll_offset().set(edge);
        }
    } else {
        let raw_delta = ev.delta_y() + ev.delta_x();
        let files = state.library.files().get_untracked();
        let timeline = state.timeline.active().get_untracked();
        let (time_res, duration) = if wf_active {
            let tr = crate::canvas::live_waterfall::time_resolution();
            let dur = crate::canvas::live_waterfall::total_columns() as f64 * tr;
            (tr, dur)
        } else if let Some(ref tl) = timeline {
            let tr = tl
                .segments
                .first()
                .and_then(|s| files.get(s.file_index))
                .map(|f| f.spectrogram.time_resolution)
                .unwrap_or(1.0);
            (tr, tl.total_duration_secs)
        } else {
            let idx = state.library.current_index().get_untracked().unwrap_or(0);
            match files.get(idx) {
                Some(file) => (file.spectrogram.time_resolution, file.audio.duration_secs),
                None => return,
            }
        };
        {
            let zoom = state.view.zoom_level().get_untracked();
            let canvas_w = state.viewmode.spectrogram_canvas_width().get_untracked();
            let visible_time = viewport::visible_time(canvas_w, zoom, time_res);
            let from_here_mode = state.playback.start_mode().get_untracked().uses_from_here();
            // Scroll proportional to visible time (like arrow keys),
            // normalized so a typical wheel tick (~100px) scrolls ~10% of the view
            let delta =
                raw_delta.signum() * visible_time * 0.1 * (raw_delta.abs() / 100.0).min(3.0);
            state.suspend_follow();
            state.view.scroll_offset().update(|s| {
                *s = viewport::clamp_scroll_for_mode(
                    *s + delta,
                    duration,
                    visible_time,
                    from_here_mode,
                );
            });
        }
    }
}
