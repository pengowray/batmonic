//! Live waterfall spectrogram for recording/listening mode.
//!
//! Stores magnitude data in a circular buffer and renders directly to the
//! spectrogram canvas, bypassing the tile cache entirely. This gives immediate
//! one-column-at-a-time display during live audio capture.

use crate::canvas::colors::{db_to_greyscale, magnitude_to_db};
use crate::canvas::spectrogram_renderer::{ColormapMode, SpectDisplaySettings};
use crate::types::SpectrogramColumn;
use std::cell::RefCell;
use web_sys::CanvasRenderingContext2d;

/// Maximum columns to keep in the circular buffer.
/// 30k columns ≈ 160s at 48kHz/hop256, or ≈ 20s at 384kHz/hop256.
/// Memory: 30000 × 513 × 4 bytes ≈ 62 MB (with FFT=1024).
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
    /// Reusable RGBA pixel buffer for `render_viewport` (avoids a ~1 MB alloc +
    /// full alpha-fill every frame — WASM has no cheap GC for this churn).
    static RENDER_PIXELS: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    /// Cached row→bin map: (map, key) where key = (img_h, total_bins, crop_lo,
    /// crop_hi). Recomputed only when the viewport height / crop changes.
    static RENDER_BIN_MAP: RefCell<(Vec<usize>, (u32, usize, u64, u64))> =
        const { RefCell::new((Vec::new(), (0, 0, 0, 0))) };
    /// Magnitude→grey LUT, indexed by the top 16 bits of the magnitude's f32 bit
    /// pattern (monotonic in value for mag ≥ 0). Replaces a per-pixel software
    /// `log10` + normalize + `powf` with one array lookup. Rebuilt only when the
    /// display settings (floor/range/gamma/gain) change.
    static GREY_LUT: RefCell<(Vec<u8>, u64)> = const { RefCell::new((Vec::new(), u64::MAX)) };
    /// Render-cost accumulator for the benchmark: (calls, total_ms, upload_ms).
    /// `upload_ms` is the `put_image_data` portion; total−upload is the pixel
    /// compute. Read+reset via `take_render_timing`.
    static RENDER_TIMING: RefCell<(u32, f64, f64)> = const { RefCell::new((0, 0.0, 0.0)) };
}

fn perf_now() -> f64 {
    web_sys::window()
        .and_then(|w| w.performance())
        .map(|p| p.now())
        .unwrap_or(0.0)
}

/// Take and reset the accumulated render timing: (calls, total_ms, upload_ms).
pub fn take_render_timing() -> (u32, f64, f64) {
    RENDER_TIMING.with(|c| {
        let v = *c.borrow();
        *c.borrow_mut() = (0, 0.0, 0.0);
        v
    })
}

/// Combine the four display-intensity settings into one cache key.
fn grey_lut_key(s: &SpectDisplaySettings) -> u64 {
    let a = s.floor_db.to_bits() as u64;
    let b = s.range_db.to_bits() as u64;
    let c = s.gamma.to_bits() as u64;
    let d = s.gain_db.to_bits() as u64;
    // Mix so distinct tuples don't collide (xor-shift fold).
    (a.wrapping_mul(0x9E3779B1) ^ b.rotate_left(16)).wrapping_mul(0x85EBCA77)
        ^ c.rotate_left(32)
        ^ d.rotate_left(48)
}

/// Build the 65536-entry magnitude→grey LUT for `settings`. Index = top 16 bits
/// of `mag.to_bits()`; the representative magnitude uses the mid-cell mantissa so
/// the result matches `db_to_greyscale(magnitude_to_db(mag), …)` to within a
/// grey level across the cell (cell ≈ 0.07 dB ≪ one of 256 output levels).
fn build_grey_lut(settings: &SpectDisplaySettings, out: &mut Vec<u8>) {
    out.clear();
    out.reserve(1 << 16);
    for i in 0..(1u32 << 16) {
        let bits = (i << 16) | 0x8000; // mid-cell representative
        let mag = f32::from_bits(bits);
        let db = magnitude_to_db(mag);
        out.push(db_to_greyscale(
            db,
            settings.floor_db,
            settings.range_db,
            settings.gamma,
            settings.gain_db,
        ));
    }
}

