// SPDX-License-Identifier: GPL-3.0-only OR MIT OR Apache-2.0
pub const PLAY_FROM_HERE_FRACTION: f64 = 0.10;
pub const FOLLOW_CURSOR_FRACTION: f64 = 0.20;
pub const FOLLOW_CURSOR_EDGE_FRACTION: f64 = 0.80;

/// Maximum zoom level (pixels per baseline column).
/// At zoom=512 the waveform shows ~1 pixel per sample; 2000 gives headroom.
pub const MAX_ZOOM: f64 = 2000.0;
pub const MIN_ZOOM: f64 = 0.02;

/// When the visible window is shorter than this (seconds), follow the cursor
/// every frame instead of waiting for the 80% edge trigger.
pub const FOLLOW_EXACT_THRESHOLD_SECS: f64 = 0.5;

pub fn uses_from_here_bounds(enabled: bool) -> bool {
    enabled
}

pub fn visible_time(canvas_width: f64, zoom: f64, time_resolution: f64) -> f64 {
    if canvas_width <= 0.0 || zoom <= 0.0 || time_resolution <= 0.0 {
        0.0
    } else {
        (canvas_width / zoom) * time_resolution
    }
}

pub fn play_from_here_time(scroll_offset: f64, visible_time: f64) -> f64 {
    scroll_offset + visible_time * PLAY_FROM_HERE_FRACTION
}

pub fn scroll_for_play_from_here(target_time: f64, visible_time: f64) -> f64 {
    target_time - visible_time * PLAY_FROM_HERE_FRACTION
}

pub fn scroll_bounds(duration: f64, visible_time: f64) -> (f64, f64) {
    if visible_time <= 0.0 {
        return (0.0, duration.max(0.0));
    }

    let lead_in = visible_time * PLAY_FROM_HERE_FRACTION;
    let min_scroll = -lead_in;
    let max_scroll = (duration - lead_in).max(min_scroll);
    (min_scroll, max_scroll)
}

pub fn standard_scroll_bounds(duration: f64, visible_time: f64) -> (f64, f64) {
    if visible_time <= 0.0 {
        return (0.0, duration.max(0.0));
    }
    (0.0, (duration - visible_time).max(0.0))
}

pub fn scroll_bounds_for_mode(
    duration: f64,
    visible_time: f64,
    from_here_mode: bool,
) -> (f64, f64) {
    if uses_from_here_bounds(from_here_mode) {
        scroll_bounds(duration, visible_time)
    } else {
        standard_scroll_bounds(duration, visible_time)
    }
}

pub fn clamp_scroll(scroll_offset: f64, duration: f64, visible_time: f64) -> f64 {
    let (min_scroll, max_scroll) = scroll_bounds(duration, visible_time);
    scroll_offset.clamp(min_scroll, max_scroll)
}

pub fn clamp_scroll_for_mode(
    scroll_offset: f64,
    duration: f64,
    visible_time: f64,
    from_here_mode: bool,
) -> f64 {
    let (min_scroll, max_scroll) = scroll_bounds_for_mode(duration, visible_time, from_here_mode);
    scroll_offset.clamp(min_scroll, max_scroll)
}

pub fn data_window(scroll_offset: f64, visible_time: f64, duration: f64) -> Option<(f64, f64)> {
    if visible_time <= 0.0 || duration <= 0.0 {
        return None;
    }

    let data_start = scroll_offset.max(0.0);
    let data_end = (scroll_offset + visible_time).min(duration);
    if data_end <= data_start {
        None
    } else {
        Some((data_start, data_end))
    }
}

/// Compute zoom level for live recording so that a comfortable duration is visible.
/// Uses sqrt(canvas_width) scaling: ~8s on mobile (320px), ~15s on desktop (1200px).
pub fn recording_zoom(canvas_width: f64, time_resolution: f64) -> f64 {
    if canvas_width <= 0.0 || time_resolution <= 0.0 {
        return 1.0;
    }
    let target_secs = (canvas_width.sqrt() * 0.45).clamp(8.0, 20.0);
    (canvas_width * time_resolution / target_secs).max(0.005)
}

/// Compute zoom level that fits `duration` seconds into the canvas.
pub fn fit_zoom(canvas_width: f64, time_resolution: f64, duration: f64) -> f64 {
    if canvas_width <= 0.0 || time_resolution <= 0.0 || duration <= 0.0 {
        return 1.0;
    }
    (canvas_width * time_resolution / duration).clamp(MIN_ZOOM, MAX_ZOOM)
}

pub fn data_region_px(
    scroll_offset: f64,
    visible_time: f64,
    duration: f64,
    canvas_width: f64,
) -> Option<(f64, f64, f64, f64)> {
    let (data_start, data_end) = data_window(scroll_offset, visible_time, duration)?;
    let px_per_sec = canvas_width / visible_time;
    let dst_x = (data_start - scroll_offset) * px_per_sec;
    let dst_w = (data_end - data_start) * px_per_sec;
    Some((data_start, data_end, dst_x, dst_w))
}
