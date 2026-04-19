// Gutter renderer: draws the checkerboard ("fogged mirror") pattern and the
// active shield/solid overlay for band and time gutters. Gutters are a
// dedicated drag surface, visually distinct from axes; dragging on one
// "clears the fog" and reveals a shield (for frequency bands) or a solid
// highlight (for time ranges).

use web_sys::CanvasRenderingContext2d;
use crate::canvas::colors::{freq_resistor_bands, freq_shield_color};
use crate::canvas::overlays::{draw_bend_shield, draw_solid_shield};
use crate::state::ShieldStyle;

/// Size (px) of one checkerboard cell. Two cells fit across the gutter's
/// short axis, giving a ~40px-wide drag surface for finger-drag on mobile.
pub const CELL: f64 = 20.0;

/// Width (px) reserved on the left side of the band gutter for static
/// frequency labels + ticks (mirrors the spectrogram's y-axis markers).
pub const LABEL_COL_WIDTH: f64 = 22.0;

/// Recommended band gutter width (px) — label column + two 20px fog cells.
pub const BAND_GUTTER_WIDTH: f64 = LABEL_COL_WIDTH + 40.0;

/// Draw the fogged-mirror checkerboard. Two colour cells alternated across
/// a rectangle. `dim` is the checker brightness multiplier (1.0 = normal,
/// < 1.0 fades the fog — used when the gutter is inactive).
pub fn draw_fog(ctx: &CanvasRenderingContext2d, x: f64, y: f64, w: f64, h: f64, dim: f64) {
    // Solid background first
    ctx.set_fill_style_str("#0d0d0d");
    ctx.fill_rect(x, y, w, h);

    let pale = (60.0 * dim) as u8;
    let dark = (38.0 * dim) as u8;
    let pale_s = format!("rgb({p},{p},{p})", p = pale);
    let dark_s = format!("rgb({d},{d},{d})", d = dark);

    let cols = (w / CELL).ceil() as i32;
    let rows = (h / CELL).ceil() as i32;
    for r in 0..rows {
        for c in 0..cols {
            let cx = x + c as f64 * CELL;
            let cy = y + r as f64 * CELL;
            let cw = (x + w - cx).min(CELL);
            let ch = (y + h - cy).min(CELL);
            let light = (r + c) & 1 == 0;
            ctx.set_fill_style_str(if light { &pale_s } else { &dark_s });
            ctx.fill_rect(cx, cy, cw, ch);
        }
    }
}

/// Pick a frequency-division interval based on the total visible range.
/// Mirrors the spectrogram's `draw_freq_markers` adaptive-interval logic.
fn pick_div_interval(range_hz: f64) -> f64 {
    if range_hz <= 5_000.0 { 1_000.0 }
    else if range_hz <= 25_000.0 { 5_000.0 }
    else { 10_000.0 }
}

