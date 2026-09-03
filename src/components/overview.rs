use crate::components::icons::Icon;
use crate::state::store_fields::*;
use crate::state::{AppState, OverviewView};
use crate::types::PreviewImage;
use leptos::prelude::*;
use std::cell::RefCell;
use wasm_bindgen::{Clamped, JsCast};
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, ImageData, MouseEvent};

/// Cached min/max envelope for overview waveform rendering.
/// Instead of iterating all samples (~57M for a 20MB MP3) on every frame,
/// we precompute a per-pixel min/max envelope once and render from that.
struct WaveformEnvelope {
    /// Interleaved [min0, max0, min1, max1, ...] for each pixel column.
    data: Vec<f32>,
    /// Cache key: (samples_ptr, samples_len, pixel_width, gain_bits)
    key: (usize, usize, u32, u64),
}

/// Cached off-screen canvas holding the fully rendered waveform bitmap.
/// On scroll we just `drawImage` from this instead of re-running ~1000
/// canvas path operations per frame.
struct WaveformCanvasCache {
    canvas: HtmlCanvasElement,
    ctx: CanvasRenderingContext2d,
    /// Cache key: (samples_ptr, samples_len, canvas_width, canvas_height, gain_bits)
    key: (usize, usize, u32, u32, u64),
}

thread_local! {
    /// Reusable off-screen canvas for the overview preview blit.
    static OVERVIEW_TMP: RefCell<Option<(HtmlCanvasElement, CanvasRenderingContext2d)>> =
        const { RefCell::new(None) };
    /// Cached identity of the last preview rendered to the tmp canvas.
    /// Stores (Arc data pointer, width, height) to detect when we need to re-render.
    static OVERVIEW_CACHED_PREVIEW: RefCell<(usize, u32, u32)> =
        const { RefCell::new((0, 0, 0)) };
    /// Cached min/max envelope for overview waveform.
    static OVERVIEW_ENVELOPE: RefCell<Option<WaveformEnvelope>> =
        const { RefCell::new(None) };
    /// Cached rendered waveform bitmap (off-screen canvas).
    static OVERVIEW_WAVEFORM_CANVAS: RefCell<Option<WaveformCanvasCache>> =
        const { RefCell::new(None) };
    /// Streaming per-pixel min/max envelope for the LIVE overview waveform.
    static LIVE_WAVE_ENV: RefCell<Option<LiveWaveEnvelope>> =
        const { RefCell::new(None) };
}

fn get_overview_tmp_canvas(
    w: u32,
    h: u32,
) -> Option<(HtmlCanvasElement, CanvasRenderingContext2d)> {
    OVERVIEW_TMP.with(|cell| {
        let mut slot = cell.borrow_mut();
        if let Some((ref c, ref ctx)) = *slot {
            if c.width() >= w && c.height() >= h {
                return Some((c.clone(), ctx.clone()));
            }
        }
        let doc = web_sys::window()?.document()?;
        let c = doc
            .create_element("canvas")
            .ok()?
            .dyn_into::<HtmlCanvasElement>()
            .ok()?;
        c.set_width(w);
        c.set_height(h);
        let ctx = c
            .get_context("2d")
            .ok()??
            .dyn_into::<CanvasRenderingContext2d>()
            .ok()?;
        *slot = Some((c.clone(), ctx.clone()));
        Some((c, ctx))
    })
}

// ── Navigation helpers ────────────────────────────────────────────────────────

fn push_nav(state: &AppState) {
    state.push_nav();
}

// ── Rendering helpers ─────────────────────────────────────────────────────────

fn get_canvas_ctx(canvas: &HtmlCanvasElement) -> Option<CanvasRenderingContext2d> {
    canvas
        .get_context("2d")
        .ok()?
        .and_then(|c| c.dyn_into::<CanvasRenderingContext2d>().ok())
}

/// Blit a PreviewImage (RGBA) to the entire canvas at full width.
/// Also draws a viewport highlight rect (matching both time AND freq range of the main view)
/// and bookmark/playhead dots.
fn draw_overview_spectrogram(
    ctx: &CanvasRenderingContext2d,
    canvas: &HtmlCanvasElement,
    preview: &PreviewImage,
    scroll_offset: f64,     // main view left edge, seconds
    zoom: f64,              // main view zoom (px per spectrogram column)
    spec_time_res: f64,     // seconds per full-FFT spectrogram column (for viewport width)
    total_duration: f64,    // total file duration in seconds (from audio, always correct)
    main_canvas_width: f64, // actual pixel width of main spectrogram canvas
    main_freq_crop_lo: f64, // 0..1: low fraction of Nyquist shown in main view
    main_freq_crop_hi: f64, // 0..1: high fraction of Nyquist shown in main view
    bookmarks: &[(f64,)],
    overview_freq_crop: f64, // 0..1: fraction shown in the overview itself
    band_ff_range: Option<(f64, f64)>, // BandFF range as (lo_frac, hi_frac) of Nyquist
    clean_view: bool,        // hide all overlays (viewport rect, bookmarks, BandFF range)
) {
    let cw = canvas.width() as f64;
    let ch = canvas.height() as f64;

    ctx.set_fill_style_str("#000");
    ctx.fill_rect(0.0, 0.0, cw, ch);

    if preview.width == 0 || preview.height == 0 {
        return;
    }

    // Vertical crop: show overview_freq_crop fraction of the image (low-to-mid freqs hidden)
    let ofc = overview_freq_crop.clamp(0.01, 1.0);
    let full_h = preview.height as f64;
    let src_y = full_h * (1.0 - ofc);
    let src_h = full_h * ofc;

    // Blit via temporary canvas, caching the preview to avoid re-creating
    // ImageData on every scroll-triggered redraw.
    if let Some((tmp, tc)) = get_overview_tmp_canvas(preview.width, preview.height) {
        let preview_id = std::sync::Arc::as_ptr(&preview.pixels) as usize;
        let needs_render = OVERVIEW_CACHED_PREVIEW.with(|cell| {
            let cached = *cell.borrow();
            if cached != (preview_id, preview.width, preview.height) {
                *cell.borrow_mut() = (preview_id, preview.width, preview.height);
                true
            } else {
                false
            }
        });
        if needs_render {
            let clamped = Clamped(&preview.pixels[..]);
            if let Ok(img) =
                ImageData::new_with_u8_clamped_array_and_sh(clamped, preview.width, preview.height)
            {
                let _ = tc.put_image_data(&img, 0.0, 0.0);
            }
        }
        let _ = ctx.draw_image_with_html_canvas_element_and_sw_and_sh_and_dx_and_dy_and_dw_and_dh(
            &tmp,
            0.0,
            src_y,
            preview.width as f64,
            src_h,
            0.0,
            0.0,
            cw,
            ch,
        );
    }

    // Convert to px/sec using the true audio duration (not preview columns × FFT hop)
    if total_duration <= 0.0 {
        return;
    }
    let px_per_sec = cw / total_duration;

    if !clean_view {
        // ── Viewport highlight rect ───────────────────────────────────────────────
        // Horizontal: scroll_offset (left edge) + visible time
        // visible_time = (canvas_px / zoom) * spec_time_res  (zoom = px per FFT column)
        let visible_cols = main_canvas_width / zoom.max(0.001);
        let visible_time = visible_cols * spec_time_res;
        let vp_x = (scroll_offset * px_per_sec).max(0.0);
        let vp_w = (visible_time * px_per_sec).max(2.0);

        // Vertical: map main view freq range into the overview's freq coordinate space.
        // overview y=0 → top freq (ofc * Nyquist), y=ch → 0 Hz.
        let vp_y1 = (ch * (1.0 - main_freq_crop_hi / ofc)).clamp(0.0, ch);
        let vp_y2 = (ch * (1.0 - main_freq_crop_lo / ofc)).clamp(0.0, ch);
        let vp_h = vp_y2 - vp_y1;

        ctx.set_fill_style_str("rgba(80, 180, 130, 0.12)");
        ctx.fill_rect(vp_x, vp_y1, vp_w, vp_h);
        ctx.set_stroke_style_str("rgba(80, 180, 130, 0.55)");
        ctx.set_line_width(1.0);
        ctx.stroke_rect(vp_x, vp_y1, vp_w, vp_h);

        // BandFF range highlight (nested inside viewport rect)
        if let Some((band_ff_lo, band_ff_hi)) = band_ff_range {
            let band_ff_y1 = (ch * (1.0 - band_ff_hi / ofc)).clamp(0.0, ch);
            let band_ff_y2 = (ch * (1.0 - band_ff_lo / ofc)).clamp(0.0, ch);
            if band_ff_y2 - band_ff_y1 > 0.5 {
                ctx.set_fill_style_str("rgba(120, 200, 160, 0.15)");
                ctx.fill_rect(vp_x, band_ff_y1, vp_w, band_ff_y2 - band_ff_y1);
                ctx.set_stroke_style_str("rgba(120, 200, 160, 0.7)");
                ctx.set_line_width(1.0);
                ctx.stroke_rect(vp_x, band_ff_y1, vp_w, band_ff_y2 - band_ff_y1);
            }
        }

        // Bookmark dots (yellow, top edge)
        ctx.set_fill_style_str("rgba(255, 200, 50, 0.9)");
        for &(t,) in bookmarks {
            let x = t * px_per_sec;
            if x >= 0.0 && x <= cw {
                ctx.begin_path();
                let _ = ctx.arc(x, 5.0, 3.0, 0.0, std::f64::consts::TAU);
                ctx.fill();
            }
        }
    }
}

