use crate::canvas::colors::{freq_marker_color, freq_marker_label, freq_resistor_bands, freq_shield_color};
use crate::state::ShieldStyle;
use crate::canvas::spectrogram_renderer::freq_to_y;
use crate::dsp::filters::harmonics_band_bounds;
use crate::state::{FftMode, SpectrogramHandle, Selection, ResizeHandlePosition};
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
    pub file_max_freq: f64,
    /// Axis drag range for lighting up color bars
    pub axis_drag_lo: Option<f64>,
    pub axis_drag_hi: Option<f64>,
    /// BandFF handle drag is active (light up BandFF range bars)
    pub band_ff_drag_active: bool,
    pub band_ff_lo: f64,
    pub band_ff_hi: f64,
    /// BandFF handles are hovered or being dragged (hide cursor indicator)
    pub band_ff_handles_active: bool,
    /// Shield/flag color bar style
    pub shield_style: ShieldStyle,
}

/// Returns true if two RGB colors are perceptually similar enough to need fimbriation.
fn colors_similar(a: [u8; 3], b: [u8; 3]) -> bool {
    let dr = (a[0] as i32 - b[0] as i32).abs();
    let dg = (a[1] as i32 - b[1] as i32).abs();
    let db = (a[2] as i32 - b[2] as i32).abs();
    // Weighted distance (human eye is more sensitive to green)
    let dist_sq = 2 * dr * dr + 4 * dg * dg + 3 * db * db;
    dist_sq < 12000
}

/// Draw a "bend" heraldic shield for 3-color resistor bands.
/// The rectangle is divided diagonally: 1st color top-left, 2nd color as a diagonal
/// bend across the middle, 3rd color bottom-right. A thin fimbriation line is drawn
/// between adjacent bands when their colors are too similar.
fn draw_bend_shield(
    ctx: &CanvasRenderingContext2d,
    x: f64, y_top: f64, w: f64, h: f64,
    bands: [[u8; 3]; 3],
    alpha: f64,
) {
    if h < 1.0 { return; } // too small to draw

    // Clip to the shield rectangle so nothing renders outside the range
    ctx.save();
    ctx.begin_path();
    ctx.rect(x, y_top, w, h);
    ctx.clip();

    // Two parallel diagonal lines divide the rectangle into 3 regions.
    // `diag_slope` controls the diagonal tilt (how far the midline deviates
    // from horizontal at each edge); `band_frac` controls the middle band's
    // vertical thickness. These MUST be independent — if they're equal the
    // two boundary lines collapse into one and the bend vanishes.
    let band_frac = 0.28_f64; // middle band vertical thickness (fraction of h)
    let diag_slope = 0.15_f64; // diagonal midline half-range (fraction of h)

    let half = band_frac / 2.0;

    // Upper boundary of bend (separates region 1 from region 2)
    let u_l = 0.5 + diag_slope - half; // left edge y-fraction
    let u_r = 0.5 - diag_slope - half; // right edge y-fraction

    // Lower boundary of bend (separates region 2 from region 3)
    let l_l = 0.5 + diag_slope + half; // left edge y-fraction
    let l_r = 0.5 - diag_slope + half; // right edge y-fraction

    let fmt = |c: [u8; 3]| format!("rgba({},{},{},{:.2})", c[0], c[1], c[2], alpha);
    let fim_color = format!("rgba(0,0,0,{:.2})", alpha * 0.5);
    let fim_w = 1.0_f64; // fimbriation width

    // Region 1 (top-left): above the upper diagonal
    ctx.begin_path();
    ctx.move_to(x, y_top);
    ctx.line_to(x + w, y_top);
    ctx.line_to(x + w, y_top + u_r * h);
    ctx.line_to(x, y_top + u_l * h);
    ctx.close_path();
    ctx.set_fill_style_str(&fmt(bands[0]));
    ctx.fill();

    // Region 2 (bend / diagonal band): the middle stripe
    ctx.begin_path();
    ctx.move_to(x, y_top + u_l * h);
    ctx.line_to(x + w, y_top + u_r * h);
    ctx.line_to(x + w, y_top + l_r * h);
    ctx.line_to(x, y_top + l_l * h);
    ctx.close_path();
    ctx.set_fill_style_str(&fmt(bands[1]));
    ctx.fill();

    // Region 3 (bottom-right): below the lower diagonal
    ctx.begin_path();
    ctx.move_to(x, y_top + l_l * h);
    ctx.line_to(x + w, y_top + l_r * h);
    ctx.line_to(x + w, y_top + h);
    ctx.line_to(x, y_top + h);
    ctx.close_path();
    ctx.set_fill_style_str(&fmt(bands[2]));
    ctx.fill();

    // Fimbriation: thin dark lines between similar adjacent colors
    ctx.set_stroke_style_str(&fim_color);
    ctx.set_line_width(fim_w);
    if colors_similar(bands[0], bands[1]) {
        ctx.begin_path();
        ctx.move_to(x, y_top + u_l * h);
        ctx.line_to(x + w, y_top + u_r * h);
        ctx.stroke();
    }
    if colors_similar(bands[1], bands[2]) {
        ctx.begin_path();
        ctx.move_to(x, y_top + l_l * h);
        ctx.line_to(x + w, y_top + l_r * h);
        ctx.stroke();
    }

    // Bordure: dark outline around the shield (vexillological border)
    ctx.set_stroke_style_str(&format!("rgba(0,0,0,{:.2})", alpha * 0.7));
    ctx.set_line_width(1.0);
    ctx.stroke_rect(x + 0.5, y_top + 0.5, w - 1.0, h - 1.0);

    ctx.restore();
}

