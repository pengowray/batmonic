// SPDX-License-Identifier: GPL-3.0-only OR MIT OR Apache-2.0
use crate::canvas::colors::{
    magnitude_to_greyscale, magnitude_to_db,
    db_to_greyscale, flow_rgb_scheme, coherence_rgb, phase_rgb,
    greyscale_to_viridis, greyscale_to_inferno,
    greyscale_to_magma, greyscale_to_plasma, greyscale_to_cividis, greyscale_to_turbo,
};
use crate::state::FlowColorScheme;
use crate::types::{PreviewImage, SpectrogramData};
use wasm_bindgen::JsCast;
use wasm_bindgen::Clamped;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, ImageData};

use crate::viewport;

// Re-export from split modules so callers don't need to change imports
pub use crate::canvas::flow::{FlowAlgo, FlowData, compute_flow_data, composite_flow, pre_render_flow_columns};
pub use crate::canvas::overlays::{
    FreqShiftMode, FreqMarkerState, TimeMarkerStyle,
    draw_freq_markers, draw_time_markers, draw_band_ff_overlay, draw_het_overlay,
    draw_pulses, draw_selection, draw_harmonic_shadows, draw_filter_overlay,
    pixel_to_time_freq, draw_notch_bands, draw_tile_debug_overlay, draw_annotations,
    draw_time_marker_lines,
};

// PreRendered and SpectDisplaySettings are defined in oversample-core::types.
pub use crate::types::{PreRendered, SpectDisplaySettings};

/// Pre-render the entire spectrogram to an RGBA pixel buffer.
/// Width = number of columns, Height = number of frequency bins.
/// Frequency axis: row 0 = highest frequency (top), last row = 0 Hz (bottom).
pub fn pre_render(data: &SpectrogramData) -> PreRendered {
    if data.columns.is_empty() {
        return PreRendered {
            width: 0,
            height: 0,
            pixels: Vec::new(),
            db_data: Vec::new(),
            flow_shifts: Vec::new(),
        };
    }

    let width = data.columns.len() as u32;
    let height = data.columns[0].magnitudes.len() as u32;

    // Find global max magnitude for normalization
    let max_mag = data
        .columns
        .iter()
        .flat_map(|c| c.magnitudes.iter())
        .copied()
        .fold(0.0f32, f32::max);

    let mut pixels = vec![0u8; (width * height * 4) as usize];

    for (col_idx, col) in data.columns.iter().enumerate() {
        for (bin_idx, &mag) in col.magnitudes.iter().enumerate() {
            let grey = magnitude_to_greyscale(mag, max_mag);
            // Flip vertically: bin 0 = lowest freq → bottom row
            let y = height as usize - 1 - bin_idx;
            let pixel_idx = (y * width as usize + col_idx) * 4;
            pixels[pixel_idx] = grey;     // R
            pixels[pixel_idx + 1] = grey; // G
            pixels[pixel_idx + 2] = grey; // B
            pixels[pixel_idx + 3] = 255;  // A
        }
    }

    PreRendered {
        width,
        height,
        pixels,
        db_data: Vec::new(),
        flow_shifts: Vec::new(),
    }
}

/// Pre-render a slice of columns (a tile) into absolute dB values.
///
/// Stores f32 absolute dB values (`20 * log10(mag)`) per pixel so that gain,
/// contrast, dynamic range, and reference level can all be adjusted at render
/// time without regenerating the tile.
pub fn pre_render_columns(
    columns: &[crate::types::SpectrogramColumn],
) -> PreRendered {
    if columns.is_empty() {
        return PreRendered { width: 0, height: 0, pixels: Vec::new(), db_data: Vec::new(), flow_shifts: Vec::new() };
    }
    let width = columns.len() as u32;
    let height = columns[0].magnitudes.len() as u32;
    let mut db_data = vec![f32::NEG_INFINITY; (width * height) as usize];
    for (col_idx, col) in columns.iter().enumerate() {
        for (bin_idx, &mag) in col.magnitudes.iter().enumerate() {
            if bin_idx >= height as usize { break; }
            let db = magnitude_to_db(mag);
            let y = height as usize - 1 - bin_idx;
            let idx = y * width as usize + col_idx;
            db_data[idx] = db;
        }
    }
    PreRendered { width, height, pixels: Vec::new(), db_data, flow_shifts: Vec::new() }
}

/// Compute the global max magnitude across a full spectrogram (for tile normalisation).
pub fn global_max_magnitude(data: &SpectrogramData) -> f32 {
    data.columns.iter().flat_map(|c| c.magnitudes.iter()).copied().fold(0.0f32, f32::max)
}

/// Selects which tile cache to read from during rendering.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TileSource {
    Normal,
    Reassigned,
    Flow,
}

/// How tile dB data (and optional flow shifts) should be converted to RGBA pixels.
#[derive(Clone, Copy, Debug)]
pub enum TileRenderMode {
    /// Normal spectrogram: dB → greyscale → colormap
    Spectrogram(ColormapMode),
    /// Flow: dB → greyscale + shift → flow_rgb/coherence_rgb/phase_rgb
    Flow {
        intensity_gate: f32,
        flow_gate: f32,
        opacity: f32,
        shift_gain: f32,
        color_gamma: f32,
        algo: FlowAlgo,
        scheme: FlowColorScheme,
    },
}

/// Convert a frequency to a canvas Y coordinate.
/// min_freq is shown at the bottom (y = canvas_height), max_freq at the top (y = 0).
#[inline]
pub fn freq_to_y(freq: f64, min_freq: f64, max_freq: f64, canvas_height: f64) -> f64 {
    canvas_height * (1.0 - (freq - min_freq) / (max_freq - min_freq))
}

