//! Pinch-to-zoom gesture helpers shared across all canvas components.

use crate::viewport;

/// Snapshot of display-freq state at the moment a 2-finger touch begins
/// on the band gutter — drives vertical pinch-to-zoom + two-finger pan
/// of `min_display_freq` / `max_display_freq` on the host view.
#[derive(Clone, Copy, Debug)]
pub struct FreqPinchState {
    /// Pixel y-distance between the two fingers at gesture start.
    pub initial_dist_y: f64,
    /// Resolved min_display_freq at gesture start.
    pub initial_min_freq: f64,
    /// Resolved max_display_freq at gesture start.
    pub initial_max_freq: f64,
    /// Gutter-canvas-local y of the midpoint at gesture start.
    pub initial_mid_canvas_y: f64,
    /// File Nyquist. new_min/new_max are clamped to [0, nyquist].
    pub nyquist: f64,
}

/// Two-finger y-geometry on a touch list — (midpoint_client_y, |y1 - y0|).
pub fn two_finger_y_geometry(touches: &web_sys::TouchList) -> Option<(f64, f64)> {
    if touches.length() != 2 {
        return None;
    }
    let t0 = touches.get(0)?;
    let t1 = touches.get(1)?;
    let y0 = t0.client_y() as f64;
    let y1 = t1.client_y() as f64;
    let mid_y = (y0 + y1) / 2.0;
    let dist = (y1 - y0).abs();
    Some((mid_y, dist))
}

/// Given a freq-pinch snapshot and current gesture geometry, compute
/// (new_min, new_max) display frequencies. The frequency under the
/// initial midpoint stays pinned to the current midpoint y, which
/// combines anchor-zoom + two-finger vertical pan in one formula.
pub fn apply_freq_pinch(
    ps: &FreqPinchState,
    current_dist_y: f64,
    current_mid_canvas_y: f64,
    canvas_h: f64,
) -> (f64, f64) {
    if canvas_h <= 0.0 || ps.initial_dist_y < 5.0 {
        return (ps.initial_min_freq, ps.initial_max_freq);
    }
    let initial_range = (ps.initial_max_freq - ps.initial_min_freq).max(1.0);

    // Larger finger-gap → narrower visible range (zoom in).
    let scale = ps.initial_dist_y / current_dist_y.max(1.0);
    let min_range_hz = 500.0_f64.min(ps.nyquist.max(500.0));
    let new_range = (initial_range * scale).clamp(min_range_hz, ps.nyquist.max(min_range_hz));

    // Anchor: freq under the initial midpoint y.
    let initial_mid_frac = (ps.initial_mid_canvas_y / canvas_h).clamp(0.0, 1.0);
    let anchor_freq = ps.initial_max_freq - initial_mid_frac * initial_range;

    // Place that freq at the CURRENT midpoint y — this handles both zoom
    // (scale change) and two-finger pan (midpoint shift) simultaneously.
    let current_mid_frac = (current_mid_canvas_y / canvas_h).clamp(0.0, 1.0);
    let mut new_max = anchor_freq + current_mid_frac * new_range;
    let mut new_min = new_max - new_range;

    if new_min < 0.0 {
        new_min = 0.0;
        new_max = new_range.min(ps.nyquist);
    }
    if new_max > ps.nyquist {
        new_max = ps.nyquist;
        new_min = (new_max - new_range).max(0.0);
    }
    (new_min, new_max)
}

/// Which axis a spectrogram pinch is zooming. Decided once per gesture (the
/// direction the fingers spread more) and then locked, so a pinch snaps to a
/// single kind of zoom instead of jittering between time and frequency.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PinchAxis {
    /// Horizontal pinch → time (zoom_level / scroll).
    Horizontal,
    /// Vertical pinch → frequency (min/max_display_freq).
    Vertical,
}

/// Snapshot of state at the moment a 2-finger touch begins.
///
/// Carries BOTH the horizontal (time) and vertical (frequency) anchors so the
/// gesture can snap to whichever axis the fingers spread more.
#[derive(Clone, Copy, Debug)]
pub struct PinchState {
    /// Horizontal pixel separation between the two fingers at gesture start.
    pub initial_dist_x: f64,
    /// Vertical pixel separation between the two fingers at gesture start.
    pub initial_dist_y: f64,
    /// zoom_level at gesture start.
    pub initial_zoom: f64,
    /// scroll_offset (seconds) at gesture start.
    pub initial_scroll: f64,
    /// Midpoint X in client coordinates at gesture start.
    pub initial_mid_client_x: f64,
    /// Midpoint Y in client coordinates at gesture start.
    pub initial_mid_client_y: f64,
    /// Resolved min_display_freq at gesture start (for vertical zoom).
    pub initial_min_freq: f64,
    /// Resolved max_display_freq at gesture start (for vertical zoom).
    pub initial_max_freq: f64,
    /// File/live Nyquist — new_min/new_max are clamped to [0, nyquist].
    pub nyquist: f64,
    /// Seconds per FFT column.
    pub time_res: f64,
    /// File duration in seconds (for scroll clamping).
    pub duration: f64,
    /// Whether FromHere viewport bounds should be used.
    pub from_here_mode: bool,
    /// During live capture: was the view following the live edge when the pinch
    /// began? If so, keep it butted against the edge while zooming; otherwise
    /// anchor-zoom in place. Always false for non-live pinches.
    pub was_following: bool,
}

