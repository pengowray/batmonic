use std::cell::RefCell;

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
    let px_per_sec = if visible_time > 0.0 {
        canvas_width / visible_time
    } else {
        0.0
    };
    let (start_time, data_x, data_width) =
        viewport::data_region_px(scroll_offset, visible_time, total_duration, canvas_width)
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
    let px_end = (vp.data_x + vp.data_width)
        .ceil()
        .min(canvas_width)
        .max(vp.data_x) as usize;

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
                if s < min_val {
                    min_val = s;
                }
                if s > max_val {
                    max_val = s;
                }
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

/// Default green used for the Simple waveform view.
pub const WAVEFORM_GREEN: &str = "#6a6";
/// Blue that matches the Frequency view's selected-band overlay, used as
/// the single-wave colour when HFR is off in Band-wave mode.
pub const WAVEFORM_BLUE: &str = "rgba(80, 140, 255, 0.9)";

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
    stroke_color: &str,
) {
    ctx.set_fill_style_str("#0a0a0a");
    ctx.fill_rect(0.0, 0.0, canvas_width, canvas_height);

    if samples.is_empty() {
        return;
    }

    let gain_linear = 10.0_f64.powf(gain_db / 20.0);
    let vp = compute_viewport(
        total_duration,
        sample_rate,
        scroll_offset,
        zoom,
        time_resolution,
        canvas_width,
        canvas_height,
        region_start_sample,
    );
    draw_selection(ctx, selection, &vp, canvas_width, canvas_height);
    draw_center_line(ctx, vp.mid_y, canvas_width);
    draw_waveform_layer(
        ctx,
        samples,
        sample_rate,
        &vp,
        canvas_width,
        stroke_color,
        gain_linear,
    );
}

// ── Min/max mip for the Waveform view (zoomed-out scan elimination) ───────────
//
// The Waveform Effect re-runs every scroll frame and `draw_waveform_layer`
// re-scans the visible window for per-pixel min/max — O(visible samples), i.e.
// millions/frame at fit zoom (benchmark: Waveform fit @384k = 32 fps vs the
// tile-cached spectrogram's 90). This caches a decimated (min,max) envelope
// anchored to ABSOLUTE sample positions and folds in only the NEW samples each
// frame — exactly like the live overview's `LiveWaveEnvelope`, just decimated —
// so a stationary buffer is free and rendering scans ~spp/MIP_D cells per pixel
// instead of spp raw samples. Lossless for min/max (max-of-cell-maxes = true
// max over the covered span). Works for BOTH a static file (fold once,
// abs_offset = 0) and the live sliding ring (fold incrementally, abs_offset =
// captured − ring_len). Used only when spp >= MIP_D; zoomed-in (spp < MIP_D)
// keeps the raw path, which needs sub-cell resolution anyway.

/// Samples per mip cell. At spp >= this, the mip render beats a raw scan.
pub const MIP_D: usize = 256;

thread_local! {
    static WAVE_MIP: RefCell<Option<WaveMip>> = const { RefCell::new(None) };
    /// Diagnostic counters for the Waveform path, read by the benchmark:
    /// (draw_calls, draw_ms, read_ms, effect_ms, effect_runs, mip_calls, rebuilds, spp).
    /// `read_ms` is the visible-window fetch (borrow vs read_region alloc);
    /// `effect_ms`/`effect_runs` is the WHOLE Waveform Effect body, so we can tell
    /// whether the per-frame cost is in the Effect at all or external to it.
    static WF_DIAG: RefCell<(u32, f64, f64, f64, u32, u32, u32, f64)> =
        const { RefCell::new((0, 0.0, 0.0, 0.0, 0, 0, 0, 0.0)) };
}

/// Record one Waveform draw for diagnostics (cheap; two `performance.now()`).
pub fn wf_diag_record(ms: f64, used_mip: bool, spp: f64) {
    WF_DIAG.with(|c| {
        let mut d = c.borrow_mut();
        d.0 += 1;
        d.1 += ms;
        if used_mip {
            d.5 += 1;
        }
        d.7 = spp;
    });
}

/// Record the per-frame visible-window fetch time (Cow build: borrow or alloc).
pub fn wf_diag_record_read(ms: f64) {
    WF_DIAG.with(|c| c.borrow_mut().2 += ms);
}

