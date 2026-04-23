use leptos::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use crate::canvas::{tile_cache, spectral_store};
use crate::canvas::tile_cache::TILE_COLS;
use crate::canvas::spectrogram_renderer::FlowAlgo;
use crate::state::AppState;

fn visible_tile_order(first_tile: usize, last_tile: usize, center_tile: usize) -> Vec<usize> {
    if first_tile > last_tile {
        return Vec::new();
    }

    let clamped_center = center_tile.clamp(first_tile, last_tile);
    let mut order = Vec::with_capacity(last_tile - first_tile + 1);
    order.push(clamped_center);

    let mut distance = 1usize;
    while order.len() < last_tile - first_tile + 1 {
        if let Some(left) = clamped_center.checked_sub(distance) {
            if left >= first_tile {
                order.push(left);
            }
        }
        let right = clamped_center + distance;
        if right <= last_tile {
            order.push(right);
        }
        distance += 1;
    }

    order
}

/// Schedule missing normal/reassignment tiles for the visible viewport.
///
/// Called from the render Effect after blitting, to ensure tiles are being
/// computed for the current viewport.
pub fn schedule_normal_tiles(
    state: AppState,
    file_idx: usize,
    total_cols: usize,
    scroll_col: f64,
    zoom: f64,
    display_w: f64,
    time_res: f64,
    is_playing: bool,
    reassign_on: bool,
    disposed: &Arc<AtomicBool>,
) {
    let ideal_lod = tile_cache::select_lod(zoom);
    let ratio = tile_cache::lod_ratio(ideal_lod);

    // Clamp vis_start to valid range (must match renderer's clamping)
    let vis_start = scroll_col.max(0.0).min((total_cols as f64 - 1.0).max(0.0));
    let vis_end = (vis_start + display_w / zoom).min(total_cols as f64);

    if vis_end <= vis_start { return; }

    // Tile range at ideal LOD
    let vis_start_lod = vis_start * ratio;
    let vis_end_lod = vis_end * ratio;
    let first_tile = (vis_start_lod / TILE_COLS as f64).floor() as usize;
    let last_tile = ((vis_end_lod - 0.001).max(0.0) / TILE_COLS as f64).floor() as usize;

    // Cancel stale in-flight entries far from viewport
    let viewport_center_tile = ((vis_start_lod + vis_end_lod) / 2.0 / TILE_COLS as f64) as usize;
    let visible_tile_count = last_tile.saturating_sub(first_tile) + 1;
    let keep_cancel = visible_tile_count.max(10) * 3;

    // During playback, also protect tiles near the pre-play scroll position
    if is_playing {
        let pre_scroll = state.pre_play_scroll.get_untracked();
        let pre_col = (pre_scroll / time_res).max(0.0).min((total_cols as f64 - 1.0).max(0.0));
        let pre_end_col = (pre_col + display_w / zoom).min(total_cols as f64);
        let pre_center = (((pre_col * ratio) + (pre_end_col * ratio)) / 2.0 / TILE_COLS as f64) as usize;

        tile_cache::cancel_far_in_flight_multi(file_idx, ideal_lod, &[
            (viewport_center_tile, keep_cancel), (pre_center, keep_cancel)
        ]);
    } else {
        tile_cache::cancel_far_in_flight(file_idx, ideal_lod, viewport_center_tile, keep_cancel);
    }

    let is_loading = state.loading_files.with_untracked(|v| !v.is_empty());
    let use_reassign = reassign_on && ideal_lod > 1;

    let tile_order = visible_tile_order(first_tile, last_tile, viewport_center_tile);
    let mut any_missing = false;

    for &t in &tile_order {
        // Schedule reassignment tiles when enabled (skip coarse overview LODs 0-1)
        if use_reassign
            && tile_cache::get_reassign_tile(file_idx, ideal_lod, t).is_none() {
                tile_cache::schedule_reassign_tile(state, file_idx, ideal_lod, t);
                any_missing = true;
            }

        // Always schedule normal tiles (for fallback and non-reassign mode)
        if tile_cache::get_tile(file_idx, ideal_lod, t).is_none() {
            tile_cache::schedule_tile_lod(state, file_idx, ideal_lod, t);
            any_missing = true;
        }
    }

    for &t in &tile_order {
        // Also ensure a baseline-LOD fallback tile exists (for smooth transitions)
        if ideal_lod != tile_cache::LOD_BASELINE {
            let (fb_tile, _, _) = tile_cache::fallback_tile_info(ideal_lod, t, tile_cache::LOD_BASELINE);
            if tile_cache::get_tile(file_idx, tile_cache::LOD_BASELINE, fb_tile).is_none()
                && !is_loading {
                    let tile_start = fb_tile * TILE_COLS;
                    let tile_end = (tile_start + TILE_COLS).min(total_cols);
                    if spectral_store::has_store(file_idx)
                        && spectral_store::tile_complete(file_idx, tile_start, tile_end)
                    {
                        tile_cache::schedule_tile_from_store(state, file_idx, fb_tile);
                    } else {
                        tile_cache::schedule_tile_on_demand(state, file_idx, fb_tile);
                    }
                    any_missing = true;
                }
        }
    }

    // When ideal LOD is the baseline, also schedule from store/on-demand
    if ideal_lod == tile_cache::LOD_BASELINE && !is_loading {
        let bl_first = (vis_start / TILE_COLS as f64).floor() as usize;
        let bl_last = ((vis_end - 0.001).max(0.0) / TILE_COLS as f64).floor() as usize;
        let bl_center = ((vis_start + vis_end) / 2.0 / TILE_COLS as f64) as usize;
        for t in visible_tile_order(bl_first, bl_last, bl_center) {
            if tile_cache::get_tile(file_idx, tile_cache::LOD_BASELINE, t).is_none() {
                let tile_start = t * TILE_COLS;
                let tile_end = (tile_start + TILE_COLS).min(total_cols);
                if spectral_store::has_store(file_idx)
                    && spectral_store::tile_complete(file_idx, tile_start, tile_end)
                {
                    tile_cache::schedule_tile_from_store(state, file_idx, t);
                } else {
                    tile_cache::schedule_tile_on_demand(state, file_idx, t);
                }
                any_missing = true;
            }
        }
    }

    // Recovery: if visible tiles are missing, force a retry after 250ms.
    // Skip the expensive count_missing_visible() call when the scheduling
    // loops above found no missing tiles (common case during panning).
    if !any_missing { return; }
    let missing = tile_cache::count_missing_visible(file_idx, ideal_lod, first_tile, last_tile);
    if missing > 0 {
        let state_recovery = state;
        let disposed_rc = disposed.clone();
        let recovery_cb = Closure::once(move || {
            if disposed_rc.load(Ordering::Relaxed) { return; }
            state_recovery.tile_ready_signal.update(|n| *n = n.wrapping_add(1));
        });
        let _ = web_sys::window().unwrap()
            .set_timeout_with_callback_and_timeout_and_arguments_0(
                recovery_cb.as_ref().unchecked_ref(), 250,
            );
        recovery_cb.forget();
    }
}

