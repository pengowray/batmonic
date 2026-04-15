use web_sys::CanvasRenderingContext2d;

use crate::viewport;

/// Common viewport calculation for waveform rendering.
struct WaveViewport {
    start_time: f64,
    data_x: f64,
    data_width: f64,
    px_per_sec: f64,
    samples_per_pixel: f64,
    mid_y: f64,
    /// Sample index corresponding to the first element of the provided buffer.
    region_start_sample: usize,
}

fn compute_viewport(
    total_duration: f64,
    sample_rate: u32,
    scroll_offset: f64,
    zoom: f64,
    time_resolution: f64,
    canvas_width: f64,
    canvas_height: f64,
    region_start_sample: usize,
) -> WaveViewport {
    let mid_y = canvas_height / 2.0;
    let visible_time = viewport::visible_time(canvas_width, zoom, time_resolution);
    let px_per_sec = if visible_time > 0.0 { canvas_width / visible_time } else { 0.0 };
    let (start_time, data_x, data_width) = viewport::data_region_px(
        scroll_offset,
        visible_time,
        total_duration,
        canvas_width,
    )
    .map(|(data_start, _data_end, dst_x, dst_w)| (data_start, dst_x, dst_w))
    .unwrap_or((0.0, 0.0, 0.0));
    let samples_per_pixel = if data_width > 0.0 {
        ((data_width / px_per_sec) * sample_rate as f64) / data_width
    } else {
        0.0
    };
    WaveViewport {
        start_time,
        data_x,
        data_width,
        px_per_sec,
        samples_per_pixel,
        mid_y,
        region_start_sample,
    }
}

/// Draw a single waveform layer with the given color.
fn draw_waveform_layer(
    ctx: &CanvasRenderingContext2d,
    samples: &[f32],
    sample_rate: u32,
    vp: &WaveViewport,
    canvas_width: f64,
    color: &str,
    gain_linear: f64,
) {
    if vp.data_width <= 0.0 || vp.px_per_sec <= 0.0 || vp.samples_per_pixel <= 0.0 {
        return;
    }

    ctx.set_stroke_style_str(color);
    ctx.set_line_width(1.0);

    let off = vp.region_start_sample;
    let px_start = vp.data_x.floor().max(0.0) as usize;
    let px_end = (vp.data_x + vp.data_width).ceil().min(canvas_width).max(vp.data_x) as usize;

    if vp.samples_per_pixel <= 2.0 {
        ctx.begin_path();
        let mut first = true;
        for px in px_start..px_end {
            let x = px as f64;
            let t = vp.start_time + ((x - vp.data_x) / vp.px_per_sec);
            let abs_idx = (t * sample_rate as f64) as usize;
            if abs_idx < off {
                continue;
            }
            let idx = abs_idx - off;
            if idx >= samples.len() {
                break;
            }
            let y = vp.mid_y - (samples[idx] as f64 * gain_linear * vp.mid_y * 0.9);
            if first {
                ctx.move_to(x, y);
                first = false;
            } else {
                ctx.line_to(x, y);
            }
        }
        ctx.stroke();
    } else {
        // Batch all vertical lines into a single path to minimize WASM→JS bridge calls.
        ctx.begin_path();
        for px in px_start..px_end {
            let x = px as f64;
            let t0 = vp.start_time + ((x - vp.data_x) / vp.px_per_sec);
            let t1 = vp.start_time + ((x + 1.0 - vp.data_x) / vp.px_per_sec);
            let abs_i0 = (t0 * sample_rate as f64) as usize;
            let abs_i1 = (t1 * sample_rate as f64) as usize;
            if abs_i1 <= off {
                continue;
            }
            let i0 = abs_i0.saturating_sub(off).min(samples.len());
            let i1 = abs_i1.saturating_sub(off).min(samples.len());

            if i0 >= i1 || i0 >= samples.len() {
                continue;
            }

            let mut min_val = f32::MAX;
            let mut max_val = f32::MIN;
            for &s in &samples[i0..i1] {
                if s < min_val { min_val = s; }
                if s > max_val { max_val = s; }
            }

            let y_min = vp.mid_y - (max_val as f64 * gain_linear * vp.mid_y * 0.9);
            let y_max = vp.mid_y - (min_val as f64 * gain_linear * vp.mid_y * 0.9);

            ctx.move_to(x, y_min);
            ctx.line_to(x, y_max);
        }
        ctx.stroke();
    }
}

