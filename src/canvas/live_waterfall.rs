//! Live waterfall spectrogram for recording/listening mode.
//!
//! Stores magnitude data in a circular buffer and renders directly to the
//! spectrogram canvas, bypassing the tile cache entirely. This gives immediate
//! one-column-at-a-time display during live audio capture.

use std::cell::RefCell;
use web_sys::CanvasRenderingContext2d;
use crate::canvas::colors::{magnitude_to_db, db_to_greyscale};
use crate::canvas::spectrogram_renderer::{ColormapMode, SpectDisplaySettings};
use crate::types::SpectrogramColumn;

/// Maximum columns to keep in the circular buffer.
/// 30k columns ≈ 120s at 48kHz/hop256, or ≈ 20s at 384kHz/hop256.
/// Memory: 30000 × 129 × 4 bytes ≈ 15 MB.
const DEFAULT_CAPACITY: usize = 30_000;

pub struct LiveWaterfall {
    /// Flat magnitude buffer: magnitudes[col * freq_bins .. (col+1) * freq_bins]
    /// Circular: write_pos wraps around.
    magnitudes: Vec<f32>,
    /// Number of frequency bins per column (fft_size / 2 + 1).
    freq_bins: usize,
    /// Circular buffer capacity in columns.
    capacity: usize,
    /// Next write position (0..capacity).
    write_pos: usize,
    /// Total columns written (monotonically increasing, used for scroll).
    total_written: usize,
    pub fft_size: usize,
    pub hop_size: usize,
    pub sample_rate: u32,
    /// Running max magnitude for auto-gain.
    pub max_magnitude: f32,
}

thread_local! {
    static WATERFALL: RefCell<Option<LiveWaterfall>> = const { RefCell::new(None) };
}

/// Create a new waterfall for live display.
pub fn create(fft_size: usize, hop_size: usize, sample_rate: u32) {
    let freq_bins = fft_size / 2 + 1;
    WATERFALL.with(|w| {
        *w.borrow_mut() = Some(LiveWaterfall {
            magnitudes: vec![0.0; freq_bins * DEFAULT_CAPACITY],
            freq_bins,
            capacity: DEFAULT_CAPACITY,
            write_pos: 0,
            total_written: 0,
            fft_size,
            hop_size,
            sample_rate,
            max_magnitude: 0.0,
        });
    });
}

/// Push new FFT columns into the waterfall.
pub fn push_columns(columns: &[SpectrogramColumn]) {
    WATERFALL.with(|w| {
        let mut wf = w.borrow_mut();
        let Some(wf) = wf.as_mut() else { return };
        for col in columns {
            let offset = wf.write_pos * wf.freq_bins;
            for (i, &mag) in col.magnitudes.iter().take(wf.freq_bins).enumerate() {
                wf.magnitudes[offset + i] = mag;
                if mag > wf.max_magnitude {
                    wf.max_magnitude = mag;
                }
            }
            wf.write_pos = (wf.write_pos + 1) % wf.capacity;
            wf.total_written += 1;
        }
    });
}

/// Clear / destroy the waterfall.
pub fn clear() {
    WATERFALL.with(|w| {
        *w.borrow_mut() = None;
    });
}

/// Whether a waterfall is currently active.
pub fn is_active() -> bool {
    WATERFALL.with(|w| w.borrow().is_some())
}

/// Total columns written so far (for scroll position calculations).
pub fn total_columns() -> usize {
    WATERFALL.with(|w| {
        w.borrow().as_ref().map(|wf| wf.total_written).unwrap_or(0)
    })
}

/// Time resolution (seconds per column).
pub fn time_resolution() -> f64 {
    WATERFALL.with(|w| {
        w.borrow().as_ref()
            .map(|wf| wf.hop_size as f64 / wf.sample_rate as f64)
            .unwrap_or(1.0)
    })
}

/// Max frequency (Nyquist).
pub fn max_freq() -> f64 {
    WATERFALL.with(|w| {
        w.borrow().as_ref()
            .map(|wf| wf.sample_rate as f64 / 2.0)
            .unwrap_or(96000.0)
    })
}

/// Get the running max magnitude (for auto-gain / ref_db).
pub fn get_max_magnitude() -> f32 {
    WATERFALL.with(|w| {
        w.borrow().as_ref()
            .map(|wf| wf.max_magnitude)
            .unwrap_or(0.0)
    })
}