/// Draw the band gutter. Paints fog over the whole rectangle, then overlays
/// a stack of per-division shield flags covering the selected
/// [band_lo, band_hi] range. The colouring algorithm matches the
/// spectrogram's frequency markers (resistor-band bend shields per
/// 10 kHz / 5 kHz / 1 kHz division, depending on total range). When HFR
/// is off, the shields are drawn dim so the user still sees the last
/// selection.
///
/// `drag_range` is the live [start, current] range of an in-progress axis
/// drag (either from the gutter itself or from the spectrogram's y-axis).
/// When present it overrides the stored band and is always drawn at full
/// alpha — mirroring the spectrogram's `axis_drag_in_range` behaviour so
/// users see the band light up during the first drag even when HFR is off.
///
/// Coordinate mapping: `max_freq` is at y=0 (top), `min_freq` is at y=h
/// (bottom). The gutter mirrors whatever range the host view is currently
/// displaying — on the spectrogram that's min/max_display_freq, on views
/// without a display range it's 0..Nyquist — so its ticks line up with
/// the host's y-axis.
pub fn draw_band_gutter(
    ctx: &CanvasRenderingContext2d,
    w: f64, h: f64,
    min_freq: f64,
    max_freq: f64,
    band_lo: f64,
    band_hi: f64,
    hfr_on: bool,
    shield_style: ShieldStyle,
    drag_range: Option<(f64, f64)>,
) {
    // Clear the label column to the container background so the fog/shield
    // strip reads as a distinct interactive surface.
    ctx.set_fill_style_str("#0d0d0d");
    ctx.fill_rect(0.0, 0.0, LABEL_COL_WIDTH, h);

    // Fog/shield area lives to the right of the label column.
    let shield_x = LABEL_COL_WIDTH;
    let shield_w = (w - shield_x).max(0.0);

    // Background fog — slightly dimmer when HFR is off and no drag is
    // active. An in-progress drag brightens the fog too, so the whole
    // strip reads as "hot" during selection.
    let fog_dim = if hfr_on || drag_range.is_some() { 1.0 } else { 0.7 };
    draw_fog(ctx, shield_x, 0.0, shield_w, h, fog_dim);

    // Label divisions use the visible range's span — same adaptive rule as
    // the spectrogram's own axis labels, so ticks sit at identical y's.
    let range = (max_freq - min_freq).max(1.0);
    let div_for_labels = pick_div_interval(range);
    draw_left_axis_labels(ctx, h, min_freq, max_freq, div_for_labels);

    // Pick the range to paint: prefer the live drag range over the stored
    // band, so a fresh drag lights up immediately (before it's committed).
    let (draw_lo, draw_hi) = match drag_range {
        Some((s, c)) => (s.min(c), s.max(c)),
        None => (band_lo, band_hi),
    };

    if range <= 0.0 || draw_hi <= draw_lo || matches!(shield_style, ShieldStyle::Off) {
        return;
    }
    // Clamp to visible range so out-of-view band shields don't leak past
    // the gutter edges when the spectrogram is zoomed in.
    let lo_clamped = draw_lo.max(min_freq).min(max_freq);
    let hi_clamped = draw_hi.max(min_freq).min(max_freq);
    if hi_clamped <= lo_clamped { return; }

    // Match the spectrogram's y-mapping: min_freq at y=h, max_freq at y=0.
    let freq_y = |f: f64| -> f64 { h - ((f - min_freq) / range).clamp(0.0, 1.0) * h };

    let div_interval = div_for_labels;
    // Drag always paints bright; steady state fades when HFR is off so the
    // dashed-outline hint still reads as "last selection, inactive".
    let is_drag = drag_range.is_some();
    let alpha_active = if hfr_on || is_drag { 0.85 } else { 0.40 };
    let alpha_minor = if hfr_on || is_drag { 0.55 } else { 0.28 };

    // Major divisions from min_freq up to max_freq (aligned to div_interval).
    let first_div = ((min_freq / div_interval).ceil() * div_interval).max(div_interval);
    let mut freq = first_div;
    while freq < max_freq {
        let bar_top = (freq + div_interval).min(max_freq);
        if bar_top > lo_clamped && freq < hi_clamped {
            // Skip major bars below 20 kHz when minor 1 kHz bars will cover them
            let has_minor_coverage = div_interval >= 5_000.0 && freq < 20_000.0;
            if !has_minor_coverage {
                let clamped_lo = freq.max(lo_clamped);
                let clamped_hi = bar_top.min(hi_clamped);
                let y_top = freq_y(clamped_hi);
                let y_bot = freq_y(clamped_lo);
                let bar_h = y_bot - y_top;
                if bar_h >= 1.0 {
                    match shield_style {
                        ShieldStyle::Resistor => {
                            let bands = freq_resistor_bands(freq);
                            draw_bend_shield(ctx, shield_x, y_top, shield_w, bar_h, bands, alpha_active);
                        }
                        ShieldStyle::Solid => {
                            let c = freq_shield_color(freq, div_interval);
                            draw_solid_shield(ctx, shield_x, y_top, shield_w, bar_h, c, alpha_active);
                        }
                        ShieldStyle::Off => {}
                    }
                }
            }
        }
        freq += div_interval;
    }

    // Sub-20 kHz 1 kHz minor bars (matches spectrogram behaviour for the low range).
    if div_interval >= 5_000.0 {
        let minor_interval = 1_000.0;
        let minor_start = (min_freq / minor_interval).ceil().max(1.0) * minor_interval;
        let minor_end = max_freq.min(20_000.0);
        let mut mf = minor_start;
        while mf < minor_end {
            let bar_top = (mf + minor_interval).min(max_freq).min(20_000.0);
            if bar_top > lo_clamped && mf < hi_clamped {
                let clamped_lo = mf.max(lo_clamped);
                let clamped_hi = bar_top.min(hi_clamped);
                let y_top = freq_y(clamped_hi);
                let y_bot = freq_y(clamped_lo);
                let bar_h = y_bot - y_top;
                if bar_h >= 1.0 {
                    match shield_style {
                        ShieldStyle::Resistor => {
                            let bands = freq_resistor_bands(mf);
                            draw_bend_shield(ctx, shield_x, y_top, shield_w, bar_h, bands, alpha_minor);
                        }
                        ShieldStyle::Solid => {
                            let c = freq_shield_color(mf, minor_interval);
                            draw_solid_shield(ctx, shield_x, y_top, shield_w, bar_h, c, alpha_minor);
                        }
                        ShieldStyle::Off => {}
                    }
                }
            }
            mf += minor_interval;
        }
    }

    // Passive orientation ticks: short coloured stubs on the right edge at
    // every major division, always visible (regardless of selection). Helps
    // the user understand which resistor colours correspond to which
    // frequency without needing a numeric label on every flag.
    draw_right_edge_ticks(ctx, w, h, min_freq, max_freq, div_interval);

    // Dashed outline around the selected range when HFR is off (signals
    // "previously selected — tap to resume listening"). Skipped during an
    // active drag — the bright shields already convey "being selected".
    if !hfr_on && !is_drag {
        let y_top = freq_y(hi_clamped);
        let y_bot = freq_y(lo_clamped);
        ctx.save();
        ctx.set_stroke_style_str("rgba(255,255,255,0.55)");
        ctx.set_line_width(1.0);
        let _ = ctx.set_line_dash(&js_sys::Array::of2(
            &wasm_bindgen::JsValue::from_f64(3.0),
            &wasm_bindgen::JsValue::from_f64(3.0),
        ));
        ctx.stroke_rect(shield_x + 0.5, y_top + 0.5, shield_w - 1.0, (y_bot - y_top) - 1.0);
        let _ = ctx.set_line_dash(&js_sys::Array::new());
        ctx.restore();
    }
}

