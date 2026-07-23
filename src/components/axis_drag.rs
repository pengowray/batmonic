// Shared helpers that drive HFR band and time selections from the
// gutter strips. Both gutters (see `gutter.rs`) and the ZC-chart's
// y-axis wrap these instead of duplicating the snap/autotoggle logic.

use crate::state::store_fields::*;
use crate::state::{ActiveFocus, AppState, Selection};
use leptos::prelude::*;

/// Frequency-dependent snap increment for y-axis selection.
/// Below 20 kHz: 1 kHz (shift: 2 kHz). At/above 20 kHz: 5 kHz (shift: 10 kHz).
pub fn freq_snap(freq: f64, shift: bool) -> f64 {
    if freq < 20_000.0 {
        if shift {
            2_000.0
        } else {
            1_000.0
        }
    } else {
        if shift {
            10_000.0
        } else {
            5_000.0
        }
    }
}

/// Apply axis drag (frequency range selection driven from a gutter).
/// Updates the shared axis_drag_start/current_freq signals and the
/// committed band_ff_range once the drag exceeds 500 Hz.
pub fn apply_axis_drag(state: AppState, raw_start: f64, freq: f64, shift: bool) {
    // Clamp to non-negative frequencies
    let raw_start = raw_start.max(0.0);
    let freq = freq.max(0.0);
    // Snap each endpoint independently based on its own frequency zone
    let snap_start = freq_snap(raw_start, shift);
    let snap_end = freq_snap(freq, shift);
    let (snapped_start, snapped_end) = if freq > raw_start {
        // Dragging up: start floors down, end ceils up
        (
            (raw_start / snap_start).floor() * snap_start,
            (freq / snap_end).ceil() * snap_end,
        )
    } else if freq < raw_start {
        // Dragging down: start ceils up, end floors down
        (
            (raw_start / snap_start).ceil() * snap_start,
            (freq / snap_end).floor() * snap_end,
        )
    } else {
        let s = (raw_start / snap_start).round() * snap_start;
        (s, s)
    };
    // Ensure snapped values don't go below 0
    let snapped_start = snapped_start.max(0.0);
    let snapped_end = snapped_end.max(0.0);
    state
        .interaction
        .axis_drag_start_freq()
        .set(Some(snapped_start));
    state
        .interaction
        .axis_drag_current_freq()
        .set(Some(snapped_end));
    // Live update BandFF range
    let lo = snapped_start.min(snapped_end);
    let hi = snapped_start.max(snapped_end);
    if hi - lo > 500.0 {
        state.set_band_ff_range(lo, hi);
    }
}

/// Select all frequencies: set BandFF range to 0..Nyquist and enable HFR.
/// Used by double-click / double-tap on the band gutter.
pub fn select_all_frequencies(state: AppState) {
    let is_mic_active =
        state.mic.recording().get_untracked() || state.mic.listening().get_untracked();
    let file_max_freq = if is_mic_active && crate::canvas::live_waterfall::is_active() {
        crate::canvas::live_waterfall::max_freq()
    } else {
        let files = state.library.files().get_untracked();
        let idx = state.library.current_index().get_untracked();
        idx.and_then(|i| files.get(i))
            .map(|f| f.spectrogram.max_freq)
            .unwrap_or(96_000.0)
    };
    state.set_band_ff_range(0.0, file_max_freq);
    let stack = state.viewmode.focus_stack().get_untracked();
    if !stack.hfr_enabled() {
        state.toggle_hfr();
    }
}

/// Select all time: create a full-duration selection.
/// If HFR is active, the selection includes frequency bounds (region);
/// otherwise it's time-only (segment).
/// Used by double-click on the time gutter.
pub fn select_all_time(state: AppState) {
    let duration = if let Some(ref tl) = state.timeline.active().get_untracked() {
        tl.total_duration_secs
    } else {
        let files = state.library.files().get_untracked();
        let idx = state.library.current_index().get_untracked();
        idx.and_then(|i| files.get(i))
            .map(|f| f.audio.duration_secs)
            .unwrap_or(0.0)
    };
    if duration <= 0.0 {
        return;
    }

    let ff = state
        .viewmode
        .focus_stack()
        .get_untracked()
        .effective_range();
    let (fl, fh) = if ff.is_active() {
        (Some(ff.lo), Some(ff.hi))
    } else {
        (None, None)
    };

    state.interaction.selection().set(Some(Selection {
        time_start: 0.0,
        time_end: duration,
        freq_low: fl,
        freq_high: fh,
    }));
    state
        .interaction
        .active_focus()
        .set(Some(ActiveFocus::TransientSelection));
}

/// Finalize axis drag — auto-enable HFR if a meaningful range was
/// selected, or toggle HFR off if it was just a tap (no meaningful drag).
pub fn finalize_axis_drag(state: AppState) {
    let start = state.interaction.axis_drag_start_freq().get_untracked();
    let current = state.interaction.axis_drag_current_freq().get_untracked();
    let was_tap = match (start, current) {
        (Some(s), Some(c)) => (s - c).abs() < 1.0, // start == end means no drag movement
        _ => true,
    };

    if was_tap {
        // Tap on the gutter: toggle HFR off (if on)
        let stack = state.viewmode.focus_stack().get_untracked();
        if stack.hfr_enabled() {
            state.toggle_hfr();
        }
    } else {
        // Drag completed: enable HFR if a meaningful range was selected
        let stack = state.viewmode.focus_stack().get_untracked();
        let range = stack.effective_range_ignoring_hfr();
        if range.hi - range.lo > 500.0 && !stack.hfr_enabled() {
            state.toggle_hfr();
        }
    }
    // Auto-combine: if there's a time-only segment, upgrade to region — only when HFR is on
    if let Some(sel) = state.interaction.selection().get_untracked() {
        if sel.freq_low.is_none()
            && sel.freq_high.is_none()
            && sel.time_end - sel.time_start > 0.0001
        {
            let ff = state
                .viewmode
                .focus_stack()
                .get_untracked()
                .effective_range();
            if ff.is_active() {
                state.interaction.selection().set(Some(Selection {
                    freq_low: Some(ff.lo),
                    freq_high: Some(ff.hi),
                    ..sel
                }));
            }
        }
    }
    // Set focus to BandFF when axis drag creates/modifies BandFF
    if !was_tap {
        state
            .interaction
            .active_focus()
            .set(Some(ActiveFocus::FrequencyFocus));
    }
    state.interaction.axis_drag_start_freq().set(None);
    state.interaction.axis_drag_current_freq().set(None);
    state.interaction.is_dragging().set(false);
}
