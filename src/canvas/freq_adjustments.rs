use std::cell::RefCell;
use leptos::prelude::*;
use crate::dsp::filters::harmonics_band_bounds;
use crate::state::{AppState, DisplayFilterMode};

thread_local! {
    /// Cache for freq_adjustments: (fingerprint, result).
    /// Avoids recomputing on every scroll when only the viewport changed.
    static FREQ_ADJ_CACHE: RefCell<Option<(u64, Option<Vec<f32>>)>> = const { RefCell::new(None) };
}

/// Build a fingerprint of all inputs that affect freq_adjustments.
fn freq_adj_fingerprint(state: &AppState, file_max_freq: f64, tile_height: usize) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    file_max_freq.to_bits().hash(&mut h);
    tile_height.hash(&mut h);
    state.display_eq.get_untracked().hash(&mut h);
    state.display_noise_filter.get_untracked().hash(&mut h);
    state.filter_enabled.get_untracked().hash(&mut h);
    state.filter_freq_low.get_untracked().to_bits().hash(&mut h);
    state.filter_freq_high.get_untracked().to_bits().hash(&mut h);
    (state.filter_db_below.get_untracked() as i32).hash(&mut h);
    (state.filter_db_selected.get_untracked() as i32).hash(&mut h);
    (state.filter_db_harmonics.get_untracked() as i32).hash(&mut h);
    (state.filter_db_above.get_untracked() as i32).hash(&mut h);
    state.filter_band_mode.get_untracked().hash(&mut h);
    state.notch_enabled.get_untracked().hash(&mut h);
    let bands = state.notch_bands.get_untracked();
    bands.len().hash(&mut h);
    for b in &bands {
        b.center_hz.to_bits().hash(&mut h);
        b.bandwidth_hz.to_bits().hash(&mut h);
        b.enabled.hash(&mut h);
        (b.strength_db as i32).hash(&mut h);
    }
    state.notch_harmonic_suppression.get_untracked().to_bits().hash(&mut h);
    state.display_filter_enabled.get_untracked().hash(&mut h);
    (state.display_filter_nr.get_untracked() as u8).hash(&mut h);
    (state.display_filter_notch.get_untracked() as u8).hash(&mut h);
    state.noise_reduce_enabled.get_untracked().hash(&mut h);
    state.noise_reduce_strength.get_untracked().to_bits().hash(&mut h);
    // Include noise floor identity (use ptr + len as proxy for content)
    let nf = state.noise_reduce_floor.get_untracked();
    nf.as_ref().map(|f| f.bin_magnitudes.len()).unwrap_or(0).hash(&mut h);
    let dnf = state.display_auto_noise_floor.get_untracked();
    dnf.as_ref().map(|f| f.bin_magnitudes.len()).unwrap_or(0).hash(&mut h);
    state.display_nr_strength.get_untracked().to_bits().hash(&mut h);
    h.finish()
}

/// Compute per-row dB adjustments for display EQ and noise filtering.
/// Returns None if no adjustments are needed (both checkboxes off).
/// Row 0 = highest frequency, row (tile_height-1) = 0 Hz.
/// Results are cached and only recomputed when filter settings change.
pub fn compute_freq_adjustments(state: &AppState, file_max_freq: f64, tile_height: usize) -> Option<Vec<f32>> {
    let fp = freq_adj_fingerprint(state, file_max_freq, tile_height);
    let cached = FREQ_ADJ_CACHE.with(|c| {
        let cache = c.borrow();
        if let Some((cached_fp, ref result)) = *cache {
            if cached_fp == fp {
                return Some(result.clone());
            }
        }
        None
    });
    if let Some(result) = cached {
        return result;
    }
    let result = compute_freq_adjustments_inner(state, file_max_freq, tile_height);
    FREQ_ADJ_CACHE.with(|c| {
        *c.borrow_mut() = Some((fp, result.clone()));
    });
    result
}