/// Map a (non-negative) magnitude to grey via the prebuilt LUT.
#[inline]
fn grey_from_lut(lut: &[u8], mag: f32) -> u8 {
    // Negative / NaN magnitudes shouldn't occur (FFT/resonator magnitudes are
    // ≥ 0); guard so a stray value can't index past the table.
    if mag > 0.0 {
        lut[(mag.to_bits() >> 16) as usize]
    } else {
        lut[0]
    }
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
    WATERFALL.with(|w| w.borrow().as_ref().map(|wf| wf.total_written).unwrap_or(0))
}

/// Total elapsed time in seconds (total_columns * time_resolution).
pub fn total_time() -> f64 {
    WATERFALL.with(|w| {
        w.borrow()
            .as_ref()
            .map(|wf| wf.total_written as f64 * wf.hop_size as f64 / wf.sample_rate as f64)
            .unwrap_or(0.0)
    })
}

/// Time of the oldest column still in the circular buffer.
/// Before this time, data has been evicted and rendering will be blank.
/// Returns 0.0 while the buffer hasn't filled yet.
pub fn oldest_time() -> f64 {
    WATERFALL.with(|w| {
        w.borrow()
            .as_ref()
            .map(|wf| {
                let oldest = wf.total_written.saturating_sub(wf.capacity);
                oldest as f64 * wf.hop_size as f64 / wf.sample_rate as f64
            })
            .unwrap_or(0.0)
    })
}

/// Render a downsampled greyscale overview image — matching the file-overview
/// look. `recent_cols` caps the window to the most-recent N columns (so the
/// overview can match the raw-sample ring the waveform shows); `None` renders
/// the full retained window `[oldest .. now]`. Either way it spans only what we
/// actually hold, so the live overview isn't "a 10-minute file with a sliver at
/// the end". Returns `None` until there's data.
pub fn render_overview(
    out_w: u32,
    out_h: u32,
    recent_cols: Option<usize>,
) -> Option<crate::types::PreviewImage> {
    use crate::canvas::colors::magnitude_to_greyscale;
    if out_w == 0 || out_h == 0 {
        return None;
    }
    WATERFALL.with(|w| {
        let wf = w.borrow();
        let wf = wf.as_ref()?;
        if wf.total_written == 0 || wf.max_magnitude <= 0.0 {
            return None;
        }
        let oldest_retained = wf.total_written.saturating_sub(wf.capacity);
        // Start at the more recent of (a) the oldest column still buffered and
        // (b) `now - recent_cols`, so we never claim more history than we hold.
        let oldest = match recent_cols {
            Some(n) => wf.total_written.saturating_sub(n).max(oldest_retained),
            None => oldest_retained,
        };
        let retained = wf.total_written - oldest;
        if retained == 0 {
            return None;
        }
        let freq_bins = wf.freq_bins;
        let ow = out_w as usize;
        let oh = out_h as usize;
        let mut pixels = vec![0u8; ow * oh * 4];
        for x in 0..ow {
            let col = (oldest + (x * retained) / ow).min(wf.total_written - 1);
            let base = (col % wf.capacity) * freq_bins;
            for y in 0..oh {
                // y=0 = top = high freq; y=h-1 = bottom = low freq.
                let bin = freq_bins - 1 - ((y * freq_bins) / oh).min(freq_bins - 1);
                let grey = magnitude_to_greyscale(wf.magnitudes[base + bin], wf.max_magnitude);
                let idx = (y * ow + x) * 4;
                pixels[idx] = grey;
                pixels[idx + 1] = grey;
                pixels[idx + 2] = grey;
                pixels[idx + 3] = 255;
            }
        }
        Some(crate::types::PreviewImage {
            width: out_w,
            height: out_h,
            pixels: std::sync::Arc::new(pixels),
        })
    })
}

/// Maximum time the circular buffer can hold (capacity × time-resolution),
/// i.e. the oldest history the waterfall could ever show. 0 when inactive.
pub fn capacity_time() -> f64 {
    WATERFALL.with(|w| {
        w.borrow()
            .as_ref()
            .map(|wf| wf.capacity as f64 * wf.hop_size as f64 / wf.sample_rate as f64)
            .unwrap_or(0.0)
    })
}