/// Draw the time gutter overlay as a thin strip positioned at the bottom
/// of the waveform canvas. Fog in empty regions, a solid highlight over
/// the selected time range (clipped to the visible window).
pub fn draw_time_gutter_overlay(
    ctx: &CanvasRenderingContext2d,
    x: f64, y: f64, w: f64, h: f64,
    visible_start: f64, visible_end: f64,
    selection: Option<(f64, f64)>,
) {
    draw_fog(ctx, x, y, w, h, 1.0);

    if let Some((sel_start, sel_end)) = selection {
        let vis_span = (visible_end - visible_start).max(f64::EPSILON);
        let clamped_start = sel_start.max(visible_start).min(visible_end);
        let clamped_end = sel_end.max(visible_start).min(visible_end);
        if clamped_end > clamped_start {
            let x0 = x + (clamped_start - visible_start) / vis_span * w;
            let x1 = x + (clamped_end - visible_start) / vis_span * w;
            ctx.set_fill_style_str("rgba(100,160,255,0.55)");
            ctx.fill_rect(x0, y + 1.0, x1 - x0, h - 2.0);
            ctx.set_stroke_style_str("rgba(160,200,255,0.9)");
            ctx.set_line_width(1.0);
            ctx.stroke_rect(x0 + 0.5, y + 1.5, x1 - x0 - 1.0, h - 3.0);
        }
    }

    // Separator line at top of the strip
    ctx.set_fill_style_str("rgba(0,0,0,0.6)");
    ctx.fill_rect(x, y, w, 1.0);
}