/// Schedule missing flow tiles for the visible viewport.
pub fn schedule_flow_tiles(
    state: AppState,
    file_idx: usize,
    total_cols: usize,
    scroll_col: f64,
    zoom: f64,
    display_w: f64,
    algo: FlowAlgo,
) {
    let ideal_lod = tile_cache::select_lod(zoom);
    let ratio = tile_cache::lod_ratio(ideal_lod);

    let vis_start = scroll_col.max(0.0).min((total_cols as f64 - 1.0).max(0.0));
    let vis_end = (vis_start + display_w / zoom).min(total_cols as f64);
    if vis_end <= vis_start { return; }

    // Convert to ideal-LOD tile space
    let vis_start_lod = vis_start * ratio;
    let vis_end_lod = vis_end * ratio;
    let first_tile = (vis_start_lod / TILE_COLS as f64).floor() as usize;
    let last_tile = ((vis_end_lod - 0.001).max(0.0) / TILE_COLS as f64).floor() as usize;

    for t in first_tile..=last_tile {
        // Schedule ideal LOD tile
        if tile_cache::get_flow_tile(file_idx, ideal_lod, t).is_none() {
            tile_cache::schedule_flow_tile(state, file_idx, ideal_lod, t, algo);
        }

        // Also ensure a baseline-LOD fallback exists for smooth transitions
        if ideal_lod != tile_cache::LOD_BASELINE {
            let (fb_tile, _, _) = tile_cache::fallback_tile_info(ideal_lod, t, tile_cache::LOD_BASELINE);
            if tile_cache::get_flow_tile(file_idx, tile_cache::LOD_BASELINE, fb_tile).is_none() {
                tile_cache::schedule_flow_tile(state, file_idx, tile_cache::LOD_BASELINE, fb_tile, algo);
            }
        }
    }
}