/// Draw selection highlight.
fn draw_selection(
    ctx: &CanvasRenderingContext2d,
    selection: Option<(f64, f64)>,
    vp: &WaveViewport,
    canvas_width: f64,
    canvas_height: f64,
) {
    if let Some((sel_start, sel_end)) = selection {
        let x0 = (vp.data_x + (sel_start - vp.start_time) * vp.px_per_sec).max(0.0);
        let x1 = (vp.data_x + (sel_end - vp.start_time) * vp.px_per_sec).min(canvas_width);
        if x1 > x0 {
            ctx.set_fill_style_str("rgba(50, 120, 200, 0.2)");
            ctx.fill_rect(x0, 0.0, x1 - x0, canvas_height);
        }
    }
}

/// Draw center line.
fn draw_center_line(ctx: &CanvasRenderingContext2d, mid_y: f64, canvas_width: f64) {
    ctx.set_stroke_style_str("#333");
    ctx.set_line_width(1.0);
    ctx.begin_path();
    ctx.move_to(0.0, mid_y);
    ctx.line_to(canvas_width, mid_y);
    ctx.stroke();
}

/// Draw waveform on a canvas context.
/// Uses min/max envelope at low zoom, individual samples at high zoom.
pub fn draw_waveform(
    ctx: &CanvasRenderingContext2d,
    samples: &[f32],
    sample_rate: u32,
    scroll_offset: f64,
    zoom: f64,
    time_resolution: f64,
    canvas_width: f64,
    canvas_height: f64,
    selection: Option<(f64, f64)>,
    gain_db: f64,
    total_duration: f64,
    region_start_sample: usize,
) {
    ctx.set_fill_style_str("#0a0a0a");
    ctx.fill_rect(0.0, 0.0, canvas_width, canvas_height);

    if samples.is_empty() {
        return;
    }

    let gain_linear = 10.0_f64.powf(gain_db / 20.0);
    let vp = compute_viewport(total_duration, sample_rate, scroll_offset, zoom, time_resolution, canvas_width, canvas_height, region_start_sample);
    draw_selection(ctx, selection, &vp, canvas_width, canvas_height);
    draw_center_line(ctx, vp.mid_y, canvas_width);
    draw_waveform_layer(ctx, samples, sample_rate, &vp, canvas_width, "#6a6", gain_linear);
}

/// Draw dual waveform for HFR mode: original in dim color, bandpass-filtered overlay in bright cyan.
pub fn draw_waveform_hfr(
    ctx: &CanvasRenderingContext2d,
    samples: &[f32],
    filtered_samples: &[f32],
    sample_rate: u32,
    scroll_offset: f64,
    zoom: f64,
    time_resolution: f64,
    canvas_width: f64,
    canvas_height: f64,
    selection: Option<(f64, f64)>,
    gain_db: f64,
    total_duration: f64,
    region_start_sample: usize,
) {
    ctx.set_fill_style_str("#0a0a0a");
    ctx.fill_rect(0.0, 0.0, canvas_width, canvas_height);

    if samples.is_empty() {
        return;
    }

    let gain_linear = 10.0_f64.powf(gain_db / 20.0);
    let vp = compute_viewport(total_duration, sample_rate, scroll_offset, zoom, time_resolution, canvas_width, canvas_height, region_start_sample);
    draw_selection(ctx, selection, &vp, canvas_width, canvas_height);
    draw_center_line(ctx, vp.mid_y, canvas_width);

    // Original waveform in dim color
    draw_waveform_layer(ctx, samples, sample_rate, &vp, canvas_width, "#444", gain_linear);

    // Filtered (HFR content) waveform overlay in bright cyan
    if !filtered_samples.is_empty() {
        draw_waveform_layer(ctx, filtered_samples, sample_rate, &vp, canvas_width, "#0cf", gain_linear);
    }
}