/// Render the waterfall directly to the canvas.
/// Returns true if anything was drawn.
pub fn render_viewport(
    ctx: &CanvasRenderingContext2d,
    viewport_w: f64,
    viewport_h: f64,
    scroll_col: f64,
    zoom: f64,
    freq_crop_lo: f64,
    freq_crop_hi: f64,
    settings: &SpectDisplaySettings,
    colormap: ColormapMode,
    live_data_cols: usize,
) -> bool {
    WATERFALL.with(|w| {
        let wf = w.borrow();
        let Some(wf) = wf.as_ref() else { return false };
        if wf.total_written == 0 { return false; }

        let img_w = viewport_w as u32;
        let img_h = viewport_h as u32;
        if img_w == 0 || img_h == 0 { return false; }

        let total_bins = wf.freq_bins;
        let oldest_available = wf.total_written.saturating_sub(wf.capacity);

        // Precompute bin mapping for each canvas row.
        // Row 0 = top = high freq, row (h-1) = bottom = low freq.
        let bin_map: Vec<usize> = (0..img_h as usize).map(|py| {
            let frac = py as f64 / viewport_h; // 0 at top, 1 at bottom
            // freq_crop_hi = top, freq_crop_lo = bottom
            let freq_frac = freq_crop_hi - frac * (freq_crop_hi - freq_crop_lo);
            (freq_frac * total_bins as f64).floor().clamp(0.0, (total_bins - 1) as f64) as usize
        }).collect();

        // Allocate RGBA pixel buffer (opaque black default).
        let pixel_count = (img_w * img_h) as usize;
        let mut pixels = vec![0u8; pixel_count * 4];
        // Set alpha to 255 for all pixels.
        for chunk in pixels.chunks_exact_mut(4) {
            chunk[3] = 255;
        }

        // Clamp rendering to live_data_cols so we don't draw past actual data.
        let data_end = live_data_cols.min(wf.total_written);

        // For each canvas column, find the corresponding waterfall column.
        for px in 0..img_w {
            let col_f = scroll_col + px as f64 / zoom;
            let col_idx = col_f.floor() as usize;
            if col_idx >= data_end || col_idx < oldest_available {
                continue; // black (already initialized)
            }

            let buf_idx = col_idx % wf.capacity;
            let col_offset = buf_idx * wf.freq_bins;

            // For each canvas row, look up the magnitude and convert to pixel.
            for (py, &bin) in bin_map.iter().enumerate() {
                let mag = wf.magnitudes[col_offset + bin];
                let db = magnitude_to_db(mag);
                let grey = db_to_greyscale(
                    db,
                    settings.floor_db,
                    settings.range_db,
                    settings.gamma,
                    settings.gain_db,
                );
                let [r, g, b] = apply_colormap_mode(colormap, grey, py, img_h as usize, total_bins);
                let idx = (py as u32 * img_w + px) as usize * 4;
                pixels[idx] = r;
                pixels[idx + 1] = g;
                pixels[idx + 2] = b;
            }
        }

        // Put pixels on canvas.
        let clamped = wasm_bindgen::Clamped(&pixels[..]);
        if let Ok(img_data) = web_sys::ImageData::new_with_u8_clamped_array_and_sh(
            clamped, img_w, img_h,
        ) {
            let _ = ctx.put_image_data(&img_data, 0.0, 0.0);
        }

        true
    })
}

/// Apply colormap mode, handling uniform and HFR focus.
#[inline]
fn apply_colormap_mode(
    mode: ColormapMode,
    grey: u8,
    canvas_row: usize,
    canvas_height: usize,
    _total_bins: usize,
) -> [u8; 3] {
    match mode {
        ColormapMode::Uniform(cm) => cm.apply(grey),
        ColormapMode::HfrFocus { colormap: cm, ff_lo_frac, ff_hi_frac } => {
            // Convert canvas row to frequency fraction.
            let h = canvas_height as f64;
            let focus_top = (h * (1.0 - ff_hi_frac)).round() as usize;
            let focus_bot = (h * (1.0 - ff_lo_frac)).round() as usize;
            if canvas_row >= focus_top && canvas_row < focus_bot {
                cm.apply(grey)
            } else {
                [grey, grey, grey]
            }
        }
    }
}