/// Record one full Waveform Effect-body run time.
pub fn wf_diag_record_effect(ms: f64) {
    WF_DIAG.with(|c| {
        let mut d = c.borrow_mut();
        d.3 += ms;
        d.4 += 1;
    });
}

/// Take + reset the Waveform diagnostic counters:
/// (draw_calls, draw_ms, read_ms, effect_ms, effect_runs, mip_calls, rebuilds, spp).
pub fn take_wf_diag() -> (u32, f64, f64, f64, u32, u32, u32, f64) {
    WF_DIAG.with(|c| {
        let mut d = c.borrow_mut();
        let r = *d;
        *d = (0, 0.0, 0.0, 0.0, 0, 0, 0, 0.0);
        r
    })
}

struct WaveMip {
    /// Ring of `(min, max)` cells; the cell for absolute cell-index `c` lives at
    /// `c % cap`. Each cell spans `MIP_D` absolute samples.
    cells: Vec<(f32, f32)>,
    cap: usize,
    /// Highest absolute cell index folded; `-1` when empty.
    head: i64,
    /// Lowest absolute cell index still valid (written and within the ring);
    /// `-1` when empty. Fences un-refilled gaps after a stall-rebuild.
    tail: i64,
    /// Absolute samples folded so far. Monotonic within a session.
    consumed: u64,
    /// Identity: `(buffer ptr, buffer len, channel id)`. A change forces a
    /// rebuild. `len` is included so a freed buffer whose heap address is reused
    /// by a different file can't be mistaken for the same buffer (ABA).
    key: (usize, usize, u8),
}

impl WaveMip {
    fn new(key: (usize, usize, u8), cap: usize) -> Self {
        let cap = cap.max(4);
        WaveMip {
            cells: vec![(0.0, 0.0); cap],
            cap,
            head: -1,
            tail: -1,
            consumed: 0,
            key,
        }
    }

    /// Fold the buffer's newest samples into the envelope. `abs_latest` is the
    /// absolute index just past the newest sample (static: buffer.len(); live:
    /// total captured). The buffer's last `len` samples are absolute
    /// `[abs_latest - len, abs_latest)`.
    fn fold(&mut self, abs_latest: u64, buf: &[f32]) {
        if abs_latest <= self.consumed {
            return; // nothing new (static after first fold, or an idle live tick)
        }
        let buf_len = buf.len() as u64;
        let mut start = self.consumed;
        let mut new = abs_latest - self.consumed;
        if new > buf_len {
            // Lost data (fresh mip, or a long live stall): restart from what the
            // buffer still holds; `tail` fences the un-refilled gap.
            self.head = -1;
            self.tail = -1;
            start = abs_latest - buf_len;
            new = buf_len;
        }
        let base = buf.len() - new as usize;
        let cap = self.cap;
        for k in 0..new as usize {
            let abs_idx = start + k as u64;
            let s = buf[base + k];
            let c = (abs_idx / MIP_D as u64) as i64;
            if c > self.head {
                if self.head < 0 {
                    self.tail = c;
                }
                self.cells[(c as usize) % cap] = (s, s);
                self.head = c;
                let min_valid = self.head - cap as i64 + 1;
                if self.tail < min_valid {
                    self.tail = min_valid;
                }
            } else {
                let cell = &mut self.cells[(c as usize) % cap];
                if s < cell.0 {
                    cell.0 = s;
                }
                if s > cell.1 {
                    cell.1 = s;
                }
            }
        }
        self.consumed = start + new;
    }

    /// `(min, max)` over absolute cell range `[c0, c1]` (inclusive), clamped to
    /// the valid window; `None` if the range holds no valid cell.
    fn range(&self, c0: i64, c1: i64) -> Option<(f32, f32)> {
        let lo = c0.max(self.tail);
        let hi = c1.min(self.head);
        if lo > hi || self.head < 0 {
            return None;
        }
        let mut mn = f32::MAX;
        let mut mx = f32::MIN;
        for c in lo..=hi {
            let (cmn, cmx) = self.cells[(c as usize) % self.cap];
            if cmn < mn {
                mn = cmn;
            }
            if cmx > mx {
                mx = cmx;
            }
        }
        Some((mn, mx))
    }
}