impl PinchState {
    /// Build a time-only pinch snapshot for views that zoom the time axis only
    /// (waveform, ZC chart, chromagram). The vertical/frequency fields are inert
    /// — only `apply_pinch` (horizontal) reads this.
    #[allow(clippy::too_many_arguments)]
    pub fn horizontal(
        initial_dist_x: f64,
        initial_zoom: f64,
        initial_scroll: f64,
        initial_mid_client_x: f64,
        time_res: f64,
        duration: f64,
        from_here_mode: bool,
    ) -> Self {
        Self {
            initial_dist_x,
            initial_dist_y: 1.0,
            initial_zoom,
            initial_scroll,
            initial_mid_client_x,
            initial_mid_client_y: 0.0,
            initial_min_freq: 0.0,
            initial_max_freq: 1.0,
            nyquist: 1.0,
            time_res,
            duration,
            from_here_mode,
            was_following: false,
        }
    }
}

/// Returns (midpoint_client_x, distance) for exactly 2 touches.
pub fn two_finger_geometry(touches: &web_sys::TouchList) -> Option<(f64, f64)> {
    if touches.length() != 2 {
        return None;
    }
    let t0 = touches.get(0)?;
    let t1 = touches.get(1)?;
    let x0 = t0.client_x() as f64;
    let x1 = t1.client_x() as f64;
    let y0 = t0.client_y() as f64;
    let y1 = t1.client_y() as f64;
    let mid_x = (x0 + x1) / 2.0;
    let dist = ((x1 - x0).powi(2) + (y1 - y0).powi(2)).sqrt();
    Some((mid_x, dist))
}

/// Decide which axis a spectrogram pinch should zoom from the per-axis finger
/// separations at gesture start vs now. Returns `None` until the fingers have
/// spread (in either axis) by at least `lock_px` — too little movement to tell
/// intent — then the axis that changed more. The caller locks the first `Some`
/// for the rest of the gesture so the zoom snaps to one axis.
pub fn decide_pinch_axis(
    initial_dist_x: f64,
    initial_dist_y: f64,
    current_dist_x: f64,
    current_dist_y: f64,
    lock_px: f64,
) -> Option<PinchAxis> {
    let dx_change = (current_dist_x - initial_dist_x).abs();
    let dy_change = (current_dist_y - initial_dist_y).abs();
    if dx_change.max(dy_change) < lock_px {
        return None;
    }
    Some(if dx_change >= dy_change {
        PinchAxis::Horizontal
    } else {
        PinchAxis::Vertical
    })
}

/// Two-finger geometry split per axis: `(mid_x, mid_y, dist_x, dist_y)` in
/// client coordinates, where `dist_x`/`dist_y` are the absolute horizontal and
/// vertical finger separations. Used to pick the dominant pinch axis.
pub fn two_finger_axes(touches: &web_sys::TouchList) -> Option<(f64, f64, f64, f64)> {
    if touches.length() != 2 {
        return None;
    }
    let t0 = touches.get(0)?;
    let t1 = touches.get(1)?;
    let x0 = t0.client_x() as f64;
    let x1 = t1.client_x() as f64;
    let y0 = t0.client_y() as f64;
    let y1 = t1.client_y() as f64;
    let mid_x = (x0 + x1) / 2.0;
    let mid_y = (y0 + y1) / 2.0;
    Some((mid_x, mid_y, (x1 - x0).abs(), (y1 - y0).abs()))
}

