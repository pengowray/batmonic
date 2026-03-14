use crate::canvas::colors::{freq_marker_color, freq_marker_label};
use crate::canvas::spectrogram_renderer::freq_to_y;
use crate::state::{SpectrogramHandle, Selection};
use web_sys::CanvasRenderingContext2d;

// Time markers extracted to crate::canvas::time_markers
pub use crate::canvas::time_markers::draw_time_markers;

/// Describes how frequency markers should show shifted output frequencies.
#[derive(Clone, Copy)]
pub enum FreqShiftMode {
    /// No shift annotation.
    None,
    /// Heterodyne: show |freq - het_freq| for markers within ±15 kHz of het_freq.
    Heterodyne(f64),
    /// Time expansion or pitch shift: all freqs divide by factor.
    Divide(f64),
    /// Shift up: all freqs multiply by factor (infrasound → audible).
    Multiply(f64),
}

/// Frequency marker hover/interaction state passed to drawing functions.
pub struct FreqMarkerState {
    pub mouse_freq: Option<f64>,
    pub mouse_in_label_area: bool,
    pub label_hover_opacity: f64,
    pub has_selection: bool,
    pub file_max_freq: f64,
    /// Axis drag range for lighting up color bars
    pub axis_drag_lo: Option<f64>,
    pub axis_drag_hi: Option<f64>,
    /// FF handle drag is active (light up FF range bars)
    pub ff_drag_active: bool,
    pub ff_lo: f64,
    pub ff_hi: f64,
    /// FF handles are hovered or being dragged (hide cursor indicator)
    pub ff_handles_active: bool,
}