/// Shared core: fold `buf` into the cached mip under `key`/`cap_cells` and render
/// the visible window from cells. `abs_offset` is the absolute sample index of
/// `buf[0]`. The caller controls the cache identity so a sliding live ring (whose
/// length fluctuates) doesn't force a rebuild — see the two wrappers below.
#[allow(clippy::too_many_arguments)]
fn draw_mip_core(
    ctx: &CanvasRenderingContext2d,
    buf: &[f32],
    abs_offset: u64,
    key: (usize, usize, u8),
    cap_cells: usize,
    sample_rate: u32,
    scroll_offset: f64,
    zoom: f64,
    time_resolution: f64,
    canvas_width: f64,
    canvas_height: f64,
    selection: Option<(f64, f64)>,
    gain_db: f64,
    total_duration: f64,
    stroke_color: &str,
) {
    ctx.set_fill_style_str("#0a0a0a");
    ctx.fill_rect(0.0, 0.0, canvas_width, canvas_height);
    if buf.is_empty() {
        return;
    }

    let gain_linear = 10.0_f64.powf(gain_db / 20.0);
    // region_start_sample = 0: the renderer works in whole-buffer (then absolute)
    // sample space, so the visible window is derived from scroll/zoom directly.
    let vp = compute_viewport(
        total_duration,
        sample_rate,
        scroll_offset,
        zoom,
        time_resolution,
        canvas_width,
        canvas_height,
        0,
    );
    draw_selection(ctx, selection, &vp, canvas_width, canvas_height);
    draw_center_line(ctx, vp.mid_y, canvas_width);

    WAVE_MIP.with(|cell| {
        let mut slot = cell.borrow_mut();
        let abs_latest = abs_offset + buf.len() as u64;
        let rebuild = match slot.as_ref() {
            // New buffer/channel, a buffer bigger than the ring cap holds, or the
            // absolute clock went backwards (new live session) → start clean.
            Some(m) => m.key != key || cap_cells > m.cap || abs_latest < m.consumed,
            None => true,
        };
        if rebuild {
            *slot = Some(WaveMip::new(key, cap_cells));
            WF_DIAG.with(|c| c.borrow_mut().6 += 1);
        }
        let mip = slot.as_mut().unwrap();
        mip.fold(abs_latest, buf);

        if vp.data_width <= 0.0 || vp.px_per_sec <= 0.0 {
            return;
        }
        let sr = sample_rate as f64;
        let px_start = vp.data_x.floor().max(0.0) as usize;
        let px_end = (vp.data_x + vp.data_width)
            .ceil()
            .min(canvas_width)
            .max(vp.data_x) as usize;
        ctx.set_stroke_style_str(stroke_color);
        ctx.set_line_width(1.0);
        ctx.begin_path();
        for px in px_start..px_end {
            let x = px as f64;
            let t0 = vp.start_time + ((x - vp.data_x) / vp.px_per_sec);
            let t1 = vp.start_time + ((x + 1.0 - vp.data_x) / vp.px_per_sec);
            let li1 = (t1 * sr) as i64; // buffer-local sample (end)
            if li1 <= 0 {
                continue;
            }
            let li0 = ((t0 * sr) as i64).max(0); // buffer-local sample (start)
                                                 // Map buffer-local → absolute → cell range covering [li0, li1).
            let a0 = abs_offset as i64 + li0;
            let a1 = abs_offset as i64 + li1;
            let c0 = a0 / MIP_D as i64;
            let c1 = (a1 - 1) / MIP_D as i64;
            let Some((mn, mx)) = mip.range(c0, c1) else {
                continue;
            };
            let y_min = vp.mid_y - (mx as f64 * gain_linear * vp.mid_y * 0.9);
            let y_max = vp.mid_y - (mn as f64 * gain_linear * vp.mid_y * 0.9);
            ctx.move_to(x, y_min);
            ctx.line_to(x, y_max);
        }
        ctx.stroke();
    });
}

/// Draw the Simple waveform from a decimated min/max mip over a STATIC whole
/// channel buffer (`buf`, `abs_offset = 0`). Cache key includes `buf.len()` so a
/// reused heap address can't serve stale cells. The caller guarantees spp >= MIP_D.
#[allow(clippy::too_many_arguments)]
pub fn draw_waveform_mipped(
    ctx: &CanvasRenderingContext2d,
    buf: &[f32],
    abs_offset: u64,
    channel_id: u8,
    sample_rate: u32,
    scroll_offset: f64,
    zoom: f64,
    time_resolution: f64,
    canvas_width: f64,
    canvas_height: f64,
    selection: Option<(f64, f64)>,
    gain_db: f64,
    total_duration: f64,
    stroke_color: &str,
) {
    let key = (buf.as_ptr() as usize, buf.len(), channel_id);
    let cap_cells = buf.len() / MIP_D + 2;
    draw_mip_core(
        ctx,
        buf,
        abs_offset,
        key,
        cap_cells,
        sample_rate,
        scroll_offset,
        zoom,
        time_resolution,
        canvas_width,
        canvas_height,
        selection,
        gain_db,
        total_duration,
        stroke_color,
    );
}