/// Convert a canvas Y coordinate back to a frequency.
#[inline]
pub fn y_to_freq(y: f64, min_freq: f64, max_freq: f64, canvas_height: f64) -> f64 {
    min_freq + (max_freq - min_freq) * (1.0 - y / canvas_height)
}

/// A base colormap LUT choice.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum Colormap {
    #[default]
    Viridis,
    Inferno,
    Magma,
    Plasma,
    Cividis,
    Turbo,
    Greyscale,
}

impl Colormap {
    /// Apply this colormap's LUT to a greyscale value.
    #[inline]
    pub fn apply(self, grey: u8) -> [u8; 3] {
        match self {
            Colormap::Viridis => greyscale_to_viridis(grey),
            Colormap::Inferno => greyscale_to_inferno(grey),
            Colormap::Magma => greyscale_to_magma(grey),
            Colormap::Plasma => greyscale_to_plasma(grey),
            Colormap::Cividis => greyscale_to_cividis(grey),
            Colormap::Turbo => greyscale_to_turbo(grey),
            Colormap::Greyscale => [grey, grey, grey],
        }
    }
}

/// Which colormap to apply when blitting the spectrogram.
#[derive(Clone, Copy, Debug)]
pub enum ColormapMode {
    /// Uniform colormap across the entire spectrogram.
    Uniform(Colormap),
    /// Colormap inside HFR focus band, greyscale outside.
    /// Fractions are relative to the full image (0 Hz = 0.0, file_max_freq = 1.0).
    HfrFocus { colormap: Colormap, band_ff_lo_frac: f64, band_ff_hi_frac: f64 },
}

/// Blit the pre-rendered spectrogram to a visible canvas, handling scroll, zoom, and freq crop.
/// `freq_crop_lo` / `freq_crop_hi` are fractions (0..1) of the full image height:
/// lo = min_display_freq / file_max_freq, hi = max_display_freq / file_max_freq.
pub fn blit_viewport(
    ctx: &CanvasRenderingContext2d,
    pre_rendered: &PreRendered,
    viewport_width: f64,
    viewport_height: f64,
    scroll_col: f64,
    zoom: f64,
    freq_crop_lo: f64,
    freq_crop_hi: f64,
    colormap: ColormapMode,
) {
    let cw = viewport_width;
    let ch = viewport_height;

    // Clear canvas
    ctx.set_fill_style_str("#000");
    ctx.fill_rect(0.0, 0.0, cw, ch);

    if pre_rendered.width == 0 || pre_rendered.height == 0 {
        return;
    }

    let fc_lo = freq_crop_lo.max(0.0);
    let fc_hi = freq_crop_hi.max(0.01);

    // How many source columns are visible at current zoom
    let natural_visible_cols = cw / zoom;
    let visible_cols = natural_visible_cols.min(pre_rendered.width as f64);
    let src_start = scroll_col.max(0.0).min((pre_rendered.width as f64 - visible_cols).max(0.0));

    // If file has fewer columns than the view span, draw at correct proportional
    // width instead of stretching.  This keeps the spectrogram aligned with the
    // time-to-pixel mapping used by the playhead, waveform, and overlays.
    let dst_w = if (pre_rendered.width as f64) < natural_visible_cols {
        cw * (pre_rendered.width as f64 / natural_visible_cols)
    } else {
        cw
    };

    // Vertical crop: row 0 = highest freq, last row = 0 Hz
    // Extract the band from fc_lo to fc_hi of the full image
    let full_h = pre_rendered.height as f64;
    let (src_y, src_h, dst_y, dst_h) = if fc_hi <= 1.0 {
        let sy = full_h * (1.0 - fc_hi);
        let sh = full_h * (fc_hi - fc_lo).max(0.001);
        (sy, sh, 0.0, ch)
    } else {
        // Display range extends above Nyquist
        let fc_range = (fc_hi - fc_lo).max(0.001);
        let data_frac = (1.0 - fc_lo) / fc_range;
        let sh = full_h * (1.0 - fc_lo);
        (0.0, sh, ch * (1.0 - data_frac), ch * data_frac)
    };

    // Apply colormap (remap greyscale pixels to RGB)
    let mapped_pixels;
    let pixel_data: &[u8] = match colormap {
        ColormapMode::Uniform(cm) => {
            if cm == Colormap::Greyscale {
                &pre_rendered.pixels
            } else {
                mapped_pixels = {
                    let mut buf = pre_rendered.pixels.clone();
                    for chunk in buf.chunks_exact_mut(4) {
                        let [r, g, b] = cm.apply(chunk[0]);
                        chunk[0] = r;
                        chunk[1] = g;
                        chunk[2] = b;
                    }
                    buf
                };
                &mapped_pixels
            }
        }
        ColormapMode::HfrFocus { colormap: cm, band_ff_lo_frac, band_ff_hi_frac } => {
            mapped_pixels = {
                let mut buf = pre_rendered.pixels.clone();
                let h = pre_rendered.height as f64;
                let w = pre_rendered.width as usize;
                // Row 0 = highest freq; last row = 0 Hz
                let focus_top = (h * (1.0 - band_ff_hi_frac)).round() as usize;
                let focus_bot = (h * (1.0 - band_ff_lo_frac)).round() as usize;
                for row in 0..pre_rendered.height as usize {
                    if row >= focus_top && row < focus_bot {
                        let base = row * w * 4;
                        for col in 0..w {
                            let i = base + col * 4;
                            let [r, g, b] = cm.apply(buf[i]);
                            buf[i] = r;
                            buf[i + 1] = g;
                            buf[i + 2] = b;
                        }
                    }
                    // Outside focus: keep greyscale
                }
                buf
            };
            &mapped_pixels
        }
    };

    // Create ImageData from pixel buffer and draw it
    let clamped = Clamped(pixel_data);
    let image_data = ImageData::new_with_u8_clamped_array_and_sh(
        clamped,
        pre_rendered.width,
        pre_rendered.height,
    );

    match image_data {
        Ok(img) => {
            let Some((tmp, tmp_ctx)) = get_tmp_canvas(pre_rendered.width, pre_rendered.height) else { return };
            let _ = tmp_ctx.put_image_data(&img, 0.0, 0.0);

            // Draw the visible portion, proportionally sized to match overlay coordinate space
            let _ = ctx.draw_image_with_html_canvas_element_and_sw_and_sh_and_dx_and_dy_and_dw_and_dh(
                &tmp,
                src_start,
                src_y,
                visible_cols,
                src_h,
                0.0,
                dst_y,
                dst_w,
                dst_h,
            );
        }
        Err(e) => {
            log::error!("Failed to create ImageData: {e:?}");
        }
    }
}