/// Compute a min/max envelope from samples: one (min, max) pair per pixel column.
/// This is O(total_samples) but only runs once; subsequent frames render from
/// the cached envelope in O(canvas_width).
fn compute_envelope(samples: &[f32], pixel_width: u32, gain_linear: f64) -> Vec<f32> {
    let pw = pixel_width as usize;
    if pw == 0 || samples.is_empty() {
        return Vec::new();
    }
    let mut envelope = vec![0.0f32; pw * 2]; // [min0, max0, min1, max1, ...]
    let samples_per_px = samples.len() as f64 / pw as f64;
    for px in 0..pw {
        let i0 = (px as f64 * samples_per_px) as usize;
        let i1 = (((px + 1) as f64 * samples_per_px) as usize).min(samples.len());
        if i0 >= i1 {
            continue;
        }
        let mut lo = f32::MAX;
        let mut hi = f32::MIN;
        for &s in &samples[i0..i1] {
            if s < lo {
                lo = s;
            }
            if s > hi {
                hi = s;
            }
        }
        envelope[px * 2] = lo * gain_linear as f32;
        envelope[px * 2 + 1] = hi * gain_linear as f32;
    }
    envelope
}

/// Absolute pixel index a sample at absolute index `abs_idx` falls into, given
/// `spp` samples per pixel. Must be computed identically in `fold` and tests
/// (same `as f64 / spp as i64` truncation) so pixel binning is exact.
#[inline]
fn pixel_of(abs_idx: u64, spp: f64) -> i64 {
    (abs_idx as f64 / spp) as i64
}

/// Streaming per-pixel min/max envelope for the LIVE overview waveform.
///
/// The live overview shows a fixed-span window whose horizontal scale is
/// constant — `spp = span * sample_rate / width` samples per pixel — so each
/// screen column maps to a fixed *absolute* sample range that never rescales.
/// We exploit that: instead of recomputing the whole ~`span`-second envelope
/// every capture tick (which both wastes CPU and *shimmers*, because the pixel
/// boundaries jitter against the ever-growing sample count), we keep a circular
/// buffer of `w` per-pixel `(min,max)` cells anchored to absolute pixel indices.
/// New samples only ever fold into the rightmost (newest) cell(s); finished
/// cells are frozen until they scroll off the left. O(new samples) per tick, and
/// a stationary pixel never changes value → no flicker. This mirrors the live
/// spectrogram waterfall, which is likewise absolute-column anchored.
///
/// Min/max are stored RAW (un-gained); gain is a positive linear scalar applied
/// at draw time, so gain changes (including frequent auto-gain) don't invalidate
/// the cache.
struct LiveWaveEnvelope {
    /// Circular buffer of `w` cells: the cell for absolute pixel `p` lives at
    /// `p % w`, holding `(min, max)` in raw sample units.
    cells: Vec<(f32, f32)>,
    /// Display width in pixels (== `cells.len()`). Part of the identity key.
    w: u32,
    /// Samples per pixel (the fixed scale). Part of the identity key.
    spp: f64,
    /// Sample rate the cache was built for. Part of the identity key.
    sample_rate: u32,
    /// Absolute pixel index of the newest (in-progress) cell; `-1` when empty.
    head: i64,
    /// Oldest absolute pixel that is currently valid (written and within the
    /// live window); `-1` when empty. Normally tracks `head - w + 1`, but after a
    /// stall-rebuild from a partial ring it sits ahead of the window's left edge
    /// so the render can blank the un-refilled gap instead of reading stale cells.
    tail: i64,
    /// Count of absolute samples folded so far (== absolute index of the next
    /// unseen sample). Monotonic within a session; a drop signals a new session.
    consumed: u64,
}

impl LiveWaveEnvelope {
    fn new(w: u32, spp: f64, sample_rate: u32) -> Self {
        LiveWaveEnvelope {
            cells: vec![(0.0, 0.0); w.max(1) as usize],
            w: w.max(1),
            spp,
            sample_rate,
            head: -1,
            tail: -1,
            consumed: 0,
        }
    }

    /// Whether this cache's geometry matches the requested one. `spp` is derived
    /// deterministically from `(span, sample_rate, w)`, so bit-equality holds
    /// tick-to-tick within a session; a change forces a rebuild.
    fn matches(&self, w: u32, spp: f64, sample_rate: u32) -> bool {
        self.w == w.max(1) && self.sample_rate == sample_rate && self.spp.to_bits() == spp.to_bits()
    }

