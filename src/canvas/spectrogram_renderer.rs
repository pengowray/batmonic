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

// Re-export from split modules so callers don't need to change imports
pub use crate::canvas::flow::{FlowAlgo, FlowData, compute_flow_data, composite_flow, pre_render_flow_columns};
pub use crate::canvas::overlays::{
    FreqShiftMode, FreqMarkerState,
    draw_freq_markers, draw_time_markers, draw_ff_overlay, draw_het_overlay,
    draw_pulses, draw_selection, draw_harmonic_shadows, draw_filter_overlay,
    pixel_to_time_freq, draw_notch_bands, draw_tile_debug_overlay, draw_annotations,
};

/// Pre-rendered spectrogram image data.
///
/// Normal spectrogram tiles store `db_data` (f32 dB values per pixel) so that
/// gain, contrast, and dynamic range can be adjusted at render time without
/// regenerating tiles.  Flow tiles store `db_data` + `flow_shifts` for deferred
/// compositing.  Coherence and chromagram tiles store pre-colored `pixels`
/// (RGBA u8) because their color encoding is coupled to the data.
pub struct PreRendered {
    pub width: u32,
    pub height: u32,
    /// RGBA pixel data (4 bytes/pixel).  Used by coherence, chromagram
    /// tiles and legacy non-tiled rendering.  Empty for dB tiles.
    pub pixels: Vec<u8>,
    /// dB values per pixel (one f32 per pixel, row-major, row 0 = highest freq).
    /// Used by normal spectrogram tiles and flow tiles.  Empty for pre-colored tiles.
    pub db_data: Vec<f32>,
    /// Per-pixel frequency shift values (same layout as db_data).
    /// Non-empty only for flow tiles.  Used with `db_data` for deferred flow compositing.
    pub flow_shifts: Vec<f32>,
}

/// Display settings for converting dB tile data to pixels at render time.
#[derive(Clone, Copy)]
pub struct SpectDisplaySettings {
    /// dB floor (e.g. -80.0).  Values below this map to black.
    pub floor_db: f32,
    /// dB range (e.g. 80.0).  `floor_db + range_db` = ceiling.
    pub range_db: f32,
    /// Gamma curve (1.0 = linear, <1 = brighter darks, >1 = more contrast).
    pub gamma: f32,
    /// Additive dB gain offset applied before floor/range mapping.
    pub gain_db: f32,
}

impl Default for SpectDisplaySettings {
    fn default() -> Self {
        Self { floor_db: -80.0, range_db: 80.0, gamma: 1.0, gain_db: 0.0 }
    }
}

impl PreRendered {
    /// Total memory footprint in bytes (for LRU cache accounting).
    pub fn byte_len(&self) -> usize {
        self.pixels.len()
            + self.db_data.len() * std::mem::size_of::<f32>()
            + self.flow_shifts.len() * std::mem::size_of::<f32>()
    }
}

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
#[derive(Clone, Copy)]
pub enum ColormapMode {
    /// Uniform colormap across the entire spectrogram.
    Uniform(Colormap),
    /// Colormap inside HFR focus band, greyscale outside.
    /// Fractions are relative to the full image (0 Hz = 0.0, file_max_freq = 1.0).
    HfrFocus { colormap: Colormap, ff_lo_frac: f64, ff_hi_frac: f64 },
}