/// Blit a `PreviewImage` as a viewport background, correctly mapping its time
/// axis to the current scroll position and visible time range.  Used as a
/// fallback while the full-resolution spectrogram tiles are being computed.
pub fn blit_preview_as_background(
    ctx: &CanvasRenderingContext2d,
    preview: &PreviewImage,
    viewport_width: f64,
    viewport_height: f64,
    scroll_offset: f64,    // left edge of viewport in seconds
    visible_time: f64,     // seconds of audio visible in viewport
    total_duration: f64,   // total file duration in seconds
    freq_crop_lo: f64,     // 0..1 fraction of Nyquist
    freq_crop_hi: f64,     // 0..1 fraction of Nyquist
    colormap: ColormapMode,
) {
    let cw = viewport_width;
    let ch = viewport_height;

    ctx.set_fill_style_str("#000");
    ctx.fill_rect(0.0, 0.0, cw, ch);

    if preview.width == 0 || preview.height == 0 || total_duration <= 0.0 {
        return;
    }

    // Map viewport time range to preview pixel columns.
    // The preview spans the entire file: column 0 = time 0, column W = total_duration.
    let pw = preview.width as f64;
    let Some((data_start, data_end, dst_x, dst_w)) = viewport::data_region_px(
        scroll_offset,
        visible_time,
        total_duration,
        cw,
    ) else { return; };
    let src_x = (data_start / total_duration * pw).clamp(0.0, pw);
    let remaining = pw - src_x;
    if remaining < 0.5 { return; }
    let src_w = (((data_end - data_start) / total_duration) * pw).max(0.5).min(remaining);

    // Vertical crop: row 0 = highest freq, last row = 0 Hz
    let fc_lo = freq_crop_lo.max(0.0);
    let fc_hi = freq_crop_hi.max(0.01);
    let full_h = preview.height as f64;
    let (src_y, src_h, dst_y, dst_h) = if fc_hi <= 1.0 {
        let sy = full_h * (1.0 - fc_hi);
        let sh = full_h * (fc_hi - fc_lo).max(0.001);
        (sy, sh, 0.0, ch)
    } else {
        let fc_range = (fc_hi - fc_lo).max(0.001);
        let data_frac = (1.0 - fc_lo) / fc_range;
        let sh = full_h * (1.0 - fc_lo);
        (0.0, sh, ch * (1.0 - data_frac), ch * data_frac)
    };

    // Apply colormap to preview pixels (preview is stored as greyscale)
    let mut pixels = preview.pixels.as_ref().clone();
    match colormap {
        ColormapMode::Uniform(cm) => apply_colormap_to_tile(&mut pixels, cm),
        ColormapMode::HfrFocus { colormap: cm, band_ff_lo_frac, band_ff_hi_frac } => {
            apply_hfr_colormap_to_tile(
                &mut pixels, preview.width, preview.height,
                cm, band_ff_lo_frac, band_ff_hi_frac,
            );
        }
    }

    let clamped = Clamped(&pixels[..]);
    let Ok(img) = ImageData::new_with_u8_clamped_array_and_sh(
        clamped, preview.width, preview.height,
    ) else { return };

    let Some((tmp, tmp_ctx)) = get_tmp_canvas(preview.width, preview.height) else { return };
    let _ = tmp_ctx.put_image_data(&img, 0.0, 0.0);

    let _ = ctx.draw_image_with_html_canvas_element_and_sw_and_sh_and_dx_and_dy_and_dw_and_dh(
        &tmp,
        src_x, src_y, src_w, src_h,
        dst_x, dst_y, dst_w, dst_h,
    );
}

// ── Tile-based rendering ─────────────────────────────────────────────────────

use crate::canvas::tile_cache::{self, TILE_COLS};
use std::cell::RefCell;
use std::collections::HashMap;

/// Entry in the tile canvas cache with LRU stamp for proper eviction.
struct TileCanvasEntry {
    canvas: HtmlCanvasElement,
    fingerprint: u64,
    stamp: u64,
}

/// LRU tile canvas cache. Evicts least-recently-used entries instead of
/// arbitrary HashMap-order entries, preventing visible tiles from being
/// evicted while panning on large files.
struct TileCanvasLru {
    entries: HashMap<(usize, u8, usize), TileCanvasEntry>,
    next_stamp: u64,
}

impl TileCanvasLru {
    fn new() -> Self {
        Self { entries: HashMap::new(), next_stamp: 0 }
    }

    fn get(&mut self, key: &(usize, u8, usize), fingerprint: u64) -> Option<HtmlCanvasElement> {
        let entry = self.entries.get_mut(key)?;
        if entry.fingerprint != fingerprint {
            return None;
        }
        self.next_stamp += 1;
        entry.stamp = self.next_stamp;
        Some(entry.canvas.clone())
    }