/// Given a pinch state snapshot and current gesture geometry, compute (new_zoom, new_scroll).
///
/// Anchor-point zoom: the time under the initial midpoint stays fixed as fingers spread/contract.
/// Two-finger pan: horizontal midpoint movement also translates scroll_offset.
pub fn apply_pinch(
    pinch: &PinchState,
    current_dist: f64,
    current_mid_client_x: f64,
    canvas_left: f64,
    canvas_width: f64,
) -> (f64, f64) {
    if canvas_width == 0.0 || pinch.initial_dist_x < 1.0 {
        return (pinch.initial_zoom, pinch.initial_scroll);
    }

    // Zoom proportional to horizontal finger-separation ratio
    let scale = current_dist / pinch.initial_dist_x;
    let new_zoom = (pinch.initial_zoom * scale).clamp(viewport::MIN_ZOOM, viewport::MAX_ZOOM);

    // What time was under the initial midpoint?
    let initial_visible_time =
        viewport::visible_time(canvas_width, pinch.initial_zoom, pinch.time_res);
    let initial_mid_canvas_x = pinch.initial_mid_client_x - canvas_left;
    let mid_frac = (initial_mid_canvas_x / canvas_width).clamp(0.0, 1.0);
    let anchor_time = pinch.initial_scroll + mid_frac * initial_visible_time;

    // New visible time at new zoom
    let new_visible_time = viewport::visible_time(canvas_width, new_zoom, pinch.time_res);

    // Scroll so anchor_time stays at the same screen fraction
    let scroll_from_anchor = anchor_time - mid_frac * new_visible_time;

    // Two-finger pan: midpoint shift → time shift
    let mid_shift_px = current_mid_client_x - pinch.initial_mid_client_x;
    let pan_dt = -(mid_shift_px / canvas_width) * new_visible_time;

    let raw_scroll = scroll_from_anchor + pan_dt;
    let new_scroll = viewport::clamp_scroll_for_mode(
        raw_scroll,
        pinch.duration,
        new_visible_time,
        pinch.from_here_mode,
    );

    (new_zoom, new_scroll)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Axis snapping ──────────────────────────────────────────────────────

    #[test]
    fn axis_undecided_below_threshold() {
        // Fingers barely moved (≤ lock_px in both axes) → no axis yet.
        assert_eq!(decide_pinch_axis(100.0, 100.0, 105.0, 103.0, 10.0), None);
    }

    #[test]
    fn axis_picks_dominant_spread_direction() {
        // Horizontal spread (Δ30) beats vertical (Δ5).
        assert_eq!(
            decide_pinch_axis(100.0, 100.0, 130.0, 105.0, 10.0),
            Some(PinchAxis::Horizontal)
        );
        // Vertical spread (Δ40) beats horizontal (Δ5).
        assert_eq!(
            decide_pinch_axis(100.0, 100.0, 105.0, 140.0, 10.0),
            Some(PinchAxis::Vertical)
        );
    }

    #[test]
    fn axis_counts_contraction_too() {
        // Pinching IN horizontally (separation shrinks) still locks horizontal.
        assert_eq!(
            decide_pinch_axis(100.0, 100.0, 78.0, 100.0, 10.0),
            Some(PinchAxis::Horizontal)
        );
    }

    #[test]
    fn axis_ties_break_horizontal() {
        // Equal spread in both → horizontal wins (>=).
        assert_eq!(
            decide_pinch_axis(100.0, 100.0, 120.0, 120.0, 10.0),
            Some(PinchAxis::Horizontal)
        );
    }

    // ── Time (horizontal) zoom ─────────────────────────────────────────────

    fn time_pinch() -> PinchState {
        // 100px initial separation, zoom 1.0, scroll 0, mid at left edge,
        // 1ms/col, long file.
        PinchState::horizontal(100.0, 1.0, 0.0, 0.0, 0.001, 1000.0, false)
    }

    #[test]
    fn time_zoom_scales_with_horizontal_separation() {
        let ps = time_pinch();
        // Spread apart → zoom in.
        let (zoom_in, _) = apply_pinch(&ps, 200.0, 0.0, 0.0, 800.0);
        assert!(
            zoom_in > ps.initial_zoom,
            "spreading should zoom in: {zoom_in}"
        );
        // Pinch together → zoom out.
        let (zoom_out, _) = apply_pinch(&ps, 50.0, 0.0, 0.0, 800.0);
        assert!(
            zoom_out < ps.initial_zoom,
            "contracting should zoom out: {zoom_out}"
        );
    }

    #[test]
    fn time_zoom_noop_when_unchanged() {
        let ps = time_pinch();
        // Same separation + same midpoint → zoom & scroll unchanged.
        let (zoom, scroll) = apply_pinch(&ps, 100.0, 0.0, 0.0, 800.0);
        assert!((zoom - ps.initial_zoom).abs() < 1e-9);
        assert!((scroll - ps.initial_scroll).abs() < 1e-9);
    }

    // ── Frequency (vertical) zoom ──────────────────────────────────────────

    fn freq_pinch() -> FreqPinchState {
        // Full 0..96k range, mid of a 200px-tall canvas, 100px finger gap.
        FreqPinchState {
            initial_dist_y: 100.0,
            initial_min_freq: 0.0,
            initial_max_freq: 96_000.0,
            initial_mid_canvas_y: 100.0,
            nyquist: 96_000.0,
        }
    }

    #[test]
    fn freq_spread_narrows_range_about_anchor() {
        let ps = freq_pinch();
        // Spread apart (100 → 200px) halves the visible range, centered.
        let (min, max) = apply_freq_pinch(&ps, 200.0, 100.0, 200.0);
        let range = max - min;
        assert!(
            range < (ps.initial_max_freq - ps.initial_min_freq),
            "should zoom in: {range}"
        );
        // Anchor freq (under the midpoint) stays centered.
        assert!(
            ((min + max) / 2.0 - 48_000.0).abs() < 1.0,
            "anchor drifted: {min}..{max}"
        );
    }

    #[test]
    fn freq_zoom_clamps_to_nyquist() {
        let ps = freq_pinch();
        // Pinch together hard → range would exceed Nyquist; clamps to 0..nyquist.
        let (min, max) = apply_freq_pinch(&ps, 25.0, 100.0, 200.0);
        assert!(min >= 0.0 && max <= ps.nyquist + 1.0);
        assert!(
            (max - min - ps.nyquist).abs() < 1.0,
            "should clamp to full range: {min}..{max}"
        );
    }
}