/// Blit the pre-rendered spectrogram to a visible canvas, handling scroll, zoom, and freq crop.
/// `freq_crop_lo` / `freq_crop_hi` are fractions (0..1) of the full image height:
/// lo = min_display_freq / file_max_freq, hi = max_display_freq / file_max_freq.
pub fn blit_viewport(
    ctx: &CanvasRenderingContext2d,
    pre_rendered: &PreRendered,
    canvas: &HtmlCanvasElement,
    scroll_col: f64,
    zoom: f64,
    freq_crop_lo: f64,
    freq_crop_hi: f64,
    colormap: ColormapMode,
) {
    let cw = canvas.width() as f64;
    let ch = canvas.height() as f64;

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
        ColormapMode::HfrFocus { colormap: cm, ff_lo_frac, ff_hi_frac } => {
            mapped_pixels = {
                let mut buf = pre_rendered.pixels.clone();
                let h = pre_rendered.height as f64;
                let w = pre_rendered.width as usize;
                // Row 0 = highest freq; last row = 0 Hz
                let focus_top = (h * (1.0 - ff_hi_frac)).round() as usize;
                let focus_bot = (h * (1.0 - ff_lo_frac)).round() as usize;
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
    canvas: &HtmlCanvasElement,
    scroll_offset: f64,    // left edge of viewport in seconds
    visible_time: f64,     // seconds of audio visible in viewport
    total_duration: f64,   // total file duration in seconds
    freq_crop_lo: f64,     // 0..1 fraction of Nyquist
    freq_crop_hi: f64,     // 0..1 fraction of Nyquist
    colormap: ColormapMode,
) {
    let cw = canvas.width() as f64;
    let ch = canvas.height() as f64;

    ctx.set_fill_style_str("#000");
    ctx.fill_rect(0.0, 0.0, cw, ch);

    if preview.width == 0 || preview.height == 0 || total_duration <= 0.0 {
        return;
    }

    // Map viewport time range to preview pixel columns.
    // The preview spans the entire file: column 0 = time 0, column W = total_duration.
    let pw = preview.width as f64;
    let src_x = (scroll_offset / total_duration * pw).clamp(0.0, pw);
    let remaining = pw - src_x;
    if remaining < 0.5 { return; } // nothing meaningful left to draw
    let src_w = (visible_time / total_duration * pw).max(0.5).min(remaining);

    // Scale destination width so the preview only covers the portion of the
    // canvas that has actual file data.  This handles both short files that fit
    // entirely in the viewport AND viewports that extend past the file end
    // (e.g. follow-cursor near the end).
    let overlap_time = (total_duration - scroll_offset.max(0.0)).clamp(0.0, visible_time);
    let dst_w = cw * (overlap_time / visible_time).min(1.0);

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
        ColormapMode::HfrFocus { colormap: cm, ff_lo_frac, ff_hi_frac } => {
            apply_hfr_colormap_to_tile(
                &mut pixels, preview.width, preview.height,
                cm, ff_lo_frac, ff_hi_frac,
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
        0.0, dst_y, dst_w, dst_h,
    );
}

// ── Tile-based rendering ─────────────────────────────────────────────────────

use crate::canvas::tile_cache::{self, TILE_COLS};
use std::cell::RefCell;

thread_local! {
    /// Reusable off-screen canvas for blitting tile ImageData.
    /// Avoids creating a new canvas element every frame for each tile.
    static TMP_CANVAS: RefCell<Option<(HtmlCanvasElement, CanvasRenderingContext2d)>> =
        const { RefCell::new(None) };
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
    colormap: Colormap, ff_lo_frac: f64, ff_hi_frac: f64,
) {
    let h = height as f64;
    let w = width as usize;
    let focus_top = (h * (1.0 - ff_hi_frac)).round() as usize;
    let focus_bot = (h * (1.0 - ff_lo_frac)).round() as usize;
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

/// Convert dB tile data to RGBA pixels with display settings and colormap applied.
/// `freq_adjustments` is an optional per-row dB offset array (length == height).
fn db_tile_to_rgba(
    db_data: &[f32],
    width: u32,
    height: u32,
    settings: &SpectDisplaySettings,
    colormap: ColormapMode,
    freq_adjustments: Option<&[f32]>,
) -> Vec<u8> {
    let total = db_data.len();
    let mut rgba = vec![0u8; total * 4];
    let w = width as usize;

    match colormap {
        ColormapMode::Uniform(cm) => {
            for (i, &db) in db_data.iter().enumerate() {
                let row = i / w;
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
        ColormapMode::HfrFocus { colormap: cm, ff_lo_frac, ff_hi_frac } => {
            let h = height as f64;
            let focus_top = (h * (1.0 - ff_hi_frac)).round() as usize;
            let focus_bot = (h * (1.0 - ff_lo_frac)).round() as usize;
            for (i, &db) in db_data.iter().enumerate() {
                let row = i / w;
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
    }

    rgba
}

/// Composite spectrogram tiles from the tile cache onto the canvas.
/// Falls back to a preview image for tiles not yet cached.
/// Returns true if at least one tile was drawn, false if nothing was available.
pub fn blit_tiles_viewport(
    ctx: &CanvasRenderingContext2d,
    canvas: &HtmlCanvasElement,
    file_idx: usize,
    total_cols: usize,
    scroll_col: f64,
    zoom: f64,
    freq_crop_lo: f64,
    freq_crop_hi: f64,
    colormap: ColormapMode,
    display_settings: &SpectDisplaySettings,
    freq_adjustments: Option<&[f32]>,
    // Preview fallback for missing tiles
    preview: Option<&PreviewImage>,
    scroll_offset: f64,    // seconds (for preview mapping)
    visible_time: f64,     // seconds (for preview mapping)
    total_duration: f64,   // seconds (for preview mapping)
    tile_source: TileSource,
) -> bool {
    use crate::canvas::tile_blit::{ViewportGeometry, compute_tile_blit_coords, for_each_visible_tile};

    let cw = canvas.width() as f64;
    let ch = canvas.height() as f64;

    // Draw colormapped preview as base layer so tile gaps show preview, not black.
    if let Some(pv) = preview {
        blit_preview_as_background(
            ctx, pv, canvas,
            scroll_offset, visible_time, total_duration,
            freq_crop_lo, freq_crop_hi, colormap,
        );
    } else {
        ctx.set_fill_style_str("#000");
        ctx.fill_rect(0.0, 0.0, cw, ch);
    }

    let Some(vg) = ViewportGeometry::new(cw, ch, total_cols, scroll_col, zoom, freq_crop_lo, freq_crop_hi)
    else {
        return preview.is_some();
    };

    // Draw a tile to the canvas given its LOD and screen clip range.
    let blit_any_tile = |tile: &tile_cache::Tile, tile_lod: u8, tile_idx: usize,
                         clip_start: f64, clip_end: f64| {
        let Some(coords) = compute_tile_blit_coords(
            &vg, tile.rendered.width as f64, tile.rendered.height as f64,
            tile_lod, tile_idx, clip_start, clip_end,
        ) else { return };

        // Convert dB tile to RGBA
        let pixels = if !tile.rendered.db_data.is_empty() {
            db_tile_to_rgba(
                &tile.rendered.db_data,
                tile.rendered.width, tile.rendered.height,
                display_settings, colormap,
                freq_adjustments,
            )
        } else {
            let mut px = tile.rendered.pixels.clone();
            match colormap {
                ColormapMode::Uniform(cm) => apply_colormap_to_tile(&mut px, cm),
                ColormapMode::HfrFocus { colormap: cm, ff_lo_frac, ff_hi_frac } => {
                    apply_hfr_colormap_to_tile(
                        &mut px, tile.rendered.width, tile.rendered.height,
                        cm, ff_lo_frac, ff_hi_frac,
                    );
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

        ctx.set_image_smoothing_enabled(tile_lod != vg.ideal_lod);
        let _ = ctx.draw_image_with_html_canvas_element_and_sw_and_sh_and_dx_and_dy_and_dw_and_dh(
            &tmp,
            coords.src_x, coords.src_y, coords.src_w, coords.src_h,
            coords.dst_x, coords.dst_y, coords.dst_w, coords.dst_h,
        );
    };

    let any_drawn = for_each_visible_tile(
        &vg,
        |tile_idx, clip_start, clip_end| {
            let borrow_fn = |fi: usize, lod: u8, ti: usize, f: &dyn Fn(&tile_cache::Tile)| -> Option<()> {
                match tile_source {
                    TileSource::Reassigned => tile_cache::borrow_reassign_tile(fi, lod, ti, |t| f(t)),
                    TileSource::Normal => tile_cache::borrow_tile(fi, lod, ti, |t| f(t)),
                }
            };
            borrow_fn(file_idx, vg.ideal_lod, tile_idx, &|tile| {
                blit_any_tile(tile, vg.ideal_lod, tile_idx, clip_start, clip_end);
            }).is_some()
        },
        |fb_tile, fb_lod, clip_start, clip_end| {
            // Fallback always uses normal tiles (cheaper, already cached)
            tile_cache::borrow_tile(file_idx, fb_lod, fb_tile, |tile| {
                blit_any_tile(tile, fb_lod, fb_tile, clip_start, clip_end);
            }).is_some()
        },
    );

    any_drawn || preview.is_some()
}

/// Blit flow tiles from the flow tile cache (MV_CACHE).
///
/// Flow tiles store already-colored RGBA pixels (2D colormap pre-applied),
/// so no colormap step is needed during blit.
/// Convert flow tile dB+shift data to RGBA pixels at render time.
///
/// For each pixel: convert dB to greyscale using display settings, then apply
/// `flow_rgb()`, `coherence_rgb()`, or `phase_rgb()` depending on algorithm.
fn db_flow_tile_to_rgba(
    db_data: &[f32],
    flow_shifts: &[f32],
    width: u32,
    settings: &SpectDisplaySettings,
    intensity_gate: f32,
    flow_gate: f32,
    opacity: f32,
    shift_gain: f32,
    color_gamma: f32,
    algo: FlowAlgo,
    scheme: FlowColorScheme,
    freq_adjustments: Option<&[f32]>,
) -> Vec<u8> {
    let total = db_data.len();
    let mut rgba = vec![0u8; total * 4];
    let w = width as usize;

    for i in 0..total {
        let row = if w > 0 { i / w } else { 0 };
        let extra = freq_adjustments.and_then(|a| a.get(row).copied()).unwrap_or(0.0);
        let grey = db_to_greyscale(
            db_data[i], settings.floor_db, settings.range_db,
            settings.gamma, settings.gain_db + extra,
        );
        let shift = if i < flow_shifts.len() { flow_shifts[i] } else { 0.0 };
        let [r, g, b] = match algo {
            FlowAlgo::Phase => phase_rgb(grey, shift, intensity_gate),
            FlowAlgo::PhaseCoherence => coherence_rgb(grey, shift, intensity_gate, flow_gate, opacity, shift_gain, color_gamma),
            _ => flow_rgb_scheme(grey, shift, intensity_gate, flow_gate, opacity, shift_gain, color_gamma, scheme),
        };
        let pi = i * 4;
        rgba[pi] = r;
        rgba[pi + 1] = g;
        rgba[pi + 2] = b;
        rgba[pi + 3] = 255;
    }

    rgba
}

pub fn blit_flow_tiles_viewport(
    ctx: &CanvasRenderingContext2d,
    canvas: &HtmlCanvasElement,
    file_idx: usize,
    total_cols: usize,
    scroll_col: f64,
    zoom: f64,
    freq_crop_lo: f64,
    freq_crop_hi: f64,
    display_settings: &SpectDisplaySettings,
    freq_adjustments: Option<&[f32]>,
    intensity_gate: f32,
    flow_gate: f32,
    opacity: f32,
    shift_gain: f32,
    color_gamma: f32,
    algo: FlowAlgo,
    scheme: FlowColorScheme,
    preview: Option<&PreviewImage>,
    scroll_offset: f64,
    visible_time: f64,
    total_duration: f64,
) -> bool {
    use crate::canvas::tile_blit::{ViewportGeometry, compute_tile_blit_coords, for_each_visible_tile};

    let cw = canvas.width() as f64;
    let ch = canvas.height() as f64;

    // Draw a dark background (no colormap-aware preview for flow mode)
    if let Some(pv) = preview {
        blit_preview_as_background(
            ctx, pv, canvas,
            scroll_offset, visible_time, total_duration,
            freq_crop_lo, freq_crop_hi, ColormapMode::Uniform(Colormap::Greyscale),
        );
    } else {
        ctx.set_fill_style_str("#000");
        ctx.fill_rect(0.0, 0.0, cw, ch);
    }

    let Some(vg) = ViewportGeometry::new(cw, ch, total_cols, scroll_col, zoom, freq_crop_lo, freq_crop_hi)
    else {
        return preview.is_some();
    };

    // Draw a flow tile to the canvas.
    let blit_flow_tile = |tile: &tile_cache::Tile, tile_lod: u8, tile_idx: usize,
                          clip_start: f64, clip_end: f64| {
        let Some(coords) = compute_tile_blit_coords(
            &vg, tile.rendered.width as f64, tile.rendered.height as f64,
            tile_lod, tile_idx, clip_start, clip_end,
        ) else { return };

        // Composite dB+shift to RGBA at render time
        let rgba = if !tile.rendered.db_data.is_empty() {
            db_flow_tile_to_rgba(
                &tile.rendered.db_data, &tile.rendered.flow_shifts,
                tile.rendered.width,
                display_settings, intensity_gate, flow_gate, opacity,
                shift_gain, color_gamma, algo, scheme,
                freq_adjustments,
            )
        } else {
            tile.rendered.pixels.clone()
        };

        let clamped = Clamped(&rgba[..]);
        let Ok(img) = ImageData::new_with_u8_clamped_array_and_sh(
            clamped, tile.rendered.width, tile.rendered.height,
        ) else { return };

        let Some((tmp, tmp_ctx)) = get_tmp_canvas(tile.rendered.width, tile.rendered.height) else { return };
        let _ = tmp_ctx.put_image_data(&img, 0.0, 0.0);

        ctx.set_image_smoothing_enabled(tile_lod != vg.ideal_lod);
        let _ = ctx.draw_image_with_html_canvas_element_and_sw_and_sh_and_dx_and_dy_and_dw_and_dh(
            &tmp,
            coords.src_x, coords.src_y, coords.src_w, coords.src_h,
            coords.dst_x, coords.dst_y, coords.dst_w, coords.dst_h,
        );
    };

    let any_drawn = for_each_visible_tile(
        &vg,
        |tile_idx, clip_start, clip_end| {
            tile_cache::borrow_flow_tile(file_idx, vg.ideal_lod, tile_idx, |tile| {
                blit_flow_tile(tile, vg.ideal_lod, tile_idx, clip_start, clip_end);
            }).is_some()
        },
        |fb_tile, fb_lod, clip_start, clip_end| {
            tile_cache::borrow_flow_tile(file_idx, fb_lod, fb_tile, |tile| {
                blit_flow_tile(tile, fb_lod, fb_tile, clip_start, clip_end);
            }).is_some()
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
    chroma_gain: f32,
    chroma_gamma: f32,
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
    let src_start = scroll_col.max(0.0).min((total_cols as f64 - 1.0).max(0.0));
    let src_end = (src_start + visible_cols).min(total_cols as f64);

    let first_tile = (src_start / TILE_COLS as f64).floor() as usize;
    let last_tile = ((src_end - 1.0).max(0.0) / TILE_COLS as f64).floor() as usize;
    let n_tiles = (total_cols + TILE_COLS - 1) / TILE_COLS;

    let mut any_drawn = false;

    for tile_idx in first_tile..=last_tile.min(n_tiles.saturating_sub(1)) {
        let tile_col_start = tile_idx * TILE_COLS;

        let drawn = tile_cache::borrow_chroma_tile(file_idx, tile_idx, |tile| {
            let tw = tile.rendered.width as f64;
            let th = tile.rendered.height as f64;
            if tw == 0.0 || th == 0.0 { return; }

            // Apply gain/gamma then 2D chromagram colormap: R=class intensity, G=note intensity, B=flow
            let apply_gain_gamma = chroma_gain != 1.0 || chroma_gamma != 1.0;
            #[inline]
            fn adjust_byte(val: u8, gain: f32, gamma: f32) -> u8 {
                let norm = (val as f32 / 255.0) * gain;
                let clamped = norm.clamp(0.0, 1.0);
                if gamma == 1.0 { (clamped * 255.0) as u8 }
                else { (clamped.powf(gamma) * 255.0) as u8 }
            }
            let mut pixels = tile.rendered.pixels.clone();
            for i in (0..pixels.len()).step_by(4) {
                let class_byte = if apply_gain_gamma { adjust_byte(pixels[i], chroma_gain, chroma_gamma) } else { pixels[i] };
                let note_byte = if apply_gain_gamma { adjust_byte(pixels[i + 1], chroma_gain, chroma_gamma) } else { pixels[i + 1] };
                let flow_byte = pixels[i + 2];
                let pixel_idx = i / 4;
                let tile_w = tile.rendered.width as usize;
                let row = pixel_idx / tile_w;

                let scale = crate::dsp::chromagram::CHROMA_RENDER_SCALE;
                let logical_row = row / scale;
                let [r, g, b] = match &mode {
                    ChromaMode::Single(cm) => cm.apply(class_byte, note_byte),
                    ChromaMode::PerPitchClass(cms) => {
                        let pc = 11usize.saturating_sub(logical_row / 10).min(11);
                        cms[pc].apply(class_byte, note_byte)
                    }
                    ChromaMode::PerOctave(cms) => {
                        let oct = 9usize.saturating_sub(logical_row % 10).min(9);
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
            let dst_x_raw = ((tile_col_start as f64 + tile_src_x) - src_start) * zoom;
            let dst_x_end_raw = ((tile_col_start as f64 + tile_src_x + tile_src_w) - src_start) * zoom;
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