/// Draw the live waveform from the SLIDING raw-sample ring, folded incrementally.
/// `abs_offset` is the absolute index of `ring[0]` (= captured − ring.len()).
/// The cache key omits the ring length (which fluctuates every tick as the ring
/// grows/trims) so the mip is NOT rebuilt each frame — only the new samples are
/// folded. `cap_cells` is sized to the ring's max retained length so old cells
/// wrap rather than reallocating. Updates at render-frame rate, not the ~1 Hz
/// source-snapshot cadence.
#[allow(clippy::too_many_arguments)]
pub fn draw_waveform_mipped_live(
    ctx: &CanvasRenderingContext2d,
    ring: &[f32],
    abs_offset: u64,
    cap_cells: usize,
    sample_rate: u32,
    scroll_offset: f64,
    zoom: f64,
    time_resolution: f64,
    canvas_width: f64,
    canvas_height: f64,
    selection: Option<(f64, f64)>,
    gain_db: f64,
    total_duration: f64,
    stroke_color: &str,
) {
    // channel_id = 1 marks the live ring's cache so it never aliases a static
    // file's mip; length is omitted from the key (it fluctuates each tick).
    let key = (ring.as_ptr() as usize, 0usize, 1u8);
    let cap_cells = cap_cells.max(ring.len() / MIP_D + 2);
    draw_mip_core(
        ctx,
        ring,
        abs_offset,
        key,
        cap_cells,
        sample_rate,
        scroll_offset,
        zoom,
        time_resolution,
        canvas_width,
        canvas_height,
        selection,
        gain_db,
        total_duration,
        stroke_color,
    );
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
    if vp.data_width <= 0.0
        || vp.px_per_sec <= 0.0
        || vp.samples_per_pixel <= 0.0
        || lane_height <= 0.0
    {
        return;
    }

    ctx.set_stroke_style_str(color);
    ctx.set_line_width(1.0);

    let mid_y = y_offset + lane_height / 2.0;
    let half_h = lane_height / 2.0;
    let off = vp.region_start_sample;
    let px_start = vp.data_x.floor().max(0.0) as usize;
    let px_end = (vp.data_x + vp.data_width)
        .ceil()
        .min(canvas_width)
        .max(vp.data_x) as usize;

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
            let y = mid_y - (samples[idx] as f64 * gain_linear * half_h * 0.9);
            if first {
                ctx.move_to(x, y);
                first = false;
            } else {
                ctx.line_to(x, y);
            }
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
                if s < min_val {
                    min_val = s;
                }
                if s > max_val {
                    max_val = s;
                }
            }

            let y_min = mid_y - (max_val as f64 * gain_linear * half_h * 0.9);
            let y_max = mid_y - (min_val as f64 * gain_linear * half_h * 0.9);
            ctx.move_to(x, y_min);
            ctx.line_to(x, y_max);
        }
        ctx.stroke();
    }
}