    /// Fold the newly captured samples into the per-pixel envelope. `abs_latest`
    /// is the absolute index of the sample just past the newest one (i.e. the
    /// total samples captured this session); `ring` is the live raw-sample ring,
    /// whose last `ring.len()` samples are absolute `[abs_latest - len, abs_latest)`.
    fn fold(&mut self, abs_latest: u64, ring: &[f32]) {
        if abs_latest <= self.consumed {
            return; // nothing new (also the common no-op tie → no work, no flicker)
        }
        let ring_len = ring.len() as u64;
        let mut start = self.consumed;
        let mut new = abs_latest - self.consumed;
        if new > ring_len {
            // Fell behind by more than the ring holds (>1 window of wall-clock
            // with no redraw, e.g. a long background stall): the lost samples are
            // gone and the existing cells are now discontinuous. Restart from
            // what the ring still contains; `tail` will fence off the un-refilled
            // left gap so the render can't read a stale cell.
            let (w, spp, sr) = (self.w, self.spp, self.sample_rate);
            *self = LiveWaveEnvelope::new(w, spp, sr);
            start = abs_latest - ring_len;
            new = ring_len;
        }
        let base = ring.len() - new as usize; // ring index of absolute sample `start`
        let w = self.w as usize;
        for k in 0..new as usize {
            let abs_idx = start + k as u64;
            let s = ring[base + k];
            let pixel = pixel_of(abs_idx, self.spp);
            if pixel > self.head {
                // A newly started pixel. With spp >= 1 (guaranteed by the caller)
                // consecutive samples advance the pixel by at most 1, so no cell
                // is ever skipped; the new pixel starts fresh at this sample.
                if self.head < 0 {
                    self.tail = pixel; // first write since (re)build
                }
                self.cells[(pixel as usize) % w] = (s, s);
                self.head = pixel;
                // Drop pixels that have scrolled out of the w-wide window.
                let min_valid = self.head - self.w as i64 + 1;
                if self.tail < min_valid {
                    self.tail = min_valid;
                }
            } else {
                // Continuing the current (head) pixel.
                let cell = &mut self.cells[(pixel as usize) % w];
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

    /// `(min, max)` raw envelope at absolute pixel `p` (caller guarantees `p` is
    /// within the live window `[head - w + 1, head]`, all of which are populated).
    #[inline]
    fn cell(&self, p: i64) -> (f32, f32) {
        self.cells[(p as usize) % self.w as usize]
    }
}

/// Paint the live waveform background + center line; returns the vertical mid.
fn draw_live_wave_chrome(ctx: &CanvasRenderingContext2d, cw: f64, ch: f64) -> f64 {
    let mid_y = ch / 2.0;
    ctx.set_fill_style_str("#0a0a0a");
    ctx.fill_rect(0.0, 0.0, cw, ch);
    ctx.set_stroke_style_str("#333");
    ctx.set_line_width(1.0);
    ctx.begin_path();
    ctx.move_to(0.0, mid_y);
    ctx.line_to(cw, mid_y);
    ctx.stroke();
    mid_y
}

/// Render the live waveform from the streaming `LiveWaveEnvelope`. The data
/// left-anchors at `t=0` while the window fills (absolute pixels `[0, head]`),
/// then scrolls (the last `w` absolute pixels `[head - w + 1, head]`) — matching
/// the live waterfall beside it. Gain is applied here (cells are stored raw).
fn draw_live_waveform_cached(
    env: &LiveWaveEnvelope,
    ctx: &CanvasRenderingContext2d,
    w: u32,
    h: u32,
    gain_db: f64,
) {
    if w == 0 || h == 0 {
        return;
    }
    let cw = w as f64;
    let ch = h as f64;
    let mid_y = draw_live_wave_chrome(ctx, cw, ch);
    if env.head < 0 {
        return;
    }
    let scale = mid_y * 0.9;
    let g = 10.0f64.powf(gain_db / 20.0) as f32;
    let wi = env.w as i64;
    // The window's left edge in absolute pixels:
    //   Fill phase  → 0      (data left-anchored, screen x == absolute pixel)
    //   Scroll phase → head-w+1 (the last w absolute pixels fill the strip)
    let lo_pix = if env.head + 1 <= wi {
        0
    } else {
        env.head - wi + 1
    };
    // Only draw pixels that have actually been written (>= tail); any left gap
    // (post-stall partial refill) stays blank. `x = p - lo_pix` keeps the data
    // positioned correctly within the strip.
    let draw_from = lo_pix.max(env.tail);

    ctx.set_stroke_style_str("#4a4");
    ctx.set_line_width(1.0);
    ctx.begin_path();
    for p in draw_from..=env.head {
        let (mn, mx) = env.cell(p);
        let x = (p - lo_pix) as f64;
        ctx.move_to(x, mid_y - (mx * g) as f64 * scale);
        ctx.line_to(x, mid_y - (mn * g) as f64 * scale);
    }
    ctx.stroke();
}

fn draw_overview_waveform(
    ctx: &CanvasRenderingContext2d,
    canvas: &HtmlCanvasElement,
    samples: &[f32],
    sample_rate: u32,
    time_resolution: f64,
    scroll_offset: f64,
    zoom: f64,
    main_canvas_width: f64,
    bookmarks: &[(f64,)],
    gain_db: f64,
    clean_view: bool,
) {
    let cw = canvas.width() as u32;
    let ch = canvas.height() as u32;
    if samples.is_empty() || cw == 0 || ch == 0 {
        return;
    }

    let total_duration = samples.len() as f64 / sample_rate as f64;

    // Cache key includes dimensions + gain so we re-render on resize or gain change.
    let cache_key = (
        samples.as_ptr() as usize,
        samples.len(),
        cw,
        ch,
        gain_db.to_bits(),
    );

    // Get or create the cached off-screen canvas with the rendered waveform.
    // The waveform bitmap only changes when the file, canvas size, or gain changes —
    // NOT on scroll. This turns per-frame cost from ~1000 path ops to a single drawImage.
    let cache_hit = OVERVIEW_WAVEFORM_CANVAS.with(|cell| {
        let slot = cell.borrow();
        if let Some(ref cached) = *slot {
            if cached.key == cache_key {
                // Cache hit — blit the pre-rendered waveform in one GPU call.
                let _ = ctx.draw_image_with_html_canvas_element(&cached.canvas, 0.0, 0.0);
                return true;
            }
        }
        false
    });

    if !cache_hit {
        let gain_linear = 10.0f64.powf(gain_db / 20.0);

        // Get or compute the min/max envelope (O(total_samples) once).
        let envelope_key = (
            samples.as_ptr() as usize,
            samples.len(),
            cw,
            gain_db.to_bits(),
        );
        let envelope = OVERVIEW_ENVELOPE.with(|cell| {
            let mut slot = cell.borrow_mut();
            if let Some(ref cached) = *slot {
                if cached.key == envelope_key {
                    return cached.data.clone();
                }
            }
            let env = compute_envelope(samples, cw, gain_linear);
            *slot = Some(WaveformEnvelope {
                data: env.clone(),
                key: envelope_key,
            });
            env
        });

        // Create or reuse the off-screen canvas and render the waveform to it.
        OVERVIEW_WAVEFORM_CANVAS.with(|cell| {
            let mut slot = cell.borrow_mut();

            // Ensure we have an off-screen canvas of the right size.
            let needs_create = match *slot {
                Some(ref c) => c.canvas.width() != cw || c.canvas.height() != ch,
                None => true,
            };
            if needs_create {
                let doc = match web_sys::window().and_then(|w| w.document()) {
                    Some(d) => d,
                    None => return,
                };
                let c = match doc
                    .create_element("canvas")
                    .ok()
                    .and_then(|e| e.dyn_into::<HtmlCanvasElement>().ok())
                {
                    Some(c) => c,
                    None => return,
                };
                c.set_width(cw);
                c.set_height(ch);
                let oc = match c
                    .get_context("2d")
                    .ok()
                    .flatten()
                    .and_then(|o| o.dyn_into::<CanvasRenderingContext2d>().ok())
                {
                    Some(oc) => oc,
                    None => return,
                };
                *slot = Some(WaveformCanvasCache {
                    canvas: c,
                    ctx: oc,
                    key: cache_key,
                });
            }

            let cached = slot.as_mut().unwrap();

            // Render the static waveform to the off-screen canvas.
            let off_ctx = &cached.ctx;
            let mid_y = ch as f64 / 2.0;
            let scale = mid_y * 0.9;

            // Background
            off_ctx.set_fill_style_str("#0a0a0a");
            off_ctx.fill_rect(0.0, 0.0, cw as f64, ch as f64);

            // Center line
            off_ctx.set_stroke_style_str("#333");
            off_ctx.set_line_width(1.0);
            off_ctx.begin_path();
            off_ctx.move_to(0.0, mid_y);
            off_ctx.line_to(cw as f64, mid_y);
            off_ctx.stroke();

            // Waveform envelope
            off_ctx.set_stroke_style_str("#4a4");
            off_ctx.set_line_width(1.0);
            off_ctx.begin_path();
            let pw = cw as usize;
            for px in 0..pw {
                let lo = envelope[px * 2] as f64;
                let hi = envelope[px * 2 + 1] as f64;
                let y_top = mid_y - hi * scale;
                let y_bot = mid_y - lo * scale;
                let x = px as f64;
                off_ctx.move_to(x, y_top);
                off_ctx.line_to(x, y_bot);
            }
            off_ctx.stroke();

            // Update cache key and blit to the main canvas.
            cached.key = cache_key;
            let _ = ctx.draw_image_with_html_canvas_element(&cached.canvas, 0.0, 0.0);
        });
    }

    if !clean_view {
        let px_per_sec = cw as f64 / total_duration;
        let visible_cols = main_canvas_width / zoom.max(0.001);
        let visible_time = visible_cols * time_resolution;
        let vp_x = (scroll_offset * px_per_sec).max(0.0);
        let vp_w = (visible_time * px_per_sec).max(2.0);
        ctx.set_fill_style_str("rgba(80, 180, 130, 0.12)");
        ctx.fill_rect(vp_x, 0.0, vp_w, ch as f64);
        ctx.set_stroke_style_str("rgba(80, 180, 130, 0.55)");
        ctx.set_line_width(1.0);
        ctx.stroke_rect(vp_x, 0.0, vp_w, ch as f64);

        // Bookmark dots
        ctx.set_fill_style_str("rgba(255, 200, 50, 0.9)");
        for &(t,) in bookmarks {
            let x = t * px_per_sec;
            if x >= 0.0 && x <= cw as f64 {
                ctx.begin_path();
                let _ = ctx.arc(x, 5.0, 3.0, 0.0, std::f64::consts::TAU);
                ctx.fill();
            }
        }
    }
}

/// Status label + accent color for the live overview / status text. The
/// benchmark and synthetic-signal test modes both drive the *real* listen
/// pipeline, so without this they'd just read "Listening" — title them as what
/// they actually are. `is_listen` distinguishes a plain listen from a recording.
fn live_status_label(is_listen: bool) -> (String, &'static str) {
    if crate::audio::synth_bench::is_running() {
        ("Benchmarking".to_string(), "#dca")
    } else if let Some(sig) = crate::audio::synthetic_mic::active_label() {
        (format!("Test signal: {sig}"), "#9c9")
    } else if is_listen {
        ("Listening".to_string(), "#6af")
    } else {
        ("Recording".to_string(), "#f66")
    }
}

/// Draw the "● Recording/Listening …" status text + VU bar. Used in the live
/// overview when there's no waterfall data to render yet.
fn draw_live_status(
    ctx: &CanvasRenderingContext2d,
    w: u32,
    h: u32,
    file: &crate::state::LoadedFile,
    state: &AppState,
) {
    ctx.set_fill_style_str("#1a1a1a");
    ctx.fill_rect(0.0, 0.0, w as f64, h as f64);
    let (label, color) = live_status_label(file.is_live_listen);
    let elapsed = crate::canvas::live_waterfall::total_time().max(file.audio.duration_secs);
    let text = if elapsed >= 1.0 {
        format!(
            "\u{25CF} {} {}:{:02}",
            label,
            elapsed as u32 / 60,
            elapsed as u32 % 60
        )
    } else {
        format!("\u{25CF} {}\u{2026}", label)
    };
    ctx.set_fill_style_str(color);
    ctx.set_font("11px system-ui");
    ctx.set_text_align("center");
    ctx.set_text_baseline("middle");
    let _ = ctx.fill_text(&text, w as f64 / 2.0, h as f64 / 2.0);
    let peak = state.mic.peak_level().get_untracked();
    if peak > 0.01 {
        let bar_w = (peak as f64 * w as f64).min(w as f64);
        ctx.set_fill_style_str(color);
        ctx.fill_rect(0.0, h as f64 - 2.0, bar_w, 2.0);
    }
}

/// The steady-state retained-ring duration (seconds) — the fixed span the live
/// overview displays. Mirrors the circular-buffer trim in the processing loop
/// (`preroll_buffer_secs.max(2) + 2 s gesture headroom`), clamped to what the
/// waterfall can actually hold so both overviews share a span there's data for.
fn live_overview_span(state: &AppState) -> f64 {
    const GESTURE_HEADROOM_SECS: u32 = 2;
    let buf_secs =
        (state.mic.preroll_buffer_secs().get_untracked().max(2) + GESTURE_HEADROOM_SECS) as f64;
    let cap = crate::canvas::live_waterfall::capacity_time();
    if cap > 0.0 {
        buf_secs.min(cap)
    } else {
        buf_secs
    }
}

/// The single time window the live overview displays, in absolute session
/// seconds: `(axis_start, span)`. Both overviews (waveform + spectrogram), the
/// time markers, the viewport rect, and click/drag mapping all derive from this
/// one window so they share one scale.
///
/// The span is **fixed** at `live_overview_span` (the steady-state ring
/// duration, ~12 s by default) rather than the current ring length — so the
/// horizontal scale never rescales while the ring fills over the first ~12 s.
/// Instead the data left-anchors at `t=0` and the right of the strip stays
/// empty until `now` reaches the span; thereafter `axis_start` slides and the
/// strip scrolls, exactly like the waterfall. Returns `None` when not live or
/// there's no data yet.
pub(crate) fn live_overview_window(state: &AppState) -> Option<(f64, f64)> {
    use crate::canvas::live_waterfall as wf;
    if !wf::is_active() {
        return None;
    }
    let now = wf::total_time();
    if now <= 0.0 {
        return None;
    }
    let span = live_overview_span(state);
    let axis_start = (now - span).max(0.0);
    Some((axis_start, span))
}

/// Fraction (0..1) of the fixed window that currently holds data — i.e. how far
/// the left-anchored data has filled toward the right edge. 1.0 once the ring is
/// full (steady-state scrolling). Used to left-anchor the live overview content.
fn live_fill_frac(axis_start: f64, span: f64) -> f64 {
    if span <= 0.0 {
        return 1.0;
    }
    let now = crate::canvas::live_waterfall::total_time();
    ((now - axis_start) / span).clamp(0.0, 1.0)
}

/// Apply a scrub (overview click/drag) to the main-view scroll. The desired
/// scroll is clamped to the range the overview actually displays — during live
/// that's the shared ring window `[axis_start, now - visible]`, so clicking the
/// far-left lands on the far-left rather than centering past it — and the live
/// waterfall follow is suspended while scrubbing (no snap-back) but re-engaged
/// when scrubbed to the live edge. `visible` is the main view's visible span.
fn scrub_apply_scroll(state: &AppState, desired_scroll: f64, visible: f64, full_duration: f64) {
    let is_live = (state.mic.recording().get_untracked() || state.mic.listening().get_untracked())
        && state.timeline.active().get_untracked().is_none()
        && crate::canvas::live_waterfall::is_active();
    if is_live {
        if let Some((axis_start, span)) = live_overview_window(state) {
            let now = axis_start + span;
            let hi = (now - visible).max(axis_start);
            let s = desired_scroll.clamp(axis_start, hi);
            state.view.scroll_offset().set(s);
            if s >= hi - visible * 0.02 {
                state.resume_waterfall_follow();
            } else {
                state.suspend_waterfall_follow(2000.0);
            }
            return;
        }
    }
    let max_scroll = (full_duration - visible).max(0.0);
    state
        .view
        .scroll_offset()
        .set(desired_scroll.clamp(0.0, max_scroll));
}

/// Draw a min/max waveform envelope for the live overview, recomputed fresh on
/// every call (no persistent cache) so it tracks the live ring at the capture
/// cadence rather than the ~1 Hz snapshot. Visual style matches the static
/// overview waveform.
///
/// The background + center line span the full strip width `w`; the envelope is
/// drawn only across `data_w` (the left, filled portion), leaving the right
/// empty until the ring fills — so the time scale stays fixed as data grows.
fn draw_live_waveform(
    ctx: &CanvasRenderingContext2d,
    w: u32,
    data_w: u32,
    h: u32,
    samples: &[f32],
    gain_db: f64,
) {
    if w == 0 || h == 0 {
        return;
    }
    let cw = w as f64;
    let ch = h as f64;
    let mid_y = draw_live_wave_chrome(ctx, cw, ch);
    let scale = mid_y * 0.9;

    let dw = data_w.min(w) as usize;
    if samples.is_empty() || dw == 0 {
        return;
    }
    let gain_linear = 10.0f64.powf(gain_db / 20.0);
    let env = compute_envelope(samples, dw as u32, gain_linear);

    ctx.set_stroke_style_str("#4a4");
    ctx.set_line_width(1.0);
    ctx.begin_path();
    for px in 0..dw {
        let lo = env[px * 2] as f64;
        let hi = env[px * 2 + 1] as f64;
        let x = px as f64;
        ctx.move_to(x, mid_y - hi * scale);
        ctx.line_to(x, mid_y - lo * scale);
    }
    ctx.stroke();
}

// ── Overview toolbar (below the overview strip) ────────────────────────────────

/// Thin strip mounted directly beneath the overview that owns the
/// spectrogram/waveform toggle, so the control reads as belonging to the
/// overview rather than floating on top of it. The button previews the OTHER
/// view (a small glyph + its name) — clicking switches to it.
#[component]
pub fn OverviewToolbar() -> impl IntoView {
    let state = expect_context::<AppState>();

    // The view the toggle switches TO — shown as a preview on the button.
    let target = move || match state.viewmode.overview_view().get() {
        OverviewView::Spectrogram => OverviewView::Waveform,
        OverviewView::Waveform => OverviewView::Spectrogram,
    };
    let target_label = move || match target() {
        OverviewView::Spectrogram => "Spectrogram",
        OverviewView::Waveform => "Waveform",
    };
    let target_icon = move || match target() {
        OverviewView::Spectrogram => view! { <Icon kind=Icon::Grid /> },
        OverviewView::Waveform => view! { <Icon kind=Icon::Wave /> },
    };

    let toggle = move |_: MouseEvent| {
        let next = match state.viewmode.overview_view().get_untracked() {
            OverviewView::Spectrogram => OverviewView::Waveform,
            OverviewView::Waveform => OverviewView::Spectrogram,
        };
        state.viewmode.overview_view().set(next);
    };

    view! {
        <Show when=move || !state.viewmode.clean_view().get()>
            <div
                class="overview-toolbar"
                on:click=|ev: MouseEvent| ev.stop_propagation()
                on:touchstart=|ev: web_sys::TouchEvent| ev.stop_propagation()
            >
                <span class="overview-toolbar-label">"OVERVIEW"</span>
                <button
                    class="overview-layer-btn"
                    on:click=toggle
                    title=move || match target() {
                        OverviewView::Spectrogram => "Show spectrogram overview",
                        OverviewView::Waveform => "Show waveform overview",
                    }
                >
                    <span class="ov-preview">{target_icon}</span>
                    <span>{target_label}</span>
                </button>
            </div>
        </Show>
    }
}

// ── Helpers for sizing a canvas to its CSS pixel dimensions ──────────────────

/// Resize a canvas element's bitmap to match its CSS layout size.
/// Returns (width, height) on success, or None if the canvas has zero dimensions.
fn size_canvas_to_display(canvas: &HtmlCanvasElement) -> Option<(u32, u32)> {
    let w = canvas.client_width() as u32;
    let h = canvas.client_height() as u32;
    if w == 0 || h == 0 {
        return None;
    }
    if canvas.width() != w {
        canvas.set_width(w);
    }
    if canvas.height() != h {
        canvas.set_height(h);
    }
    Some((w, h))
}

// ── Main OverviewPanel component ──────────────────────────────────────────────

#[component]
pub fn OverviewPanel() -> impl IntoView {
    let state = expect_context::<AppState>();
    let canvas_ref = NodeRef::<leptos::html::Canvas>::new();
    let overlay_ref = NodeRef::<leptos::html::Canvas>::new();

    // Dragging state: (is_dragging, initial_client_x, initial_scroll)
    let drag_active = RwSignal::new(false);
    let drag_start_x = RwSignal::new(0.0f64);
    let drag_start_scroll = RwSignal::new(0.0f64);

    // Smoothed live "now" (right edge of the overview window), eased toward the
    // real latest-data time on each overlay redraw so the time markers slide
    // smoothly instead of stepping at the ~20 Hz capture tick — matching the
    // smooth main-view time axis. Non-reactive: it's interpolation state, not a
    // signal anything subscribes to.
    let smooth_now = StoredValue::new(0.0f64);

    // ── Background Effect ── draws waveform/spectrogram content.
    // Does NOT subscribe to scroll_offset or zoom_level, so panning/zooming
    // only triggers the cheap overlay Effect below instead of re-blitting
    // the entire waveform from the off-screen cache on every frame.
    Effect::new(move || {
        state.library.files().track();
        let files = state.library.files().get_untracked();
        let _timeline_trigger = state.timeline.active().get();
        let idx = state.library.current_index().get();
        let overview_view = state.viewmode.overview_view().get();
        let cv = state.viewmode.channel_view().get();
        let _mic_recording = state.mic.recording().get();
        let _mic_listening = state.mic.listening().get();
        // Re-render the live overview on each capture tick. `live_data_cols` is
        // bumped by the processing loop while recording/listening and stays 0
        // otherwise, so static files don't pay for this.
        let _live_cols = state.mic.live_data_cols().get();
        let auto_gain = state.gain.auto().get();
        let gain_db = if auto_gain {
            state.compute_auto_gain_untracked()
        } else {
            state.gain.db().get()
        };
        // Re-read canvas dimensions when sidebar layout changes
        let _sidebar = state.panels.left_collapsed().get();
        let _sidebar_width = state.panels.left_width().get();
        let _rsidebar = state.panels.right_collapsed().get();
        let _rsidebar_width = state.panels.right_width().get();

        let Some(canvas_el) = canvas_ref.get() else {
            return;
        };
        let canvas: &HtmlCanvasElement = canvas_el.as_ref();

        let Some((w, h)) = size_canvas_to_display(canvas) else {
            return;
        };

        let Some(ctx) = get_canvas_ctx(canvas) else {
            return;
        };
        ctx.set_fill_style_str("#000");
        ctx.fill_rect(0.0, 0.0, w as f64, h as f64);

        let timeline = state.timeline.active().get_untracked();

        if let Some(ref tl) = timeline {
            // ── Timeline mode: render segment previews ──
            let total_duration = tl.total_duration_secs;
            if total_duration <= 0.0 {
                return;
            }
            let cw = w as f64;
            let ch = h as f64;
            let px_per_sec = cw / total_duration;

            for seg in &tl.segments {
                let seg_file = match files.get(seg.file_index) {
                    Some(f) => f,
                    None => continue,
                };
                let overview_src = seg_file
                    .overview_image
                    .as_ref()
                    .or(seg_file.preview.as_ref());
                let Some(preview) = overview_src else {
                    continue;
                };

                let seg_x = seg.timeline_offset_secs * px_per_sec;
                let seg_w = seg.duration_secs * px_per_sec;
                if seg_w < 1.0 {
                    continue;
                }

                ctx.save();
                ctx.begin_path();
                ctx.rect(seg_x, 0.0, seg_w, ch);
                ctx.clip();

                if let Some((tmp, tc)) = get_overview_tmp_canvas(preview.width, preview.height) {
                    let clamped = wasm_bindgen::Clamped(preview.pixels.as_slice());
                    if let Ok(img) = web_sys::ImageData::new_with_u8_clamped_array_and_sh(
                        clamped,
                        preview.width,
                        preview.height,
                    ) {
                        let _ = tc.put_image_data(&img, 0.0, 0.0);
                        let _ = ctx.draw_image_with_html_canvas_element_and_dw_and_dh(
                            &tmp, seg_x, 0.0, seg_w, ch,
                        );
                    }
                }

                ctx.restore();
            }
            // Viewport rect, gap indicators, and time markers are drawn by the overlay Effect.
        } else {
            // ── Single file overview ──
            let file_opt = idx.and_then(|i| files.get(i));

            if file_opt.is_none() {
                let is_rec = state.mic.recording().get_untracked();
                let is_lis = state.mic.listening().get_untracked();
                if is_rec || is_lis {
                    ctx.set_fill_style_str("#1a1a1a");
                    ctx.fill_rect(0.0, 0.0, w as f64, h as f64);
                    let (label, color) = live_status_label(!is_rec);
                    ctx.set_fill_style_str(color);
                    ctx.set_font("11px system-ui");
                    ctx.set_text_align("center");
                    ctx.set_text_baseline("middle");
                    let _ = ctx.fill_text(
                        &format!("\u{25CF} {}\u{2026}", label),
                        w as f64 / 2.0,
                        h as f64 / 2.0,
                    );
                    let peak = state.mic.peak_level().get_untracked();
                    if peak > 0.01 {
                        let bar_w = (peak as f64 * w as f64).min(w as f64);
                        ctx.set_fill_style_str(color);
                        ctx.fill_rect(0.0, h as f64 - 2.0, bar_w, 2.0);
                    }
                }
                return;
            }
            let file = file_opt.unwrap();

            // During live listening/recording, the overview shows a FIXED-span
            // window (the steady-state ring duration, ~12 s by default). The data
            // left-anchors at t=0 and the right of the strip stays empty until the
            // ring fills — drawn linearly like the waterfall, so the horizontal
            // scale never rescales (no jumpy markers while filling). The
            // spectrogram, waveform, and time markers all share this one window.
            let is_live = (file.is_recording || file.is_live_listen)
                && crate::canvas::live_waterfall::is_active();
            if is_live {
                let (axis_start, span) = live_overview_window(&state).unwrap_or((0.0, 1.0));
                // Filled-left width in pixels; the rest of the strip is empty.
                let frac = live_fill_frac(axis_start, span);
                let data_w = ((w as f64 * frac).round() as u32).min(w);
                match overview_view {
                    OverviewView::Spectrogram => {
                        let recent_cols = (span / crate::canvas::live_waterfall::time_resolution())
                            .ceil() as usize;
                        // Render the retained columns into just the filled width,
                        // then blit them at the left (black background already drawn).
                        let img = if data_w > 0 {
                            crate::canvas::live_waterfall::render_overview(
                                data_w,
                                h,
                                Some(recent_cols),
                            )
                        } else {
                            None
                        };
                        match img {
                            Some(img) if img.width > 0 => {
                                if let Some((tmp, tc)) =
                                    get_overview_tmp_canvas(img.width, img.height)
                                {
                                    let clamped = Clamped(&img.pixels[..]);
                                    if let Ok(image) = ImageData::new_with_u8_clamped_array_and_sh(
                                        clamped, img.width, img.height,
                                    ) {
                                        let _ = tc.put_image_data(&image, 0.0, 0.0);
                                        let _ = ctx.draw_image_with_html_canvas_element_and_sw_and_sh_and_dx_and_dy_and_dw_and_dh(
                                            &tmp,
                                            0.0, 0.0, img.width as f64, img.height as f64,
                                            0.0, 0.0, data_w as f64, h as f64,
                                        );
                                    }
                                }
                            }
                            _ => draw_live_status(&ctx, w, h, file, &state),
                        }
                    }
                    OverviewView::Waveform => {
                        // Fold the live ring into the streaming per-pixel envelope
                        // (O(new samples)/tick, absolute-pixel anchored) instead of
                        // recomputing the whole ~span-second min/max every tick —
                        // which both wasted CPU and shimmered. See LiveWaveEnvelope.
                        let sr = file.audio.sample_rate;
                        let drew =
                            crate::audio::mic_backend::with_live_samples(state.is_tauri, |ring| {
                                if ring.is_empty() {
                                    return false;
                                }
                                let spp = span * sr as f64 / w as f64;
                                if spp < 1.0 {
                                    // Degenerate (tiny window): fall back to a full
                                    // recompute — streaming needs >=1 sample/pixel.
                                    if data_w == 0 {
                                        return false;
                                    }
                                    let n = ((span * sr as f64) as usize).clamp(1, ring.len());
                                    draw_live_waveform(
                                        &ctx,
                                        w,
                                        data_w,
                                        h,
                                        &ring[ring.len() - n..],
                                        gain_db,
                                    );
                                    return true;
                                }
                                let abs_latest =
                                    (crate::canvas::live_waterfall::total_time() * sr as f64)
                                        .floor() as u64;
                                LIVE_WAVE_ENV.with(|cell| {
                                    let mut slot = cell.borrow_mut();
                                    let rebuild = match slot.as_ref() {
                                        // New geometry, or `total_time` went backwards
                                        // (a fresh live session) → start clean.
                                        Some(e) => {
                                            !e.matches(w, spp, sr) || abs_latest < e.consumed
                                        }
                                        None => true,
                                    };
                                    if rebuild {
                                        *slot = Some(LiveWaveEnvelope::new(w, spp, sr));
                                    }
                                    let env = slot.as_mut().unwrap();
                                    env.fold(abs_latest, ring);
                                    draw_live_waveform_cached(env, &ctx, w, h, gain_db);
                                });
                                true
                            });
                        if !drew {
                            draw_live_status(&ctx, w, h, file, &state);
                        }
                    }
                }
            } else {
                match overview_view {
                    OverviewView::Spectrogram => {
                        let overview_src = file.overview_image.as_ref().or(file.preview.as_ref());
                        if let Some(preview) = overview_src {
                            // Draw spectrogram image only (clean_view=true skips all overlays).
                            draw_overview_spectrogram(
                                &ctx,
                                canvas,
                                preview,
                                0.0,
                                1.0,
                                file.spectrogram.time_resolution,
                                file.audio.duration_secs,
                                0.0,
                                0.0,
                                1.0,
                                &[],
                                1.0,
                                None,
                                true, // clean_view — overlays drawn by overlay Effect
                            );
                        } else if file.is_recording && !file.audio.samples.is_empty() {
                            let scroll = state.view.scroll_offset().get_untracked();
                            let is_live_wf = (file.is_live_listen || file.is_recording)
                                && crate::canvas::live_waterfall::is_active();
                            let buf_scroll = if is_live_wf {
                                let wf_total = crate::canvas::live_waterfall::total_time();
                                let offset = (wf_total - file.audio.duration_secs).max(0.0);
                                (scroll - offset).clamp(0.0, file.audio.duration_secs)
                            } else {
                                scroll
                            };
                            draw_overview_waveform(
                                &ctx,
                                canvas,
                                &file.audio.samples,
                                file.audio.sample_rate,
                                file.spectrogram.time_resolution,
                                buf_scroll,
                                1.0,
                                0.0,
                                &[],
                                gain_db,
                                true,
                            );
                        } else if file.is_recording {
                            ctx.set_fill_style_str("#1a1a1a");
                            ctx.fill_rect(0.0, 0.0, w as f64, h as f64);
                            let is_listen = file.is_live_listen;
                            let (label, color) = live_status_label(is_listen);
                            let elapsed = if is_listen && crate::canvas::live_waterfall::is_active()
                            {
                                crate::canvas::live_waterfall::total_time()
                            } else {
                                file.audio.duration_secs
                            };
                            let text = if elapsed >= 1.0 {
                                let mins = elapsed as u32 / 60;
                                let secs = elapsed as u32 % 60;
                                format!("\u{25CF} {} {}:{:02}", label, mins, secs)
                            } else {
                                format!("\u{25CF} {}\u{2026}", label)
                            };
                            ctx.set_fill_style_str(color);
                            ctx.set_font("11px system-ui");
                            ctx.set_text_align("center");
                            ctx.set_text_baseline("middle");
                            let _ = ctx.fill_text(&text, w as f64 / 2.0, h as f64 / 2.0);
                            let peak = state.mic.peak_level().get_untracked();
                            if peak > 0.01 {
                                let bar_w = (peak as f64 * w as f64).min(w as f64);
                                ctx.set_fill_style_str(color);
                                ctx.fill_rect(0.0, h as f64 - 2.0, bar_w, 2.0);
                            }
                        } else if file.loading_id.is_some() {
                            // A real file still being decoded — show a loading hint.
                            ctx.set_fill_style_str("#333");
                            ctx.fill_rect(0.0, 0.0, w as f64, h as f64);
                            ctx.set_fill_style_str("#666");
                            ctx.set_font("11px system-ui");
                            ctx.set_text_align("center");
                            ctx.set_text_baseline("middle");
                            let _ =
                                ctx.fill_text("Loading\u{2026}", w as f64 / 2.0, h as f64 / 2.0);
                        }
                        // else: an empty / armed live placeholder (no samples, no
                        // preview, not loading) — leave the strip blank rather than
                        // showing a misleading "Loading…".
                    }
                    OverviewView::Waveform => {
                        let ov_buf;
                        // Cap non-MonoMix reads to file.audio.samples.len() so
                        // streaming sources don't allocate gigabytes for
                        // multi-hour files.
                        let read_len = file.audio.samples.len();
                        let ov_samples: &[f32] = match cv {
                            // Stereo (the default view) and MonoMix both resolve to the
                            // mono buffer (read_samples → read_mono), so borrow it
                            // zero-copy instead of read_region's full-length alloc+copy
                            // — which, on a gain-drag redraw, would re-copy the entire
                            // (potentially hundreds-of-MB) buffer to reproduce identical
                            // data. Per-channel / Difference views still go through it.
                            crate::audio::source::ChannelView::MonoMix
                            | crate::audio::source::ChannelView::Stereo => &file.audio.samples,
                            _ => {
                                ov_buf = file.audio.source.read_region(cv, 0, read_len);
                                &ov_buf
                            }
                        };
                        draw_overview_waveform(
                            &ctx,
                            canvas,
                            ov_samples,
                            file.audio.sample_rate,
                            file.spectrogram.time_resolution,
                            0.0,
                            1.0,
                            0.0,
                            &[],
                            gain_db,
                            true, // clean_view — overlays drawn by overlay Effect
                        );
                    }
                }
            } // end else (not live)
              // Time markers are drawn by the overlay Effect.
        }
    });

    // ── Overlay Effect ── draws viewport rect, bookmarks, time markers on a
    // transparent canvas layered on top. This subscribes to scroll_offset and
    // zoom_level but is very cheap (no drawImage blit, just a few shapes).
    Effect::new(move || {
        let scroll = state.view.scroll_offset().get();
        let zoom = state.view.zoom_level().get();
        let bookmarks = state.viewmode.bookmarks().get();
        let clean_view = state.viewmode.clean_view().get();
        let main_canvas_w = state.viewmode.spectrogram_canvas_width().get();
        let min_display_freq = state.view.min_display_freq().get();
        let max_display_freq = state.view.max_display_freq().get();
        let band_ff_lo_hz = state.filter.band_ff_freq_lo().get();
        let band_ff_hi_hz = state.filter.band_ff_freq_hi().get();
        let overview_view = state.viewmode.overview_view().get();
        // Re-sync overlay dimensions when sidebar layout changes
        let _sidebar = state.panels.left_collapsed().get();
        let _sidebar_width = state.panels.left_width().get();
        let _rsidebar = state.panels.right_collapsed().get();
        let _rsidebar_width = state.panels.right_width().get();
        // Redraw when file changes (duration, freq info)
        state.library.files().track();
        let _idx = state.library.current_index().get();
        // During live capture the displayed window advances every tick; track
        // live_data_cols so the markers/viewport advance with the data (not only
        // when the follow-scroll animation happens to nudge scroll_offset).
        let _live_cols = state.mic.live_data_cols().get();

        let Some(canvas_el) = overlay_ref.get() else {
            return;
        };
        let canvas: &HtmlCanvasElement = canvas_el.as_ref();

        let Some((w, h)) = size_canvas_to_display(canvas) else {
            return;
        };

        let Some(ctx) = get_canvas_ctx(canvas) else {
            return;
        };
        ctx.clear_rect(0.0, 0.0, w as f64, h as f64);

        if clean_view {
            return;
        }

        let cw = w as f64;
        let ch = h as f64;

        let timeline = state.timeline.active().get_untracked();

        if let Some(ref tl) = timeline {
            // ── Timeline overlay ──
            let total_duration = tl.total_duration_secs;
            if total_duration <= 0.0 {
                return;
            }
            let px_per_sec = cw / total_duration;

            let files = state.library.files().get_untracked();
            let primary_file = tl.segments.first().and_then(|s| files.get(s.file_index));
            let max_freq = primary_file
                .map(|f| f.spectrogram.max_freq)
                .unwrap_or(96_000.0);
            let spec_time_res = primary_file
                .map(|f| f.spectrogram.time_resolution)
                .unwrap_or(1.0);

            let main_freq_crop_hi = max_display_freq
                .map(|mdf| (mdf / max_freq).clamp(0.001, 1.0))
                .unwrap_or(1.0);
            let main_freq_crop_lo = min_display_freq
                .map(|mdf| (mdf / max_freq).clamp(0.0, 1.0))
                .unwrap_or(0.0);

            // Viewport rectangle
            let visible_cols = main_canvas_w / zoom.max(0.001);
            let visible_time_span = visible_cols * spec_time_res;
            let vp_x = (scroll * px_per_sec).max(0.0);
            let vp_w = (visible_time_span * px_per_sec).max(2.0);
            let vp_y1 = (ch * (1.0 - main_freq_crop_hi)).clamp(0.0, ch);
            let vp_y2 = (ch * (1.0 - main_freq_crop_lo)).clamp(0.0, ch);
            let vp_h = (vp_y2 - vp_y1).max(1.0);

            ctx.set_stroke_style_str("rgba(255, 255, 255, 0.7)");
            ctx.set_line_width(1.5);
            ctx.stroke_rect(vp_x, vp_y1, vp_w, vp_h);

            // Gap indicators
            for i in 0..tl.segments.len() {
                let seg_end = tl.segments[i].timeline_offset_secs + tl.segments[i].duration_secs;
                if i + 1 < tl.segments.len() {
                    let next_start = tl.segments[i + 1].timeline_offset_secs;
                    if next_start > seg_end + 0.001 {
                        let gap_x = seg_end * px_per_sec;
                        let gap_w = (next_start - seg_end) * px_per_sec;
                        ctx.set_fill_style_str("rgba(50, 50, 80, 0.5)");
                        ctx.fill_rect(gap_x, 0.0, gap_w, ch);
                    }
                }
            }

            // Time markers
            let clock_cfg = if tl.origin_epoch_ms > 0.0 {
                Some(crate::canvas::time_markers::ClockTimeConfig {
                    recording_start_epoch_ms: tl.origin_epoch_ms,
                })
            } else {
                None
            };
            crate::canvas::time_markers::draw_time_markers(
                &ctx,
                0.0,
                total_duration,
                cw,
                ch,
                total_duration,
                clock_cfg,
                state.timeline.show_clock_time().get(),
                1.0,
            );
        } else {
            // ── Single file overlay ──
            let files = state.library.files().get_untracked();
            let idx = state.library.current_index().get_untracked();
            let file_opt = idx.and_then(|i| files.get(i));
            let Some(file) = file_opt else { return };

            let max_freq = if file.spectrogram.max_freq > 0.0 {
                file.spectrogram.max_freq
            } else {
                file.audio.sample_rate as f64 / 2.0
            };
            // During live, the overview spans the shared raw-sample-ring window
            // [now - audio_dur, now] (same as the waveform/spectrogram drawn
            // beneath); otherwise the whole file. axis_start offsets the viewport
            // rect / markers so they line up with the background.
            let is_live = (file.is_recording || file.is_live_listen)
                && crate::canvas::live_waterfall::is_active();
            let (axis_start, total_duration) = if is_live {
                match live_overview_window(&state) {
                    Some((axis_real, span)) => {
                        // Ease the displayed right edge toward the real latest-
                        // data time so markers don't step at the capture tick.
                        // Snap on first use / large gaps (new session, long
                        // pause), interpolate otherwise. The window keeps the
                        // real ring width and slides as a whole.
                        let now_real = axis_real + span;
                        let prev = smooth_now.get_value();
                        let now_smooth = if prev <= 0.0 || (now_real - prev).abs() > span {
                            now_real
                        } else {
                            prev + (now_real - prev) * 0.35
                        };
                        smooth_now.set_value(now_smooth);
                        ((now_smooth - span).max(0.0), span)
                    }
                    None => (0.0, file.audio.duration_secs),
                }
            } else {
                smooth_now.set_value(0.0); // reset between live sessions
                (0.0, file.audio.duration_secs)
            };
            if total_duration <= 0.0 {
                return;
            }
            let spec_time_res = file.spectrogram.time_resolution;
            let px_per_sec = cw / total_duration;

            // Viewport rectangle (offset by axis_start for the live window)
            let visible_cols = main_canvas_w / zoom.max(0.001);
            let visible_time = visible_cols * spec_time_res;
            let vp_x = ((scroll - axis_start) * px_per_sec).max(0.0);
            let vp_w = (visible_time * px_per_sec).max(2.0);

            match overview_view {
                OverviewView::Spectrogram => {
                    // Fractions of Nyquist shown in the main view
                    let main_freq_crop_hi = max_display_freq
                        .map(|mdf| (mdf / max_freq).clamp(0.001, 1.0))
                        .unwrap_or(1.0);
                    let main_freq_crop_lo = min_display_freq
                        .map(|mdf| (mdf / max_freq).clamp(0.0, 1.0))
                        .unwrap_or(0.0);

                    let ofc = 1.0; // overview always shows full frequency range
                    let vp_y1 = (ch * (1.0 - main_freq_crop_hi / ofc)).clamp(0.0, ch);
                    let vp_y2 = (ch * (1.0 - main_freq_crop_lo / ofc)).clamp(0.0, ch);
                    let vp_h = vp_y2 - vp_y1;

                    ctx.set_fill_style_str("rgba(80, 180, 130, 0.12)");
                    ctx.fill_rect(vp_x, vp_y1, vp_w, vp_h);
                    ctx.set_stroke_style_str("rgba(80, 180, 130, 0.55)");
                    ctx.set_line_width(1.0);
                    ctx.stroke_rect(vp_x, vp_y1, vp_w, vp_h);

                    // BandFF range highlight
                    if band_ff_hi_hz > band_ff_lo_hz {
                        let lo_frac = (band_ff_lo_hz / max_freq).clamp(0.0, 1.0);
                        let hi_frac = (band_ff_hi_hz.min(max_freq) / max_freq).clamp(0.0, 1.0);
                        let band_ff_y1 = (ch * (1.0 - hi_frac / ofc)).clamp(0.0, ch);
                        let band_ff_y2 = (ch * (1.0 - lo_frac / ofc)).clamp(0.0, ch);
                        if band_ff_y2 - band_ff_y1 > 0.5 {
                            ctx.set_fill_style_str("rgba(120, 200, 160, 0.15)");
                            ctx.fill_rect(vp_x, band_ff_y1, vp_w, band_ff_y2 - band_ff_y1);
                            ctx.set_stroke_style_str("rgba(120, 200, 160, 0.7)");
                            ctx.set_line_width(1.0);
                            ctx.stroke_rect(vp_x, band_ff_y1, vp_w, band_ff_y2 - band_ff_y1);
                        }
                    }
                }
                OverviewView::Waveform => {
                    // Full-height viewport rect
                    ctx.set_fill_style_str("rgba(80, 180, 130, 0.12)");
                    ctx.fill_rect(vp_x, 0.0, vp_w, ch);
                    ctx.set_stroke_style_str("rgba(80, 180, 130, 0.55)");
                    ctx.set_line_width(1.0);
                    ctx.stroke_rect(vp_x, 0.0, vp_w, ch);
                }
            }

            // Bookmark dots (offset by axis_start for the live window)
            ctx.set_fill_style_str("rgba(255, 200, 50, 0.9)");
            for bm in bookmarks.iter() {
                let x = (bm.time - axis_start) * px_per_sec;
                if x >= 0.0 && x <= cw {
                    ctx.begin_path();
                    let _ = ctx.arc(x, 5.0, 3.0, 0.0, std::f64::consts::TAU);
                    ctx.fill();
                }
            }

            // Time markers — span the live retained window [axis_start, now] or
            // the whole file. (scroll_offset arg = left-edge time, visible_time =
            // span shown, duration = end time.)
            let clock_cfg = file.recording_start_epoch_ms().map(|ms| {
                crate::canvas::time_markers::ClockTimeConfig {
                    recording_start_epoch_ms: ms,
                }
            });
            crate::canvas::time_markers::draw_time_markers(
                &ctx,
                axis_start,
                total_duration,
                cw,
                ch,
                axis_start + total_duration,
                clock_cfg,
                state.timeline.show_clock_time().get(),
                1.0,
            );
        }
    });

    // ── Mouse handlers ────────────────────────────────────────────────────────

    // Get the true total duration (timeline, live waterfall, or single file)
    let file_duration = move || -> f64 {
        if let Some(ref tl) = state.timeline.active().get_untracked() {
            return tl.total_duration_secs;
        }
        let is_live =
            state.mic.recording().get_untracked() || state.mic.listening().get_untracked();
        if is_live && crate::canvas::live_waterfall::is_active() {
            return crate::canvas::live_waterfall::total_time();
        }
        let files = state.library.files().get_untracked();
        let idx = state.library.current_index().get_untracked();
        idx.and_then(|i| files.get(i))
            .map(|f| f.audio.duration_secs)
            .unwrap_or(0.0)
    };

    // Convert a click x-coordinate to a time offset (seconds). During live the
    // overview spans only the shared ring window [axis_start, now], so map
    // within that window rather than [0, now].
    let x_to_time = move |canvas_x: f64, canvas_w: f64| -> Option<f64> {
        if canvas_w <= 0.0 {
            return None;
        }
        let is_live = (state.mic.recording().get_untracked()
            || state.mic.listening().get_untracked())
            && state.timeline.active().get_untracked().is_none();
        if is_live {
            if let Some((axis_start, span)) = live_overview_window(&state) {
                return Some(axis_start + (canvas_x / canvas_w) * span);
            }
        }
        let dur = file_duration();
        if dur <= 0.0 {
            return None;
        }
        Some((canvas_x / canvas_w) * dur)
    };

    // The time span the overview currently displays across its full width:
    // the shared ring window during live, else the whole file/timeline.
    let overview_span = move || -> f64 {
        if state.timeline.active().get_untracked().is_none() {
            let is_live =
                state.mic.recording().get_untracked() || state.mic.listening().get_untracked();
            if is_live {
                if let Some((_, span)) = live_overview_window(&state) {
                    return span;
                }
            }
        }
        file_duration()
    };

    // Compute half the visible time window for centering clicks
    let half_visible_time = move || -> f64 {
        let files = state.library.files().get_untracked();
        let idx = state.library.current_index().get_untracked();
        idx.and_then(|i| files.get(i))
            .map(|f| {
                let zoom = state.view.zoom_level().get_untracked();
                let canvas_w = state.viewmode.spectrogram_canvas_width().get_untracked();
                (canvas_w / zoom) * f.spectrogram.time_resolution / 2.0
            })
            .unwrap_or(0.0)
    };

    let on_pointerdown = move |ev: web_sys::PointerEvent| {
        if state.status.viewport_zoomed().get_untracked() {
            return;
        }
        ev.prevent_default();
        let Some(canvas_el) = overlay_ref.get_untracked() else {
            return;
        };
        let canvas: &HtmlCanvasElement = canvas_el.as_ref();
        let rect = canvas.get_bounding_client_rect();
        let canvas_x = ev.client_x() as f64 - rect.left();
        let cw = rect.width();
        if let Some(t) = x_to_time(canvas_x, cw) {
            push_nav(&state);
            let visible = half_visible_time() * 2.0;
            state.suspend_follow();
            scrub_apply_scroll(&state, t - half_visible_time(), visible, file_duration());
        }
        drag_active.set(true);
        drag_start_x.set(ev.client_x() as f64);
        drag_start_scroll.set(state.view.scroll_offset().get_untracked());
        // Capture pointer so drag continues when cursor leaves the overview strip
        if let Some(target) = ev.target() {
            if let Ok(el) = target.dyn_into::<web_sys::Element>() {
                let _ = el.set_pointer_capture(ev.pointer_id());
            }
        }
    };

    let on_pointermove = move |ev: web_sys::PointerEvent| {
        if !drag_active.get_untracked() {
            return;
        }
        let Some(canvas_el) = overlay_ref.get_untracked() else {
            return;
        };
        let canvas: &HtmlCanvasElement = canvas_el.as_ref();
        let rect = canvas.get_bounding_client_rect();
        let cw = rect.width();
        let full_duration = file_duration();
        if full_duration <= 0.0 || cw <= 0.0 {
            return;
        }
        let dx = ev.client_x() as f64 - drag_start_x.get_untracked();
        // Map per-pixel motion against the span the overview actually displays.
        let dt = (dx / cw) * overview_span();
        let visible_time = {
            let files = state.library.files().get_untracked();
            let idx = state.library.current_index().get_untracked();
            idx.and_then(|i| files.get(i))
                .map(|f| {
                    let zoom = state.view.zoom_level().get_untracked();
                    let canvas_w = state.viewmode.spectrogram_canvas_width().get_untracked();
                    (canvas_w / zoom) * f.spectrogram.time_resolution
                })
                .unwrap_or(0.0)
        };
        state.suspend_follow();
        scrub_apply_scroll(
            &state,
            drag_start_scroll.get_untracked() + dt,
            visible_time,
            full_duration,
        );
    };

    let on_pointerup = move |_: web_sys::PointerEvent| {
        drag_active.set(false);
    };

    // ── Touch event handlers (mobile) ──────────────────────────────────────────
    let on_touchstart = move |ev: web_sys::TouchEvent| {
        if state.status.viewport_zoomed().get_untracked() {
            return;
        }
        let touches = ev.touches();
        if touches.length() != 1 {
            return;
        }
        ev.prevent_default();
        let touch = touches.get(0).unwrap();
        let Some(canvas_el) = overlay_ref.get_untracked() else {
            return;
        };
        let canvas: &HtmlCanvasElement = canvas_el.as_ref();
        let rect = canvas.get_bounding_client_rect();
        let canvas_x = touch.client_x() as f64 - rect.left();
        let cw = rect.width();
        if let Some(t) = x_to_time(canvas_x, cw) {
            push_nav(&state);
            let visible = half_visible_time() * 2.0;
            state.suspend_follow();
            scrub_apply_scroll(&state, t - half_visible_time(), visible, file_duration());
        }
        drag_active.set(true);
        drag_start_x.set(touch.client_x() as f64);
        drag_start_scroll.set(state.view.scroll_offset().get_untracked());
    };

    let on_touchmove = move |ev: web_sys::TouchEvent| {
        if !drag_active.get_untracked() {
            return;
        }
        let touches = ev.touches();
        if touches.length() != 1 {
            return;
        }
        ev.prevent_default();
        let touch = touches.get(0).unwrap();
        let Some(canvas_el) = overlay_ref.get_untracked() else {
            return;
        };
        let canvas: &HtmlCanvasElement = canvas_el.as_ref();
        let rect = canvas.get_bounding_client_rect();
        let cw = rect.width();
        let full_duration = file_duration();
        if full_duration <= 0.0 || cw <= 0.0 {
            return;
        }
        let dx = touch.client_x() as f64 - drag_start_x.get_untracked();
        // Map per-pixel motion against the span the overview actually displays.
        let dt = (dx / cw) * overview_span();
        let visible_time = {
            let files = state.library.files().get_untracked();
            let idx = state.library.current_index().get_untracked();
            idx.and_then(|i| files.get(i))
                .map(|f| {
                    let zoom = state.view.zoom_level().get_untracked();
                    let canvas_w = state.viewmode.spectrogram_canvas_width().get_untracked();
                    (canvas_w / zoom) * f.spectrogram.time_resolution
                })
                .unwrap_or(0.0)
        };
        state.suspend_follow();
        scrub_apply_scroll(
            &state,
            drag_start_scroll.get_untracked() + dt,
            visible_time,
            full_duration,
        );
    };

    let on_touchend = move |_ev: web_sys::TouchEvent| {
        drag_active.set(false);
    };

    let on_wheel = move |ev: web_sys::WheelEvent| {
        ev.prevent_default();
        let raw_delta = ev.delta_y() + ev.delta_x();
        let total_duration = file_duration();
        let visible_time = {
            let files = state.library.files().get_untracked();
            let idx = state.library.current_index().get_untracked();
            idx.and_then(|i| files.get(i))
                .map(|f| {
                    let zoom = state.view.zoom_level().get_untracked();
                    let canvas_w = state.viewmode.spectrogram_canvas_width().get_untracked();
                    (canvas_w / zoom) * f.spectrogram.time_resolution
                })
                .unwrap_or(0.0)
        };
        let delta = raw_delta.signum() * visible_time * 0.1 * (raw_delta.abs() / 100.0).min(3.0);
        state.suspend_follow();
        let cur = state.view.scroll_offset().get_untracked();
        scrub_apply_scroll(&state, cur + delta, visible_time, total_duration);
    };

    view! {
        <div class="overview-strip">
            // Background canvas — static waveform/spectrogram (no scroll redraw)
            <canvas
                node_ref=canvas_ref
                style="cursor: crosshair; touch-action: none; pointer-events: none;"
            />

            // Overlay canvas — viewport rect, bookmarks, time markers (cheap per-frame)
            <canvas
                node_ref=overlay_ref
                on:pointerdown=on_pointerdown
                on:pointermove=on_pointermove
                on:pointerup=on_pointerup
                on:wheel=on_wheel
                on:touchstart=on_touchstart
                on:touchmove=on_touchmove
                on:touchend=on_touchend
                style=move || {
                    let ta = if state.status.viewport_zoomed().get() { "pinch-zoom" } else { "none" };
                    let pe = if state.status.viewport_zoomed().get() { "none" } else { "auto" };
                    format!("position: absolute; top: 0; left: 0; cursor: crosshair; touch-action: {ta}; pointer-events: {pe};")
                }
            />

            // DOM playhead dot overlay — decoupled from heavy canvas redraws
            <div
                class="playhead-dot"
                style:left=move || {
                    let playhead = state.playback.playhead_time().get();
                    let duration = if let Some(ref tl) = state.timeline.active().get_untracked() {
                        tl.total_duration_secs
                    } else {
                        let is_live = state.mic.recording().get_untracked()
                            || state.mic.listening().get_untracked();
                        if is_live && crate::canvas::live_waterfall::is_active() {
                            crate::canvas::live_waterfall::total_time()
                        } else {
                            let files = state.library.files().get_untracked();
                            let idx = state.library.current_index().get_untracked();
                            idx.and_then(|i| files.get(i))
                                .map(|f| f.audio.duration_secs)
                                .unwrap_or(0.0)
                        }
                    };
                    let pct = if duration > 0.0 {
                        (playhead / duration * 100.0).clamp(0.0, 100.0)
                    } else { 0.0 };
                    format!("{:.2}%", pct)
                }
                style:display=move || if state.playback.is_playing().get() && !state.viewmode.clean_view().get() { "block" } else { "none" }
            />

        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::{pixel_of, LiveWaveEnvelope};
    use std::collections::HashMap;

    /// Deterministic pseudo-signal in roughly [-1, 1] with sample-to-sample
    /// variation (so per-pixel min/max is non-trivial).
    fn sample(i: u64) -> f32 {
        let x = i as f64;
        let s = (x * 0.013).sin() * 0.6 + (x * 0.31).sin() * 0.3;
        let h = ((i.wrapping_mul(2654435761) >> 16) & 0xff) as f64 / 255.0 - 0.5;
        (s + h * 0.2).clamp(-1.0, 1.0) as f32
    }

    /// Reference: per-absolute-pixel min/max over the whole signal, then read the
    /// displayed window exactly as `draw_live_waveform_cached` does.
    fn ref_window(full: &[f32], spp: f64, w: usize) -> Vec<(f32, f32)> {
        let n = full.len() as u64;
        if n == 0 {
            return vec![];
        }
        let head = pixel_of(n - 1, spp);
        let mut cells: HashMap<i64, (f32, f32)> = HashMap::new();
        for idx in 0..n {
            let p = pixel_of(idx, spp);
            let s = full[idx as usize];
            let e = cells.entry(p).or_insert((s, s));
            if s < e.0 {
                e.0 = s;
            }
            if s > e.1 {
                e.1 = s;
            }
        }
        let (lo_pix, n_render) = if head + 1 <= w as i64 {
            (0i64, (head + 1) as usize)
        } else {
            (head - w as i64 + 1, w)
        };
        (0..n_render).map(|i| cells[&(lo_pix + i as i64)]).collect()
    }

    fn env_window(env: &LiveWaveEnvelope) -> Vec<(f32, f32)> {
        if env.head < 0 {
            return vec![];
        }
        let wi = env.w as i64;
        let (lo_pix, n_render) = if env.head + 1 <= wi {
            (0i64, (env.head + 1) as usize)
        } else {
            (env.head - wi + 1, env.w as usize)
        };
        (0..n_render).map(|i| env.cell(lo_pix + i as i64)).collect()
    }

    /// Simulate live capture: the absolute sample count grows by `step` per tick;
    /// the ring holds the last `ring_cap` samples. Fold each tick, then assert the
    /// displayed window is bit-identical to a from-scratch recompute.
    fn run_sim(total: u64, spp: f64, w: u32, step: u64, ring_cap: usize) {
        let full: Vec<f32> = (0..total).map(sample).collect();
        let mut env = LiveWaveEnvelope::new(w, spp, 384_000);
        let mut abs = 0u64;
        while abs < total {
            abs = (abs + step).min(total);
            let lo = abs.saturating_sub(ring_cap as u64) as usize;
            env.fold(abs, &full[lo..abs as usize]);
        }
        let got = env_window(&env);
        let want = ref_window(&full, spp, w as usize);
        assert_eq!(got.len(), want.len(), "window length");
        for (i, (g, e)) in got.iter().zip(want.iter()).enumerate() {
            assert_eq!(
                g.0.to_bits(),
                e.0.to_bits(),
                "min mismatch at screen px {i}"
            );
            assert_eq!(
                g.1.to_bits(),
                e.1.to_bits(),
                "max mismatch at screen px {i}"
            );
        }
    }

    #[test]
    fn streaming_matches_reference_scroll_phase() {
        // ~200 absolute pixels of data into a 64px window → scroll phase.
        let (spp, w, total) = (100.0, 64u32, 20_000u64);
        run_sim(total, spp, w, 2000, 8000); // chunky ticks
        run_sim(total, spp, w, 137, 8000); // small odd ticks (tick != pixel boundary)
        run_sim(total, spp, w, total, 25_000); // single bulk fold
    }

    #[test]
    fn streaming_matches_reference_fill_phase() {
        // Stop before the window fills (30 pixels < 64px) → fill phase.
        run_sim(3_000, 100.0, 64, 250, 4_000);
    }

    #[test]
    fn fold_ties_and_stale_are_noops() {
        // A re-fold with no advance (or an older `abs_latest`) must change nothing
        // — that's what keeps a stationary live waveform from flickering.
        let mut env = LiveWaveEnvelope::new(32, 50.0, 384_000);
        let full: Vec<f32> = (0..5_000).map(sample).collect();
        env.fold(5_000, &full);
        let head0 = env.head;
        let win0 = env_window(&env);
        env.fold(5_000, &full); // tie → no-op
        env.fold(4_000, &full[..4_000]); // stale → no-op
        assert_eq!(env.head, head0);
        assert_eq!(env_window(&env), win0);
    }

    #[test]
    fn stall_rebuilds_and_fences_stale_cells() {
        // A long stall (abs_latest jumps past one full window) with only a PARTIAL
        // ring available must rebuild from the ring and fence the un-refilled left
        // gap via `tail` — never reading a stale cell from the prior session.
        let (spp, w) = (100.0, 64u32);
        let mut env = LiveWaveEnvelope::new(w, spp, 384_000);
        // Session A: fill a full window so all cells hold (stale) data.
        let a: Vec<f32> = (0..12_000).map(sample).collect();
        let mut abs = 0u64;
        while abs < 12_000 {
            abs = (abs + 500).min(12_000);
            let lo = abs.saturating_sub(8_000) as usize;
            env.fold(abs, &a[lo..abs as usize]);
        }
        // Stall: jump to 51_500 (new = 39_500 >> ring_len 1_500) with a 1_500-sample
        // (15-pixel) ring covering absolute [50_000, 51_500).
        let ring: Vec<f32> = (50_000..51_500).map(sample).collect();
        env.fold(51_500, &ring);
        assert_eq!(env.head, pixel_of(51_499, spp)); // 514
        assert_eq!(env.tail, pixel_of(50_000, spp)); // 500 — fences the gap

        // Every valid cell [tail, head] must equal a fresh recompute of the ring;
        // the displayed window's left gap [head-w+1, tail) is simply not drawn.
        let mut want: HashMap<i64, (f32, f32)> = HashMap::new();
        for (j, &s) in ring.iter().enumerate() {
            let p = pixel_of(50_000 + j as u64, spp);
            let e = want.entry(p).or_insert((s, s));
            if s < e.0 {
                e.0 = s;
            }
            if s > e.1 {
                e.1 = s;
            }
        }
        for p in env.tail..=env.head {
            assert_eq!(
                env.cell(p).0.to_bits(),
                want[&p].0.to_bits(),
                "min at pixel {p}"
            );
            assert_eq!(
                env.cell(p).1.to_bits(),
                want[&p].1.to_bits(),
                "max at pixel {p}"
            );
        }
        // The window's left edge sits below tail, so the gap exists and is fenced.
        let lo_pix = env.head - w as i64 + 1;
        assert!(
            lo_pix < env.tail,
            "expected an un-refilled left gap to fence"
        );
    }
}