    fn insert(&mut self, key: (usize, u8, usize), canvas: HtmlCanvasElement, fingerprint: u64) {
        self.next_stamp += 1;
        let stamp = self.next_stamp;
        self.entries.insert(key, TileCanvasEntry { canvas, fingerprint, stamp });
        if self.entries.len() > 256 {
            // Evict oldest entries by LRU stamp
            let mut stamps: Vec<((usize, u8, usize), u64)> = self.entries.iter()
                .map(|(&k, e)| (k, e.stamp))
                .collect();
            stamps.sort_unstable_by_key(|&(_, s)| s);
            let to_remove = self.entries.len() - 128;
            for (k, _) in stamps.into_iter().take(to_remove) {
                self.entries.remove(&k);
            }
        }
    }

    fn clear(&mut self) {
        self.entries.clear();
    }

    fn retain(&mut self, f: impl Fn(&(usize, u8, usize)) -> bool) {
        self.entries.retain(|k, _| f(k));
    }
}

thread_local! {
    /// Reusable off-screen canvas for blitting tile ImageData.
    /// Avoids creating a new canvas element every frame for each tile.
    static TMP_CANVAS: RefCell<Option<(HtmlCanvasElement, CanvasRenderingContext2d)>> =
        const { RefCell::new(None) };
    /// Per-tile offscreen canvas cache with LRU eviction.
    /// Avoids re-running db_tile_to_rgba + ImageData + put_image_data on every frame
    /// when only scroll position changes (panning).
    static TILE_CANVAS_CACHE: RefCell<TileCanvasLru> =
        RefCell::new(TileCanvasLru::new());
}

/// Compute a fingerprint of the rendering parameters that affect tile RGBA output.
/// When this changes, cached tile canvases must be re-rendered.
fn tile_render_fingerprint(
    settings: &SpectDisplaySettings,
    render_mode: &TileRenderMode,
    freq_adj_hash: u64,
) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325; // FNV offset basis
    let mix = |h: &mut u64, v: u64| {
        *h ^= v;
        *h = h.wrapping_mul(0x100000001b3); // FNV prime
    };
    mix(&mut h, settings.floor_db.to_bits() as u64);
    mix(&mut h, settings.range_db.to_bits() as u64);
    mix(&mut h, settings.gamma.to_bits() as u64);
    mix(&mut h, settings.gain_db.to_bits() as u64);
    match render_mode {
        TileRenderMode::Spectrogram(colormap) => match colormap {
            ColormapMode::Uniform(cm) => {
                mix(&mut h, 0);
                mix(&mut h, *cm as u64);
            }
            ColormapMode::HfrFocus { colormap: cm, band_ff_lo_frac, band_ff_hi_frac } => {
                mix(&mut h, 1);
                mix(&mut h, *cm as u64);
                mix(&mut h, band_ff_lo_frac.to_bits());
                mix(&mut h, band_ff_hi_frac.to_bits());
            }
        },
        TileRenderMode::Flow { intensity_gate, flow_gate, opacity, shift_gain, color_gamma, algo, scheme } => {
            mix(&mut h, 2);
            mix(&mut h, intensity_gate.to_bits() as u64);
            mix(&mut h, flow_gate.to_bits() as u64);
            mix(&mut h, opacity.to_bits() as u64);
            mix(&mut h, shift_gain.to_bits() as u64);
            mix(&mut h, color_gamma.to_bits() as u64);
            mix(&mut h, *algo as u64);
            mix(&mut h, *scheme as u64);
        }
    }
    mix(&mut h, freq_adj_hash);
    h
}