/// Draw horizontal frequency marker lines with subtle, interactive UI.
/// Labels are white; colored range bars indicate the resistor-band color.
pub fn draw_freq_markers(
    ctx: &CanvasRenderingContext2d,
    min_freq: f64,
    max_freq: f64,
    canvas_height: f64,
    canvas_width: f64,
    shift_mode: FreqShiftMode,
    ms: &FreqMarkerState,
    het_cutoff: f64,
    labels_on_right: bool,
) {
    let cutoff = het_cutoff;
    let color_bar_w = 6.0;
    let (color_bar_x, label_x, tick_len, right_tick_len) = if labels_on_right {
        (canvas_width - color_bar_w, canvas_width - color_bar_w - 3.0, 15.0, 22.0)
    } else {
        (0.0, color_bar_w + 3.0, 22.0, 15.0)
    };

    // Collect all division freqs within visible range.
    // Adapt division interval to visible range so we always get ~3-12 markers.
    let range = max_freq - min_freq;
    let div_interval = if range <= 5_000.0 {
        1_000.0
    } else if range <= 25_000.0 {
        5_000.0
    } else {
        10_000.0
    };
    let mut divisions: Vec<f64> = Vec::new();
    let first_div = ((min_freq / div_interval).ceil() * div_interval).max(div_interval);
    let mut freq = first_div;
    while freq < max_freq {
        divisions.push(freq);
        freq += div_interval;
    }

    // Check if top of display is nyquist
    let is_nyquist_top = (max_freq - ms.file_max_freq).abs() < 1.0;
    // Find topmost division for nyquist overlap check
    let topmost_div = divisions.last().copied().unwrap_or(0.0);
    let topmost_div_y_frac = if max_freq > min_freq { (topmost_div - min_freq) / (max_freq - min_freq) } else { 0.0 };
    let hide_topmost_for_nyquist = is_nyquist_top && topmost_div_y_frac > 0.95;

    for &freq in &divisions {
        let y = freq_to_y(freq, min_freq, max_freq, canvas_height);

        // Skip topmost division if it would overlap nyquist marker
        if hide_topmost_for_nyquist && freq == topmost_div && !ms.mouse_in_label_area {
            continue;
        }

        let color = freq_marker_color(freq);

        // Determine alpha based on HET audible band
        let base_alpha = match shift_mode {
            FreqShiftMode::Heterodyne(hf) => {
                if (freq - hf).abs() <= cutoff { 0.8 } else { 0.3 }
            }
            _ => 0.7,
        };

        // --- Color range bar (covering the interval above this division) ---
        let bar_top_freq = (freq + div_interval).min(max_freq);
        let mouse_in_range = ms.mouse_freq.map_or(false, |mf| mf >= freq && mf < bar_top_freq);
        let axis_drag_in_range = match (ms.axis_drag_lo, ms.axis_drag_hi) {
            (Some(lo), Some(hi)) => bar_top_freq > lo && freq < hi,
            _ => false,
        };
        let ff_drag_in_range = ms.ff_drag_active && ms.ff_hi > ms.ff_lo && bar_top_freq > ms.ff_lo && freq < ms.ff_hi;
        if ms.has_selection || mouse_in_range || axis_drag_in_range || ff_drag_in_range {
            let bar_alpha = if axis_drag_in_range || ff_drag_in_range { 0.8 } else if ms.has_selection { 0.6 } else { 0.8 };
            let bar_y_top = freq_to_y(bar_top_freq, min_freq, max_freq, canvas_height);
            let bar_y_bot = freq_to_y(freq, min_freq, max_freq, canvas_height);
            ctx.set_fill_style_str(&format!("rgba({},{},{},{:.2})", color[0], color[1], color[2], bar_alpha));
            ctx.fill_rect(color_bar_x, bar_y_top, color_bar_w, bar_y_bot - bar_y_top);
        }

        // --- White text label (drawn ABOVE the division line) ---
        ctx.set_font("11px sans-serif");
        ctx.set_text_baseline("bottom"); // text sits above the line
        let base_label = freq_marker_label(freq);
        let label_alpha = base_alpha;

        // Build label with optional kHz suffix and shift info
        let label = match shift_mode {
            FreqShiftMode::Heterodyne(hf) => {
                if ms.label_hover_opacity > 0.01 {
                    let diff = (freq - hf).abs();
                    if diff <= cutoff {
                        let diff_khz = (diff / 1000.0).round() as u32;
                        format!("{base_label} kHz \u{2192} {diff_khz} kHz")
                    } else {
                        format!("{base_label} kHz")
                    }
                } else {
                    base_label.clone()
                }
            }
            FreqShiftMode::Divide(factor) if factor > 1.0 => {
                if ms.label_hover_opacity > 0.01 {
                    let shifted = freq / factor;
                    let shifted_khz = shifted / 1000.0;
                    if shifted_khz >= 1.0 {
                        format!("{base_label} kHz \u{2192} {:.0} kHz", shifted_khz)
                    } else {
                        format!("{base_label} kHz \u{2192} {:.0} Hz", shifted)
                    }
                } else {
                    base_label.clone()
                }
            }
            FreqShiftMode::Multiply(factor) if factor > 1.0 => {
                if ms.label_hover_opacity > 0.01 {
                    let shifted = freq * factor;
                    let shifted_khz = shifted / 1000.0;
                    if shifted_khz >= 1.0 {
                        format!("{base_label} kHz \u{2192} {:.0} kHz", shifted_khz)
                    } else {
                        format!("{base_label} kHz \u{2192} {:.0} Hz", shifted)
                    }
                } else {
                    base_label.clone()
                }
            }
            _ => {
                // For FreqShiftMode::None, never include " kHz" here;
                // it's drawn separately below with a smooth fade.
                base_label.clone()
            }
        };

        // kHz fade: use opacity^2 for faster visual fade
        let khz_fade = ms.label_hover_opacity * ms.label_hover_opacity;
        if matches!(shift_mode, FreqShiftMode::None) && ms.label_hover_opacity > 0.001 {
            // Split rendering: number at full alpha, " kHz" suffix fading
            let full_label_for_measure = if khz_fade > 0.01 {
                format!("{} kHz", base_label)
            } else {
                base_label.clone()
            };
            let bg_metrics = ctx.measure_text(&full_label_for_measure).unwrap();
            let bg_w = bg_metrics.width() + 4.0;
            let bg_h = 14.0;
            let text_x = if labels_on_right { label_x - bg_metrics.width() } else { label_x };
            let bg_x = text_x - 2.0;
            ctx.set_fill_style_str("rgba(0,0,0,0.6)");
            ctx.fill_rect(bg_x, y - 2.0 - bg_h, bg_w, bg_h);

            ctx.set_fill_style_str(&format!("rgba(255,255,255,{:.2})", label_alpha));
            let _ = ctx.fill_text(&base_label, text_x, y - 2.0);
            let khz_alpha = label_alpha * khz_fade;
            if khz_alpha > 0.002 {
                let metrics = ctx.measure_text(&base_label).unwrap();
                let num_w = metrics.width();
                ctx.set_fill_style_str(&format!("rgba(255,255,255,{:.2})", khz_alpha));
                let _ = ctx.fill_text(" kHz", text_x + num_w, y - 2.0);
            }
        } else {
            let bg_metrics = ctx.measure_text(&label).unwrap();
            let bg_w = bg_metrics.width() + 4.0;
            let bg_h = 14.0;
            let text_x = if labels_on_right { label_x - bg_metrics.width() } else { label_x };
            let bg_x = text_x - 2.0;
            ctx.set_fill_style_str("rgba(0,0,0,0.6)");
            ctx.fill_rect(bg_x, y - 2.0 - bg_h, bg_w, bg_h);

            ctx.set_fill_style_str(&format!("rgba(255,255,255,{:.2})", label_alpha));
            let _ = ctx.fill_text(&label, text_x, y - 2.0);
        }

        // --- Short left tick line (lightly colored, under the label) ---
        // Blend: mostly white with a hint of the marker color
        let tr = 200 + (color[0] as u16 * 55 / 255) as u8;
        let tg = 200 + (color[1] as u16 * 55 / 255) as u8;
        let tb = 200 + (color[2] as u16 * 55 / 255) as u8;
        ctx.set_stroke_style_str(&format!("rgba({},{},{},{:.2})", tr, tg, tb, base_alpha * 0.5));
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.move_to(0.0, y);
        ctx.line_to(tick_len, y);
        ctx.stroke();

        // --- Short right tick line (same tint) ---
        ctx.begin_path();
        ctx.move_to(canvas_width - right_tick_len, y);
        ctx.line_to(canvas_width, y);
        ctx.stroke();

        // --- Full-width line (fades in when hovering label area, white) ---
        if ms.label_hover_opacity > 0.001 {
            let full_alpha = ms.label_hover_opacity * 0.7 * base_alpha;
            ctx.set_stroke_style_str(&format!("rgba(255,255,255,{:.3})", full_alpha));
            ctx.set_line_width(1.0);
            ctx.begin_path();
            ctx.move_to(tick_len, y);
            ctx.line_to(canvas_width - right_tick_len, y);
            ctx.stroke();
        }
    }

    // --- Nyquist / MAX marker ---
    if is_nyquist_top && !ms.mouse_in_label_area {
        let ny_y = 2.0; // just below top edge
        let ny_khz = ms.file_max_freq / 1000.0;
        let ny_label = if ny_khz == ny_khz.round() {
            format!("{:.0}k MAX", ny_khz)
        } else {
            format!("{:.1}k MAX", ny_khz)
        };
        ctx.set_fill_style_str("rgba(255,255,255,0.45)");
        ctx.set_font("10px sans-serif");
        ctx.set_text_baseline("top");
        let ny_text_x = if labels_on_right {
            let m = ctx.measure_text(&ny_label).unwrap();
            label_x - m.width()
        } else {
            label_x
        };
        let _ = ctx.fill_text(&ny_label, ny_text_x, ny_y);
        ctx.set_stroke_style_str("rgba(255,255,255,0.3)");
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.move_to(0.0, 0.5);
        ctx.line_to(tick_len, 0.5);
        ctx.stroke();
        // Right tick
        ctx.begin_path();
        ctx.move_to(canvas_width - right_tick_len, 0.5);
        ctx.line_to(canvas_width, 0.5);
        ctx.stroke();
    }

    // --- Cursor frequency indicator (hidden when FF handles are active) ---
    if let Some(mf) = ms.mouse_freq {
        if !ms.mouse_in_label_area && !ms.ff_handles_active && mf > min_freq && mf < max_freq {
            let y = freq_to_y(mf, min_freq, max_freq, canvas_height);

            // Label (above the dashed line, starting around midpoint)
            let freq_label = if mf >= 1000.0 {
                format!("{:.1} kHz", mf / 1000.0)
            } else {
                format!("{:.0} Hz", mf)
            };
            let cursor_line_len = canvas_width * 0.5;
            let label_start_x = cursor_line_len * 0.5;
            ctx.set_font("10px sans-serif");
            ctx.set_text_baseline("bottom");
            ctx.set_fill_style_str("rgba(0,210,240,0.8)");
            let _ = ctx.fill_text(&freq_label, label_start_x, y - 2.0);

            // Dashed line (cyan)
            ctx.set_stroke_style_str("rgba(0,210,240,0.45)");
            ctx.set_line_width(1.0);
            let _ = ctx.set_line_dash(&js_sys::Array::of2(
                &wasm_bindgen::JsValue::from_f64(4.0),
                &wasm_bindgen::JsValue::from_f64(4.0),
            ));
            ctx.begin_path();
            ctx.move_to(0.0, y);
            ctx.line_to(cursor_line_len, y);
            ctx.stroke();
            let _ = ctx.set_line_dash(&js_sys::Array::new());

            // Right-side frequency label
            ctx.set_text_baseline("middle");
            ctx.set_fill_style_str("rgba(0,210,240,0.7)");
            let metrics = ctx.measure_text(&freq_label).unwrap();
            let text_w = metrics.width();
            let _ = ctx.fill_text(&freq_label, canvas_width - text_w - right_tick_len - 4.0, y);
        }
    }

    ctx.set_text_baseline("alphabetic"); // reset
}