/// Draw a solid-color shield (single color fill with dark border).
fn draw_solid_shield(
    ctx: &CanvasRenderingContext2d,
    x: f64, y_top: f64, w: f64, h: f64,
    color: [u8; 3],
    alpha: f64,
) {
    if h < 1.0 { return; }
    ctx.set_fill_style_str(&format!("rgba({},{},{},{:.2})", color[0], color[1], color[2], alpha));
    ctx.fill_rect(x, y_top, w, h);
    ctx.set_stroke_style_str(&format!("rgba(0,0,0,{:.2})", alpha * 0.7));
    ctx.set_line_width(1.0);
    ctx.stroke_rect(x + 0.5, y_top + 0.5, w - 1.0, h - 1.0);
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
    let color_bar_w = 10.0;
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
        // Only show bands within: axis drag range, BandFF handle drag range, or active BandFF range.
        // Skip major bars below 20 kHz when minor bars will provide finer coverage.
        let has_minor_coverage = div_interval >= 5_000.0 && freq < 20_000.0;
        let bar_top_freq = (freq + div_interval).min(max_freq);
        if !has_minor_coverage {
            let axis_drag_in_range = match (ms.axis_drag_lo, ms.axis_drag_hi) {
                (Some(lo), Some(hi)) => bar_top_freq > lo && freq < hi,
                _ => false,
            };
            let band_ff_drag_in_range = ms.band_ff_drag_active && ms.band_ff_hi > ms.band_ff_lo && bar_top_freq > ms.band_ff_lo && freq < ms.band_ff_hi;
            let band_ff_active_in_range = !ms.band_ff_drag_active && ms.band_ff_hi > ms.band_ff_lo && bar_top_freq > ms.band_ff_lo && freq < ms.band_ff_hi;
            if axis_drag_in_range || band_ff_drag_in_range || band_ff_active_in_range {
                let bar_alpha = if axis_drag_in_range || band_ff_drag_in_range { 0.8 } else { 0.6 };
                // Clamp bar extent to active range boundaries
                let (clamp_lo, clamp_hi) = if axis_drag_in_range {
                    (ms.axis_drag_lo.unwrap(), ms.axis_drag_hi.unwrap())
                } else {
                    (ms.band_ff_lo, ms.band_ff_hi)
                };
                let clamped_lo = freq.max(clamp_lo);
                let clamped_hi = bar_top_freq.min(clamp_hi);
                let bar_y_top = freq_to_y(clamped_hi, min_freq, max_freq, canvas_height);
                let bar_y_bot = freq_to_y(clamped_lo, min_freq, max_freq, canvas_height);
                let bar_h = bar_y_bot - bar_y_top;
                match ms.shield_style {
                    ShieldStyle::Resistor => {
                        let bands = freq_resistor_bands(freq);
                        draw_bend_shield(ctx, color_bar_x, bar_y_top, color_bar_w, bar_h, bands, bar_alpha);
                    }
                    ShieldStyle::Solid => {
                        let c = freq_shield_color(freq, div_interval);
                        draw_solid_shield(ctx, color_bar_x, bar_y_top, color_bar_w, bar_h, c, bar_alpha);
                    }
                    ShieldStyle::Off => {}
                }
            }
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

    // --- Sub-20kHz 1kHz minor dividers ---
    if div_interval >= 5_000.0 {
        let minor_interval = 1_000.0;
        let minor_start = ((min_freq / minor_interval).ceil() * minor_interval).max(minor_interval);
        let minor_end = max_freq.min(20_000.0);
        let mut mf = minor_start;
        while mf < minor_end {
            // Check if this minor freq coincides with a main division
            let ratio = mf / div_interval;
            let is_major = (ratio - ratio.round()).abs() < 0.001;

            // Draw tick marks and labels only for non-major frequencies
            if !is_major {
                let y = freq_to_y(mf, min_freq, max_freq, canvas_height);
                let minor_color = freq_marker_color(mf);
                let minor_alpha = 0.3;

                // Short left tick
                let tr = minor_color[0]; let tg = minor_color[1]; let tb = minor_color[2];
                ctx.set_stroke_style_str(&format!("rgba({},{},{},{:.2})", tr, tg, tb, minor_alpha));
                ctx.set_line_width(1.0);
                ctx.begin_path();
                ctx.move_to(0.0, y);
                ctx.line_to(tick_len * 0.5, y);
                ctx.stroke();

                // Short right tick
                ctx.begin_path();
                ctx.move_to(canvas_width - right_tick_len * 0.5, y);
                ctx.line_to(canvas_width, y);
                ctx.stroke();

                // Small label (only when hovering label area)
                if ms.label_hover_opacity > 0.01 {
                    let mlabel = freq_marker_label(mf);
                    ctx.set_font("9px sans-serif");
                    ctx.set_text_baseline("bottom");
                    let label_alpha_m = 0.4 * ms.label_hover_opacity;
                    ctx.set_fill_style_str(&format!("rgba(255,255,255,{:.2})", label_alpha_m));
                    let text_x = if labels_on_right {
                        let m = ctx.measure_text(&mlabel).unwrap();
                        label_x - m.width()
                    } else {
                        label_x
                    };
                    let _ = ctx.fill_text(&mlabel, text_x, y - 2.0);
                }
            }

            // Minor color bar — always draw (including at major-division frequencies
            // to fill gaps left by suppressed major bars)
            let bar_top_freq_m = (mf + minor_interval).min(max_freq).min(20_000.0);
            let axis_drag_in_m = match (ms.axis_drag_lo, ms.axis_drag_hi) {
                (Some(lo), Some(hi)) => bar_top_freq_m > lo && mf < hi,
                _ => false,
            };
            let band_ff_drag_in_m = ms.band_ff_drag_active && ms.band_ff_hi > ms.band_ff_lo && bar_top_freq_m > ms.band_ff_lo && mf < ms.band_ff_hi;
            let band_ff_active_in_m = !ms.band_ff_drag_active && ms.band_ff_hi > ms.band_ff_lo && bar_top_freq_m > ms.band_ff_lo && mf < ms.band_ff_hi;
            if axis_drag_in_m || band_ff_drag_in_m || band_ff_active_in_m {
                let bar_alpha_m = if axis_drag_in_m || band_ff_drag_in_m { 0.6 } else { 0.4 };
                // Clamp bar extent to active range boundaries
                let (clamp_lo, clamp_hi) = if axis_drag_in_m {
                    (ms.axis_drag_lo.unwrap(), ms.axis_drag_hi.unwrap())
                } else {
                    (ms.band_ff_lo, ms.band_ff_hi)
                };
                let clamped_lo = mf.max(clamp_lo);
                let clamped_hi = bar_top_freq_m.min(clamp_hi);
                if clamped_hi > clamped_lo {
                    let by_top = freq_to_y(clamped_hi, min_freq, max_freq, canvas_height);
                    let by_bot = freq_to_y(clamped_lo, min_freq, max_freq, canvas_height);
                    let bar_h = by_bot - by_top;
                    match ms.shield_style {
                        ShieldStyle::Resistor => {
                            let bands = freq_resistor_bands(mf);
                            draw_bend_shield(ctx, color_bar_x, by_top, color_bar_w, bar_h, bands, bar_alpha_m);
                        }
                        ShieldStyle::Solid => {
                            let c = freq_shield_color(mf, minor_interval);
                            draw_solid_shield(ctx, color_bar_x, by_top, color_bar_w, bar_h, c, bar_alpha_m);
                        }
                        ShieldStyle::Off => {}
                    }
                }
            }

            mf += minor_interval;
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


    ctx.set_text_baseline("alphabetic"); // reset
}

/// Draw the Frequency Focus overlay: dim outside the BandFF range, amber edge lines with drag handles.
/// Handles are diamond-shaped and centered horizontally. They appear on hover, pointer-down,
/// or always on mobile.
pub fn draw_band_ff_overlay(
    ctx: &CanvasRenderingContext2d,
    band_ff_lo: f64,
    band_ff_hi: f64,
    min_freq: f64,
    max_freq: f64,
    canvas_height: f64,
    canvas_width: f64,
    hover_handle: Option<SpectrogramHandle>,
    drag_handle: Option<SpectrogramHandle>,
    is_mobile: bool,
    band_ff_focused: bool,
    pointer_down: bool,
    mouse_freq: Option<f64>,
) {
    if band_ff_hi <= band_ff_lo { return; }

    let y_top = freq_to_y(band_ff_hi.min(max_freq), min_freq, max_freq, canvas_height);
    let y_bottom = freq_to_y(band_ff_lo.max(min_freq), min_freq, max_freq, canvas_height);

    // Dim outside the BandFF range
    ctx.set_fill_style_str("rgba(0, 0, 0, 0.45)");
    if y_top > 0.0 {
        ctx.fill_rect(0.0, 0.0, canvas_width, y_top);
    }
    if y_bottom < canvas_height {
        ctx.fill_rect(0.0, y_bottom, canvas_width, canvas_height - y_bottom);
    }

    let any_band_ff_active = matches!(hover_handle, Some(SpectrogramHandle::BandFfUpper | SpectrogramHandle::BandFfLower | SpectrogramHandle::BandFfMiddle))
        || matches!(drag_handle, Some(SpectrogramHandle::BandFfUpper | SpectrogramHandle::BandFfLower | SpectrogramHandle::BandFfMiddle));

    let is_active = |handle: SpectrogramHandle| -> bool {
        drag_handle == Some(handle) || hover_handle == Some(handle)
    };

    let center_x = canvas_width / 2.0;
    let handle_zone_half = crate::canvas::hit_test::FF_HANDLE_HALF_WIDTH;

    // Mouse Y position in canvas pixels (for proximity checks)
    let mouse_y = mouse_freq.map(|f| freq_to_y(f.clamp(min_freq, max_freq), min_freq, max_freq, canvas_height));

    // Edge lines (full width) + centered diamond drag handles
    // Focused: dotted yellow lines; Unfocused: solid muted blue-gray lines
    for &(y, handle) in &[(y_top, SpectrogramHandle::BandFfUpper), (y_bottom, SpectrogramHandle::BandFfLower)] {
        let active = is_active(handle);
        if band_ff_focused {
            // Focused: dotted yellow/amber lines
            let line_alpha = if active { 0.9 } else { 0.5 };
            let width = if active { 2.0 } else { 1.5 };
            ctx.set_stroke_style_str(&format!("rgba(255, 220, 60, {:.2})", line_alpha));
            ctx.set_line_width(width);
            let dash = js_sys::Array::new();
            dash.push(&wasm_bindgen::JsValue::from_f64(6.0));
            dash.push(&wasm_bindgen::JsValue::from_f64(4.0));
            let _ = ctx.set_line_dash(&dash);
        } else {
            // Unfocused: solid muted teal/blue-gray lines
            let line_alpha = if active { 0.7 } else { 0.35 };
            let width = if active { 1.5 } else { 1.0 };
            ctx.set_stroke_style_str(&format!("rgba(140, 180, 200, {:.2})", line_alpha));
            ctx.set_line_width(width);
            let _ = ctx.set_line_dash(&js_sys::Array::new());
        }
        ctx.begin_path();
        ctx.move_to(0.0, y);
        ctx.line_to(canvas_width, y);
        ctx.stroke();
        // Reset line dash
        let _ = ctx.set_line_dash(&js_sys::Array::new());

        // Diamond handle at center — visible when BandFF is focused and
        // hovering/dragging any BandFF handle, pointer is held down, or on mobile
        let show_handle = band_ff_focused && (active || any_band_ff_active || is_mobile || pointer_down);
        if show_handle {
            let handle_size = if active { 8.0 } else if is_mobile { 6.0 } else { 5.0 };
            let handle_alpha = if active { 0.9 } else if is_mobile { 0.5 } else { 0.45 };
            ctx.set_fill_style_str(&format!("rgba(255, 180, 60, {:.2})", handle_alpha));
            ctx.begin_path();
            ctx.move_to(center_x, y - handle_size);              // top
            ctx.line_to(center_x + handle_size, y);              // right
            ctx.line_to(center_x, y + handle_size);              // bottom
            ctx.line_to(center_x - handle_size, y);              // left
            ctx.close_path();
            ctx.fill();

            // Short horizontal line through handle zone for visual affordance
            let line_half = handle_zone_half * 0.6;
            ctx.set_stroke_style_str(&format!("rgba(255, 180, 60, {:.2})", handle_alpha * 0.6));
            ctx.set_line_width(1.0);
            ctx.begin_path();
            ctx.move_to(center_x - line_half, y);
            ctx.line_to(center_x + line_half, y);
            ctx.stroke();
        }
    }

    // Middle handle (diamond at midpoint, centered)
    // Only shown when mouse is within ~100px of the midpoint (or mobile/actively dragging)
    let mid_y = (y_top + y_bottom) / 2.0;
    let mid_active = is_active(SpectrogramHandle::BandFfMiddle);
    let mid_near = mouse_y.map_or(false, |my| (my - mid_y).abs() < 100.0);
    let show_mid = band_ff_focused && (mid_active || is_mobile
        || ((any_band_ff_active || pointer_down) && mid_near));
    if show_mid {
        let mid_size = if mid_active { 7.0 } else if is_mobile { 5.0 } else { 4.0 };
        let mid_alpha = if mid_active { 0.9 } else if is_mobile { 0.4 } else { 0.35 };
        ctx.set_fill_style_str(&format!("rgba(255, 180, 60, {:.2})", mid_alpha));
        ctx.begin_path();
        ctx.move_to(center_x, mid_y - mid_size);
        ctx.line_to(center_x + mid_size, mid_y);
        ctx.line_to(center_x, mid_y + mid_size);
        ctx.line_to(center_x - mid_size, mid_y);
        ctx.close_path();
        ctx.fill();
    }

    // BandFF range labels — only when hovering/dragging BandFF handles specifically
    let band_ff_handle_active = any_band_ff_active
        || matches!(drag_handle, Some(SpectrogramHandle::BandFfUpper | SpectrogramHandle::BandFfLower | SpectrogramHandle::BandFfMiddle));
    if band_ff_focused && band_ff_handle_active {
        ctx.set_font("11px sans-serif");
        let label_x = center_x + handle_zone_half + 8.0;

        // Top frequency label: just above the upper BandFF line
        let top_label = format!("{:.1} kHz", band_ff_hi / 1000.0);
        ctx.set_text_baseline("bottom");
        let top_metrics = ctx.measure_text(&top_label).unwrap();
        let bg_w = top_metrics.width() + 6.0;
        let bg_h = 15.0;
        ctx.set_fill_style_str("rgba(0, 0, 0, 0.7)");
        ctx.fill_rect(label_x - 3.0, y_top - 4.0 - bg_h, bg_w, bg_h);
        ctx.set_fill_style_str("rgba(255, 180, 60, 0.9)");
        let _ = ctx.fill_text(&top_label, label_x, y_top - 4.0);

        // Bottom frequency label: just below the lower BandFF line
        let bottom_label = format!("{:.1} kHz", band_ff_lo / 1000.0);
        ctx.set_text_baseline("top");
        let bot_metrics = ctx.measure_text(&bottom_label).unwrap();
        let bg_w = bot_metrics.width() + 6.0;
        ctx.set_fill_style_str("rgba(0, 0, 0, 0.7)");
        ctx.fill_rect(label_x - 3.0, y_bottom + 4.0, bg_w, bg_h);
        ctx.set_fill_style_str("rgba(255, 180, 60, 0.9)");
        let _ = ctx.fill_text(&bottom_label, label_x, y_bottom + 4.0);

        ctx.set_text_baseline("alphabetic");
    }
}

/// Draw the heterodyne frequency overlay: cyan center + band edge lines (no dimming — BandFF handles that).
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
            ctx.fill();
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
        ctx.fill();
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

    // Dotted border (distinguishes transient selection from annotations)
    let _ = ctx.set_line_dash(&js_sys::Array::of2(
        &wasm_bindgen::JsValue::from_f64(1.0),
        &wasm_bindgen::JsValue::from_f64(3.0),
    ));
    ctx.set_stroke_style_str("rgba(120, 190, 255, 0.95)");
    ctx.set_line_width(1.0);
    ctx.stroke_rect(x0, y0, x1 - x0, y1 - y0);
    let _ = ctx.set_line_dash(&js_sys::Array::new());
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
    let harmonics_bounds = harmonics_band_bounds(freq_low, freq_high, band_mode);

    // Determine the frequency range for the hovered band
    let (band_lo, band_hi, color) = match hovered_band {
        0 => (0.0, freq_low, "rgba(255, 80, 80, 0.15)"),       // below — red tint
        1 => (freq_low, freq_high, "rgba(80, 255, 120, 0.15)"), // selected — green
        2 if harmonics_bounds.is_some() => {
            let (harmonics_lower, harmonics_upper) = harmonics_bounds.expect("checked is_some");
            (harmonics_lower, harmonics_upper, "rgba(80, 120, 255, 0.15)")
        }
        3 => {
            let lo = harmonics_bounds.map(|(_, harmonics_upper)| harmonics_upper).unwrap_or(freq_high);
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
/// rendered (ideal vs fallback). Colors: LOD5 = magenta, LOD4 = cyan,
/// LOD3 = green, LOD2 = blue, LOD1 = yellow, LOD0 = orange, missing = red.
pub fn draw_tile_debug_overlay(
    ctx: &CanvasRenderingContext2d,
    viewport_width: f64,
    viewport_height: f64,
    file_idx: usize,
    total_cols: usize,
    scroll_col: f64,
    zoom: f64,
    fft_mode: FftMode,
    flow_on: bool,
) {
    use crate::canvas::tile_cache::{self, TILE_COLS};

    let cw = viewport_width;
    let ch = viewport_height;
    if total_cols == 0 || zoom <= 0.0 { return; }

    let ideal_lod = tile_cache::select_lod(zoom);
    let ratio = tile_cache::lod_ratio(ideal_lod);

    let vis_start = scroll_col.max(0.0).min((total_cols as f64 - 1.0).max(0.0));
    let vis_end = (vis_start + cw / zoom).min(total_cols as f64);

    let vis_start_lod = vis_start * ratio;
    let vis_end_lod = vis_end * ratio;

    let first_tile = (vis_start_lod / tile_cache::TILE_COLS as f64).floor() as usize;
    let last_tile = ((vis_end_lod - 0.001).max(0.0) / tile_cache::TILE_COLS as f64).floor() as usize;
    let stats = if flow_on {
        tile_cache::flow_debug_stats(file_idx, ideal_lod, first_tile, last_tile)
    } else {
        tile_cache::magnitude_debug_stats(file_idx, ideal_lod, first_tile, last_tile)
    };

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
            let c = match ideal_lod {
                5 => "#f6f",
                4 => "#0ff",
                3 => "#0f0",
                2 => "#48f",
                1 => "#ff0",
                0 => "#fa0",
                _ => "#fff",
            };
            (ideal_lod, tile_idx, label, c)
        } else if if flow_on {
            tile_cache::flow_tile_active(file_idx, ideal_lod, tile_idx)
        } else {
            tile_cache::magnitude_tile_active(file_idx, ideal_lod, tile_idx)
        } {
            (ideal_lod, tile_idx, "..".to_string(), "#fa0")
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
                    let c = match l {
                        0 => "#fa0",
                        1 => "#ff0",
                        2 => "#48f",
                        3 => "#0f0",
                        4 => "#0ff",
                        _ => "#f6f",
                    };
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
            let actual_fft = fft_mode.fft_for_lod(displayed_lod);
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
        let label_y = 20.0;
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

    // Draw telemetry panel in bottom-right so it doesn't overlap tile labels.
    let ideal_hop = tile_cache::LOD_CONFIGS[ideal_lod as usize].hop_size;
    let actual_fft = fft_mode.fft_for_lod(ideal_lod);
    let used_mb = stats.used_bytes as f64 / 1_048_576.0;
    let max_mb = stats.max_bytes as f64 / 1_048_576.0;
    let headroom_mb = stats.max_bytes.saturating_sub(stats.used_bytes) as f64 / 1_048_576.0;
    let fullness_pct = if stats.max_bytes > 0 {
        stats.used_bytes as f64 * 100.0 / stats.max_bytes as f64
    } else {
        0.0
    };
    let pressure = if fullness_pct >= 95.0 {
        "FULL"
    } else if fullness_pct >= 90.0 {
        "HIGH"
    } else if fullness_pct >= 75.0 {
        "WARM"
    } else {
        "OK"
    };
    let panel_lines = [format!("z={zoom:.1} LOD{ideal_lod} fft={actual_fft} hop={ideal_hop}"),
        format!("visible c:{} f:{} m:{}", stats.visible_cached, stats.visible_in_flight, stats.visible_missing),
        format!("cache {} / {} tiles", stats.total_cached, stats.total_in_flight),
        format!("mem {used_mb:.1} / {max_mb:.0} MB"),
        format!("headroom {headroom_mb:.1} MB  {fullness_pct:.0}%  {pressure}"),
        format!("range T{first_tile}..T{last_tile}")];
    let label_w = 248.0;
    let label_h = 14.0 * panel_lines.len() as f64 + 6.0;
    let panel_x = cw - label_w - 6.0;
    let panel_y = (ch - label_h - 6.0).max(3.0);
    ctx.set_fill_style_str("rgba(0,0,0,0.6)");
    ctx.fill_rect(panel_x, panel_y, label_w, label_h);
    for (idx, line) in panel_lines.iter().enumerate() {
        let color = if idx == 1 && stats.visible_missing > 0 {
            "#f88"
        } else if idx == 4 {
            match pressure {
                "FULL" => "#f66",
                "HIGH" => "#fa0",
                "WARM" => "#fd6",
                _ => "#9f9",
            }
        } else {
            "#fff"
        };
        ctx.set_fill_style_str(color);
        let _ = ctx.fill_text(line, panel_x + 4.0, panel_y + 3.0 + idx as f64 * 14.0);
    }

    ctx.restore();
}

/// Draw saved annotation selections as semi-transparent overlays.
pub fn draw_annotations(
    ctx: &web_sys::CanvasRenderingContext2d,
    annotation_set: &crate::annotations::AnnotationSet,
    selected_ids: &[String],
    hover_handle: Option<(&str, ResizeHandlePosition)>,
    min_freq: f64,
    max_freq: f64,
    scroll_offset: f64,
    time_resolution: f64,
    zoom: f64,
    canvas_width: f64,
    canvas_height: f64,
    is_mobile: bool,
    annotations_focused: bool,
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

        // Fill with dark-top → transparent gradient (distinguishes annotations from transient selection)
        let grad = ctx.create_linear_gradient(x0, y0, x0, y1);
        let (top_stop, bot_stop) = if is_selected {
            ("rgba(230, 170, 60, 0.42)", "rgba(230, 170, 60, 0.00)")
        } else {
            ("rgba(70, 220, 140, 0.32)", "rgba(70, 220, 140, 0.00)")
        };
        let _ = grad.add_color_stop(0.0, top_stop);
        let _ = grad.add_color_stop(1.0, bot_stop);
        ctx.set_fill_style_canvas_gradient(&grad);
        ctx.fill_rect(x0, y0, x1 - x0, y1 - y0);

        // Solid border
        let stroke_color = if is_selected {
            "rgba(255, 200, 80, 0.9)"
        } else {
            "rgba(100, 230, 160, 0.75)"
        };
        ctx.set_stroke_style_str(stroke_color);
        ctx.set_line_width(1.0);
        ctx.stroke_rect(x0, y0, x1 - x0, y1 - y0);

        // Label
        if let Some(ref label) = sel.label {
            let is_default = annotation.label_default.unwrap_or(false);
            let font = if is_default {
                "italic 11px monospace"
            } else {
                "11px monospace"
            };
            ctx.set_font(font);
            let label_color = if is_default {
                "rgba(170, 200, 185, 0.55)"
            } else {
                "rgba(210, 255, 220, 0.9)"
            };
            ctx.set_fill_style_str(label_color);
            let _ = ctx.fill_text(label, x0 + 3.0, y0 + 12.0);
        }

        // Resize handles for selected annotations (only when annotations have focus and not locked)
        if is_selected && annotations_focused && !sel.is_locked() {
            let handles = crate::canvas::hit_test::get_annotation_handle_positions(
                sel.time_start, sel.time_end,
                sel.freq_low, sel.freq_high,
                scroll_offset, time_resolution, zoom, canvas_width,
                min_freq, max_freq, canvas_height,
            );

            for (pos, hx, hy) in &handles {
                let is_hovered = hover_handle
                    .as_ref()
                    .is_some_and(|(hid, hp)| *hid == annotation.id && *hp == *pos);

                let size = if is_hovered {
                    if is_mobile { 8.0 } else { 4.0 }
                } else if is_mobile { 6.0 } else { 3.0 };

                let fill = if is_hovered {
                    "rgba(255, 220, 100, 1.0)"
                } else {
                    "rgba(255, 200, 80, 0.9)"
                };

                ctx.set_fill_style_str(fill);
                ctx.fill_rect(hx - size, hy - size, size * 2.0, size * 2.0);
                ctx.set_stroke_style_str("rgba(180, 120, 20, 0.9)");
                ctx.set_line_width(1.0);
                ctx.stroke_rect(hx - size, hy - size, size * 2.0, size * 2.0);
            }
        }
    }
}

/// Style for a time-position marker drawn as a vertical line + optional label.
#[derive(Clone, Copy, PartialEq)]
pub enum TimeMarkerStyle {
    /// File-embedded (WAV cue, M4A chapter) — muted cyan dashed line.
    FileEmbedded,
    /// User annotation marker — warm yellow dashed line.
    Annotation,
}

impl TimeMarkerStyle {
    fn stroke(self) -> &'static str {
        match self {
            Self::FileEmbedded => "rgba(120, 200, 220, 0.72)",
            Self::Annotation   => "rgba(255, 210, 90, 0.85)",
        }
    }
    fn label_bg(self) -> &'static str {
        match self {
            Self::FileEmbedded => "rgba(20, 45, 55, 0.78)",
            Self::Annotation   => "rgba(60, 45, 10, 0.85)",
        }
    }
    fn label_fg(self) -> &'static str {
        match self {
            Self::FileEmbedded => "rgba(200, 235, 245, 0.95)",
            Self::Annotation   => "rgba(255, 230, 160, 0.95)",
        }
    }
}

/// Draw vertical dashed lines at a list of timestamps, with optional labels
/// at the top of the canvas. Used for WAV cue points, M4A chapters, and
/// user-created annotation markers. All inputs share the same time/zoom
/// coordinate system as the spectrogram/waveform Effects.
pub fn draw_time_marker_lines(
    ctx: &CanvasRenderingContext2d,
    markers: &[(f64, Option<String>)],
    style: TimeMarkerStyle,
    scroll_offset: f64,
    time_resolution: f64,
    zoom: f64,
    canvas_width: f64,
    canvas_height: f64,
) {
    if markers.is_empty() || canvas_width <= 0.0 || canvas_height <= 0.0 { return; }
    let visible_time = (canvas_width / zoom) * time_resolution;
    if visible_time <= 0.0 { return; }
    let start_time = scroll_offset;
    let end_time = start_time + visible_time;
    let px_per_sec = canvas_width / visible_time;

    let dash = js_sys::Array::new();
    dash.push(&wasm_bindgen::JsValue::from_f64(5.0));
    dash.push(&wasm_bindgen::JsValue::from_f64(4.0));

    ctx.save();
    ctx.set_stroke_style_str(style.stroke());
    ctx.set_line_width(1.0);
    let _ = ctx.set_line_dash(&dash);
    ctx.set_font("11px system-ui, -apple-system, sans-serif");

    for (time, label) in markers {
        if *time < start_time || *time > end_time { continue; }
        let x = (time - start_time) * px_per_sec;

        ctx.begin_path();
        ctx.move_to(x + 0.5, 0.0);
        ctx.line_to(x + 0.5, canvas_height);
        ctx.stroke();

        if let Some(label) = label.as_ref().filter(|s| !s.is_empty()) {
            let tm = ctx.measure_text(label).ok();
            let text_w = tm.as_ref().map(|m| m.width()).unwrap_or(0.0);
            let pad_x = 4.0;
            let box_w = text_w + pad_x * 2.0;
            let box_h = 15.0;
            // Place label to the right of the line; if clipped, flip to the left.
            let box_x = if x + 2.0 + box_w <= canvas_width {
                x + 2.0
            } else {
                (x - 2.0 - box_w).max(0.0)
            };
            let text_x = box_x + pad_x;
            ctx.set_fill_style_str(style.label_bg());
            ctx.fill_rect(box_x, 2.0, box_w, box_h);
            ctx.set_fill_style_str(style.label_fg());
            let _ = ctx.fill_text(label, text_x, 13.0);
        }
    }

    let _ = ctx.set_line_dash(&js_sys::Array::new());
    ctx.restore();
}