/// Draw a waveform layer into a vertical sub-region of the canvas.
/// `y_offset` and `lane_height` define the vertical band to render into.
fn draw_waveform_layer_lane(
    ctx: &CanvasRenderingContext2d,
    samples: &[f32],
    sample_rate: u32,
    vp: &WaveViewport,
    canvas_width: f64,
    color: &str,
    gain_linear: f64,
    y_offset: f64,
    lane_height: f64,
) {
    if vp.data_width <= 0.0 || vp.px_per_sec <= 0.0 || vp.samples_per_pixel <= 0.0 || lane_height <= 0.0 {
        return;
    }

    ctx.set_stroke_style_str(color);
    ctx.set_line_width(1.0);

    let mid_y = y_offset + lane_height / 2.0;
    let half_h = lane_height / 2.0;
    let off = vp.region_start_sample;
    let px_start = vp.data_x.floor().max(0.0) as usize;
    let px_end = (vp.data_x + vp.data_width).ceil().min(canvas_width).max(vp.data_x) as usize;

    if vp.samples_per_pixel <= 2.0 {
        ctx.begin_path();
        let mut first = true;
        for px in px_start..px_end {
            let x = px as f64;
            let t = vp.start_time + ((x - vp.data_x) / vp.px_per_sec);
            let abs_idx = (t * sample_rate as f64) as usize;
            if abs_idx < off { continue; }
            let idx = abs_idx - off;
            if idx >= samples.len() { break; }
            let y = mid_y - (samples[idx] as f64 * gain_linear * half_h * 0.9);
            if first { ctx.move_to(x, y); first = false; } else { ctx.line_to(x, y); }
        }
        ctx.stroke();
    } else {
        ctx.begin_path();
        for px in px_start..px_end {
            let x = px as f64;
            let t0 = vp.start_time + ((x - vp.data_x) / vp.px_per_sec);
            let t1 = vp.start_time + ((x + 1.0 - vp.data_x) / vp.px_per_sec);
            let abs_i0 = (t0 * sample_rate as f64) as usize;
            let abs_i1 = (t1 * sample_rate as f64) as usize;
            if abs_i1 <= off { continue; }
            let i0 = abs_i0.saturating_sub(off).min(samples.len());
            let i1 = abs_i1.saturating_sub(off).min(samples.len());
            if i0 >= i1 || i0 >= samples.len() { continue; }

            let mut min_val = f32::MAX;
            let mut max_val = f32::MIN;
            for &s in &samples[i0..i1] {
                if s < min_val { min_val = s; }
                if s > max_val { max_val = s; }
            }

            let y_min = mid_y - (max_val as f64 * gain_linear * half_h * 0.9);
            let y_max = mid_y - (min_val as f64 * gain_linear * half_h * 0.9);
            ctx.move_to(x, y_min);
            ctx.line_to(x, y_max);
        }
        ctx.stroke();
    }
}

/// Draw frequency overlay waveform: full waveform in dim green behind,
/// selected frequency band in semi-transparent blue on top.
pub fn draw_waveform_freq(
    ctx: &CanvasRenderingContext2d,
    samples: &[f32],
    filtered_samples: &[f32],
    sample_rate: u32,
    scroll_offset: f64,
    zoom: f64,
    time_resolution: f64,
    canvas_width: f64,
    canvas_height: f64,
    selection: Option<(f64, f64)>,
    gain_db: f64,
    total_duration: f64,
    region_start_sample: usize,
) {
    ctx.set_fill_style_str("#0a0a0a");
    ctx.fill_rect(0.0, 0.0, canvas_width, canvas_height);

    if samples.is_empty() {
        return;
    }

    let gain_linear = 10.0_f64.powf(gain_db / 20.0);
    let vp = compute_viewport(total_duration, sample_rate, scroll_offset, zoom, time_resolution, canvas_width, canvas_height, region_start_sample);
    draw_selection(ctx, selection, &vp, canvas_width, canvas_height);
    draw_center_line(ctx, vp.mid_y, canvas_width);

    // Full waveform behind in dim green
    draw_waveform_layer(ctx, samples, sample_rate, &vp, canvas_width, "rgba(100, 170, 100, 0.35)", gain_linear);

    // Selected frequency band overlay in semi-transparent blue
    if !filtered_samples.is_empty() {
        draw_waveform_layer(ctx, filtered_samples, sample_rate, &vp, canvas_width, "rgba(80, 140, 255, 0.7)", gain_linear);
    }
}

/// Draw triple-band waveform: three stacked channels for above, selected, and below.
pub fn draw_waveform_triple(
    ctx: &CanvasRenderingContext2d,
    below_samples: &[f32],
    selected_samples: &[f32],
    above_samples: &[f32],
    sample_rate: u32,
    scroll_offset: f64,
    zoom: f64,
    time_resolution: f64,
    canvas_width: f64,
    canvas_height: f64,
    selection: Option<(f64, f64)>,
    gain_db: f64,
    total_duration: f64,
    region_start_sample: usize,
) {
    ctx.set_fill_style_str("#0a0a0a");
    ctx.fill_rect(0.0, 0.0, canvas_width, canvas_height);

    let gain_linear = 10.0_f64.powf(gain_db / 20.0);
    let vp = compute_viewport(total_duration, sample_rate, scroll_offset, zoom, time_resolution, canvas_width, canvas_height, region_start_sample);
    draw_selection(ctx, selection, &vp, canvas_width, canvas_height);

    let lane_height = canvas_height / 3.0;

    // Draw lane dividers
    ctx.set_stroke_style_str("#333");
    ctx.set_line_width(1.0);
    for i in 0..3 {
        let mid = i as f64 * lane_height + lane_height / 2.0;
        ctx.begin_path();
        ctx.move_to(0.0, mid);
        ctx.line_to(canvas_width, mid);
        ctx.stroke();
    }
    // Draw lane borders
    ctx.set_stroke_style_str("#222");
    for i in 1..3 {
        let y = i as f64 * lane_height;
        ctx.begin_path();
        ctx.move_to(0.0, y);
        ctx.line_to(canvas_width, y);
        ctx.stroke();
    }

    // Above band (top lane) — orange/amber
    if !above_samples.is_empty() {
        draw_waveform_layer_lane(ctx, above_samples, sample_rate, &vp, canvas_width,
            "rgba(220, 160, 60, 0.8)", gain_linear, 0.0, lane_height);
    }

    // Selected band (middle lane) — blue
    if !selected_samples.is_empty() {
        draw_waveform_layer_lane(ctx, selected_samples, sample_rate, &vp, canvas_width,
            "rgba(80, 140, 255, 0.85)", gain_linear, lane_height, lane_height);
    }

    // Below band (bottom lane) — green
    if !below_samples.is_empty() {
        draw_waveform_layer_lane(ctx, below_samples, sample_rate, &vp, canvas_width,
            "rgba(100, 200, 100, 0.7)", gain_linear, lane_height * 2.0, lane_height);
    }

    // Lane labels
    ctx.set_fill_style_str("rgba(255, 255, 255, 0.3)");
    ctx.set_font("10px sans-serif");
    let _ = ctx.fill_text("Above", 4.0, 12.0);
    let _ = ctx.fill_text("Selected", 4.0, lane_height + 12.0);
    let _ = ctx.fill_text("Below", 4.0, lane_height * 2.0 + 12.0);
}

