/// Pinch-to-zoom gesture helpers shared across all canvas components.

use crate::viewport;

/// Snapshot of state at the moment a 2-finger touch begins.
#[derive(Clone, Copy, Debug)]
pub struct PinchState {
    /// Pixel distance between the two fingers at gesture start.
    pub initial_dist: f64,
    /// zoom_level at gesture start.
    pub initial_zoom: f64,
    /// scroll_offset (seconds) at gesture start.
    pub initial_scroll: f64,
    /// Midpoint X in client coordinates at gesture start.
    pub initial_mid_client_x: f64,
    /// Seconds per FFT column.
    pub time_res: f64,
    /// File duration in seconds (for scroll clamping).
    pub duration: f64,
    /// Whether FromHere viewport bounds should be used.
    pub from_here_mode: bool,
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
    if canvas_width == 0.0 || pinch.initial_dist < 10.0 {
        return (pinch.initial_zoom, pinch.initial_scroll);
    }

    // Zoom proportional to finger distance ratio
    let scale = current_dist / pinch.initial_dist;
    let new_zoom = (pinch.initial_zoom * scale).clamp(0.1, 400.0);

    // What time was under the initial midpoint?
    let initial_visible_time = viewport::visible_time(canvas_width, pinch.initial_zoom, pinch.time_res);
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