/// Draw numeric frequency labels and short coloured ticks in the left
/// label column of the band gutter. Matches the spectrogram's y-axis
/// marker style so the two scales read as one consistent system. Always
/// drawn (independent of selection) — this is the static frequency axis
/// for the gutter.
fn draw_left_axis_labels(
    ctx: &CanvasRenderingContext2d,
    h: f64,
    min_freq: f64,
    max_freq: f64,
    div_interval: f64,
) {
    let range = (max_freq - min_freq).max(1.0);
    if h <= 0.0 || range <= 0.0 { return; }

    // Tick spans the 4px immediately left of the shield edge; label
    // right-aligned just inside that, with a 2px gap from the tick.
    let tick_x1 = LABEL_COL_WIDTH;
    let tick_x0 = LABEL_COL_WIDTH - 4.0;
    let label_right_x = LABEL_COL_WIDTH - 6.0;

    ctx.save();
    ctx.set_font("10px sans-serif");
    ctx.set_text_baseline("middle");
    ctx.set_text_align("right");

    // Skip labels near the very top/bottom so they don't crowd the gutter
    // ends (and so the implicit 0 Hz / Nyquist bounds stay uncluttered).
    let first_div = ((min_freq / div_interval).ceil() * div_interval).max(div_interval);
    let mut freq = first_div;
    while freq < max_freq {
        let y = freq_to_y(freq, min_freq, max_freq, h);
        if y < 8.0 || y > h - 6.0 {
            freq += div_interval;
            continue;
        }

        // Tick: colour-tinted by the shield resistor colour, lightened so
        // it reads against the dark label column.
        let c = freq_shield_color(freq, div_interval);
        let r = 160 + (c[0] as u16 * 95 / 255) as u8;
        let g = 160 + (c[1] as u16 * 95 / 255) as u8;
        let b = 160 + (c[2] as u16 * 95 / 255) as u8;
        ctx.set_stroke_style_str(&format!("rgba({},{},{},0.85)", r, g, b));
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.move_to(tick_x0, y + 0.5);
        ctx.line_to(tick_x1, y + 0.5);
        ctx.stroke();

        // Shadow first (for contrast), then the label in soft white.
        let label = format!("{}", (freq / 1000.0).round() as u32);
        ctx.set_fill_style_str("rgba(0,0,0,0.85)");
        let _ = ctx.fill_text(&label, label_right_x + 0.5, y + 0.5);
        ctx.set_fill_style_str("rgba(230,230,230,0.92)");
        let _ = ctx.fill_text(&label, label_right_x, y);

        freq += div_interval;
    }

    ctx.restore();
}

/// Draw small coloured tick stubs on the right edge at each major division
/// and faint 1 kHz minor ticks below 20 kHz. Colour tint hints at the
/// resistor-band digit so the user can orient themselves without reading
/// labels. Always drawn (independent of selection).
fn draw_right_edge_ticks(
    ctx: &CanvasRenderingContext2d,
    w: f64, h: f64,
    min_freq: f64,
    max_freq: f64,
    div_interval: f64,
) {
    let range = (max_freq - min_freq).max(1.0);
    if h <= 0.0 || range <= 0.0 { return; }

    // Major ticks — 4 px, 70% alpha, tinted with the frequency's marker colour.
    let first_div = ((min_freq / div_interval).ceil() * div_interval).max(div_interval);
    let mut freq = first_div;
    while freq < max_freq {
        let y = freq_to_y(freq, min_freq, max_freq, h);
        let c = freq_shield_color(freq, div_interval);
        // Lighten toward white so ticks read against dark fog.
        let r = 160 + (c[0] as u16 * 95 / 255) as u8;
        let g = 160 + (c[1] as u16 * 95 / 255) as u8;
        let b = 160 + (c[2] as u16 * 95 / 255) as u8;
        ctx.set_fill_style_str(&format!("rgba({r},{g},{b},0.7)"));
        ctx.fill_rect(w - 4.0, y - 0.5, 4.0, 1.0);
        freq += div_interval;
    }

    // Minor 1 kHz ticks below 20 kHz (only when major interval is coarse).
    if div_interval >= 5_000.0 {
        let minor_start = (min_freq / 1_000.0).ceil().max(1.0) * 1_000.0;
        let minor_end = max_freq.min(20_000.0);
        let mut mf = minor_start;
        while mf < minor_end {
            // Skip where a major tick already sits
            let ratio = mf / div_interval;
            let is_major = (ratio - ratio.round()).abs() < 0.001;
            if !is_major {
                let y = freq_to_y(mf, min_freq, max_freq, h);
                ctx.set_fill_style_str("rgba(180,180,180,0.3)");
                ctx.fill_rect(w - 2.0, y - 0.5, 2.0, 1.0);
            }
            mf += 1_000.0;
        }
    }
}

/// Map a frequency (Hz) to a Y pixel in a gutter of height `h` where
/// `max_freq` is at the top (y=0) and `min_freq` is at the bottom (y=h).
pub fn freq_to_y(freq: f64, min_freq: f64, max_freq: f64, h: f64) -> f64 {
    let range = (max_freq - min_freq).max(1.0);
    let f = freq.clamp(min_freq, max_freq);
    h - ((f - min_freq) / range) * h
}

/// Inverse of `freq_to_y`: map a Y pixel to a frequency (Hz) within the
/// visible range.
pub fn y_to_freq(y: f64, min_freq: f64, max_freq: f64, h: f64) -> f64 {
    if h <= 0.0 { return min_freq; }
    let range = (max_freq - min_freq).max(0.0);
    let frac = (1.0 - (y / h)).clamp(0.0, 1.0);
    min_freq + frac * range
}