/// Schedule missing resonator tiles for the visible viewport.
///
/// Mirrors `schedule_flow_tiles` but targets the resonator cache. Also
/// schedules a baseline-LOD fallback so switching zoom feels smooth.
pub fn schedule_resonator_tiles(
    state: AppState,
    file_idx: usize,
    total_cols: usize,
    scroll_col: f64,
    zoom: f64,
    display_w: f64,
) {
    let ideal_lod = tile_cache::select_lod(zoom);
    let ratio = tile_cache::lod_ratio(ideal_lod);

    let vis_start = scroll_col.max(0.0).min((total_cols as f64 - 1.0).max(0.0));
    let vis_end = (vis_start + display_w / zoom).min(total_cols as f64);
    if vis_end <= vis_start { return; }

    let vis_start_lod = vis_start * ratio;
    let vis_end_lod = vis_end * ratio;
    let first_tile = (vis_start_lod / TILE_COLS as f64).floor() as usize;
    let last_tile = ((vis_end_lod - 0.001).max(0.0) / TILE_COLS as f64).floor() as usize;

    for t in first_tile..=last_tile {
        if tile_cache::get_resonator_tile(file_idx, ideal_lod, t).is_none() {
            tile_cache::schedule_resonator_tile(state, file_idx, ideal_lod, t);
        }
        if ideal_lod != tile_cache::LOD_BASELINE {
            let (fb_tile, _, _) = tile_cache::fallback_tile_info(ideal_lod, t, tile_cache::LOD_BASELINE);
            if tile_cache::get_resonator_tile(file_idx, tile_cache::LOD_BASELINE, fb_tile).is_none() {
                tile_cache::schedule_resonator_tile(state, file_idx, tile_cache::LOD_BASELINE, fb_tile);
            }
        }
    }
}

/// Set up all tile-cache-clearing Effects. Call once from the component body.
pub fn setup_cache_clearing_effects(state: AppState) {
    // Clear flow tile cache when algorithm or enabled state changes
    Effect::new(move || {
        let _display = state.spectrogram_display.get();
        let _enabled = state.flow_enabled.get();
        crate::canvas::tile_cache::clear_flow_cache();
    });

    // Clear all tiles when FFT mode changes
    Effect::new(move || {
        let _fft = state.spect_fft_mode.get();
        crate::canvas::tile_cache::clear_all_tiles();
        crate::canvas::tile_cache::clear_flow_cache();
        crate::canvas::tile_cache::clear_reassign_cache();
        state.tile_ready_signal.update(|n| *n = n.wrapping_add(1));
    });

    // Clear magnitude tiles when display transform/decimation toggles
    {
        let prev_xform = RwSignal::new(false);
        let prev_decim = RwSignal::new(0u32);
        Effect::new(move || {
            let xform_on = state.display_transform.get();
            let _mode = state.playback_mode.get();
            let _het = state.het_frequency.get();
            let _het_cut = state.het_cutoff.get();
            let _te = state.te_factor.get();
            let _ps = state.ps_factor.get();
            let _pv = state.pv_factor.get();
            let _zc = state.zc_factor.get();
            let decim = state.display_decimate_effective.get();
            let decim_changed = decim != prev_decim.get_untracked();
            if xform_on || prev_xform.get_untracked() || decim_changed {
                crate::canvas::tile_cache::clear_all_tiles();
                state.tile_ready_signal.update(|n| *n = n.wrapping_add(1));
            }
            prev_xform.set(xform_on);
            prev_decim.set(decim);
        });
    }

    // Clear reassignment tile cache when toggle changes
    Effect::new(move || {
        let _reassign = state.reassign_enabled.get();
        crate::canvas::tile_cache::clear_reassign_cache();
    });

    // Clear resonator tile cache when bandwidth or bin-count mode changes
    Effect::new(move || {
        let _bw = state.resonator_bandwidth_hz.get();
        let _mode = state.resonator_fft_mode.get();
        crate::canvas::tile_cache::clear_resonator_cache();
        state.tile_ready_signal.update(|n| *n = n.wrapping_add(1));
    });
}
