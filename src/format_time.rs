// SPDX-License-Identifier: GPL-3.0-only OR MIT OR Apache-2.0
//! Centralized time formatting for the entire app.
//!
//! All time values are in seconds. Two main entry points:
//!
//! - **`format_time_label`** — adaptive precision for canvas timeline labels (compact)
//! - **`format_time_display`** — fixed precision for UI text (annotations, metadata, etc.)

// ── Adaptive-precision (canvas timeline) ────────────────────────────────

/// Format a time value as a compact label for canvas timeline ticks.
///
/// Precision adapts to the tick `interval` (in seconds) so labels aren't
/// over- or under-specified.  `use_ms` forces millisecond notation for
/// sub-second values (should be true only when the max visible time ≤ 100 ms).
pub fn format_time_label(seconds: f64, interval: f64, use_ms: bool) -> String {
    let abs = seconds.abs();

    if abs < 1.0 {
        if use_ms {
            let ms = seconds * 1000.0;
            return if interval < 0.01 {
                format!("{:.1}ms", ms)
            } else {
                format!("{:.0}ms", ms)
            };
        }
        return format_seconds_adaptive(seconds, interval);
    }

    if abs < 60.0 {
        return format_seconds_adaptive(seconds, interval);
    }

    if abs < 3600.0 {
        return format_minutes_seconds_adaptive(seconds, interval);
    }

    format_hours_minutes_seconds_adaptive(seconds, interval)
}

/// Format a relative time offset as "+50ms", "−0.2s", etc.
pub fn format_relative_label(offset: f64, interval: f64) -> String {
    let abs = offset.abs();
    if abs < 0.0005 {
        return String::new();
    }
    let sign = if offset >= 0.0 { "+" } else { "\u{2212}" }; // − (Unicode minus)

    if abs < 1.0 && interval < 0.1 {
        let ms = abs * 1000.0;
        if interval < 0.001 {
            format!("{}{:.1}ms", sign, ms)
        } else {
            format!("{}{:.0}ms", sign, ms)
        }
    } else if interval >= 1.0 {
        format!("{}{:.0}s", sign, abs)
    } else if interval >= 0.1 {
        format!("{}{:.1}s", sign, abs)
    } else {
        format!("{}{:.2}s", sign, abs)
    }
}

// ── Fixed-precision (UI display) ────────────────────────────────────────

/// Format a time position with fixed decimal precision for UI display.
///
/// Examples (precision=3): `5.250s`, `1m30.500s`, `1h05m30.000s`
pub fn format_time_display(seconds: f64, precision: u8) -> String {
    let sign = if seconds < 0.0 { "-" } else { "" };
    let abs = seconds.abs();

    if abs < 60.0 {
        format!("{}{:.prec$}s", sign, abs, prec = precision as usize)
    } else if abs < 3600.0 {
        let mins = (abs / 60.0).floor() as u32;
        let secs = abs - mins as f64 * 60.0;
        format!("{}{}m{:0>width$.prec$}s", sign, mins, secs,
            width = 3 + precision as usize, // "00." = 3 chars + decimals
            prec = precision as usize)
    } else {
        let hours = (abs / 3600.0).floor() as u32;
        let rem = abs - hours as f64 * 3600.0;
        let mins = (rem / 60.0).floor() as u32;
        let secs = rem - mins as f64 * 60.0;
        format!("{}{}h{:02}m{:0>width$.prec$}s", sign, hours, mins, secs,
            width = 3 + precision as usize,
            prec = precision as usize)
    }
}

/// Format a duration (always positive) with fixed precision.
///
/// Same as `format_time_display` but takes the absolute value.
pub fn format_duration(seconds: f64, precision: u8) -> String {
    format_time_display(seconds.abs(), precision)
}