/// Format a frequency in kHz without trailing zeros (e.g. 20, 22.05, 60).
fn fmt_khz(hz: f64) -> String {
    let khz = hz / 1000.0;
    if (khz - khz.round()).abs() < 0.05 {
        format!("{:.0}", khz)
    } else {
        format!("{:.1}", khz)
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
    freq_low: f64,
    freq_high: f64,
) {
    ctx.set_fill_style_str("#0a0a0a");
    ctx.fill_rect(0.0, 0.0, canvas_width, canvas_height);

    if samples.is_empty() {
        return;
    }

    let gain_linear = 10.0_f64.powf(gain_db / 20.0);
    let vp = compute_viewport(
        total_duration,
        sample_rate,
        scroll_offset,
        zoom,
        time_resolution,
        canvas_width,
        canvas_height,
        region_start_sample,
    );
    draw_selection(ctx, selection, &vp, canvas_width, canvas_height);
    draw_center_line(ctx, vp.mid_y, canvas_width);

    // Full waveform behind in dim green
    draw_waveform_layer(
        ctx,
        samples,
        sample_rate,
        &vp,
        canvas_width,
        "rgba(100, 170, 100, 0.35)",
        gain_linear,
    );

    // Selected frequency band overlay in semi-transparent blue
    if !filtered_samples.is_empty() {
        draw_waveform_layer(
            ctx,
            filtered_samples,
            sample_rate,
            &vp,
            canvas_width,
            "rgba(80, 140, 255, 0.7)",
            gain_linear,
        );
    }

    // Label with the band's frequency range
    ctx.set_fill_style_str("rgba(255, 255, 255, 0.35)");
    ctx.set_font("10px sans-serif");
    let _ = ctx.fill_text(
        &format!("{}\u{2013}{} kHz", fmt_khz(freq_low), fmt_khz(freq_high)),
        4.0,
        12.0,
    );
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
    freq_low: f64,
    freq_high: f64,
) {
    ctx.set_fill_style_str("#0a0a0a");
    ctx.fill_rect(0.0, 0.0, canvas_width, canvas_height);

    let gain_linear = 10.0_f64.powf(gain_db / 20.0);
    let vp = compute_viewport(
        total_duration,
        sample_rate,
        scroll_offset,
        zoom,
        time_resolution,
        canvas_width,
        canvas_height,
        region_start_sample,
    );
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
        draw_waveform_layer_lane(
            ctx,
            above_samples,
            sample_rate,
            &vp,
            canvas_width,
            "rgba(220, 160, 60, 0.8)",
            gain_linear,
            0.0,
            lane_height,
        );
    }

    // Selected band (middle lane) — blue
    if !selected_samples.is_empty() {
        draw_waveform_layer_lane(
            ctx,
            selected_samples,
            sample_rate,
            &vp,
            canvas_width,
            "rgba(80, 140, 255, 0.85)",
            gain_linear,
            lane_height,
            lane_height,
        );
    }

    // Below band (bottom lane) — green
    if !below_samples.is_empty() {
        draw_waveform_layer_lane(
            ctx,
            below_samples,
            sample_rate,
            &vp,
            canvas_width,
            "rgba(100, 200, 100, 0.7)",
            gain_linear,
            lane_height * 2.0,
            lane_height,
        );
    }

    // Lane labels — actual frequency ranges
    let nyquist = sample_rate as f64 / 2.0;
    let above_label = format!("{}\u{2013}{} kHz", fmt_khz(freq_high), fmt_khz(nyquist));
    let selected_label = format!("{}\u{2013}{} kHz", fmt_khz(freq_low), fmt_khz(freq_high));
    let below_label = format!("0\u{2013}{} kHz", fmt_khz(freq_low));

    ctx.set_fill_style_str("rgba(255, 255, 255, 0.35)");
    ctx.set_font("10px sans-serif");
    let _ = ctx.fill_text(&above_label, 4.0, 12.0);
    let _ = ctx.fill_text(&selected_label, 4.0, lane_height + 12.0);
    let _ = ctx.fill_text(&below_label, 4.0, lane_height * 2.0 + 12.0);
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
    let Some((start_time, end_time, data_x, _data_width)) =
        viewport::data_region_px(scroll_offset, visible_time, total_duration, canvas_width)
    else {
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

#[cfg(test)]
mod tests {
    use super::{WaveMip, MIP_D};

    /// Deterministic pseudo-signal with sample-to-sample variation.
    fn sample(i: u64) -> f32 {
        let x = i as f64;
        let s = (x * 0.017).sin() * 0.6 + (x * 0.29).sin() * 0.3;
        let h = ((i.wrapping_mul(2654435761) >> 16) & 0xff) as f64 / 255.0 - 0.5;
        (s + h * 0.2).clamp(-1.0, 1.0) as f32
    }

    fn window_bits(m: &WaveMip) -> Vec<(u32, u32)> {
        (m.tail..=m.head)
            .map(|c| {
                let (mn, mx) = m.cells[(c as usize) % m.cap];
                (mn.to_bits(), mx.to_bits())
            })
            .collect()
    }

    #[test]
    fn mip_incremental_matches_oneshot() {
        // Folding the whole buffer at once must equal folding it in growing
        // chunks (the static one-shot vs the live grows-each-tick path).
        let n = 20_000u64;
        let full: Vec<f32> = (0..n).map(sample).collect();
        let cap = n as usize / MIP_D + 2;

        let mut one = WaveMip::new((1, 0, 0), cap);
        one.fold(n, &full);

        let mut inc = WaveMip::new((1, 0, 0), cap);
        let mut abs = 0u64;
        while abs < n {
            abs = (abs + 777).min(n); // odd step → folds cross cell boundaries
            inc.fold(abs, &full[..abs as usize]);
        }

        assert_eq!(one.head, inc.head);
        assert_eq!(one.tail, inc.tail);
        assert_eq!(window_bits(&one), window_bits(&inc));
    }

    #[test]
    fn mip_cells_are_exact_minmax() {
        // Each cell must be the exact min/max of its MIP_D raw samples (lossless).
        let n = 5_000u64;
        let full: Vec<f32> = (0..n).map(sample).collect();
        let mut m = WaveMip::new((2, 0, 0), n as usize / MIP_D + 2);
        m.fold(n, &full);
        for c in 0..=m.head {
            let lo = c as usize * MIP_D;
            let hi = (lo + MIP_D).min(full.len());
            if lo >= hi {
                continue;
            }
            let (mut mn, mut mx) = (f32::MAX, f32::MIN);
            for &s in &full[lo..hi] {
                if s < mn {
                    mn = s;
                }
                if s > mx {
                    mx = s;
                }
            }
            let (cmn, cmx) = m.cells[(c as usize) % m.cap];
            assert_eq!(cmn.to_bits(), mn.to_bits(), "cell {c} min");
            assert_eq!(cmx.to_bits(), mx.to_bits(), "cell {c} max");
        }
    }

    #[test]
    fn mip_range_matches_raw_window() {
        // `range(c0,c1)` == direct min/max over the covered raw span.
        let n = 8_000u64;
        let full: Vec<f32> = (0..n).map(sample).collect();
        let mut m = WaveMip::new((3, 0, 0), n as usize / MIP_D + 2);
        m.fold(n, &full);
        let (c0, c1) = (3i64, 9i64);
        let (mn, mx) = m.range(c0, c1).unwrap();
        let lo = c0 as usize * MIP_D;
        let hi = ((c1 as usize + 1) * MIP_D).min(full.len());
        let (mut rmn, mut rmx) = (f32::MAX, f32::MIN);
        for &s in &full[lo..hi] {
            if s < rmn {
                rmn = s;
            }
            if s > rmx {
                rmx = s;
            }
        }
        assert_eq!(mn.to_bits(), rmn.to_bits());
        assert_eq!(mx.to_bits(), rmx.to_bits());
    }

    #[test]
    fn mip_live_ring_wraps_and_fences() {
        // Simulate a small sliding ring: fold absolute samples past `cap` cells so
        // the ring wraps and `tail` advances. Cells in [tail,head] stay exact.
        let cap = 16usize; // 16 cells = 16*MIP_D samples retained
        let mut m = WaveMip::new((4, 0, 0), cap);
        let ring_len = (cap - 1) * MIP_D; // a bit under capacity
                                          // Feed up to absolute N in ticks; the "ring" is the last ring_len samples.
        let n = (cap as u64 + 40) * MIP_D as u64; // many cells past cap → wraps
        let full: Vec<f32> = (0..n).map(sample).collect();
        let mut abs = 0u64;
        while abs < n {
            abs = (abs + MIP_D as u64 * 3).min(n);
            let lo = abs.saturating_sub(ring_len as u64) as usize;
            m.fold(abs, &full[lo..abs as usize]);
        }
        assert_eq!(m.head, (n - 1) as i64 / MIP_D as i64);
        // tail fences to the last `cap` cells.
        assert_eq!(m.tail, m.head - cap as i64 + 1);
        // Every valid cell equals the exact raw min/max (no stale wrap data).
        for c in m.tail..=m.head {
            let lo = c as usize * MIP_D;
            let hi = (lo + MIP_D).min(full.len());
            let (mut mn, mut mx) = (f32::MAX, f32::MIN);
            for &s in &full[lo..hi] {
                if s < mn {
                    mn = s;
                }
                if s > mx {
                    mx = s;
                }
            }
            let (cmn, cmx) = m.cells[(c as usize) % m.cap];
            assert_eq!(cmn.to_bits(), mn.to_bits(), "wrapped cell {c} min");
            assert_eq!(cmx.to_bits(), mx.to_bits(), "wrapped cell {c} max");
        }
    }
}