fn compute_freq_adjustments_inner(state: &AppState, file_max_freq: f64, tile_height: usize) -> Option<Vec<f32>> {
    let show_eq = state.display_eq.get_untracked();
    let show_noise = state.display_noise_filter.get_untracked();
    if !show_eq && !show_noise {
        return None;
    }
    if tile_height == 0 { return None; }

    let mut adj = vec![0.0f32; tile_height];

    // EQ: apply per-band dB offsets
    if show_eq && state.filter_enabled.get_untracked() {
        let freq_low = state.filter_freq_low.get_untracked();
        let freq_high = state.filter_freq_high.get_untracked();
        let db_below = state.filter_db_below.get_untracked() as f32;
        let db_selected = state.filter_db_selected.get_untracked() as f32;
        let db_harmonics = state.filter_db_harmonics.get_untracked() as f32;
        let db_above = state.filter_db_above.get_untracked() as f32;
        let band_mode = state.filter_band_mode.get_untracked();
        let harmonics_bounds = harmonics_band_bounds(freq_low, freq_high, band_mode);

        for (row, adj_val) in adj.iter_mut().enumerate().take(tile_height) {
            let bin = tile_height - 1 - row; // bin 0 = DC
            let freq = file_max_freq * bin as f64 / (tile_height - 1).max(1) as f64;
            let eq_db = if freq < freq_low {
                db_below
            } else if freq <= freq_high || band_mode <= 2 {
                db_selected
            } else if let Some((harmonics_lower, harmonics_upper)) = harmonics_bounds {
                if freq >= harmonics_lower && freq <= harmonics_upper {
                    db_harmonics
                } else {
                    db_above
                }
            } else {
                db_above
            };
            *adj_val += eq_db;
        }
    }

    // Noise filtering: notch bands + spectral subtraction
    if show_noise {
        // Notch bands: check DSP filter state to determine if notch should show
        let show_notch = {
            let dsp_on = state.display_filter_enabled.get_untracked();
            if dsp_on {
                // DSP panel controls notch display
                match state.display_filter_notch.get_untracked() {
                    DisplayFilterMode::Off => false,
                    DisplayFilterMode::Auto | DisplayFilterMode::Same => state.notch_enabled.get_untracked(),
                    DisplayFilterMode::Custom => false,
                }
            } else {
                // Legacy: notch shows when playback notch is on
                state.notch_enabled.get_untracked()
            }
        };
        if show_notch {
            let bands = state.notch_bands.get_untracked();
            let harm_supp = state.notch_harmonic_suppression.get_untracked();
            for (row, adj_val) in adj.iter_mut().enumerate().take(tile_height) {
                let bin = tile_height - 1 - row;
                let freq = file_max_freq * bin as f64 / (tile_height - 1).max(1) as f64;
                for band in &bands {
                    if !band.enabled { continue; }
                    let half_bw = band.bandwidth_hz / 2.0;
                    // Primary notch
                    if (freq - band.center_hz).abs() <= half_bw {
                        *adj_val -= band.strength_db as f32;
                    }
                    // Harmonic suppression at 2x and 3x
                    if harm_supp > 0.0 {
                        for harmonic in [2.0, 3.0] {
                            let hfreq = band.center_hz * harmonic;
                            if (freq - hfreq).abs() <= half_bw * harmonic {
                                *adj_val -= (band.strength_db * harm_supp) as f32;
                            }
                        }
                    }
                }
            }
        }

        // Spectral subtraction: use display auto noise floor when DSP NR is Auto,
        // custom strength when Custom, or playback noise floor when Same/fallback.
        {
            let dsp_enabled = state.display_filter_enabled.get_untracked();
            let nr_mode = state.display_filter_nr.get_untracked();

            let (nf_opt, strength) = if dsp_enabled && matches!(nr_mode, DisplayFilterMode::Auto) {
                // Auto: use display-specific auto-learned floor
                (state.display_auto_noise_floor.get_untracked(), 0.8)
            } else if dsp_enabled && matches!(nr_mode, DisplayFilterMode::Custom) {
                // Custom: prefer display auto floor with custom strength
                let floor = state.display_auto_noise_floor.get_untracked()
                    .or_else(|| state.noise_reduce_floor.get_untracked());
                (floor, state.display_nr_strength.get_untracked())
            } else if state.noise_reduce_enabled.get_untracked() {
                // Same/fallback: use playback noise floor
                (state.noise_reduce_floor.get_untracked(), state.noise_reduce_strength.get_untracked())
            } else {
                (None, 0.0)
            };

            if let Some(nf) = nf_opt {
                let nf_bins = nf.bin_magnitudes.len();
                let nf_max_freq = nf.sample_rate as f64 / 2.0;
                for (row, adj_val) in adj.iter_mut().enumerate().take(tile_height) {
                    let bin = tile_height - 1 - row;
                    let freq = file_max_freq * bin as f64 / (tile_height - 1).max(1) as f64;
                    let nf_bin = ((freq / nf_max_freq) * (nf_bins - 1) as f64).round() as usize;
                    if nf_bin < nf_bins {
                        let noise_mag = nf.bin_magnitudes[nf_bin];
                        if noise_mag > 1e-15 {
                            let noise_db = 20.0 * (noise_mag as f32).log10();
                            *adj_val -= noise_db * strength as f32;
                        }
                    }
                }
            }
        }
    }

    Some(adj)
}