/// Format a duration compactly for UI lists (file lengths, gaps, etc.).
///
/// - Under 90s: `45.1s`
/// - 90s–1h: `5m30s`
/// - Over 1h: `9h44m22s`
pub fn format_duration_compact(seconds: f64) -> String {
    let abs = seconds.abs();
    if abs < 90.0 {
        format!("{abs:.1}s")
    } else if abs < 3600.0 {
        let mins = (abs / 60.0).floor() as u32;
        let secs = (abs - mins as f64 * 60.0).round() as u32;
        if secs == 60 {
            format!("{}m00s", mins + 1)
        } else {
            format!("{mins}m{secs:02}s")
        }
    } else {
        let hours = (abs / 3600.0).floor() as u32;
        let rem = abs - hours as f64 * 3600.0;
        let mins = (rem / 60.0).floor() as u32;
        let secs = (rem - mins as f64 * 60.0).round() as u32;
        if secs == 60 {
            format!("{hours}h{:02}m00s", mins + 1)
        } else {
            format!("{hours}h{mins:02}m{secs:02}s")
        }
    }
}

/// Format a time range as "start–end" (en-dash separated).
///
/// Example: `5.000–10.500s`
pub fn format_time_range(start: f64, end: f64, precision: u8) -> String {
    format!("{}–{}", format_time_display(start, precision), format_time_display(end, precision))
}

// ── GUANO timestamp helper ─────────────────────────────────────────────

/// Build an ISO 8601 timestamp with local timezone offset for a recording
/// that started `duration_secs` ago.
/// Returns e.g. "2024-03-15T10:30:00+10:00".
pub fn recording_timestamp(duration_secs: f64) -> String {
    let now = js_sys::Date::new_0();
    let start_ms = now.get_time() - (duration_secs * 1000.0);
    let start = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(start_ms));

    let tz_offset_minutes = start.get_timezone_offset() as i32;
    let offset_total = -tz_offset_minutes;
    let offset_sign = if offset_total < 0 { '-' } else { '+' };
    let offset_abs = offset_total.unsigned_abs();
    let offset_h = offset_abs / 60;
    let offset_m = offset_abs % 60;
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}{}{:02}:{:02}",
        start.get_full_year(),
        start.get_month() + 1,
        start.get_date(),
        start.get_hours(),
        start.get_minutes(),
        start.get_seconds(),
        offset_sign,
        offset_h,
        offset_m,
    )
}

// ── Private adaptive helpers ────────────────────────────────────────────

fn format_seconds_adaptive(seconds: f64, interval: f64) -> String {
    if interval >= 1.0 {
        format!("{:.0}s", seconds)
    } else if interval >= 0.1 {
        // Drop ".0" when seconds are whole
        let rounded = (seconds * 10.0).round() / 10.0;
        if (rounded - rounded.round()).abs() < 0.01 {
            format!("{:.0}s", rounded)
        } else {
            format!("{:.1}s", seconds)
        }
    } else if interval >= 0.01 {
        format!("{:.2}s", seconds)
    } else {
        format!("{:.3}s", seconds)
    }
}

fn format_minutes_seconds_adaptive(seconds: f64, interval: f64) -> String {
    let sign = if seconds < 0.0 { "-" } else { "" };
    let abs = seconds.abs();
    let mins = (abs / 60.0).floor() as u32;
    let secs = abs - mins as f64 * 60.0;
    if interval >= 1.0 {
        format!("{}{}m{:02.0}s", sign, mins, secs)
    } else if interval >= 0.1 {
        // Drop ".0" when seconds are whole
        let rounded = (secs * 10.0).round() / 10.0;
        if (rounded - rounded.round()).abs() < 0.01 {
            format!("{}{}m{:02.0}s", sign, mins, rounded)
        } else {
            format!("{}{}m{:04.1}s", sign, mins, secs)
        }
    } else {
        format!("{}{}m{:06.3}s", sign, mins, secs)
    }
}

fn format_hours_minutes_seconds_adaptive(seconds: f64, interval: f64) -> String {
    let sign = if seconds < 0.0 { "-" } else { "" };
    let abs = seconds.abs();
    let hours = (abs / 3600.0).floor() as u32;
    let rem = abs - hours as f64 * 3600.0;
    let mins = (rem / 60.0).floor() as u32;
    let secs = rem - mins as f64 * 60.0;
    if interval >= 1.0 {
        format!("{}{}h{:02}m{:02.0}s", sign, hours, mins, secs)
    } else {
        format!("{}{}h{:02}m{:04.1}s", sign, hours, mins, secs)
    }
}