/// Draw the Frequency Focus overlay: dim outside the FF range, amber edge lines with drag handles.
pub fn draw_ff_overlay(
    ctx: &CanvasRenderingContext2d,
    ff_lo: f64,
    ff_hi: f64,
    min_freq: f64,
    max_freq: f64,
    canvas_height: f64,
    canvas_width: f64,
    hover_handle: Option<SpectrogramHandle>,
    drag_handle: Option<SpectrogramHandle>,
) {
    if ff_hi <= ff_lo { return; }

    let y_top = freq_to_y(ff_hi.min(max_freq), min_freq, max_freq, canvas_height);
    let y_bottom = freq_to_y(ff_lo.max(min_freq), min_freq, max_freq, canvas_height);

    // Dim outside the FF range
    ctx.set_fill_style_str("rgba(0, 0, 0, 0.45)");
    if y_top > 0.0 {
        ctx.fill_rect(0.0, 0.0, canvas_width, y_top);
    }
    if y_bottom < canvas_height {
        ctx.fill_rect(0.0, y_bottom, canvas_width, canvas_height - y_bottom);
    }

    let is_active = |handle: SpectrogramHandle| -> bool {
        drag_handle == Some(handle) || hover_handle == Some(handle)
    };

    // Amber edge lines + triangular drag handles
    for &(y, handle) in &[(y_top, SpectrogramHandle::FfUpper), (y_bottom, SpectrogramHandle::FfLower)] {
        let active = is_active(handle);
        let alpha = if active { 0.9 } else { 0.4 };
        let width = if active { 2.0 } else { 1.0 };
        ctx.set_stroke_style_str(&format!("rgba(255, 180, 60, {:.2})", alpha));
        ctx.set_line_width(width);
        ctx.begin_path();
        ctx.move_to(0.0, y);
        ctx.line_to(canvas_width, y);
        ctx.stroke();

        // Triangle handle at right edge
        let handle_size = if active { 10.0 } else { 6.0 };
        let handle_alpha = if active { 0.9 } else { 0.4 };
        ctx.set_fill_style_str(&format!("rgba(255, 180, 60, {:.2})", handle_alpha));
        ctx.begin_path();
        ctx.move_to(canvas_width, y - handle_size);
        ctx.line_to(canvas_width - handle_size, y);
        ctx.line_to(canvas_width, y + handle_size);
        ctx.close_path();
        let _ = ctx.fill();
    }

    // Middle handle (triangle at midpoint on right edge)
    let mid_y = (y_top + y_bottom) / 2.0;
    let mid_active = is_active(SpectrogramHandle::FfMiddle);
    let mid_alpha = if mid_active { 0.9 } else { 0.3 };
    let mid_size = if mid_active { 8.0 } else { 5.0 };
    ctx.set_fill_style_str(&format!("rgba(255, 180, 60, {:.2})", mid_alpha));
    ctx.begin_path();
    ctx.move_to(canvas_width, mid_y - mid_size);
    ctx.line_to(canvas_width - mid_size, mid_y);
    ctx.line_to(canvas_width, mid_y + mid_size);
    ctx.close_path();
    let _ = ctx.fill();

    // FF range labels (only when handles are active): top and bottom frequencies
    if hover_handle.is_some() || drag_handle.is_some() {
        ctx.set_fill_style_str("rgba(255, 180, 60, 0.8)");
        ctx.set_font("11px sans-serif");
        let label_x = canvas_width * 0.35;

        // Top frequency label: just above the upper FF line
        let top_label = format!("{:.1} kHz", ff_hi / 1000.0);
        ctx.set_text_baseline("bottom");
        let _ = ctx.fill_text(&top_label, label_x, y_top - 4.0);

        // Bottom frequency label: just below the lower FF line
        let bottom_label = format!("{:.1} kHz", ff_lo / 1000.0);
        ctx.set_text_baseline("top");
        let _ = ctx.fill_text(&bottom_label, label_x, y_bottom + 4.0);

        ctx.set_text_baseline("alphabetic");
    }
}