/// Compute a simple hash of freq_adjustments for cache invalidation.
fn hash_freq_adjustments(adj: Option<&[f32]>) -> u64 {
    let Some(a) = adj else { return 0 };
    let mut h: u64 = a.len() as u64;
    // Sample a few values for a fast approximate hash
    for &idx in &[0, a.len() / 4, a.len() / 2, 3 * a.len() / 4, a.len().saturating_sub(1)] {
        if idx < a.len() {
            h ^= a[idx].to_bits() as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    h
}

/// Invalidate all cached tile canvases (e.g. on file change or cache clear).
pub fn clear_tile_canvas_cache() {
    TILE_CANVAS_CACHE.with(|c| c.borrow_mut().clear());
}

/// Evict tile canvas cache entries for a specific file.
pub fn evict_tile_canvas_cache_for_file(file_idx: usize) {
    TILE_CANVAS_CACHE.with(|c| {
        c.borrow_mut().retain(|&(fi, _, _)| fi != file_idx);
    });
}

/// Get or create a reusable off-screen canvas of at least the given dimensions.
fn get_tmp_canvas(w: u32, h: u32) -> Option<(HtmlCanvasElement, CanvasRenderingContext2d)> {
    TMP_CANVAS.with(|cell| {
        let mut slot = cell.borrow_mut();
        if let Some((ref c, ref ctx)) = *slot {
            if c.width() >= w && c.height() >= h {
                return Some((c.clone(), ctx.clone()));
            }
        }
        // Create new
        let doc = web_sys::window()?.document()?;
        let c = doc.create_element("canvas").ok()?
            .dyn_into::<HtmlCanvasElement>().ok()?;
        c.set_width(w);
        c.set_height(h);
        let ctx = c.get_context("2d").ok()??.dyn_into::<CanvasRenderingContext2d>().ok()?;
        *slot = Some((c.clone(), ctx.clone()));
        Some((c, ctx))
    })
}

/// Apply a colormap LUT to greyscale RGBA pixels in-place.
fn apply_colormap_to_tile(pixels: &mut [u8], colormap: Colormap) {
    if colormap == Colormap::Greyscale {
        return;
    }
    for chunk in pixels.chunks_exact_mut(4) {
        let [r, g, b] = colormap.apply(chunk[0]);
        chunk[0] = r;
        chunk[1] = g;
        chunk[2] = b;
    }
}

/// Apply HFR-focus colormap: color inside focus band, greyscale outside.
fn apply_hfr_colormap_to_tile(
    pixels: &mut [u8], width: u32, height: u32,
    colormap: Colormap, band_ff_lo_frac: f64, band_ff_hi_frac: f64,
) {
    let h = height as f64;
    let w = width as usize;
    let focus_top = (h * (1.0 - band_ff_hi_frac)).round() as usize;
    let focus_bot = (h * (1.0 - band_ff_lo_frac)).round() as usize;
    for row in 0..height as usize {
        if row >= focus_top && row < focus_bot {
            let base = row * w * 4;
            for col in 0..w {
                let i = base + col * 4;
                let [r, g, b] = colormap.apply(pixels[i]);
                pixels[i] = r;
                pixels[i + 1] = g;
                pixels[i + 2] = b;
            }
        }
    }
}

/// Convert tile dB data (and optional flow shifts) to RGBA pixels.
/// Dispatches on `TileRenderMode` to apply either colormap or flow-specific coloring.
fn tile_to_rgba(
    rendered: &PreRendered,
    settings: &SpectDisplaySettings,
    render_mode: &TileRenderMode,
    freq_adjustments: Option<&[f32]>,
) -> Vec<u8> {
    let db_data = &rendered.db_data;
    let total = db_data.len();
    let mut rgba = vec![0u8; total * 4];
    let w = rendered.width as usize;

    match render_mode {
        TileRenderMode::Spectrogram(colormap) => match colormap {
            ColormapMode::Uniform(cm) => {
                for (i, &db) in db_data.iter().enumerate() {
                    let row = if w > 0 { i / w } else { 0 };
                    let extra = freq_adjustments.and_then(|a| a.get(row).copied()).unwrap_or(0.0);
                    let grey = db_to_greyscale(db, settings.floor_db, settings.range_db, settings.gamma, settings.gain_db + extra);
                    let [r, g, b] = cm.apply(grey);
                    let pi = i * 4;
                    rgba[pi] = r;
                    rgba[pi + 1] = g;
                    rgba[pi + 2] = b;
                    rgba[pi + 3] = 255;
                }
            }
            ColormapMode::HfrFocus { colormap: cm, band_ff_lo_frac, band_ff_hi_frac } => {
                let h = rendered.height as f64;
                let focus_top = (h * (1.0 - band_ff_hi_frac)).round() as usize;
                let focus_bot = (h * (1.0 - band_ff_lo_frac)).round() as usize;
                for (i, &db) in db_data.iter().enumerate() {
                    let row = if w > 0 { i / w } else { 0 };
                    let extra = freq_adjustments.and_then(|a| a.get(row).copied()).unwrap_or(0.0);
                    let grey = db_to_greyscale(db, settings.floor_db, settings.range_db, settings.gamma, settings.gain_db + extra);
                    let [r, g, b] = if row >= focus_top && row < focus_bot {
                        cm.apply(grey)
                    } else {
                        [grey, grey, grey]
                    };
                    let pi = i * 4;
                    rgba[pi] = r;
                    rgba[pi + 1] = g;
                    rgba[pi + 2] = b;
                    rgba[pi + 3] = 255;
                }
            }
        },
        TileRenderMode::Flow { intensity_gate, flow_gate, opacity, shift_gain, color_gamma, algo, scheme } => {
            let flow_shifts = &rendered.flow_shifts;
            for (i, &db) in db_data.iter().enumerate() {
                let row = if w > 0 { i / w } else { 0 };
                let extra = freq_adjustments.and_then(|a| a.get(row).copied()).unwrap_or(0.0);
                let grey = db_to_greyscale(
                    db, settings.floor_db, settings.range_db,
                    settings.gamma, settings.gain_db + extra,
                );
                let shift = if i < flow_shifts.len() { flow_shifts[i] } else { 0.0 };
                let [r, g, b] = match algo {
                    FlowAlgo::Phase => phase_rgb(grey, shift, *intensity_gate),
                    FlowAlgo::PhaseCoherence => coherence_rgb(grey, shift, *intensity_gate, *flow_gate, *opacity, *shift_gain, *color_gamma),
                    _ => flow_rgb_scheme(grey, shift, *intensity_gate, *flow_gate, *opacity, *shift_gain, *color_gamma, *scheme),
                };
                let pi = i * 4;
                rgba[pi] = r;
                rgba[pi + 1] = g;
                rgba[pi + 2] = b;
                rgba[pi + 3] = 255;
            }
        }
    }

    rgba
}

/// Composite spectrogram or flow tiles from the tile cache onto the canvas.
/// Falls back to a preview image for tiles not yet cached.
/// Returns true if at least one tile was drawn, false if nothing was available.
pub fn blit_tiles_viewport(
    ctx: &CanvasRenderingContext2d,
    viewport_width: f64,
    viewport_height: f64,
    file_idx: usize,
    total_cols: usize,
    scroll_col: f64,
    zoom: f64,
    freq_crop_lo: f64,
    freq_crop_hi: f64,
    render_mode: TileRenderMode,
    display_settings: &SpectDisplaySettings,
    freq_adjustments: Option<&[f32]>,
    preview: Option<&PreviewImage>,
    scroll_offset: f64,
    visible_time: f64,
    total_duration: f64,
    tile_source: TileSource,
) -> bool {
    use crate::canvas::tile_blit::{ViewportGeometry, compute_tile_blit_coords, for_each_visible_tile};

    let cw = viewport_width;
    let ch = viewport_height;

    // For preview fallback, flow mode uses greyscale; spectrogram uses its colormap.
    let preview_colormap = match &render_mode {
        TileRenderMode::Spectrogram(cm) => *cm,
        TileRenderMode::Flow { .. } => ColormapMode::Uniform(Colormap::Greyscale),
    };

    let Some(vg) = ViewportGeometry::new(cw, ch, total_cols, scroll_col, zoom, freq_crop_lo, freq_crop_hi)
    else {
        if let Some(pv) = preview {
            blit_preview_as_background(
                ctx, pv, cw, ch,
                scroll_offset, visible_time, total_duration,
                freq_crop_lo, freq_crop_hi, preview_colormap,
            );
        } else {
            ctx.set_fill_style_str("#000");
            ctx.fill_rect(0.0, 0.0, cw, ch);
        }
        return preview.is_some();
    };

    // Quick check: do all visible tiles exist at the ideal LOD (or any fallback)?
    let all_tiles_covered = {
        let check_fn = |fi: usize, lod: u8, ti: usize| -> bool {
            match tile_source {
                TileSource::Normal => tile_cache::get_tile(fi, lod, ti).is_some(),
                TileSource::Reassigned => tile_cache::get_reassign_tile(fi, lod, ti).is_some(),
                TileSource::Flow => tile_cache::get_flow_tile(fi, lod, ti).is_some(),
            }
        };
        let fallback_fn = |fi: usize, lod: u8, ti: usize| -> bool {
            match tile_source {
                TileSource::Flow => tile_cache::get_flow_tile(fi, lod, ti).is_some(),
                _ => tile_cache::get_tile(fi, lod, ti).is_some(),
            }
        };
        let mut covered = true;
        for tile_idx in vg.first_tile..=vg.last_tile {
            if vg.tile_clip_range(tile_idx).is_none() { continue; }
            if check_fn(file_idx, vg.ideal_lod, tile_idx) { continue; }
            let mut found_fallback = false;
            for fb_lod in (0..vg.ideal_lod).rev() {
                let (fb_tile, _, _) = tile_cache::fallback_tile_info(vg.ideal_lod, tile_idx, fb_lod);
                if fallback_fn(file_idx, fb_lod, fb_tile) {
                    found_fallback = true;
                    break;
                }
            }
            if !found_fallback {
                covered = false;
                break;
            }
        }
        covered
    };

    if !all_tiles_covered {
        if let Some(pv) = preview {
            blit_preview_as_background(
                ctx, pv, cw, ch,
                scroll_offset, visible_time, total_duration,
                freq_crop_lo, freq_crop_hi, preview_colormap,
            );
        } else {
            ctx.set_fill_style_str("#000");
            ctx.fill_rect(0.0, 0.0, cw, ch);
        }
    } else {
        ctx.set_fill_style_str("#000");
        ctx.fill_rect(0.0, 0.0, cw, ch);
    }

    // Compute fingerprint for tile canvas cache invalidation.
    let adj_hash = hash_freq_adjustments(freq_adjustments);
    let fingerprint = tile_render_fingerprint(display_settings, &render_mode, adj_hash);

    // Draw a tile to the canvas given its LOD and screen clip range.
    // Uses a per-tile offscreen canvas cache to avoid re-running tile_to_rgba
    // + ImageData + put_image_data when only the scroll position changes.
    let blit_any_tile = |tile: &tile_cache::Tile, tile_lod: u8, tile_idx: usize,
                         clip_start: f64, clip_end: f64| {
        let Some(coords) = compute_tile_blit_coords(
            &vg, tile.rendered.width as f64, tile.rendered.height as f64,
            tile_lod, tile_idx, clip_start, clip_end,
        ) else { return };

        let cache_key = (tile.file_idx, tile_lod, tile_idx);

        // Check if we have a cached canvas for this tile with matching settings.
        let cached = TILE_CANVAS_CACHE.with(|c| {
            c.borrow_mut().get(&cache_key, fingerprint)
        });

        let tile_canvas = if let Some(c) = cached {
            c
        } else {
            // Render tile to a new offscreen canvas and cache it.
            let pixels = if !tile.rendered.db_data.is_empty() {
                tile_to_rgba(&tile.rendered, display_settings, &render_mode, freq_adjustments)
            } else {
                let mut px = tile.rendered.pixels.clone();
                if let TileRenderMode::Spectrogram(colormap) = &render_mode {
                    match colormap {
                        ColormapMode::Uniform(cm) => apply_colormap_to_tile(&mut px, *cm),
                        ColormapMode::HfrFocus { colormap: cm, band_ff_lo_frac, band_ff_hi_frac } => {
                            apply_hfr_colormap_to_tile(
                                &mut px, tile.rendered.width, tile.rendered.height,
                                *cm, *band_ff_lo_frac, *band_ff_hi_frac,
                            );
                        }
                    }
                }
                px
            };

            let clamped = Clamped(&pixels[..]);
            let Ok(img) = ImageData::new_with_u8_clamped_array_and_sh(
                clamped, tile.rendered.width, tile.rendered.height,
            ) else { return };

            let Some((tmp, tmp_ctx)) = get_tmp_canvas(tile.rendered.width, tile.rendered.height) else { return };
            let _ = tmp_ctx.put_image_data(&img, 0.0, 0.0);

            // Create a dedicated offscreen canvas for this tile's cache
            let doc = match web_sys::window().and_then(|w| w.document()) {
                Some(d) => d,
                None => {
                    // Enable smoothing for fallback tiles and for coarse overview LODs
                    // (which downscale significantly and would otherwise look glittery)
                    ctx.set_image_smoothing_enabled(
                        tile_lod != vg.ideal_lod || vg.ideal_lod < tile_cache::LOD_BASELINE
                    );
                    let _ = ctx.draw_image_with_html_canvas_element_and_sw_and_sh_and_dx_and_dy_and_dw_and_dh(
                        &tmp,
                        coords.src_x, coords.src_y, coords.src_w, coords.src_h,
                        coords.dst_x, coords.dst_y, coords.dst_w, coords.dst_h,
                    );
                    return;
                }
            };
            let tc = doc.create_element("canvas").ok()
                .and_then(|el| el.dyn_into::<HtmlCanvasElement>().ok());
            let Some(tc) = tc else {
                ctx.set_image_smoothing_enabled(
                    tile_lod != vg.ideal_lod || vg.ideal_lod < tile_cache::LOD_BASELINE
                );
                let _ = ctx.draw_image_with_html_canvas_element_and_sw_and_sh_and_dx_and_dy_and_dw_and_dh(
                    &tmp,
                    coords.src_x, coords.src_y, coords.src_w, coords.src_h,
                    coords.dst_x, coords.dst_y, coords.dst_w, coords.dst_h,
                );
                return;
            };
            tc.set_width(tile.rendered.width);
            tc.set_height(tile.rendered.height);
            if let Some(tc_ctx) = tc.get_context("2d").ok().flatten()
                .and_then(|c| c.dyn_into::<CanvasRenderingContext2d>().ok())
            {
                let _ = tc_ctx.draw_image_with_html_canvas_element(&tmp, 0.0, 0.0);
            }

            TILE_CANVAS_CACHE.with(|c| {
                c.borrow_mut().insert(cache_key, tc.clone(), fingerprint);
            });

            tc
        };

        // Enable smoothing for fallback tiles and for coarse overview LODs
        // (which downscale significantly and would otherwise look glittery)
        ctx.set_image_smoothing_enabled(
            tile_lod != vg.ideal_lod || vg.ideal_lod < tile_cache::LOD_BASELINE
        );
        let _ = ctx.draw_image_with_html_canvas_element_and_sw_and_sh_and_dx_and_dy_and_dw_and_dh(
            &tile_canvas,
            coords.src_x, coords.src_y, coords.src_w, coords.src_h,
            coords.dst_x, coords.dst_y, coords.dst_w, coords.dst_h,
        );
    };

    let any_drawn = for_each_visible_tile(
        &vg,
        |tile_idx, clip_start, clip_end| {
            let borrow_fn = |fi: usize, lod: u8, ti: usize, f: &dyn Fn(&tile_cache::Tile)| -> Option<()> {
                match tile_source {
                    TileSource::Normal => tile_cache::borrow_tile(fi, lod, ti, |t| f(t)),
                    TileSource::Reassigned => tile_cache::borrow_reassign_tile(fi, lod, ti, |t| f(t)),
                    TileSource::Flow => tile_cache::borrow_flow_tile(fi, lod, ti, |t| f(t)),
                }
            };
            borrow_fn(file_idx, vg.ideal_lod, tile_idx, &|tile| {
                blit_any_tile(tile, vg.ideal_lod, tile_idx, clip_start, clip_end);
            }).is_some()
        },
        |fb_tile, fb_lod, clip_start, clip_end| {
            // For flow tiles, fallback must also use flow cache
            match tile_source {
                TileSource::Flow => {
                    tile_cache::borrow_flow_tile(file_idx, fb_lod, fb_tile, |tile| {
                        blit_any_tile(tile, fb_lod, fb_tile, clip_start, clip_end);
                    }).is_some()
                }
                _ => {
                    tile_cache::borrow_tile(file_idx, fb_lod, fb_tile, |tile| {
                        blit_any_tile(tile, fb_lod, fb_tile, clip_start, clip_end);
                    }).is_some()
                }
            }
        },
    );

    any_drawn || preview.is_some()
}

/// Compute RGB for a chromagram flow pixel.
/// class_byte (R), note_byte (G), flow_byte (B: 128=neutral, 0=decrease, 255=increase).
fn chromagram_flow_pixel(class_byte: u8, note_byte: u8, flow_byte: u8) -> [u8; 3] {
    use crate::canvas::colormap_2d::hsl_to_rgb;

    let class = class_byte as f32 / 255.0;
    let note = note_byte as f32 / 255.0;
    let brightness = class * 0.4 + note * 0.6;
    let shift = (flow_byte as f32 - 128.0) / 128.0; // -1.0 to 1.0

    // Base hue = 60 (warm yellow)
    // Increase (shift > 0) pushes toward 120 (green)
    // Decrease (shift < 0) pushes toward 300 (purple/magenta)
    let hue = if shift >= 0.0 {
        60.0 + shift * 60.0 // 60..120
    } else {
        60.0 + shift * 120.0 // wraps: 60 - 120 = -60 -> 300
    };
    let hue = ((hue % 360.0) + 360.0) % 360.0;
    let saturation = shift.abs() * 0.8;

    hsl_to_rgb(hue, saturation, brightness * 0.5)
}

/// Blit chromagram tiles from the chromagram tile cache.
///
/// Chromagram tiles store packed (class_intensity, note_intensity) in R/G channels.
/// A 2D colormap is applied during blit to convert to final RGB.
pub fn blit_chromagram_tiles_viewport(
    ctx: &CanvasRenderingContext2d,
    canvas: &HtmlCanvasElement,
    file_idx: usize,
    total_cols: usize,
    scroll_col: f64,
    zoom: f64,
    chroma_colormap: crate::state::ChromaColormap,
    chroma_gamma: f32,
    num_octaves: usize,
) -> bool {
    use crate::state::ChromaColormap;
    use crate::canvas::colormap_2d::{
        build_chromagram_colormap, build_chromagram_pitch_class_colormaps,
        build_chromagram_solid_colormaps, build_chromagram_octave_colormaps, Colormap2D,
    };

    enum ChromaMode {
        Single(Colormap2D),
        PerPitchClass([Colormap2D; 12]),
        PerOctave([Colormap2D; 10]),
        FlowInline,
    }

    let mode = match chroma_colormap {
        ChromaColormap::Warm => ChromaMode::Single(build_chromagram_colormap()),
        ChromaColormap::PitchClass => ChromaMode::PerPitchClass(build_chromagram_pitch_class_colormaps()),
        ChromaColormap::Solid => ChromaMode::PerPitchClass(build_chromagram_solid_colormaps()),
        ChromaColormap::Octave => ChromaMode::PerOctave(build_chromagram_octave_colormaps()),
        ChromaColormap::Flow => ChromaMode::FlowInline,
    };

    let cw = canvas.width() as f64;
    let ch = canvas.height() as f64;

    ctx.set_fill_style_str("#000");
    ctx.fill_rect(0.0, 0.0, cw, ch);

    if total_cols == 0 || zoom <= 0.0 {
        return false;
    }

    let visible_cols = cw / zoom;
    let vis_start = scroll_col;
    let vis_end = scroll_col + visible_cols;
    let src_start = vis_start.max(0.0);
    let src_end = vis_end.min(total_cols as f64);
    if src_end <= src_start {
        return false;
    }

    let first_tile = (src_start / TILE_COLS as f64).floor() as usize;
    let last_tile = ((src_end - 1.0).max(0.0) / TILE_COLS as f64).floor() as usize;
    let n_tiles = total_cols.div_ceil(TILE_COLS);

    let mut any_drawn = false;

    for tile_idx in first_tile..=last_tile.min(n_tiles.saturating_sub(1)) {
        let tile_col_start = tile_idx * TILE_COLS;

        let drawn = tile_cache::borrow_chroma_tile(file_idx, tile_idx, |tile| {
            let tw = tile.rendered.width as f64;
            let th = tile.rendered.height as f64;
            if tw == 0.0 || th == 0.0 { return; }

            // Apply gamma then 2D chromagram colormap: R=class intensity, G=note intensity, B=flow
            // (Gain is baked into tiles at pre-render time for full dynamic range.)
            let apply_gamma = chroma_gamma != 1.0;
            #[inline]
            fn adjust_gamma(val: u8, gamma: f32) -> u8 {
                let norm = val as f32 / 255.0;
                (norm.powf(gamma) * 255.0) as u8
            }
            let mut pixels = tile.rendered.pixels.clone();
            for i in (0..pixels.len()).step_by(4) {
                let class_byte = if apply_gamma { adjust_gamma(pixels[i], chroma_gamma) } else { pixels[i] };
                let note_byte = if apply_gamma { adjust_gamma(pixels[i + 1], chroma_gamma) } else { pixels[i + 1] };
                let flow_byte = pixels[i + 2];
                let pixel_idx = i / 4;
                let tile_w = tile.rendered.width as usize;
                let row = pixel_idx / tile_w;

                let scale = crate::dsp::chromagram::CHROMA_RENDER_SCALE;
                const NUM_PITCH_CLASSES: usize = crate::dsp::chromagram::NUM_PITCH_CLASSES;
                let logical_row = row / scale;
                let [r, g, b] = match &mode {
                    ChromaMode::Single(cm) => cm.apply(class_byte, note_byte),
                    ChromaMode::PerPitchClass(cms) => {
                        let pc = (NUM_PITCH_CLASSES - 1).saturating_sub(logical_row / num_octaves).min(NUM_PITCH_CLASSES - 1);
                        cms[pc].apply(class_byte, note_byte)
                    }
                    ChromaMode::PerOctave(cms) => {
                        let oct = (num_octaves - 1).saturating_sub(logical_row % num_octaves).min(9);
                        cms[oct].apply(class_byte, note_byte)
                    }
                    ChromaMode::FlowInline => {
                        chromagram_flow_pixel(class_byte, note_byte, flow_byte)
                    }
                };
                pixels[i] = r;
                pixels[i + 1] = g;
                pixels[i + 2] = b;
            }

            let clamped = Clamped(&pixels[..]);
            let Ok(img) = ImageData::new_with_u8_clamped_array_and_sh(
                clamped, tile.rendered.width, tile.rendered.height,
            ) else { return };

            let Some((tmp, tmp_ctx)) = get_tmp_canvas(tile.rendered.width, tile.rendered.height) else { return };
            if tmp.width() != tile.rendered.width || tmp.height() != tile.rendered.height {
                tmp.set_width(tile.rendered.width);
                tmp.set_height(tile.rendered.height);
            }
            let _ = tmp_ctx.put_image_data(&img, 0.0, 0.0);

            let tile_src_x = (src_start - tile_col_start as f64).max(0.0);
            let tile_src_end = (src_end - tile_col_start as f64).min(tw);
            let tile_src_w = (tile_src_end - tile_src_x).max(0.0);
            if tile_src_w <= 0.0 { return; }

            // No frequency cropping for chromagram — show full height
            let dst_x_raw = ((tile_col_start as f64 + tile_src_x) - vis_start) * zoom;
            let dst_x_end_raw = ((tile_col_start as f64 + tile_src_x + tile_src_w) - vis_start) * zoom;
            let dst_x = dst_x_raw.floor();
            let dst_w = (dst_x_end_raw.ceil() - dst_x).max(1.0);

            // Enable smoothing for upscaling (chromagram has few rows)
            ctx.set_image_smoothing_enabled(true);
            let _ = ctx.draw_image_with_html_canvas_element_and_sw_and_sh_and_dx_and_dy_and_dw_and_dh(
                &tmp,
                tile_src_x, 0.0, tile_src_w, th,
                dst_x, 0.0, dst_w, ch,
            );
        });

        if drawn.is_some() {
            any_drawn = true;
        }
    }

    any_drawn
}