/// Draw a zero-crossing rate graph from pre-computed bins.
/// `bins` is a slice of (rate_hz, is_armed) with fixed `bin_duration` spacing.
pub fn draw_zc_rate(
    ctx: &CanvasRenderingContext2d,
    bins: &[(f64, bool)],
    bin_duration: f64,
    total_duration: f64,
    scroll_offset: f64,
    zoom: f64,
    time_resolution: f64,
    canvas_width: f64,
    canvas_height: f64,
    selection: Option<(f64, f64)>,
    max_freq_khz: f64,
) {
    ctx.set_fill_style_str("#0a0a0a");
    ctx.fill_rect(0.0, 0.0, canvas_width, canvas_height);

    if bins.is_empty() {
        return;
    }

    let visible_time = viewport::visible_time(canvas_width, zoom, time_resolution);
    let Some((start_time, end_time, data_x, _data_width)) = viewport::data_region_px(
        scroll_offset,
        visible_time,
        total_duration,
        canvas_width,
    ) else {
        return;
    };
    let px_per_sec = canvas_width / visible_time;

    // Selection highlight
    if let Some((sel_start, sel_end)) = selection {
        let x0 = (data_x + (sel_start - start_time) * px_per_sec).max(0.0);
        let x1 = (data_x + (sel_end - start_time) * px_per_sec).min(canvas_width);
        if x1 > x0 {
            ctx.set_fill_style_str("rgba(50, 120, 200, 0.2)");
            ctx.fill_rect(x0, 0.0, x1 - x0, canvas_height);
        }
    }

    let max_freq_hz = max_freq_khz * 1000.0;

    // Horizontal grid lines
    ctx.set_stroke_style_str("#222");
    ctx.set_line_width(1.0);
    let grid_freqs = [20.0, 40.0, 60.0, 80.0, 100.0, 120.0];
    ctx.set_fill_style_str("#555");
    ctx.set_font("10px monospace");
    for &freq_khz in &grid_freqs {
        if freq_khz >= max_freq_khz {
            break;
        }
        let y = canvas_height * (1.0 - freq_khz / max_freq_khz);
        ctx.begin_path();
        ctx.move_to(0.0, y);
        ctx.line_to(canvas_width, y);
        ctx.stroke();
        let _ = ctx.fill_text(&format!("{:.0}k", freq_khz), 2.0, y - 2.0);
    }

    // Only iterate visible bins
    let first_bin = ((start_time / bin_duration) as usize).saturating_sub(1);
    let last_bin = ((end_time / bin_duration) as usize + 2).min(bins.len());

    for (bin_idx, &(rate_hz, armed)) in bins.iter().enumerate().take(last_bin).skip(first_bin) {
        if rate_hz <= 0.0 {
            continue;
        }

        let bin_time = bin_idx as f64 * bin_duration;
        let x = data_x + (bin_time - start_time) * px_per_sec;
        let bar_w = (bin_duration * px_per_sec).max(1.0);

        let bar_h = (rate_hz / max_freq_hz * canvas_height).min(canvas_height);
        let y = canvas_height - bar_h;

        if armed {
            ctx.set_fill_style_str("rgba(100, 200, 100, 0.8)");
        } else {
            ctx.set_fill_style_str("rgba(60, 130, 60, 0.35)");
        }
        ctx.fill_rect(x, y, bar_w, bar_h);
    }
}