/// Draw the heterodyne frequency overlay: cyan center + band edge lines (no dimming — FF handles that).
pub fn draw_het_overlay(
    ctx: &CanvasRenderingContext2d,
    het_freq: f64,
    het_cutoff: f64,
    min_freq: f64,
    max_freq: f64,
    canvas_height: f64,
    canvas_width: f64,
    hover_handle: Option<SpectrogramHandle>,
    drag_handle: Option<SpectrogramHandle>,
    interactive: bool,
) {
    let cutoff = het_cutoff;
    let band_low = (het_freq - cutoff).max(min_freq);
    let band_high = (het_freq + cutoff).min(max_freq);

    let y_center = freq_to_y(het_freq, min_freq, max_freq, canvas_height);
    let y_band_top = freq_to_y(band_high, min_freq, max_freq, canvas_height);
    let y_band_bottom = freq_to_y(band_low, min_freq, max_freq, canvas_height);

    // Opacity multiplier: lower when non-interactive (auto mode without hover)
    let op = if interactive { 1.0 } else { 0.5 };

    let is_active = |handle: SpectrogramHandle| -> bool {
        drag_handle == Some(handle) || hover_handle == Some(handle)
    };

    // Band edge lines
    for &(y, handle) in &[(y_band_top, SpectrogramHandle::HetBandUpper), (y_band_bottom, SpectrogramHandle::HetBandLower)] {
        let active = interactive && is_active(handle);
        let alpha = (if active { 0.7 } else { 0.3 }) * op;
        let width = if active { 2.0 } else { 1.0 };
        ctx.set_stroke_style_str(&format!("rgba(0, 200, 255, {:.2})", alpha));
        ctx.set_line_width(width);
        ctx.begin_path();
        ctx.move_to(0.0, y);
        ctx.line_to(canvas_width, y);
        ctx.stroke();

        // Draw handle triangle at right edge (only when interactive)
        if interactive {
            let handle_size = if active { 10.0 } else { 6.0 };
            let handle_alpha = if active { 0.9 } else { 0.4 };
            ctx.set_fill_style_str(&format!("rgba(0, 200, 255, {:.2})", handle_alpha));
            ctx.begin_path();
            ctx.move_to(canvas_width, y - handle_size);
            ctx.line_to(canvas_width - handle_size, y);
            ctx.line_to(canvas_width, y + handle_size);
            ctx.close_path();
            let _ = ctx.fill();
        }
    }

    // Center line at het_freq
    let center_active = interactive && is_active(SpectrogramHandle::HetCenter);
    let center_dragging = interactive && drag_handle == Some(SpectrogramHandle::HetCenter);
    if center_dragging {
        ctx.set_stroke_style_str("rgba(0, 230, 255, 1.0)");
        ctx.set_line_width(2.0);
    } else if center_active {
        ctx.set_stroke_style_str("rgba(0, 230, 255, 1.0)");
        ctx.set_line_width(2.0);
        let _ = ctx.set_line_dash(&js_sys::Array::of2(
            &wasm_bindgen::JsValue::from_f64(6.0),
            &wasm_bindgen::JsValue::from_f64(4.0),
        ));
    } else {
        ctx.set_stroke_style_str(&format!("rgba(0, 230, 255, {:.1})", 0.8 * op));
        ctx.set_line_width(1.5);
        let _ = ctx.set_line_dash(&js_sys::Array::of2(
            &wasm_bindgen::JsValue::from_f64(6.0),
            &wasm_bindgen::JsValue::from_f64(4.0),
        ));
    }
    ctx.begin_path();
    ctx.move_to(0.0, y_center);
    ctx.line_to(canvas_width, y_center);
    ctx.stroke();
    let _ = ctx.set_line_dash(&js_sys::Array::new());

    // Center handle triangle (only when interactive)
    if interactive {
        let handle_size = if center_active { 10.0 } else { 6.0 };
        let handle_alpha = if center_active { 0.9 } else { 0.5 };
        ctx.set_fill_style_str(&format!("rgba(0, 230, 255, {:.2})", handle_alpha));
        ctx.begin_path();
        ctx.move_to(canvas_width, y_center - handle_size);
        ctx.line_to(canvas_width - handle_size, y_center);
        ctx.line_to(canvas_width, y_center + handle_size);
        ctx.close_path();
        let _ = ctx.fill();
    }

    // Label at center line
    ctx.set_fill_style_str(&format!("rgba(0, 230, 255, {:.1})", 0.9 * op));
    ctx.set_font("bold 12px sans-serif");
    let label = format!("HET {:.1} kHz", het_freq / 1000.0);
    let _ = ctx.fill_text(&label, 55.0, y_center - 5.0);

    // LP cutoff label near band edges (show when any HET handle is active)
    if interactive && (hover_handle.is_some() || drag_handle.is_some()) {
        ctx.set_fill_style_str("rgba(0, 200, 255, 0.7)");
        ctx.set_font("11px sans-serif");
        let lp_label = format!("LP ±{:.1} kHz", het_cutoff / 1000.0);
        let _ = ctx.fill_text(&lp_label, 55.0, y_band_bottom + 14.0);
    }
}

