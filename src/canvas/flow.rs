use crate::canvas::colors::{
    magnitude_to_greyscale, magnitude_to_db, flow_rgb,
};
use crate::canvas::spectrogram_renderer::PreRendered;
use crate::types::SpectrogramData;

/// Algorithm selector for flow detection.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FlowAlgo {
    Optical,
    PhaseCoherence,
    Centroid,
    Gradient,
    Phase,
}

/// Cached intermediate data: greyscale intensities + shift values per pixel.
/// The expensive shift computation only needs to run when file or algorithm changes.
/// Color mapping (gates, opacity) can then be applied cheaply via `composite_flow`.
pub struct FlowData {
    pub width: u32,
    pub height: u32,
    /// Greyscale intensity per pixel (row-major, flipped: row 0 = highest freq).
    pub greys: Vec<u8>,
    /// Frequency shift value per pixel (same layout as greys).
    pub shifts: Vec<f32>,
}

/// Compute flow data (expensive): greyscale + shift values for every pixel.
/// Only needs to re-run when the file or algorithm changes.
pub fn compute_flow_data(data: &SpectrogramData, algo: FlowAlgo) -> FlowData {
    if data.columns.is_empty() {
        return FlowData {
            width: 0,
            height: 0,
            greys: Vec::new(),
            shifts: Vec::new(),
        };
    }

    let width = data.columns.len() as u32;
    let height = data.columns[0].magnitudes.len() as u32;
    let h = height as usize;
    let total = (width as usize) * (height as usize);

    let max_mag = data
        .columns
        .iter()
        .flat_map(|c| c.magnitudes.iter())
        .copied()
        .fold(0.0f32, f32::max);

    let mut greys = vec![0u8; total];
    let mut shifts = vec![0.0f32; total];

    for (col_idx, col) in data.columns.iter().enumerate() {
        let prev = if col_idx > 0 {
            Some(&data.columns[col_idx - 1].magnitudes)
        } else {
            None
        };

        for (bin_idx, &mag) in col.magnitudes.iter().enumerate() {
            let grey = magnitude_to_greyscale(mag, max_mag);
            let shift = match prev {
                None => 0.0,
                Some(prev_mags) => match algo {
                    FlowAlgo::Centroid => compute_centroid_shift(prev_mags, &col.magnitudes, bin_idx, h),
                    FlowAlgo::Gradient => compute_gradient_shift(prev_mags, &col.magnitudes, bin_idx, h),
                    FlowAlgo::Optical => compute_flow_shift(prev_mags, &col.magnitudes, bin_idx, h),
                    FlowAlgo::PhaseCoherence | FlowAlgo::Phase => 0.0, // these use their own compute paths
                },
            };

            let y = height as usize - 1 - bin_idx;
            let idx = y * width as usize + col_idx;
            greys[idx] = grey;
            shifts[idx] = shift;
        }
    }

    FlowData { width, height, greys, shifts }
}

/// Composite flow data into RGBA pixels (cheap).
/// Re-runs when intensity_gate, flow_gate, or opacity changes.
pub fn composite_flow(
    md: &FlowData,
    intensity_gate: f32,
    flow_gate: f32,
    opacity: f32,
) -> PreRendered {
    let total = (md.width as usize) * (md.height as usize);
    let mut pixels = vec![0u8; total * 4];

    for i in 0..total {
        let [r, g, b] = flow_rgb(md.greys[i], md.shifts[i], intensity_gate, flow_gate, opacity, 3.0, 1.0);
        let pi = i * 4;
        pixels[pi] = r;
        pixels[pi + 1] = g;
        pixels[pi + 2] = b;
        pixels[pi + 3] = 255;
    }

    PreRendered {
        width: md.width,
        height: md.height,
        pixels,
        db_data: Vec::new(),
        flow_shifts: Vec::new(),
    }
}

/// Spectral centroid shift: compute local weighted centroid in a ±radius window
/// around `bin` for both prev and current column, return the difference.
fn compute_centroid_shift(prev: &[f32], curr: &[f32], bin: usize, h: usize) -> f32 {
    let radius: usize = 3;
    let lo = bin.saturating_sub(radius);
    let hi = (bin + radius + 1).min(h);

    let centroid = |mags: &[f32]| -> f32 {
        let mut sum_w = 0.0f32;
        let mut sum_wf = 0.0f32;
        for i in lo..hi {
            let w = mags[i] * mags[i]; // weight by energy
            sum_w += w;
            sum_wf += w * i as f32;
        }
        if sum_w > 0.0 {
            sum_wf / sum_w
        } else {
            bin as f32
        }
    };

    let c_prev = centroid(prev);
    let c_curr = centroid(curr);
    // Normalize by radius so result is roughly in [-1, 1]
    (c_curr - c_prev) / radius as f32
}