/// Time resolution (seconds per column).
pub fn time_resolution() -> f64 {
    WATERFALL.with(|w| {
        w.borrow()
            .as_ref()
            .map(|wf| wf.hop_size as f64 / wf.sample_rate as f64)
            .unwrap_or(1.0)
    })
}

/// Max frequency (Nyquist).
pub fn max_freq() -> f64 {
    WATERFALL.with(|w| {
        w.borrow()
            .as_ref()
            .map(|wf| wf.sample_rate as f64 / 2.0)
            .unwrap_or(96000.0)
    })
}

/// Capture sample rate (Hz), or 0 when no waterfall is active.
pub fn sample_rate() -> u32 {
    WATERFALL.with(|w| w.borrow().as_ref().map(|wf| wf.sample_rate).unwrap_or(0))
}

/// Get the running max magnitude (for auto-gain / ref_db).
pub fn get_max_magnitude() -> f32 {
    WATERFALL.with(|w| {
        w.borrow()
            .as_ref()
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
        if wf.total_written == 0 {
            return false;
        }

        let img_w = viewport_w as u32;
        let img_h = viewport_h as u32;
        if img_w == 0 || img_h == 0 {
            return false;
        }

        let total_bins = wf.freq_bins;
        let oldest_available = wf.total_written.saturating_sub(wf.capacity);
        let iw = img_w as usize;
        let ih = img_h as usize;
        let pixel_count = iw * ih;

        // Refresh the cached row→bin map only when height / crop changes.
        let bin_key = (
            img_h,
            total_bins,
            freq_crop_lo.to_bits(),
            freq_crop_hi.to_bits(),
        );
        RENDER_BIN_MAP.with(|cell| {
            let mut bm = cell.borrow_mut();
            if bm.1 != bin_key {
                bm.0.clear();
                bm.0.extend((0..ih).map(|py| {
                    let frac = py as f64 / viewport_h; // 0 at top, 1 at bottom
                                                       // freq_crop_hi = top, freq_crop_lo = bottom
                    let freq_frac = freq_crop_hi - frac * (freq_crop_hi - freq_crop_lo);
                    (freq_frac * total_bins as f64)
                        .floor()
                        .clamp(0.0, (total_bins - 1) as f64) as usize
                }));
                bm.1 = bin_key;
            }
        });

        // Refresh the magnitude→grey LUT only when the display settings change.
        let lut_key = grey_lut_key(settings);
        GREY_LUT.with(|cell| {
            let mut lut = cell.borrow_mut();
            if lut.1 != lut_key {
                build_grey_lut(settings, &mut lut.0);
                lut.1 = lut_key;
            }
        });

        // Clamp rendering to live_data_cols so we don't draw past actual data.
        let data_end = live_data_cols.min(wf.total_written);
        // Columns per pixel — when zoomed out (cols_per_px > 1) we average
        // multiple waterfall columns into each canvas pixel to avoid aliasing
        // shimmer caused by point-sampling with fractional scroll offsets.
        let cols_per_px = 1.0 / zoom;

        let t_start = perf_now();
        RENDER_PIXELS.with(|px_cell| {
            let mut pixels = px_cell.borrow_mut();
            let need = pixel_count * 4;
            // Resize only on change; set alpha to 255 once (the loop only writes
            // RGB, so reuse keeps alpha intact across frames).
            if pixels.len() != need {
                pixels.resize(need, 0);
                for i in (3..need).step_by(4) {
                    pixels[i] = 255;
                }
            }

            RENDER_BIN_MAP.with(|bm_cell| {
                let bm = bm_cell.borrow();
                let bin_map = &bm.0;
                GREY_LUT.with(|lut_cell| {
                    let lc = lut_cell.borrow();
                    let lut = &lc.0;

                    // For each canvas column, find the corresponding waterfall column(s).
                    for px in 0..iw {
                        let col_start_f = scroll_col + px as f64 * cols_per_px;
                        let col_end_f = col_start_f + cols_per_px;
                        let col_lo = col_start_f.floor().max(oldest_available as f64) as usize;
                        let col_hi = col_end_f.ceil().min(data_end as f64) as usize;
                        if col_lo >= col_hi {
                            // No data for this column — paint it black (the reused
                            // buffer would otherwise keep the previous frame here).
                            for py in 0..ih {
                                let idx = (py * iw + px) * 4;
                                pixels[idx] = 0;
                                pixels[idx + 1] = 0;
                                pixels[idx + 2] = 0;
                            }
                            continue;
                        }

                        let n_cols = col_hi - col_lo;
                        for (py, &bin) in bin_map.iter().enumerate() {
                            let mag = if n_cols == 1 {
                                let buf_idx = col_lo % wf.capacity;
                                wf.magnitudes[buf_idx * wf.freq_bins + bin]
                            } else {
                                let mut sum = 0.0f32;
                                for c in col_lo..col_hi {
                                    let buf_idx = c % wf.capacity;
                                    sum += wf.magnitudes[buf_idx * wf.freq_bins + bin];
                                }
                                sum / n_cols as f32
                            };
                            // LUT replaces a per-pixel software log10 + powf.
                            let grey = grey_from_lut(lut, mag);
                            let [r, g, b] = apply_colormap_mode(colormap, grey, py, ih, total_bins);
                            let idx = (py * iw + px) * 4;
                            pixels[idx] = r;
                            pixels[idx + 1] = g;
                            pixels[idx + 2] = b;
                        }
                    }

                    // Put pixels on canvas.
                    let t_upload = perf_now();
                    let clamped = wasm_bindgen::Clamped(&pixels[..need]);
                    if let Ok(img_data) =
                        web_sys::ImageData::new_with_u8_clamped_array_and_sh(clamped, img_w, img_h)
                    {
                        let _ = ctx.put_image_data(&img_data, 0.0, 0.0);
                    }
                    let t_end = perf_now();
                    RENDER_TIMING.with(|c| {
                        let mut t = c.borrow_mut();
                        t.0 += 1;
                        t.1 += t_end - t_start; // total render
                        t.2 += t_end - t_upload; // put_image_data only
                    });
                });
            });
        });

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
        ColormapMode::HfrFocus {
            colormap: cm,
            band_ff_lo_frac,
            band_ff_hi_frac,
        } => {
            // Convert canvas row to frequency fraction.
            let h = canvas_height as f64;
            let focus_top = (h * (1.0 - band_ff_hi_frac)).round() as usize;
            let focus_bot = (h * (1.0 - band_ff_lo_frac)).round() as usize;
            if canvas_row >= focus_top && canvas_row < focus_bot {
                cm.apply(grey)
            } else {
                [grey, grey, grey]
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The mag→grey LUT must match the direct log+map path to within a single
    /// grey level across many orders of magnitude and several settings.
    fn check_lut(settings: &SpectDisplaySettings) {
        let mut lut = Vec::new();
        build_grey_lut(settings, &mut lut);
        assert_eq!(lut.len(), 1 << 16);
        let mut max_diff = 0i32;
        for e in -9..2 {
            for m in 1..10 {
                let mag = (m as f32) * 10f32.powi(e);
                let direct = db_to_greyscale(
                    magnitude_to_db(mag),
                    settings.floor_db,
                    settings.range_db,
                    settings.gamma,
                    settings.gain_db,
                );
                let via = grey_from_lut(&lut, mag);
                max_diff = max_diff.max((direct as i32 - via as i32).abs());
            }
        }
        assert!(
            max_diff <= 1,
            "LUT off by {max_diff} grey levels (floor {}, range {}, gamma {}, gain {})",
            settings.floor_db,
            settings.range_db,
            settings.gamma,
            settings.gain_db
        );
        // Zero / negative magnitudes clamp to the darkest level.
        assert_eq!(grey_from_lut(&lut, 0.0), 0);
        assert_eq!(grey_from_lut(&lut, -1.0), 0);
    }

    #[test]
    fn grey_lut_matches_direct_linear() {
        check_lut(&SpectDisplaySettings {
            floor_db: -90.0,
            range_db: 90.0,
            gamma: 1.0,
            gain_db: 6.0,
        });
    }

    #[test]
    fn grey_lut_matches_direct_gamma() {
        check_lut(&SpectDisplaySettings {
            floor_db: -120.0,
            range_db: 120.0,
            gamma: 2.2,
            gain_db: 0.0,
        });
        check_lut(&SpectDisplaySettings {
            floor_db: -60.0,
            range_db: 60.0,
            gamma: 0.5,
            gain_db: -10.0,
        });
    }
}