/// Draw detected pulse markers as vertical bands on the spectrogram.
pub fn draw_pulses(
    ctx: &CanvasRenderingContext2d,
    pulses: &[crate::dsp::pulse_detect::DetectedPulse],
    selected_index: Option<usize>,
    scroll_offset: f64,
    time_resolution: f64,
    zoom: f64,
    canvas_width: f64,
    canvas_height: f64,
) {
    if pulses.is_empty() {
        return;
    }

    let visible_time = (canvas_width / zoom) * time_resolution;
    let start_time = scroll_offset;
    let end_time = start_time + visible_time;
    let px_per_sec = canvas_width / visible_time;

    for pulse in pulses {
        // Skip pulses not in view
        if pulse.end_time < start_time || pulse.start_time > end_time {
            continue;
        }

        let x0 = ((pulse.start_time - start_time) * px_per_sec).max(0.0);
        let x1 = ((pulse.end_time - start_time) * px_per_sec).min(canvas_width);
        if x1 <= x0 {
            continue;
        }

        let is_selected = selected_index == Some(pulse.index);

        // Fill — full-height vertical band
        if is_selected {
            ctx.set_fill_style_str("rgba(255, 180, 50, 0.20)");
        } else {
            ctx.set_fill_style_str("rgba(50, 200, 120, 0.08)");
        }
        ctx.fill_rect(x0, 0.0, x1 - x0, canvas_height);

        // Edge lines
        if is_selected {
            ctx.set_stroke_style_str("rgba(255, 200, 80, 0.8)");
            ctx.set_line_width(1.5);
        } else {
            ctx.set_stroke_style_str("rgba(80, 220, 150, 0.4)");
            ctx.set_line_width(0.5);
        }
        ctx.begin_path();
        ctx.move_to(x0, 0.0);
        ctx.line_to(x0, canvas_height);
        ctx.stroke();

        // Pulse number label at top (only if wide enough)
        if x1 - x0 > 12.0 {
            if is_selected {
                ctx.set_fill_style_str("rgba(255, 200, 80, 0.9)");
            } else {
                ctx.set_fill_style_str("rgba(80, 220, 150, 0.7)");
            }
            ctx.set_font("9px sans-serif");
            ctx.set_text_baseline("top");
            let _ = ctx.fill_text(&format!("{}", pulse.index), x0 + 2.0, 2.0);
        }
    }
}

/// Draw selection rectangle overlay on spectrogram.
pub fn draw_selection(
    ctx: &CanvasRenderingContext2d,
    selection: &Selection,
    min_freq: f64,
    max_freq: f64,
    scroll_offset: f64,
    time_resolution: f64,
    zoom: f64,
    canvas_width: f64,
    canvas_height: f64,
) {
    let visible_time = (canvas_width / zoom) * time_resolution;
    let start_time = scroll_offset;
    let px_per_sec = canvas_width / visible_time;

    let x0 = ((selection.time_start - start_time) * px_per_sec).max(0.0);
    let x1 = ((selection.time_end - start_time) * px_per_sec).min(canvas_width);

    if x1 <= x0 {
        return;
    }

    // If frequency bounds are set, draw a bounded rectangle; otherwise full-height strip
    let (y0, y1) = match (selection.freq_high, selection.freq_low) {
        (Some(fh), Some(fl)) => {
            let y0 = freq_to_y(fh, min_freq, max_freq, canvas_height).max(0.0);
            let y1 = freq_to_y(fl, min_freq, max_freq, canvas_height).min(canvas_height);
            if y1 <= y0 { return; }
            (y0, y1)
        }
        _ => (0.0, canvas_height),
    };

    // Fill
    ctx.set_fill_style_str("rgba(50, 120, 200, 0.15)");
    ctx.fill_rect(x0, y0, x1 - x0, y1 - y0);

    // Border
    ctx.set_stroke_style_str("rgba(80, 160, 255, 0.7)");
    ctx.set_line_width(1.0);
    ctx.stroke_rect(x0, y0, x1 - x0, y1 - y0);
}

/// Draw shadow selection boxes one octave higher and lower to highlight harmonics.
/// Only drawn when the selection spans less than 1 octave.
pub fn draw_harmonic_shadows(
    ctx: &CanvasRenderingContext2d,
    selection: &Selection,
    min_freq: f64,
    max_freq: f64,
    scroll_offset: f64,
    time_resolution: f64,
    zoom: f64,
    canvas_width: f64,
    canvas_height: f64,
) {
    // Need frequency bounds for harmonic shadows
    let (freq_low, freq_high) = match (selection.freq_low, selection.freq_high) {
        (Some(fl), Some(fh)) => (fl, fh),
        _ => return,
    };

    // Only show shadows if selection is less than 1 octave
    if freq_low <= 0.0 || freq_high / freq_low >= 2.0 {
        return;
    }

    let visible_time = (canvas_width / zoom) * time_resolution;
    let start_time = scroll_offset;
    let px_per_sec = canvas_width / visible_time;

    let x0 = ((selection.time_start - start_time) * px_per_sec).max(0.0);
    let x1 = ((selection.time_end - start_time) * px_per_sec).min(canvas_width);
    if x1 <= x0 {
        return;
    }
    let w = x1 - x0;

    // Set up dashed border style
    let _ = ctx.set_line_dash(&js_sys::Array::of2(
        &wasm_bindgen::JsValue::from_f64(4.0),
        &wasm_bindgen::JsValue::from_f64(4.0),
    ));

    // Octave higher
    let hi_low = freq_low * 2.0;
    let hi_high = freq_high * 2.0;
    if hi_low < max_freq {
        let y0 = freq_to_y(hi_high.min(max_freq), min_freq, max_freq, canvas_height).max(0.0);
        let y1 = freq_to_y(hi_low, min_freq, max_freq, canvas_height).min(canvas_height);
        if y1 > y0 {
            ctx.set_fill_style_str("rgba(50, 120, 200, 0.06)");
            ctx.fill_rect(x0, y0, w, y1 - y0);
            ctx.set_stroke_style_str("rgba(80, 160, 255, 0.3)");
            ctx.set_line_width(1.0);
            ctx.stroke_rect(x0, y0, w, y1 - y0);
        }
    }

    // Octave lower
    let lo_low = freq_low / 2.0;
    let lo_high = freq_high / 2.0;
    {
        let y0 = freq_to_y(lo_high, min_freq, max_freq, canvas_height).max(0.0);
        let y1 = freq_to_y(lo_low.max(min_freq), min_freq, max_freq, canvas_height).min(canvas_height);
        if y1 > y0 {
            ctx.set_fill_style_str("rgba(50, 120, 200, 0.06)");
            ctx.fill_rect(x0, y0, w, y1 - y0);
            ctx.set_stroke_style_str("rgba(80, 160, 255, 0.3)");
            ctx.set_line_width(1.0);
            ctx.stroke_rect(x0, y0, w, y1 - y0);
        }
    }

    // Reset dash
    let _ = ctx.set_line_dash(&js_sys::Array::new());
}