/// Vertical gradient of temporal difference.
fn compute_gradient_shift(prev: &[f32], curr: &[f32], bin: usize, h: usize) -> f32 {
    // diff at neighboring bins
    let diff_above = if bin + 1 < h {
        curr[bin + 1] - prev[bin + 1]
    } else {
        0.0
    };
    let diff_below = if bin > 0 {
        curr[bin - 1] - prev[bin - 1]
    } else {
        0.0
    };
    let max_energy = curr[bin].max(prev[bin]).max(1e-10);
    // Positive gradient means energy is appearing above & disappearing below → upward shift
    (diff_above - diff_below) / (2.0 * max_energy)
}

/// 1D vertical optical flow via cross-correlation in a small window.
/// Returns fractional bin displacement (positive = upward shift).
fn compute_flow_shift(prev: &[f32], curr: &[f32], bin: usize, h: usize) -> f32 {
    let radius: usize = 3;
    let max_disp: isize = 2;

    let lo = bin.saturating_sub(radius);
    let hi = (bin + radius + 1).min(h);

    // Check there's enough energy to bother
    let energy: f32 = (lo..hi).map(|i| curr[i]).sum();
    if energy < 1e-8 {
        return 0.0;
    }

    let mut best_corr = f32::NEG_INFINITY;
    let mut best_d: isize = 0;

    for d in -max_disp..=max_disp {
        let mut corr = 0.0f32;
        for i in lo..hi {
            let j = (i as isize + d) as usize;
            if j < h {
                corr += curr[i] * prev[j];
            }
        }
        if corr > best_corr {
            best_corr = corr;
            best_d = d;
        }
    }

    // Sub-pixel refinement using parabolic interpolation
    if best_d.abs() < max_disp {
        let c0 = {
            let d = best_d - 1;
            (lo..hi)
                .map(|i| {
                    let j = (i as isize + d) as usize;
                    if j < h { curr[i] * prev[j] } else { 0.0 }
                })
                .sum::<f32>()
        };
        let c2 = {
            let d = best_d + 1;
            (lo..hi)
                .map(|i| {
                    let j = (i as isize + d) as usize;
                    if j < h { curr[i] * prev[j] } else { 0.0 }
                })
                .sum::<f32>()
        };
        let denom = 2.0 * (2.0 * best_corr - c0 - c2);
        if denom.abs() > 1e-10 {
            let sub = (c0 - c2) / denom;
            return (best_d as f32 + sub) / max_disp as f32;
        }
    }

    best_d as f32 / max_disp as f32
}

/// Pre-render a tile of flow columns: stores dB values + shift values for deferred compositing.
///
/// `prev_column_mags`: magnitudes of the last column from the previous tile,
/// needed for shift computation at the boundary. `None` for the first tile.
///
/// Returns a `PreRendered` with `db_data` + `flow_shifts` populated (no RGBA pixels).
/// Color compositing is deferred to blit time so gate/opacity changes are instant.
pub fn pre_render_flow_columns(
    columns: &[crate::types::SpectrogramColumn],
    prev_column_mags: Option<&[f32]>,
    algo: FlowAlgo,
) -> PreRendered {
    if columns.is_empty() {
        return PreRendered { width: 0, height: 0, pixels: Vec::new(), db_data: Vec::new(), flow_shifts: Vec::new() };
    }

    let width = columns.len() as u32;
    let height = columns[0].magnitudes.len() as u32;
    let h = height as usize;
    let total = (width * height) as usize;
    let mut db_data = vec![f32::NEG_INFINITY; total];
    let mut shifts = vec![0.0f32; total];

    for (col_idx, col) in columns.iter().enumerate() {
        let prev_mags = if col_idx > 0 {
            Some(columns[col_idx - 1].magnitudes.as_slice())
        } else {
            prev_column_mags
        };

        for (bin_idx, &mag) in col.magnitudes.iter().enumerate() {
            let db = magnitude_to_db(mag);

            let shift = match prev_mags {
                None => 0.0,
                Some(prev) => match algo {
                    FlowAlgo::Centroid => compute_centroid_shift(prev, &col.magnitudes, bin_idx, h),
                    FlowAlgo::Gradient => compute_gradient_shift(prev, &col.magnitudes, bin_idx, h),
                    FlowAlgo::Optical => compute_flow_shift(prev, &col.magnitudes, bin_idx, h),
                    FlowAlgo::PhaseCoherence | FlowAlgo::Phase => 0.0, // these use their own compute paths
                },
            };

            let y = height as usize - 1 - bin_idx;
            let idx = y * width as usize + col_idx;
            db_data[idx] = db;
            shifts[idx] = shift;
        }
    }

    PreRendered { width, height, pixels: Vec::new(), db_data, flow_shifts: shifts }
}