/// Draw filter EQ band overlay on the spectrogram.
///
/// Highlights the frequency region of the currently hovered band slider.
/// band: 0=below, 1=selected, 2=harmonics, 3=above
pub fn draw_filter_overlay(
    ctx: &CanvasRenderingContext2d,
    hovered_band: u8,
    freq_low: f64,
    freq_high: f64,
    band_mode: u8,
    min_freq: f64,
    max_freq: f64,
    canvas_width: f64,
    canvas_height: f64,
) {
    let harmonics_active = band_mode >= 4 && freq_low > 0.0 && freq_high / freq_low < 2.0;
    let harmonics_upper = freq_high * 2.0;

    // Determine the frequency range for the hovered band
    let (band_lo, band_hi, color) = match hovered_band {
        0 => (0.0, freq_low, "rgba(255, 80, 80, 0.15)"),       // below — red tint
        1 => (freq_low, freq_high, "rgba(80, 255, 120, 0.15)"), // selected — green
        2 if harmonics_active => (freq_high, harmonics_upper, "rgba(80, 120, 255, 0.15)"), // harmonics — blue
        3 => {
            let lo = if harmonics_active { harmonics_upper } else { freq_high };
            (lo, max_freq, "rgba(255, 180, 60, 0.15)")          // above — orange
        }
        _ => return,
    };

    let y_top = freq_to_y(band_hi.min(max_freq), min_freq, max_freq, canvas_height).max(0.0);
    let y_bot = freq_to_y(band_lo.max(min_freq), min_freq, max_freq, canvas_height).min(canvas_height);

    if y_bot <= y_top {
        return;
    }

    // Fill the band region
    ctx.set_fill_style_str(color);
    ctx.fill_rect(0.0, y_top, canvas_width, y_bot - y_top);

    // Edge lines
    let edge_color = match hovered_band {
        0 => "rgba(255, 80, 80, 0.5)",
        1 => "rgba(80, 255, 120, 0.5)",
        2 => "rgba(80, 120, 255, 0.5)",
        3 => "rgba(255, 180, 60, 0.5)",
        _ => return,
    };
    ctx.set_stroke_style_str(edge_color);
    ctx.set_line_width(1.0);
    for &y in &[y_top, y_bot] {
        ctx.begin_path();
        ctx.move_to(0.0, y);
        ctx.line_to(canvas_width, y);
        ctx.stroke();
    }
}

/// Convert pixel coordinates on the spectrogram canvas to (time, frequency).
pub fn pixel_to_time_freq(
    px_x: f64,
    px_y: f64,
    min_freq: f64,
    max_freq: f64,
    scroll_offset: f64,
    time_resolution: f64,
    zoom: f64,
    canvas_width: f64,
    canvas_height: f64,
) -> (f64, f64) {
    let visible_time = (canvas_width / zoom) * time_resolution;
    let time = scroll_offset + (px_x / canvas_width) * visible_time;
    let freq = crate::canvas::spectrogram_renderer::y_to_freq(px_y, min_freq, max_freq, canvas_height);
    (time, freq)
}

/// Draw notch filter band markers as semi-transparent horizontal overlays.
/// When `harmonic_suppression` > 0, also draws dashed lines at 2x and 3x harmonics.
pub fn draw_notch_bands(
    ctx: &web_sys::CanvasRenderingContext2d,
    min_freq: f64,
    max_freq: f64,
    canvas_height: f64,
    canvas_width: f64,
    bands: &[crate::dsp::notch::NoiseBand],
    notch_enabled: bool,
    hovered_index: Option<usize>,
    harmonic_suppression: f64,
) {
    for (band_idx, band) in bands.iter().enumerate() {
        let center = band.center_hz;
        let half_bw = band.bandwidth_hz / 2.0;
        let freq_lo = center - half_bw;
        let freq_hi = center + half_bw;

        // Skip if entirely outside visible range
        if freq_hi < min_freq || freq_lo > max_freq {
            continue;
        }

        let y_top = freq_to_y(freq_hi.min(max_freq), min_freq, max_freq, canvas_height);
        let y_bot = freq_to_y(freq_lo.max(min_freq), min_freq, max_freq, canvas_height);
        let y_center = freq_to_y(center, min_freq, max_freq, canvas_height);
        let band_h = (y_bot - y_top).max(1.0);

        let is_hovered = hovered_index == Some(band_idx);

        let (fill, line, label_color, line_width) = if is_hovered {
            ("rgba(255, 220, 40, 0.25)", "rgba(255, 220, 40, 0.9)", "rgba(255, 240, 100, 1.0)", 2.0)
        } else if notch_enabled && band.enabled {
            ("rgba(255, 40, 40, 0.12)", "rgba(255, 60, 60, 0.6)", "rgba(255, 100, 100, 0.8)", 1.0)
        } else {
            ("rgba(128, 128, 128, 0.08)", "rgba(128, 128, 128, 0.3)", "rgba(160, 160, 160, 0.5)", 1.0)
        };

        // Band fill
        ctx.set_fill_style_str(fill);
        ctx.fill_rect(0.0, y_top, canvas_width, band_h);

        // Center line
        ctx.set_stroke_style_str(line);
        ctx.set_line_width(line_width);
        ctx.begin_path();
        ctx.move_to(0.0, y_center);
        ctx.line_to(canvas_width, y_center);
        ctx.stroke();

        // Frequency label
        ctx.set_fill_style_str(label_color);
        ctx.set_font(if is_hovered { "bold 11px sans-serif" } else { "10px sans-serif" });
        ctx.set_text_baseline("bottom");
        let label = if center >= 1000.0 {
            format!("{:.1}k", center / 1000.0)
        } else {
            format!("{:.0}", center)
        };
        let _ = ctx.fill_text(&label, canvas_width - 40.0, y_center - 2.0);
    }

    // Draw harmonic markers (dashed orange lines at 2x and 3x)
    if harmonic_suppression > 0.0 && notch_enabled {
        let alpha = (harmonic_suppression * 0.6).min(0.6);
        let dash = js_sys::Array::new();
        dash.push(&wasm_bindgen::JsValue::from_f64(4.0));
        dash.push(&wasm_bindgen::JsValue::from_f64(4.0));

        for band in bands.iter().filter(|b| b.enabled) {
            for &multiplier in &[2.0_f64, 3.0] {
                let harmonic_hz = band.center_hz * multiplier;
                if harmonic_hz < min_freq || harmonic_hz > max_freq {
                    continue;
                }
                let y = freq_to_y(harmonic_hz, min_freq, max_freq, canvas_height);

                ctx.set_stroke_style_str(&format!("rgba(255, 120, 40, {:.2})", alpha));
                ctx.set_line_width(1.0);
                let _ = ctx.set_line_dash(&dash);
                ctx.begin_path();
                ctx.move_to(0.0, y);
                ctx.line_to(canvas_width, y);
                ctx.stroke();

                // Small label
                let label = format!("{}x", multiplier as u32);
                ctx.set_fill_style_str(&format!("rgba(255, 140, 60, {:.2})", alpha));
                ctx.set_font("9px sans-serif");
                ctx.set_text_baseline("bottom");
                let _ = ctx.fill_text(&label, canvas_width - 22.0, y - 2.0);
            }
        }

        // Reset line dash
        let _ = ctx.set_line_dash(&js_sys::Array::new());
    }
}

/// Draw tile debug overlay: colored borders and LOD labels for each visible tile.
///
/// Shows the ideal LOD tile grid with colors indicating which LOD is actually
/// rendered (ideal vs fallback). Colors: LOD3 = cyan, LOD2 = green, LOD1 = blue,
/// LOD0 = yellow, missing = red.
pub fn draw_tile_debug_overlay(
    ctx: &CanvasRenderingContext2d,
    canvas: &web_sys::HtmlCanvasElement,
    file_idx: usize,
    total_cols: usize,
    scroll_col: f64,
    zoom: f64,
    user_fft: usize,
    flow_on: bool,
) {
    use crate::canvas::tile_cache::{self, TILE_COLS};

    let cw = canvas.width() as f64;
    let ch = canvas.height() as f64;
    if total_cols == 0 || zoom <= 0.0 { return; }

    let ideal_lod = tile_cache::select_lod(zoom);
    let ratio = tile_cache::lod_ratio(ideal_lod);

    let vis_start = scroll_col.max(0.0).min((total_cols as f64 - 1.0).max(0.0));
    let vis_end = (vis_start + cw / zoom).min(total_cols as f64);

    let vis_start_lod = vis_start * ratio;
    let vis_end_lod = vis_end * ratio;

    let first_tile = (vis_start_lod / tile_cache::TILE_COLS as f64).floor() as usize;
    let last_tile = ((vis_end_lod - 0.001).max(0.0) / tile_cache::TILE_COLS as f64).floor() as usize;

    ctx.save();
    ctx.set_line_width(1.0);
    ctx.set_font("11px monospace");
    ctx.set_text_baseline("top");

    for tile_idx in first_tile..=last_tile {
        let tile_lod1_start = tile_idx as f64 * TILE_COLS as f64 / ratio;
        let tile_lod1_end = tile_lod1_start + TILE_COLS as f64 / ratio;

        // Determine which LOD is actually rendered for this tile
        let has_tile = |fi, lod, ti| {
            if flow_on { tile_cache::get_flow_tile(fi, lod, ti).is_some() }
            else { tile_cache::get_tile(fi, lod, ti).is_some() }
        };
        let (displayed_lod, displayed_tile, lod_label, color) = if has_tile(file_idx, ideal_lod, tile_idx) {
            let label = format!("L{ideal_lod}");
            let c = match ideal_lod { 3 => "#0ff", 2 => "#0f0", 0 => "#ff0", _ => "#48f" };
            (ideal_lod, tile_idx, label, c)
        } else {
            // Check fallback LODs
            let mut found = None;
            for fb_lod in (0..ideal_lod).rev() {
                let (fb_tile, _, _) = tile_cache::fallback_tile_info(ideal_lod, tile_idx, fb_lod);
                if has_tile(file_idx, fb_lod, fb_tile) {
                    found = Some((fb_lod, fb_tile));
                    break;
                }
            }
            match found {
                Some((l, ft)) => {
                    let label = format!("L{l}fb");
                    let c = match l { 0 => "#ff0", 1 => "#48f", 2 => "#0f0", _ => "#0ff" };
                    (l, ft, label, c)
                }
                None => (255, 0, "--".to_string(), "#f44"),
            }
        };

        // Tile destination rectangle on canvas
        let tile_x_start = (tile_lod1_start - vis_start) * zoom;
        let tile_x_end = (tile_lod1_end - vis_start) * zoom;
        let dx = tile_x_start.max(0.0);
        let dw = (tile_x_end.min(cw) - dx).max(0.0);
        if dw <= 0.0 { continue; }

        // Draw border
        ctx.set_stroke_style_str(color);
        ctx.stroke_rect(dx + 0.5, 0.5, dw - 1.0, ch - 1.0);

        // Actual FFT used for this LOD (same logic as schedule_tile_lod)
        let (res_line, tex_line) = if displayed_lod < tile_cache::NUM_LODS as u8 {
            let cfg = &tile_cache::LOD_CONFIGS[displayed_lod as usize];
            let actual_fft = user_fft.max(cfg.hop_size);
            let res = format!("fft={} hop={}", actual_fft, cfg.hop_size);
            // Get tile texture dimensions
            let tex = if flow_on {
                tile_cache::borrow_flow_tile(file_idx, displayed_lod, displayed_tile, |t| {
                    format!("{}x{}px", t.rendered.width, t.rendered.height)
                })
            } else {
                tile_cache::borrow_tile(file_idx, displayed_lod, displayed_tile, |t| {
                    format!("{}x{}px", t.rendered.width, t.rendered.height)
                })
            }.unwrap_or_else(|| "?".to_string());
            (res, tex)
        } else {
            ("no tile".to_string(), String::new())
        };

        // Draw label background (three lines)
        let label = format!("T{tile_idx} {lod_label}");
        let label_x = dx + 3.0;
        let label_y = 3.0;
        ctx.set_fill_style_str("rgba(0,0,0,0.6)");
        ctx.fill_rect(label_x - 1.0, label_y - 1.0, 100.0, 40.0);

        // Draw label text — line 1: tile id + LOD
        ctx.set_fill_style_str(color);
        let _ = ctx.fill_text(&label, label_x, label_y);
        // Line 2: fft + hop
        ctx.set_fill_style_str("#aaa");
        let _ = ctx.fill_text(&res_line, label_x, label_y + 13.0);
        // Line 3: texture pixel size
        ctx.set_fill_style_str("#888");
        let _ = ctx.fill_text(&tex_line, label_x, label_y + 26.0);
    }

    // Draw zoom level + ideal LOD + resolution in top-right corner
    let ideal_hop = tile_cache::LOD_CONFIGS[ideal_lod as usize].hop_size;
    let actual_fft = user_fft.max(ideal_hop);
    let zoom_label = format!("z={zoom:.1} LOD{ideal_lod} fft={actual_fft} hop={ideal_hop}");
    let label_w = 220.0;
    ctx.set_fill_style_str("rgba(0,0,0,0.6)");
    ctx.fill_rect(cw - label_w - 3.0, 3.0, label_w, 14.0);
    ctx.set_fill_style_str("#fff");
    let _ = ctx.fill_text(&zoom_label, cw - label_w - 1.0, 4.0);

    ctx.restore();
}

/// Draw saved annotation selections as semi-transparent overlays.
pub fn draw_annotations(
    ctx: &web_sys::CanvasRenderingContext2d,
    annotation_set: &crate::annotations::AnnotationSet,
    selected_ids: &[String],
    min_freq: f64,
    max_freq: f64,
    scroll_offset: f64,
    time_resolution: f64,
    zoom: f64,
    canvas_width: f64,
    canvas_height: f64,
) {
    let visible_time = (canvas_width / zoom) * time_resolution;
    let start_time = scroll_offset;
    let end_time = start_time + visible_time;
    let px_per_sec = canvas_width / visible_time;

    for annotation in &annotation_set.annotations {
        let sel = match &annotation.kind {
            crate::annotations::AnnotationKind::Region(s) => s,
            _ => continue,
        };

        // Skip if completely outside visible range
        if sel.time_end < start_time || sel.time_start > end_time {
            continue;
        }

        let x0 = ((sel.time_start - start_time) * px_per_sec).max(0.0);
        let x1 = ((sel.time_end - start_time) * px_per_sec).min(canvas_width);

        if x1 <= x0 {
            continue;
        }

        let (y0, y1) = match (sel.freq_high, sel.freq_low) {
            (Some(fh), Some(fl)) => {
                let y0 = freq_to_y(fh, min_freq, max_freq, canvas_height).max(0.0);
                let y1 = freq_to_y(fl, min_freq, max_freq, canvas_height).min(canvas_height);
                if y1 <= y0 { continue; }
                (y0, y1)
            }
            _ => (0.0, canvas_height),
        };

        let is_selected = selected_ids.contains(&annotation.id);

        // Fill
        let fill_color = if is_selected {
            "rgba(200, 150, 50, 0.15)"
        } else {
            "rgba(50, 200, 120, 0.10)"
        };
        ctx.set_fill_style_str(fill_color);
        ctx.fill_rect(x0, y0, x1 - x0, y1 - y0);

        // Dashed border
        let _ = ctx.set_line_dash(&js_sys::Array::of2(
            &wasm_bindgen::JsValue::from_f64(4.0),
            &wasm_bindgen::JsValue::from_f64(3.0),
        ));
        let stroke_color = if is_selected {
            "rgba(255, 200, 80, 0.8)"
        } else {
            "rgba(80, 220, 140, 0.5)"
        };
        ctx.set_stroke_style_str(stroke_color);
        ctx.set_line_width(1.0);
        ctx.stroke_rect(x0, y0, x1 - x0, y1 - y0);
        let _ = ctx.set_line_dash(&js_sys::Array::new());

        // Label
        if let Some(ref label) = sel.label {
            ctx.set_font("11px monospace");
            ctx.set_fill_style_str("rgba(200, 255, 200, 0.8)");
            let _ = ctx.fill_text(label, x0 + 3.0, y0 + 12.0);
        }
    }
}
