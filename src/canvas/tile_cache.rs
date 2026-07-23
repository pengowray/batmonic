// SPDX-License-Identifier: GPL-3.0-only OR MIT OR Apache-2.0
//! Progressive tile cache for spectrogram rendering.
//!
//! Google Maps-style LOD system: each LOD level has its own tile index space.
//! All tiles have the same number of columns (`TILE_COLS = 256`), but different
//! LODs cover different time ranges: (note: these change in different modes)
//!
//! - LOD 0: hop=8192, covers ~2M samples/tile (ultra-wide, quality downscaled)
//! - LOD 1: hop=2048, covers ~524K samples/tile (wide overview, quality downscaled)
//! - LOD 2: hop=512, covers ~131K samples/tile (normal — baseline)
//! - LOD 3: hop=128, covers ~33K samples/tile (zoomed in)
//! - LOD 4: hop=32, covers ~8K samples/tile (deep zoom)
//! - LOD 5: hop=8, covers ~2K samples/tile (extreme zoom)
//! - LOD 6: hop=2, covers ~512 samples/tile (sample-level zoom)
//! - LOD 7: hop=1, covers ~256 samples/tile (finest; hop cannot go below 1)
//!
//! Levels 0–6 step 4× apart; LOD 7 is 2× finer than LOD 6 (since hop must be
//! an integer ≥ 1). The renderer picks the ideal LOD
//! for the current zoom and falls back to coarser LODs when tiles aren't cached.
//! FFT size is adaptive per LOD via `FftMode::fft_for_lod()`.
//!
//! Four independent caches with separate LRU eviction budgets:
//! - Magnitude (512 MB) — standard STFT spectrogram tiles
//! - Flow (120 MB) — optical-flow tiles
//! - Reassignment (120 MB) — reassigned spectrogram tiles (LOD 2+ only)
//! - Chromagram (64 MB) — chromagram tiles
//!
//! Tiles are computed asynchronously via `spawn_local` with `setTimeout(0)`
//! yielding to keep the UI responsive. Concurrency is capped at
//! `MAX_CONCURRENT_SPAWNS` across all cache types to prevent
//! wasm-bindgen-futures RefCell reentrant borrow panics. Completed tiles
//! bump `tile_ready_signal` to trigger re-rendering and schedule remaining
//! missing tiles.

use crate::audio::streaming_playback::PV_HQ_OVERLAP;
use crate::audio::streaming_source;
use crate::canvas::spectrogram_renderer::{self, FlowAlgo, PreRendered};
use crate::state::store_fields::*;
use crate::state::{AppState, LoadedFile, PlaybackMode};
use crate::viewport;
use leptos::prelude::*;
use std::cell::RefCell;
use std::collections::HashMap;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::spawn_local;

/// Number of spectrogram columns per tile (constant across all LODs).
pub const TILE_COLS: usize = 256;

/// Main magnitude spectrogram cache budget.
/// Sized for adaptive FFT modes where L0/L1 tiles can be 4–8× larger than base.
const MAGNITUDE_MAX_BYTES: usize = 512 * 1024 * 1024;
/// Flow cache budget.
const FLOW_MAX_BYTES: usize = 120 * 1024 * 1024;
/// Reassignment cache budget.
const REASSIGN_MAX_BYTES: usize = 120 * 1024 * 1024;
/// Chromagram cache budget.
const CHROMA_MAX_BYTES: usize = 64 * 1024 * 1024;
/// Resonator cache budget. Resonator tiles have the same layout as magnitude
/// tiles (dB per pixel) so this sizing mirrors the magnitude cache.
const RESONATOR_MAX_BYTES: usize = 256 * 1024 * 1024;

/// Maximum time (ms) a tile can be in-flight before being considered stuck.
const IN_FLIGHT_TIMEOUT_MS: f64 = 10_000.0;

/// Maximum number of concurrent spawn_local tile tasks across all categories.
/// Prevents wasm-bindgen-futures RefCell reentrant borrow panics when many
/// tiles are scheduled simultaneously (e.g. large files).
const MAX_CONCURRENT_SPAWNS: usize = 8;

fn visible_window_for_file(state: &AppState, file_idx: usize) -> Option<(f64, f64)> {
    let files = state.library.files().get_untracked();
    let file = files.get(file_idx)?;
    let file_time_res = file.spectrogram.time_resolution;
    let file_duration = file.audio.duration_secs;
    let scroll = state.view.scroll_offset().get_untracked();
    let zoom = state.view.zoom_level().get_untracked();
    let canvas_w = state.viewmode.spectrogram_canvas_width().get_untracked();

    if state.library.current_index().get_untracked() == Some(file_idx) {
        let visible_time = viewport::visible_time(canvas_w, zoom, file_time_res);
        return viewport::data_window(scroll, visible_time, file_duration);
    }

    let timeline = state.timeline.active().get_untracked();
    let tl = timeline.as_ref()?;
    let global_time_res = tl
        .segments
        .first()
        .and_then(|s| files.get(s.file_index))
        .map(|f| f.spectrogram.time_resolution)
        .unwrap_or(file_time_res);
    let visible_time = viewport::visible_time(canvas_w, zoom, global_time_res);
    if visible_time <= 0.0 {
        return None;
    }
    let visible_start = scroll;
    let visible_end = scroll + visible_time;

    tl.segments
        .iter()
        .filter(|seg| seg.file_index == file_idx)
        .find_map(|seg| {
            let seg_start = seg.timeline_offset_secs;
            let seg_end = seg_start + seg.duration_secs;
            if seg_start >= visible_end || seg_end <= visible_start {
                return None;
            }
            let local_start = (visible_start - seg_start).max(0.0);
            let local_end = (visible_end - seg_start).min(seg.duration_secs);
            if local_end > local_start {
                Some((local_start, local_end))
            } else {
                None
            }
        })
}

fn visible_tile_focus_for_file(
    state: &AppState,
    file_idx: usize,
    total_cols: usize,
    time_res: f64,
) -> Option<(usize, usize, usize)> {
    if total_cols == 0 || time_res <= 0.0 {
        return None;
    }

    let (local_start, local_end) = visible_window_for_file(state, file_idx)?;
    let max_col = total_cols.saturating_sub(1) as f64;
    let start_col = (local_start / time_res).clamp(0.0, max_col);
    let end_col = ((local_end / time_res) - 0.001).clamp(0.0, max_col);
    if end_col < start_col {
        return None;
    }

    let first_tile = (start_col / TILE_COLS as f64).floor() as usize;
    let last_tile = (end_col / TILE_COLS as f64).floor() as usize;
    let center_col = ((local_start + local_end) * 0.5 / time_res).clamp(0.0, max_col);
    let center_tile = (center_col / TILE_COLS as f64).floor() as usize;
    Some((
        first_tile,
        last_tile,
        center_tile.clamp(first_tile, last_tile),
    ))
}

fn full_tile_order(n_tiles: usize, center_tile: usize) -> Vec<usize> {
    if n_tiles == 0 {
        return Vec::new();
    }

    let clamped_center = center_tile.min(n_tiles - 1);
    let mut order = Vec::with_capacity(n_tiles);
    order.push(clamped_center);

    let mut distance = 1usize;
    while order.len() < n_tiles {
        if let Some(left) = clamped_center.checked_sub(distance) {
            order.push(left);
        }
        let right = clamped_center + distance;
        if right < n_tiles {
            order.push(right);
        }
        distance += 1;
    }

    order
}

/// Check if a file is currently visible and should be prioritised.
fn is_current_file(state: &AppState, file_idx: usize) -> bool {
    visible_window_for_file(state, file_idx).is_some()
}

// ── LOD configuration ────────────────────────────────────────────────────────

pub struct LodConfig {
    pub fft_size: usize,
    pub hop_size: usize,
}

pub const NUM_LODS: usize = 8;

/// The LOD level used as the spatial coordinate baseline (hop=512).
/// All scroll positions, total_cols, etc. are expressed in this LOD's column space.
pub const LOD_BASELINE: u8 = 2;

/// Hop size of the baseline LOD — used for coordinate-space calculations.
pub const BASELINE_HOP: usize = 512;

pub const LOD_CONFIGS: [LodConfig; NUM_LODS] = [
    LodConfig {
        fft_size: 256,
        hop_size: 8192,
    }, // LOD 0 — ultra-wide overview (quality downscaled)
    LodConfig {
        fft_size: 256,
        hop_size: 2048,
    }, // LOD 1 — wide overview (quality downscaled)
    LodConfig {
        fft_size: 256,
        hop_size: 512,
    }, // LOD 2 — normal resolution (baseline)
    LodConfig {
        fft_size: 256,
        hop_size: 128,
    }, // LOD 3 — zoomed in
    LodConfig {
        fft_size: 256,
        hop_size: 32,
    }, // LOD 4 — deep zoom
    LodConfig {
        fft_size: 256,
        hop_size: 8,
    }, // LOD 5 — extreme zoom
    LodConfig {
        fft_size: 256,
        hop_size: 2,
    }, // LOD 6 — sample-level zoom
    LodConfig {
        fft_size: 128,
        hop_size: 1,
    }, // LOD 7 — per-sample zoom (finest possible)
];

/// Select the ideal LOD level for the current zoom.
/// `zoom` is pixels per baseline (LOD2) column.
pub fn select_lod(zoom: f64) -> u8 {
    // LOD 7 is only 2× finer than LOD 6 (hop 1 vs 2), so its threshold is 2×.
    if zoom >= 256.0 {
        7
    } else if zoom >= 128.0 {
        6
    } else if zoom >= 32.0 {
        5
    } else if zoom >= 8.0 {
        4
    } else if zoom >= 2.0 {
        3
    } else if zoom >= 0.5 {
        2
    } else if zoom >= 0.125 {
        1
    } else {
        0
    }
}

/// Ratio of baseline (LOD2) columns to LOD_L columns (how many LOD_L cols per baseline col).
/// LOD0: 0.0625, LOD1: 0.25, LOD2: 1.0, LOD3: 4.0, LOD4: 16.0, LOD5: 64.0, LOD6: 256.0, LOD7: 512.0
pub fn lod_ratio(lod: u8) -> f64 {
    BASELINE_HOP as f64 / LOD_CONFIGS[lod as usize].hop_size as f64
}

/// Tile count at a given LOD for a file with `total_samples` audio samples.
pub fn tile_count_for_samples(total_samples: usize, lod: u8) -> usize {
    let config = &LOD_CONFIGS[lod as usize];
    if total_samples < config.fft_size {
        return 0;
    }
    let total_cols = (total_samples - config.fft_size) / config.hop_size + 1;
    total_cols.div_ceil(TILE_COLS)
}

/// Map a tile index from one LOD to the corresponding tile at a lower (coarser) LOD.
/// Returns (fallback_tile_idx, sub_col_start, sub_col_end) — the sub-region within
/// the fallback tile that covers the same time range.
pub fn fallback_tile_info(
    target_lod: u8,
    target_tile: usize,
    fallback_lod: u8,
) -> (usize, f64, f64) {
    let target_hop = LOD_CONFIGS[target_lod as usize].hop_size;
    let fb_hop = LOD_CONFIGS[fallback_lod as usize].hop_size;

    // Sample range of the target tile
    let sample_start = target_tile * TILE_COLS * target_hop;
    let sample_end = sample_start + TILE_COLS * target_hop;

    // Convert to fallback tile/column space
    let fb_col_start = sample_start as f64 / fb_hop as f64;
    let fb_col_end = sample_end as f64 / fb_hop as f64;

    let fb_tile = (fb_col_start / TILE_COLS as f64).floor() as usize;
    let fb_src_start = fb_col_start - (fb_tile * TILE_COLS) as f64;
    let fb_src_end = fb_col_end - (fb_tile * TILE_COLS) as f64;

    (fb_tile, fb_src_start, fb_src_end)
}

/// Aggregate dense STFT columns into fewer columns by taking the max magnitude
/// per frequency bin across each group of `factor` columns. This preserves
/// transient peaks (like bat calls) that sparse hop sizes would miss.
fn aggregate_columns_max(
    cols: &[crate::types::SpectrogramColumn],
    factor: usize,
) -> Vec<crate::types::SpectrogramColumn> {
    if factor <= 1 || cols.is_empty() {
        return cols.to_vec();
    }
    let n_output = cols.len() / factor;
    (0..n_output)
        .map(|i| {
            let group_start = i * factor;
            let group_end = (group_start + factor).min(cols.len());
            let group = &cols[group_start..group_end];
            let n_bins = group[0].magnitudes.len();
            let mut max_mags = vec![0.0f32; n_bins];
            for col in group {
                for (bin, &mag) in col.magnitudes.iter().enumerate().take(n_bins) {
                    if mag > max_mags[bin] {
                        max_mags[bin] = mag;
                    }
                }
            }
            crate::types::SpectrogramColumn {
                magnitudes: max_mags,
                time_offset: group[0].time_offset,
            }
        })
        .collect()
}

// ── Cache data structures ────────────────────────────────────────────────────

// The tile container, `Tile`, and the in-flight timeout/spawn-count helpers
// live in `oversample-core` so their eviction / byte-accounting / timeout logic
// is host-testable (the WASM frontend crate can't be `cargo test`ed). See
// `oversample-core/src/canvas/tile_cache_core.rs`. `Tile` is re-exported so
// external callers keep using `tile_cache::Tile`; the wasm-coupled machinery
// (thread-locals, scheduling, generations, `js_sys::Date` time) stays here.
use oversample_core::canvas::tile_cache_core::{
    in_flight_active_count, in_flight_is_active, ChromaKey, TileCache,
};
pub use oversample_core::canvas::tile_cache_core::{Tile, TileKey};

thread_local! {
    /// Unified magnitude tile cache — all LOD levels in one cache.
    static CACHE: RefCell<TileCache> = RefCell::new(TileCache::new(MAGNITUDE_MAX_BYTES));
    /// Map of (file_idx, lod, tile_idx) → timestamp (ms) for tiles currently being generated.
    /// Entries older than IN_FLIGHT_TIMEOUT_MS are considered stuck and can be re-scheduled.
    static IN_FLIGHT: RefCell<HashMap<TileKey, f64>> =
        RefCell::new(HashMap::new());
    /// Generation counter — incremented on every clear_all_tiles(). In-flight
    /// async tasks capture the generation at spawn time and discard their result
    /// if a clear happened while they were computing (prevents stale tiles).
    static CACHE_GENERATION: RefCell<u64> = const { RefCell::new(0) };

    /// Flow-mode tile cache — multi-LOD, same TileKey as magnitude tiles.
    static FLOW_CACHE: RefCell<TileCache> = RefCell::new(TileCache::new(FLOW_MAX_BYTES));
    static FLOW_IN_FLIGHT: RefCell<HashMap<TileKey, f64>> =
        RefCell::new(HashMap::new());
    static FLOW_CACHE_GENERATION: RefCell<u64> = const { RefCell::new(0) };

    /// Reassignment spectrogram tile cache — multi-LOD, same TileKey as magnitude tiles.
    static REASSIGN_CACHE: RefCell<TileCache> = RefCell::new(TileCache::new(REASSIGN_MAX_BYTES));
    static REASSIGN_IN_FLIGHT: RefCell<HashMap<TileKey, f64>> =
        RefCell::new(HashMap::new());
    static REASSIGN_CACHE_GENERATION: RefCell<u64> = const { RefCell::new(0) };

    /// Chromagram tile cache (baseline-LOD only). 512 LRU-compaction floor
    /// matches the original ChromaTileCache (vs 1024 for the multi-LOD caches).
    static CHROMA_CACHE: RefCell<TileCache<ChromaKey>> =
        RefCell::new(TileCache::with_floor(CHROMA_MAX_BYTES, 512));
    static CHROMA_IN_FLIGHT: RefCell<HashMap<ChromaKey, f64>> =
        RefCell::new(HashMap::new());

    /// Cached per-file global chromagram normalisation maxima (max_class, max_note).
    static CHROMA_GLOBAL_MAX: RefCell<HashMap<usize, (f32, f32)>> =
        RefCell::new(HashMap::new());

    /// Resonator tile cache — multi-LOD, same TileKey as magnitude tiles.
    /// Stores absolute dB values per pixel (identical layout to magnitude tiles).
    static RESONATOR_CACHE: RefCell<TileCache> = RefCell::new(TileCache::new(RESONATOR_MAX_BYTES));
    static RESONATOR_IN_FLIGHT: RefCell<HashMap<TileKey, f64>> =
        RefCell::new(HashMap::new());
    static RESONATOR_CACHE_GENERATION: RefCell<u64> = const { RefCell::new(0) };
}

// ── IN_FLIGHT helpers ────────────────────────────────────────────────────────

/// Returns true if the total number of active in-flight tasks across all
/// categories has reached `MAX_CONCURRENT_SPAWNS`.
fn at_spawn_limit() -> bool {
    let now = js_sys::Date::now();
    let mag = IN_FLIGHT.with(|s| in_flight_active_count(&s.borrow(), now, IN_FLIGHT_TIMEOUT_MS));
    let flow =
        FLOW_IN_FLIGHT.with(|s| in_flight_active_count(&s.borrow(), now, IN_FLIGHT_TIMEOUT_MS));
    let reassign =
        REASSIGN_IN_FLIGHT.with(|s| in_flight_active_count(&s.borrow(), now, IN_FLIGHT_TIMEOUT_MS));
    let chroma =
        CHROMA_IN_FLIGHT.with(|s| in_flight_active_count(&s.borrow(), now, IN_FLIGHT_TIMEOUT_MS));
    let reson = RESONATOR_IN_FLIGHT
        .with(|s| in_flight_active_count(&s.borrow(), now, IN_FLIGHT_TIMEOUT_MS));
    mag + flow + reassign + chroma + reson >= MAX_CONCURRENT_SPAWNS
}

fn has_active_in_flight<K: Eq + std::hash::Hash>(map: &mut HashMap<K, f64>, key: &K) -> bool {
    in_flight_is_active(map, key, js_sys::Date::now(), IN_FLIGHT_TIMEOUT_MS)
}

// ── CacheCtx: bundles one cache's thread-locals ──────────────────────────────
//
// The magnitude / flow / reassign / resonator caches each own a (TileCache,
// in-flight map, generation counter) triple and had a full set of near-identical
// get / borrow / clear / stats / still-active wrappers. CacheCtx bundles the
// three thread-local handles so that synchronous wrapper layer is written once;
// the public `*_tile` / `clear_*` functions below are thin shims that pick a
// ctx. The wasm scheduling in `schedule_*` still drives the thread-locals
// directly — only this wrapper layer is deduplicated.
//
// Chroma is intentionally excluded: its key is (file_idx, tile_idx) and it has
// bespoke global-max bookkeeping, so it keeps its own wrappers.
struct CacheCtx {
    cache: &'static std::thread::LocalKey<RefCell<TileCache>>,
    in_flight: &'static std::thread::LocalKey<RefCell<HashMap<TileKey, f64>>>,
    generation: &'static std::thread::LocalKey<RefCell<u64>>,
}

impl CacheCtx {
    fn get(&self, file_idx: usize, lod: u8, tile_idx: usize) -> Option<()> {
        self.cache
            .with(|c| c.borrow().get(file_idx, lod, tile_idx).map(|_| ()))
    }

    /// Touch (mark recently-used) then borrow a cached tile. The double-`with`
    /// dance drops the `borrow_mut` before re-borrowing shared, avoiding a
    /// reentrant RefCell panic across the thread-local boundary.
    fn borrow_tile<R>(
        &self,
        file_idx: usize,
        lod: u8,
        tile_idx: usize,
        f: impl FnOnce(&Tile) -> R,
    ) -> Option<R> {
        self.cache.with(|c| {
            let mut cache = c.borrow_mut();
            let key = TileKey {
                file_idx,
                lod,
                tile_idx,
            };
            if cache.contains_key(&key) {
                cache.touch(key);
                drop(cache);
                self.cache.with(|c| c.borrow().get_keyed(&key).map(f))
            } else {
                None
            }
        })
    }

    fn request_still_active(&self, key: &TileKey) -> bool {
        self.in_flight
            .with(|s| has_active_in_flight(&mut s.borrow_mut(), key))
    }

    fn tile_active(&self, file_idx: usize, lod: u8, tile_idx: usize) -> bool {
        self.request_still_active(&TileKey {
            file_idx,
            lod,
            tile_idx,
        })
    }

    fn bump_generation(&self) {
        self.generation.with(|g| *g.borrow_mut() += 1);
    }

    /// Clear every tile + in-flight entry for this cache and bump its generation
    /// so any still-running async task discards its result on completion.
    fn clear_all(&self) {
        self.cache.with(|c| c.borrow_mut().clear_all());
        self.in_flight.with(|s| s.borrow_mut().clear());
        self.bump_generation();
    }

    fn clear_file(&self, file_idx: usize) {
        self.cache.with(|c| c.borrow_mut().clear_for_file(file_idx));
        self.in_flight
            .with(|s| s.borrow_mut().retain(|k, _| k.file_idx != file_idx));
        self.bump_generation();
    }

    fn debug_stats(
        &self,
        file_idx: usize,
        lod: u8,
        first_tile: usize,
        last_tile: usize,
    ) -> TileDebugStats {
        self.cache.with(|c| {
            let cache = c.borrow();
            self.in_flight.with(|s| {
                let mut inflight = s.borrow_mut();
                let mut stats = collect_debug_stats(
                    cache.len(),
                    &mut inflight,
                    (first_tile..=last_tile).map(|tile_idx| TileKey {
                        file_idx,
                        lod,
                        tile_idx,
                    }),
                    |key| cache.contains_key(key),
                );
                stats.used_bytes = cache.total_bytes();
                stats.max_bytes = cache.max_bytes();
                stats
            })
        })
    }
}

fn magnitude_ctx() -> CacheCtx {
    CacheCtx {
        cache: &CACHE,
        in_flight: &IN_FLIGHT,
        generation: &CACHE_GENERATION,
    }
}
fn flow_ctx() -> CacheCtx {
    CacheCtx {
        cache: &FLOW_CACHE,
        in_flight: &FLOW_IN_FLIGHT,
        generation: &FLOW_CACHE_GENERATION,
    }
}
fn reassign_ctx() -> CacheCtx {
    CacheCtx {
        cache: &REASSIGN_CACHE,
        in_flight: &REASSIGN_IN_FLIGHT,
        generation: &REASSIGN_CACHE_GENERATION,
    }
}
fn resonator_ctx() -> CacheCtx {
    CacheCtx {
        cache: &RESONATOR_CACHE,
        in_flight: &RESONATOR_IN_FLIGHT,
        generation: &RESONATOR_CACHE_GENERATION,
    }
}

// ── Public API: magnitude tile cache ─────────────────────────────────────────

pub fn get_tile(file_idx: usize, lod: u8, tile_idx: usize) -> Option<()> {
    magnitude_ctx().get(file_idx, lod, tile_idx)
}

pub fn borrow_tile<R>(
    file_idx: usize,
    lod: u8,
    tile_idx: usize,
    f: impl FnOnce(&Tile) -> R,
) -> Option<R> {
    magnitude_ctx().borrow_tile(file_idx, lod, tile_idx, f)
}

pub fn clear_file(file_idx: usize) {
    magnitude_ctx().clear_file(file_idx);
}

/// Clear all magnitude tiles (all files, all LODs). Used when global
/// settings like FFT size change and all cached tiles become stale.
pub fn clear_all_tiles() {
    magnitude_ctx().clear_all();
    crate::canvas::spectrogram_renderer::clear_tile_canvas_cache();
}

/// Clear all tile caches (main, flow, reassign, chroma, resonator). Used when
/// a global parameter like channel_view changes and all tiles need recomputation.
pub fn clear_all_caches() {
    clear_all_tiles();
    clear_flow_cache();
    clear_reassign_cache();
    clear_chroma_cache();
    clear_resonator_cache();
}

pub fn evict_far(file_idx: usize, lod: u8, center_tile: usize, keep_radius: usize) {
    CACHE.with(|c| {
        c.borrow_mut()
            .evict_far_from(file_idx, lod, center_tile, keep_radius)
    });
}

/// Evict tiles far from ALL given centers. A tile is only evicted if it falls
/// outside every (center, radius) zone — tiles near any center are kept.
pub fn evict_far_multi(file_idx: usize, lod: u8, centers: &[(usize, usize)]) {
    CACHE.with(|c| {
        c.borrow_mut().clear_keys(|k| {
            k.file_idx == file_idx
                && k.lod == lod
                && centers
                    .iter()
                    .all(|&(center, radius)| k.tile_idx.abs_diff(center) > radius)
        });
    });
}

/// Remove IN_FLIGHT entries far from the current viewport center.
/// Prevents old in-flight computations from blocking cache resources
/// when the user scrolls fast past them.
pub fn cancel_far_in_flight(file_idx: usize, lod: u8, center_tile: usize, keep_radius: usize) {
    IN_FLIGHT.with(|s| {
        let mut map = s.borrow_mut();
        if map.is_empty() {
            return;
        }
        map.retain(|k, _| {
            k.file_idx != file_idx
                || k.lod != lod
                || k.tile_idx.abs_diff(center_tile) <= keep_radius
        });
    });
}

/// Cancel in-flight entries far from ALL given centers.
pub fn cancel_far_in_flight_multi(file_idx: usize, lod: u8, centers: &[(usize, usize)]) {
    IN_FLIGHT.with(|s| {
        let mut map = s.borrow_mut();
        if map.is_empty() {
            return;
        }
        map.retain(|k, _| {
            k.file_idx != file_idx
                || k.lod != lod
                || centers
                    .iter()
                    .any(|&(center, radius)| k.tile_idx.abs_diff(center) <= radius)
        });
    });
}

/// Returns the count of visible tiles that are neither cached nor in-flight.
/// Used by the render effect to detect stuck states needing recovery.
pub fn count_missing_visible(
    file_idx: usize,
    lod: u8,
    first_tile: usize,
    last_tile: usize,
) -> usize {
    CACHE.with(|c| {
        let cache = c.borrow();
        IN_FLIGHT.with(|s| {
            let mut inflight = s.borrow_mut();
            (first_tile..=last_tile)
                .filter(|&t| {
                    let key = TileKey {
                        file_idx,
                        lod,
                        tile_idx: t,
                    };
                    !cache.contains_key(&key) && !has_active_in_flight(&mut inflight, &key)
                })
                .count()
        })
    })
}

/// Returns (used_bytes, max_bytes) for the magnitude tile cache.
pub fn cache_usage() -> (usize, usize) {
    CACHE.with(|c| {
        let cache = c.borrow();
        (cache.total_bytes(), cache.max_bytes())
    })
}

#[derive(Clone, Copy)]
pub struct TileDebugStats {
    pub visible_cached: usize,
    pub visible_in_flight: usize,
    pub visible_missing: usize,
    pub total_cached: usize,
    pub total_in_flight: usize,
    pub used_bytes: usize,
    pub max_bytes: usize,
}

fn collect_debug_stats<K>(
    tiles_len: usize,
    inflight: &mut HashMap<K, f64>,
    visible_keys: impl Iterator<Item = K>,
    contains_tile: impl Fn(&K) -> bool,
) -> TileDebugStats
where
    K: Eq + std::hash::Hash + Copy,
{
    let mut visible_cached = 0usize;
    let mut visible_in_flight = 0usize;
    let mut visible_missing = 0usize;

    for key in visible_keys {
        if contains_tile(&key) {
            visible_cached += 1;
        } else if has_active_in_flight(inflight, &key) {
            visible_in_flight += 1;
        } else {
            visible_missing += 1;
        }
    }

    let inflight_keys: Vec<K> = inflight.keys().copied().collect();
    let total_in_flight = inflight_keys
        .into_iter()
        .filter(|key| has_active_in_flight(inflight, key))
        .count();

    TileDebugStats {
        visible_cached,
        visible_in_flight,
        visible_missing,
        total_cached: tiles_len,
        total_in_flight,
        used_bytes: 0,
        max_bytes: 0,
    }
}

pub fn magnitude_debug_stats(
    file_idx: usize,
    lod: u8,
    first_tile: usize,
    last_tile: usize,
) -> TileDebugStats {
    magnitude_ctx().debug_stats(file_idx, lod, first_tile, last_tile)
}

pub fn flow_debug_stats(
    file_idx: usize,
    lod: u8,
    first_tile: usize,
    last_tile: usize,
) -> TileDebugStats {
    flow_ctx().debug_stats(file_idx, lod, first_tile, last_tile)
}

pub fn magnitude_tile_active(file_idx: usize, lod: u8, tile_idx: usize) -> bool {
    magnitude_ctx().tile_active(file_idx, lod, tile_idx)
}

pub fn flow_tile_active(file_idx: usize, lod: u8, tile_idx: usize) -> bool {
    flow_ctx().tile_active(file_idx, lod, tile_idx)
}

fn magnitude_request_still_active(key: &TileKey) -> bool {
    magnitude_ctx().request_still_active(key)
}

fn flow_request_still_active(key: &TileKey) -> bool {
    flow_ctx().request_still_active(key)
}

fn reassign_request_still_active(key: &TileKey) -> bool {
    reassign_ctx().request_still_active(key)
}

fn resonator_request_still_active(key: &TileKey) -> bool {
    resonator_ctx().request_still_active(key)
}

fn active_baseline_fft(state: AppState) -> usize {
    state
        .spect
        .fft_mode()
        .get_untracked()
        .fft_for_lod(LOD_BASELINE)
}

fn spectrogram_fft_size(data: &crate::types::SpectrogramData) -> Option<usize> {
    if let Some(first) = data.columns.first() {
        return Some(first.magnitudes.len().saturating_sub(1) * 2);
    }
    if data.freq_resolution > 0.0 {
        return Some((data.sample_rate as f64 / data.freq_resolution).round() as usize);
    }
    None
}

/// Returns the number of complete baseline-LOD tiles for a file currently in the cache.
pub fn tiles_ready(file_idx: usize, n_tiles: usize) -> usize {
    CACHE.with(|c| {
        let cache = c.borrow();
        (0..n_tiles)
            .filter(|&i| {
                cache.contains_key(&TileKey {
                    file_idx,
                    lod: LOD_BASELINE,
                    tile_idx: i,
                })
            })
            .count()
    })
}

// ── Generic LOD tile scheduling ──────────────────────────────────────────────

/// Schedule a tile at any LOD level. Computes STFT from audio samples.
/// Uses the user's chosen FFT mode (from `state.spect.fft_mode()`).
/// For single-FFT mode, the size is clamped to at least the LOD's hop size.
/// For multi-resolution mode, each band uses its own FFT size.
pub fn schedule_tile_lod(state: AppState, file_idx: usize, lod: u8, tile_idx: usize) {
    use crate::dsp::fft::compute_stft_columns;

    let key = TileKey {
        file_idx,
        lod,
        tile_idx,
    };
    if CACHE.with(|c| c.borrow().contains_key(&key)) {
        return;
    }
    if IN_FLIGHT.with(|s| has_active_in_flight(&mut s.borrow_mut(), &key)) {
        return;
    }
    if at_spawn_limit() {
        return;
    }

    // Bounds check: reject tiles that are entirely past the audio data.
    // This prevents futile async work and IN_FLIGHT entries that never resolve.
    let total_samples = state.library.files().with_untracked(|files| {
        files
            .get(file_idx)
            .map(|f| f.audio.source.total_samples() as usize)
            .unwrap_or(0)
    });
    let max_tiles = tile_count_for_samples(total_samples, lod);
    if tile_idx >= max_tiles {
        return;
    }

    IN_FLIGHT.with(|s| s.borrow_mut().insert(key, js_sys::Date::now()));

    // Capture generation so we can discard the result if a clear happened mid-compute
    let gen = CACHE_GENERATION.with(|g| *g.borrow());

    let config_hop = LOD_CONFIGS[lod as usize].hop_size;
    let fft_mode = state.spect.fft_mode().get_untracked();
    let actual_fft = fft_mode.fft_for_lod(lod);

    spawn_local(async move {
        yield_to_browser().await;

        if !magnitude_request_still_active(&key) {
            return;
        }

        // Extra yields for expensive LODs (LOD3+) and non-current files
        if lod >= 3 {
            yield_to_browser().await;
            if !magnitude_request_still_active(&key) {
                return;
            }
        }
        let is_current = is_current_file(&state, file_idx);
        if !is_current {
            for _ in 0..3 {
                yield_to_browser().await;
                if !magnitude_request_still_active(&key) {
                    return;
                }
            }
        }

        let audio = state
            .library
            .files()
            .with_untracked(|files| files.get(file_idx).map(|f| f.audio.clone()));
        let Some(audio) = audio else {
            IN_FLIGHT.with(|s| s.borrow_mut().remove(&key));
            return;
        };

        // Compute STFT columns for this tile using channel-aware samples
        let cv = state.viewmode.channel_view().get_untracked();
        let col_start = tile_idx * TILE_COLS;

        // Read only the sample region needed for this tile
        let sample_start = col_start * config_hop;
        let sample_len = TILE_COLS * config_hop + actual_fft;

        // When xform is active with a DSP mode that has per-chunk edge artifacts
        // (phase vocoder, pitch shift), read extra pre-padding samples so the
        // transform's onset fade/warmup falls on discarded samples rather than
        // visible tile content.
        let xform_on = state.display.transform().get_untracked();
        let needs_padding = xform_on
            && matches!(
                state.playback.mode().get_untracked(),
                PlaybackMode::PhaseVocoder | PlaybackMode::PitchShift | PlaybackMode::TimeExpansion
            );
        // Pad by PV_HQ_OVERLAP to ensure complete overlap-add warmup
        // before the tile's actual samples begin.
        let xform_pad = if needs_padding { PV_HQ_OVERLAP } else { 0 };
        let padded_start = sample_start.saturating_sub(xform_pad);
        let pre_pad_used = sample_start - padded_start;
        let padded_len = pre_pad_used + sample_len;

        // Prefetch for streaming sources
        let (did_seek, is_vbr) = streaming_source::prefetch_streaming(
            audio.source.as_ref(),
            padded_start as u64,
            padded_len,
        )
        .await;
        if did_seek {
            if is_vbr {
                state.show_info_toast("VBR MP3: seek position may be approximate");
            } else {
                state.show_info_toast("Seeking in streaming MP3");
            }
        }

        if !magnitude_request_still_active(&key) {
            return;
        }

        let raw_samples = audio
            .source
            .read_region(cv, padded_start as u64, padded_len);

        // Apply DSP transform (heterodyne, pitch shift, etc.) when display_transform is active
        let samples = if xform_on {
            let transformed = apply_display_transform(&raw_samples, audio.sample_rate, state);
            // Trim pre-padding so only the tile's actual transformed samples remain
            if pre_pad_used > 0 && transformed.len() > pre_pad_used {
                transformed[pre_pad_used..].to_vec()
            } else {
                transformed
            }
        } else {
            raw_samples
        };

        // Apply decimation if active — produces fewer samples, so STFT yields fewer columns per tile
        let decim_target = state.display.decimate_effective().get_untracked();
        let (samples, effective_rate) = if decim_target > 0 && decim_target < audio.sample_rate {
            let decimated =
                crate::dsp::filters::decimate(&samples, audio.sample_rate, decim_target);
            let rate = crate::dsp::filters::decimated_rate(audio.sample_rate, decim_target);
            (decimated, rate)
        } else {
            (samples, audio.sample_rate)
        };

        // For coarse LODs (hop > baseline), use quality downscaling:
        // compute STFT at baseline resolution and aggregate via per-bin max.
        let cols = if config_hop > BASELINE_HOP {
            let oversample = config_hop / BASELINE_HOP;
            let compute_fft = fft_mode.fft_for_lod(LOD_BASELINE);
            let dense_cols = compute_stft_columns(
                &samples,
                effective_rate,
                compute_fft,
                BASELINE_HOP,
                0,
                TILE_COLS * oversample,
            );
            aggregate_columns_max(&dense_cols, oversample)
        } else {
            compute_stft_columns(
                &samples,
                effective_rate,
                actual_fft,
                config_hop,
                0,
                TILE_COLS,
            )
        };
        IN_FLIGHT.with(|s| s.borrow_mut().remove(&key));

        // Discard result if the cache was cleared while we were computing
        // (e.g. user toggled xform mode — these tiles are stale)
        let current_gen = CACHE_GENERATION.with(|g| *g.borrow());
        if current_gen != gen {
            state
                .viewmode
                .tile_ready_signal()
                .update(|n| *n = n.wrapping_add(1));
            return;
        }

        if cols.is_empty() {
            // Still bump the signal so the render effect re-evaluates
            // (e.g. to schedule tiles at clamped positions after fast scrolling)
            state
                .viewmode
                .tile_ready_signal()
                .update(|n| *n = n.wrapping_add(1));
            return;
        }

        let rendered = spectrogram_renderer::pre_render_columns(&cols);
        CACHE.with(|c| c.borrow_mut().insert(file_idx, lod, tile_idx, rendered));
        state
            .viewmode
            .tile_ready_signal()
            .update(|n| *n = n.wrapping_add(1));
    });
}

// ── Baseline-LOD scheduling (from in-memory columns / spectral store) ───────

/// Schedule generation of a baseline-LOD tile from in-memory spectrogram columns.
/// Used during initial file loading when LoadedFile.spectrogram.columns is available.
pub fn schedule_tile(state: AppState, file: LoadedFile, file_idx: usize, tile_idx: usize) {
    let expected_fft = active_baseline_fft(state);
    if spectrogram_fft_size(&file.spectrogram) != Some(expected_fft) {
        schedule_tile_on_demand(state, file_idx, tile_idx);
        return;
    }

    let key = TileKey {
        file_idx,
        lod: LOD_BASELINE,
        tile_idx,
    };
    if CACHE.with(|c| c.borrow().contains_key(&key)) {
        return;
    }
    if IN_FLIGHT.with(|s| has_active_in_flight(&mut s.borrow_mut(), &key)) {
        return;
    }
    IN_FLIGHT.with(|s| s.borrow_mut().insert(key, js_sys::Date::now()));

    let gen = CACHE_GENERATION.with(|g| *g.borrow());

    spawn_local(async move {
        yield_to_browser().await;

        if !magnitude_request_still_active(&key) {
            return;
        }

        let is_current = is_current_file(&state, file_idx);
        if !is_current {
            for _ in 0..3 {
                yield_to_browser().await;
                if !magnitude_request_still_active(&key) {
                    return;
                }
            }
        }

        let still_loaded = state.library.files().with_untracked(|files| {
            files
                .get(file_idx)
                .map(|f| f.name == file.name)
                .unwrap_or(false)
        });
        if !still_loaded {
            IN_FLIGHT.with(|s| s.borrow_mut().remove(&key));
            return;
        }

        let col_start = tile_idx * TILE_COLS;
        let col_end = (col_start + TILE_COLS).min(file.spectrogram.columns.len());
        if col_start >= col_end {
            IN_FLIGHT.with(|s| s.borrow_mut().remove(&key));
            return;
        }

        let rendered =
            spectrogram_renderer::pre_render_columns(&file.spectrogram.columns[col_start..col_end]);

        let current_gen = CACHE_GENERATION.with(|g| *g.borrow());
        if current_gen != gen {
            IN_FLIGHT.with(|s| s.borrow_mut().remove(&key));
            state
                .viewmode
                .tile_ready_signal()
                .update(|n| *n = n.wrapping_add(1));
            return;
        }

        CACHE.with(|c| {
            c.borrow_mut()
                .insert(file_idx, LOD_BASELINE, tile_idx, rendered)
        });
        IN_FLIGHT.with(|s| s.borrow_mut().remove(&key));
        state
            .viewmode
            .tile_ready_signal()
            .update(|n| *n = n.wrapping_add(1));
    });
}

/// Schedule generation of all baseline-LOD tiles for a file (called after file load).
pub fn schedule_all_tiles(state: AppState, file: LoadedFile, file_idx: usize) {
    let total_cols = if file.spectrogram.total_columns > 0 {
        file.spectrogram.total_columns
    } else {
        file.spectrogram.columns.len()
    };
    if total_cols == 0 {
        return;
    }
    let n_tiles = total_cols.div_ceil(TILE_COLS);

    let tile_order = visible_tile_focus_for_file(
        &state,
        file_idx,
        total_cols,
        file.spectrogram.time_resolution,
    )
    .map(|(_, _, center_tile)| full_tile_order(n_tiles, center_tile))
    .unwrap_or_else(|| (0..n_tiles).collect());

    for tile_idx in tile_order {
        schedule_tile(state, file.clone(), file_idx, tile_idx);
    }
}

/// Render a baseline-LOD tile synchronously from the spectral column store.
pub fn render_tile_from_store_sync(file_idx: usize, tile_idx: usize, expected_fft: usize) -> bool {
    use crate::canvas::spectral_store;

    let key = TileKey {
        file_idx,
        lod: LOD_BASELINE,
        tile_idx,
    };
    if CACHE.with(|c| c.borrow().contains_key(&key)) {
        return true;
    }
    if !spectral_store::fft_matches(file_idx, expected_fft) {
        return false;
    }

    let col_start = tile_idx * TILE_COLS;
    let col_end = col_start + TILE_COLS;

    let rendered = spectral_store::with_columns(file_idx, col_start, col_end, |cols, _max_mag| {
        spectrogram_renderer::pre_render_columns(cols)
    });

    if let Some(rendered) = rendered {
        CACHE.with(|c| {
            c.borrow_mut()
                .insert(file_idx, LOD_BASELINE, tile_idx, rendered)
        });
        true
    } else {
        false
    }
}

/// Render a partial (live) baseline-LOD tile from the spectral store.
pub fn render_live_tile_sync(
    file_idx: usize,
    tile_idx: usize,
    col_start: usize,
    available_cols: usize,
    expected_fft: usize,
) -> bool {
    use crate::canvas::spectral_store;

    if !spectral_store::fft_matches(file_idx, expected_fft) {
        return false;
    }

    let col_end = col_start + available_cols;
    let rendered = spectral_store::with_columns(file_idx, col_start, col_end, |cols, _max_mag| {
        let partial = spectrogram_renderer::pre_render_columns(cols);

        if partial.width == 0 || partial.height == 0 {
            return partial;
        }

        if available_cols >= TILE_COLS {
            return partial;
        }

        let full_width = TILE_COLS as u32;
        let height = partial.height;

        if !partial.db_data.is_empty() {
            let mut full_db = vec![f32::NEG_INFINITY; (full_width * height) as usize];
            for y in 0..height {
                let src_start = (y * partial.width) as usize;
                let src_end = src_start + partial.width as usize;
                let dst_start = (y * full_width) as usize;
                let dst_end = dst_start + partial.width as usize;
                if src_end <= partial.db_data.len() {
                    full_db[dst_start..dst_end]
                        .copy_from_slice(&partial.db_data[src_start..src_end]);
                }
            }
            PreRendered {
                width: full_width,
                height,
                pixels: Vec::new(),
                db_data: full_db,
                flow_shifts: Vec::new(),
            }
        } else {
            let mut full_pixels = vec![0u8; (full_width * height * 4) as usize];
            for y in 0..height {
                let src_start = (y * partial.width * 4) as usize;
                let src_end = src_start + (partial.width * 4) as usize;
                let dst_start = (y * full_width * 4) as usize;
                let dst_end = dst_start + (partial.width * 4) as usize;
                if src_end <= partial.pixels.len() {
                    full_pixels[dst_start..dst_end]
                        .copy_from_slice(&partial.pixels[src_start..src_end]);
                }
            }
            PreRendered {
                width: full_width,
                height,
                pixels: full_pixels,
                db_data: Vec::new(),
                flow_shifts: Vec::new(),
            }
        }
    });

    if let Some(rendered) = rendered {
        CACHE.with(|c| {
            c.borrow_mut()
                .insert(file_idx, LOD_BASELINE, tile_idx, rendered)
        });
        true
    } else {
        false
    }
}

/// Schedule baseline-LOD tile generation from the spectral column store.
pub fn schedule_tile_from_store(state: AppState, file_idx: usize, tile_idx: usize) {
    use crate::canvas::spectral_store;

    let expected_fft = active_baseline_fft(state);
    if !spectral_store::fft_matches(file_idx, expected_fft) {
        schedule_tile_on_demand(state, file_idx, tile_idx);
        return;
    }

    let key = TileKey {
        file_idx,
        lod: LOD_BASELINE,
        tile_idx,
    };
    if CACHE.with(|c| c.borrow().contains_key(&key)) {
        return;
    }
    if IN_FLIGHT.with(|s| has_active_in_flight(&mut s.borrow_mut(), &key)) {
        return;
    }
    if at_spawn_limit() {
        return;
    }
    IN_FLIGHT.with(|s| s.borrow_mut().insert(key, js_sys::Date::now()));

    let gen = CACHE_GENERATION.with(|g| *g.borrow());

    spawn_local(async move {
        yield_to_browser().await;

        if !magnitude_request_still_active(&key) {
            return;
        }

        // Defer while audio is playing — playback chunk processing needs
        // uncontested CPU and I/O to avoid gaps
        if state.playback.is_playing().get_untracked() {
            IN_FLIGHT.with(|s| s.borrow_mut().remove(&key));
            return;
        }

        let is_current = is_current_file(&state, file_idx);
        if !is_current {
            for _ in 0..3 {
                yield_to_browser().await;
                if !magnitude_request_still_active(&key) {
                    return;
                }
            }
        }

        let still_loaded = state
            .library
            .files()
            .with_untracked(|files| file_idx < files.len());
        if !still_loaded {
            IN_FLIGHT.with(|s| s.borrow_mut().remove(&key));
            return;
        }

        let col_start = tile_idx * TILE_COLS;
        let col_end = col_start + TILE_COLS;

        let rendered =
            spectral_store::with_columns(file_idx, col_start, col_end, |cols, _max_mag| {
                spectrogram_renderer::pre_render_columns(cols)
            });

        IN_FLIGHT.with(|s| s.borrow_mut().remove(&key));

        let current_gen = CACHE_GENERATION.with(|g| *g.borrow());
        if current_gen != gen {
            state
                .viewmode
                .tile_ready_signal()
                .update(|n| *n = n.wrapping_add(1));
            return;
        }

        if let Some(rendered) = rendered {
            CACHE.with(|c| {
                c.borrow_mut()
                    .insert(file_idx, LOD_BASELINE, tile_idx, rendered)
            });
        }
        // Always bump signal so render effect retries even if store had no data
        state
            .viewmode
            .tile_ready_signal()
            .update(|n| *n = n.wrapping_add(1));
    });
}

/// Schedule visible baseline-LOD tiles from the spectral store.
pub fn schedule_visible_tiles_from_store(state: AppState, file_idx: usize, total_cols: usize) {
    if total_cols == 0 {
        return;
    }
    let n_tiles = total_cols.div_ceil(TILE_COLS);

    let time_res = state.library.files().with_untracked(|files| {
        files
            .get(file_idx)
            .map(|f| f.spectrogram.time_resolution)
            .unwrap_or(0.01)
    });
    let center_tile = visible_tile_focus_for_file(&state, file_idx, total_cols, time_res)
        .map(|(_, _, center_tile)| center_tile)
        .unwrap_or(0);

    let max_schedule = 20.min(n_tiles);
    let mut scheduled = 0;
    let mut dist = 0usize;
    while scheduled < max_schedule {
        let tiles: Vec<usize> = if dist == 0 {
            vec![center_tile]
        } else {
            let mut v = Vec::new();
            if let Some(l) = center_tile.checked_sub(dist) {
                if l < n_tiles {
                    v.push(l);
                }
            }
            if center_tile + dist < n_tiles {
                v.push(center_tile + dist);
            }
            v
        };
        if tiles.is_empty() {
            break;
        }
        for t in tiles {
            schedule_tile_from_store(state, file_idx, t);
            scheduled += 1;
        }
        dist += 1;
    }
}

/// Pre-fetch tiles around a target time position and from the start of the file.
///
/// Schedules tiles covering `ahead_secs` seconds ahead of `center_time`, plus
/// `initial_secs` from the start, at the ideal LOD for the current zoom level.
///
/// The existing `IN_FLIGHT` sets prevent duplicate work with viewport scheduling.
pub fn schedule_prefetch_tiles(
    state: AppState,
    file_idx: usize,
    total_samples: usize,
    sample_rate: u32,
    center_time: f64,
    ahead_secs: f64,
    initial_secs: f64,
    zoom: f64,
    flow_algo: Option<FlowAlgo>,
    reassign: bool,
    max_prefetch: usize,
) {
    let lod = select_lod(zoom);
    let hop = LOD_CONFIGS[lod as usize].hop_size;
    let max_tiles = tile_count_for_samples(total_samples, lod);
    if max_tiles == 0 {
        return;
    }

    let time_to_tile = |t: f64| -> usize {
        let sample = (t * sample_rate as f64) as usize;
        let col = sample / hop;
        col / TILE_COLS
    };

    let mut tiles: Vec<usize> = Vec::with_capacity(max_prefetch + 10);

    // Region 1: ahead of center_time
    let center_tile = time_to_tile(center_time);
    let ahead_end = time_to_tile(center_time + ahead_secs).min(max_tiles.saturating_sub(1));
    for t in center_tile..=ahead_end {
        if tiles.len() >= max_prefetch {
            break;
        }
        tiles.push(t);
    }

    // Region 2: first initial_secs from file start
    let initial_end = time_to_tile(initial_secs).min(max_tiles.saturating_sub(1));
    for t in 0..=initial_end {
        if tiles.len() >= max_prefetch {
            break;
        }
        if !tiles.contains(&t) {
            tiles.push(t);
        }
    }

    for t in tiles {
        // Always schedule magnitude tiles (base layer / fallback)
        schedule_tile_lod(state, file_idx, lod, t);

        if let Some(algo) = flow_algo {
            schedule_flow_tile(state, file_idx, lod, t, algo);
        }

        if reassign && lod > 1 {
            schedule_reassign_tile(state, file_idx, lod, t);
        }
    }
}

/// Schedule baseline-LOD on-demand tile computation from audio samples.
pub fn schedule_tile_on_demand(state: AppState, file_idx: usize, tile_idx: usize) {
    use crate::canvas::spectral_store;
    use crate::dsp::fft::compute_stft_columns;

    let key = TileKey {
        file_idx,
        lod: LOD_BASELINE,
        tile_idx,
    };
    if CACHE.with(|c| c.borrow().contains_key(&key)) {
        return;
    }
    if IN_FLIGHT.with(|s| has_active_in_flight(&mut s.borrow_mut(), &key)) {
        return;
    }
    if at_spawn_limit() {
        return;
    }

    // Bounds check: reject tiles past the audio data
    let total_samples = state.library.files().with_untracked(|files| {
        files
            .get(file_idx)
            .map(|f| f.audio.source.total_samples() as usize)
            .unwrap_or(0)
    });
    let max_tiles = tile_count_for_samples(total_samples, LOD_BASELINE);
    if tile_idx >= max_tiles {
        return;
    }

    IN_FLIGHT.with(|s| s.borrow_mut().insert(key, js_sys::Date::now()));

    let gen = CACHE_GENERATION.with(|g| *g.borrow());

    spawn_local(async move {
        yield_to_browser().await;

        if !magnitude_request_still_active(&key) {
            return;
        }

        // Defer while audio is playing — playback needs uncontested I/O and CPU
        if state.playback.is_playing().get_untracked() {
            IN_FLIGHT.with(|s| s.borrow_mut().remove(&key));
            return;
        }

        let is_current = is_current_file(&state, file_idx);
        if !is_current {
            for _ in 0..3 {
                yield_to_browser().await;
                if !magnitude_request_still_active(&key) {
                    return;
                }
            }
        }

        let audio = state
            .library
            .files()
            .with_untracked(|files| files.get(file_idx).map(|f| f.audio.clone()));
        let Some(audio) = audio else {
            IN_FLIGHT.with(|s| s.borrow_mut().remove(&key));
            return;
        };

        let cv = state.viewmode.channel_view().get_untracked();
        let col_start = tile_idx * TILE_COLS;
        let hop_size = BASELINE_HOP;
        let fft_size = state
            .spect
            .fft_mode()
            .get_untracked()
            .fft_for_lod(LOD_BASELINE);

        // Read only the sample region needed for this tile
        let sample_start = col_start * hop_size;
        let sample_len = TILE_COLS * hop_size + fft_size;

        // Prefetch for streaming sources
        let (did_seek, is_vbr) = streaming_source::prefetch_streaming(
            audio.source.as_ref(),
            sample_start as u64,
            sample_len,
        )
        .await;
        if did_seek {
            if is_vbr {
                state.show_info_toast("VBR MP3: seek position may be approximate");
            } else {
                state.show_info_toast("Seeking in streaming MP3");
            }
        }

        if !magnitude_request_still_active(&key) {
            return;
        }

        let samples = audio
            .source
            .read_region(cv, sample_start as u64, sample_len);
        let cols = compute_stft_columns(
            &samples,
            audio.sample_rate,
            fft_size,
            hop_size,
            0,
            TILE_COLS,
        );
        if cols.is_empty() {
            IN_FLIGHT.with(|s| s.borrow_mut().remove(&key));
            // Bump signal so render effect retries (e.g. after fast scroll clamping)
            state
                .viewmode
                .tile_ready_signal()
                .update(|n| *n = n.wrapping_add(1));
            return;
        }

        spectral_store::insert_columns(file_idx, col_start, &cols);

        let rendered = spectrogram_renderer::pre_render_columns(&cols);

        let current_gen = CACHE_GENERATION.with(|g| *g.borrow());
        if current_gen != gen {
            IN_FLIGHT.with(|s| s.borrow_mut().remove(&key));
            state
                .viewmode
                .tile_ready_signal()
                .update(|n| *n = n.wrapping_add(1));
            return;
        }

        CACHE.with(|c| {
            c.borrow_mut()
                .insert(file_idx, LOD_BASELINE, tile_idx, rendered)
        });
        IN_FLIGHT.with(|s| s.borrow_mut().remove(&key));
        state
            .viewmode
            .tile_ready_signal()
            .update(|n| *n = n.wrapping_add(1));
    });
}

// ── Flow tile cache ─────────────────────────────────────────────────────────

pub fn get_flow_tile(file_idx: usize, lod: u8, tile_idx: usize) -> Option<()> {
    flow_ctx().get(file_idx, lod, tile_idx)
}

pub fn borrow_flow_tile<R>(
    file_idx: usize,
    lod: u8,
    tile_idx: usize,
    f: impl FnOnce(&Tile) -> R,
) -> Option<R> {
    flow_ctx().borrow_tile(file_idx, lod, tile_idx, f)
}

pub fn clear_flow_cache() {
    flow_ctx().clear_all();
}

pub fn clear_flow_file(file_idx: usize) {
    flow_ctx().clear_file(file_idx);
}

/// Schedule a flow tile for background generation at any LOD.
///
/// All algorithms (Optical, Centroid, Gradient, PhaseCoherence, Phase) compute
/// from raw audio using the user's FFT size and the LOD's hop size, so FFT
/// changes and LOD selection both work correctly.
pub fn schedule_flow_tile(
    state: AppState,
    file_idx: usize,
    lod: u8,
    tile_idx: usize,
    algo: FlowAlgo,
) {
    use crate::dsp::fft::compute_stft_columns;

    let key = TileKey {
        file_idx,
        lod,
        tile_idx,
    };
    if FLOW_CACHE.with(|c| c.borrow().contains_key(&key)) {
        return;
    }
    if FLOW_IN_FLIGHT.with(|s| has_active_in_flight(&mut s.borrow_mut(), &key)) {
        return;
    }
    if at_spawn_limit() {
        return;
    }

    let total_samples = state.library.files().with_untracked(|files| {
        files
            .get(file_idx)
            .map(|f| f.audio.source.total_samples() as usize)
            .unwrap_or(0)
    });
    let max_tiles = tile_count_for_samples(total_samples, lod);
    if tile_idx >= max_tiles {
        return;
    }

    FLOW_IN_FLIGHT.with(|s| s.borrow_mut().insert(key, js_sys::Date::now()));

    let gen = FLOW_CACHE_GENERATION.with(|g| *g.borrow());

    let config_hop = LOD_CONFIGS[lod as usize].hop_size;
    let actual_fft = state.spect.fft_mode().get_untracked().fft_for_lod(lod);

    spawn_local(async move {
        yield_to_browser().await;

        if !flow_request_still_active(&key) {
            return;
        }

        // Extra yields for expensive LODs (LOD3+) and non-current files
        if lod >= 3 {
            yield_to_browser().await;
            if !flow_request_still_active(&key) {
                return;
            }
        }
        let is_current = is_current_file(&state, file_idx);
        if !is_current {
            for _ in 0..3 {
                yield_to_browser().await;
                if !flow_request_still_active(&key) {
                    return;
                }
            }
        }

        let audio = state
            .library
            .files()
            .with_untracked(|files| files.get(file_idx).map(|f| f.audio.clone()));
        let Some(audio) = audio else {
            FLOW_IN_FLIGHT.with(|s| s.borrow_mut().remove(&key));
            return;
        };

        let cv = state.viewmode.channel_view().get_untracked();
        let col_start = tile_idx * TILE_COLS;

        let rendered = match algo {
            FlowAlgo::Phase | FlowAlgo::PhaseCoherence => {
                use crate::dsp::harmonics;

                let sample_start = col_start * config_hop;
                let extra = if algo == FlowAlgo::PhaseCoherence {
                    TILE_COLS + 1
                } else {
                    TILE_COLS
                };
                let sample_len = extra * config_hop + actual_fft;

                // Prefetch for streaming sources
                streaming_source::prefetch_streaming(
                    audio.source.as_ref(),
                    sample_start as u64,
                    sample_len,
                )
                .await;

                if !flow_request_still_active(&key) {
                    return;
                }

                let total = audio.source.total_samples() as usize;
                let sample_end = (sample_start + sample_len).min(total);
                if sample_start >= total || sample_start >= sample_end {
                    FLOW_IN_FLIGHT.with(|s| s.borrow_mut().remove(&key));
                    return;
                }

                let ch_samples =
                    audio
                        .source
                        .read_region(cv, sample_start as u64, sample_end - sample_start);
                let samples = &ch_samples[..];

                yield_to_browser().await;

                if !flow_request_still_active(&key) {
                    return;
                }

                if algo == FlowAlgo::Phase {
                    harmonics::compute_tile_phase_angle_data(
                        samples, TILE_COLS, actual_fft, config_hop,
                    )
                } else {
                    harmonics::compute_tile_phase_data(samples, TILE_COLS, actual_fft, config_hop)
                }
            }
            FlowAlgo::Optical | FlowAlgo::Centroid | FlowAlgo::Gradient => {
                // Read the sample region needed (including one previous column for flow diff)
                let extra_cols = if tile_idx > 0 { 1 } else { 0 };
                let region_col_start = col_start.saturating_sub(extra_cols);
                let region_sample_start = region_col_start * config_hop;
                let region_cols = TILE_COLS + extra_cols;
                let region_sample_len = region_cols * config_hop + actual_fft;

                streaming_source::prefetch_streaming(
                    audio.source.as_ref(),
                    region_sample_start as u64,
                    region_sample_len,
                )
                .await;

                if !flow_request_still_active(&key) {
                    return;
                }

                let region_samples =
                    audio
                        .source
                        .read_region(cv, region_sample_start as u64, region_sample_len);

                let prev_col = if extra_cols > 0 {
                    let prev_cols = compute_stft_columns(
                        &region_samples,
                        audio.sample_rate,
                        actual_fft,
                        config_hop,
                        0,
                        1,
                    );
                    prev_cols.first().map(|c| c.magnitudes.clone())
                } else {
                    None
                };

                yield_to_browser().await;

                if !flow_request_still_active(&key) {
                    return;
                }

                let cols = compute_stft_columns(
                    &region_samples,
                    audio.sample_rate,
                    actual_fft,
                    config_hop,
                    extra_cols,
                    TILE_COLS,
                );
                if cols.is_empty() {
                    FLOW_IN_FLIGHT.with(|s| s.borrow_mut().remove(&key));
                    return;
                }

                spectrogram_renderer::pre_render_flow_columns(&cols, prev_col.as_deref(), algo)
            }
        };

        let current_gen = FLOW_CACHE_GENERATION.with(|g| *g.borrow());
        if current_gen != gen {
            FLOW_IN_FLIGHT.with(|s| s.borrow_mut().remove(&key));
            state
                .viewmode
                .tile_ready_signal()
                .update(|n| *n = n.wrapping_add(1));
            return;
        }

        FLOW_CACHE.with(|c| c.borrow_mut().insert(file_idx, lod, tile_idx, rendered));
        FLOW_IN_FLIGHT.with(|s| s.borrow_mut().remove(&key));
        state
            .viewmode
            .tile_ready_signal()
            .update(|n| *n = n.wrapping_add(1));
    });
}

// ── Reassignment spectrogram tile cache ──────────────────────────────────────

pub fn get_reassign_tile(file_idx: usize, lod: u8, tile_idx: usize) -> Option<()> {
    reassign_ctx().get(file_idx, lod, tile_idx)
}

pub fn borrow_reassign_tile<R>(
    file_idx: usize,
    lod: u8,
    tile_idx: usize,
    f: impl FnOnce(&Tile) -> R,
) -> Option<R> {
    reassign_ctx().borrow_tile(file_idx, lod, tile_idx, f)
}

pub fn clear_reassign_cache() {
    reassign_ctx().clear_all();
}

pub fn clear_reassign_file(file_idx: usize) {
    reassign_ctx().clear_file(file_idx);
}

/// Schedule a reassignment spectrogram tile for background generation.
///
/// Performs 3 FFTs per frame (standard, time-ramped, derivative-windowed) to
/// compute corrected time-frequency positions, producing sharper spectrograms.
pub fn schedule_reassign_tile(state: AppState, file_idx: usize, lod: u8, tile_idx: usize) {
    use crate::dsp::fft::compute_reassigned_tile;

    let key = TileKey {
        file_idx,
        lod,
        tile_idx,
    };
    if REASSIGN_CACHE.with(|c| c.borrow().contains_key(&key)) {
        return;
    }
    if REASSIGN_IN_FLIGHT.with(|s| has_active_in_flight(&mut s.borrow_mut(), &key)) {
        return;
    }
    if at_spawn_limit() {
        return;
    }

    let total_samples = state.library.files().with_untracked(|files| {
        files
            .get(file_idx)
            .map(|f| f.audio.source.total_samples() as usize)
            .unwrap_or(0)
    });
    let max_tiles = tile_count_for_samples(total_samples, lod);
    if tile_idx >= max_tiles {
        return;
    }

    REASSIGN_IN_FLIGHT.with(|s| s.borrow_mut().insert(key, js_sys::Date::now()));

    let gen = REASSIGN_CACHE_GENERATION.with(|g| *g.borrow());

    let config_hop = LOD_CONFIGS[lod as usize].hop_size;
    let actual_fft = state.spect.fft_mode().get_untracked().fft_for_lod(lod);

    spawn_local(async move {
        yield_to_browser().await;

        if !reassign_request_still_active(&key) {
            return;
        }

        // Extra yields: 3x FFT cost + expensive LODs (LOD3+)
        if lod >= 3 {
            yield_to_browser().await;
            if !reassign_request_still_active(&key) {
                return;
            }
        }
        yield_to_browser().await;

        if !reassign_request_still_active(&key) {
            return;
        }

        let is_current = is_current_file(&state, file_idx);
        if !is_current {
            for _ in 0..3 {
                yield_to_browser().await;
                if !reassign_request_still_active(&key) {
                    return;
                }
            }
        }

        let audio = state
            .library
            .files()
            .with_untracked(|files| files.get(file_idx).map(|f| f.audio.clone()));
        let Some(audio) = audio else {
            REASSIGN_IN_FLIGHT.with(|s| s.borrow_mut().remove(&key));
            return;
        };

        let cv = state.viewmode.channel_view().get_untracked();
        let sample_start = tile_idx * TILE_COLS * config_hop;
        let sample_len = TILE_COLS * config_hop + actual_fft;

        // Prefetch for streaming sources
        streaming_source::prefetch_streaming(
            audio.source.as_ref(),
            sample_start as u64,
            sample_len,
        )
        .await;

        if !reassign_request_still_active(&key) {
            return;
        }

        let total = audio.source.total_samples() as usize;
        let sample_end = (sample_start + sample_len).min(total);
        if sample_start >= total || sample_start >= sample_end {
            REASSIGN_IN_FLIGHT.with(|s| s.borrow_mut().remove(&key));
            return;
        }

        let ch_samples =
            audio
                .source
                .read_region(cv, sample_start as u64, sample_end - sample_start);
        let samples = &ch_samples[..];

        yield_to_browser().await;

        if !reassign_request_still_active(&key) {
            return;
        }

        let rendered = compute_reassigned_tile(samples, TILE_COLS, actual_fft, config_hop, -60.0);

        let current_gen = REASSIGN_CACHE_GENERATION.with(|g| *g.borrow());
        if current_gen != gen {
            REASSIGN_IN_FLIGHT.with(|s| s.borrow_mut().remove(&key));
            state
                .viewmode
                .tile_ready_signal()
                .update(|n| *n = n.wrapping_add(1));
            return;
        }

        REASSIGN_CACHE.with(|c| c.borrow_mut().insert(file_idx, lod, tile_idx, rendered));
        REASSIGN_IN_FLIGHT.with(|s| s.borrow_mut().remove(&key));
        state
            .viewmode
            .tile_ready_signal()
            .update(|n| *n = n.wrapping_add(1));
    });
}

// ── Chromagram tile cache (baseline-LOD only) ───────────────────────────────

pub fn get_chroma_tile(file_idx: usize, tile_idx: usize) -> Option<()> {
    CHROMA_CACHE.with(|c| c.borrow().get(file_idx, tile_idx).map(|_| ()))
}

pub fn borrow_chroma_tile<R>(
    file_idx: usize,
    tile_idx: usize,
    f: impl FnOnce(&Tile) -> R,
) -> Option<R> {
    CHROMA_CACHE.with(|c| {
        let mut cache = c.borrow_mut();
        let key = ChromaKey { file_idx, tile_idx };
        if cache.contains_key(&key) {
            cache.touch(key);
            drop(cache);
            CHROMA_CACHE.with(|c| c.borrow().get_keyed(&key).map(f))
        } else {
            None
        }
    })
}

pub fn clear_chroma_cache() {
    CHROMA_CACHE.with(|c| c.borrow_mut().clear_all());
    CHROMA_IN_FLIGHT.with(|s| s.borrow_mut().clear());
    CHROMA_GLOBAL_MAX.with(|m| m.borrow_mut().clear());
}

/// Clear rendered chroma tiles for a single file while preserving the
/// running global-max cache. Used by the resonator chroma path when a
/// freshly-computed tile raises the global max enough that previously
/// rendered (over-bright) tiles need to be redone with the new max.
/// In-flight requests are left alone — they'll overwrite when they finish
/// and any still-stale tiles will be caught by the next growth event.
fn clear_chroma_tiles_for_file_keep_max(file_idx: usize) {
    CHROMA_CACHE.with(|c| c.borrow_mut().clear_keys(|k| k.file_idx == file_idx));
}

/// Schedule a chromagram tile for background generation (baseline LOD).
///
/// The compute source is selected by `state.chroma.source()`:
/// - [`ChromaSource::Fft`] re-bins linear STFT magnitudes into notes,
///   reusing cached STFT columns from `spectral_store` when available.
/// - [`ChromaSource::Resonators`] (default) builds a constant-Q resonator
///   bank with one resonator per note and reads its magnitudes directly,
///   giving uniform pitch selectivity from sub-bass to ultrasound.
pub fn schedule_chroma_tile(state: AppState, file_idx: usize, tile_idx: usize) {
    use crate::state::ChromaSource;

    let key = ChromaKey { file_idx, tile_idx };
    if CHROMA_CACHE.with(|c| c.borrow().contains_key(&key)) {
        return;
    }
    if CHROMA_IN_FLIGHT.with(|s| has_active_in_flight(&mut s.borrow_mut(), &key)) {
        return;
    }
    if at_spawn_limit() {
        return;
    }
    CHROMA_IN_FLIGHT.with(|s| s.borrow_mut().insert(key, js_sys::Date::now()));

    match state.chroma.source().get_untracked() {
        ChromaSource::Fft => schedule_chroma_tile_fft(state, file_idx, tile_idx, key),
        ChromaSource::Resonators => schedule_chroma_tile_resonator(state, file_idx, tile_idx, key),
    }
}

/// FFT-sourced chromagram tile: re-bins linear STFT magnitudes into notes.
/// Reuses `spectral_store` columns when present, falls back to file-embedded
/// columns, then to on-demand STFT computation from audio samples.
fn schedule_chroma_tile_fft(state: AppState, file_idx: usize, tile_idx: usize, key: ChromaKey) {
    use crate::canvas::spectral_store;
    use crate::dsp::chromagram;
    use crate::dsp::fft::compute_stft_columns;

    spawn_local(async move {
        yield_to_browser().await;

        let is_current = is_current_file(&state, file_idx);
        if !is_current {
            for _ in 0..3 {
                yield_to_browser().await;
            }
        }

        let still_loaded = state
            .library
            .files()
            .with_untracked(|files| file_idx < files.len());
        if !still_loaded {
            CHROMA_IN_FLIGHT.with(|s| s.borrow_mut().remove(&key));
            return;
        }

        let col_start = tile_idx * TILE_COLS;

        let freq_res = state
            .library
            .files()
            .with_untracked(|files| files.get(file_idx).map(|f| f.spectrogram.freq_resolution))
            .unwrap_or(1.0);

        let (min_octave, num_octaves) = state.chroma.range().get_untracked().octave_params();
        let gain_db = state.chroma.gain().get_untracked();
        let adapt = state.chroma.adapt().get_untracked();
        let floor_db = state.chroma.floor_db().get_untracked();

        // Try spectral_store first, then file columns, then compute on-demand from audio
        // This path needs OWNED columns (used later, outside the store borrow),
        // so clone here — unlike the immediate-use tile renders above.
        let cols_from_store =
            spectral_store::with_columns(file_idx, col_start, col_start + TILE_COLS, |cols, _| {
                cols.iter()
                    .map(|&c| c.clone())
                    .collect::<Vec<crate::types::SpectrogramColumn>>()
            });
        let cols_from_file = if cols_from_store.is_some() {
            None
        } else {
            state.library.files().with_untracked(|files| {
                files.get(file_idx).and_then(|f| {
                    if f.spectrogram.columns.is_empty() {
                        return None;
                    }
                    let end = (col_start + TILE_COLS).min(f.spectrogram.columns.len());
                    if col_start >= end {
                        return None;
                    }
                    Some(f.spectrogram.columns[col_start..end].to_vec())
                })
            })
        };

        let stft_cols = if let Some(c) = cols_from_store {
            c
        } else if let Some(c) = cols_from_file {
            c
        } else {
            // On-demand: compute STFT from audio samples (same as schedule_tile_on_demand)
            let audio = state
                .library
                .files()
                .with_untracked(|files| files.get(file_idx).map(|f| f.audio.clone()));
            let Some(audio) = audio else {
                CHROMA_IN_FLIGHT.with(|s| s.borrow_mut().remove(&key));
                return;
            };

            let cv = state.viewmode.channel_view().get_untracked();
            let hop_size = BASELINE_HOP;
            let fft_size = active_baseline_fft(state);
            let sample_start = col_start * hop_size;
            let sample_len = TILE_COLS * hop_size + fft_size;

            streaming_source::prefetch_streaming(
                audio.source.as_ref(),
                sample_start as u64,
                sample_len,
            )
            .await;

            let samples = audio
                .source
                .read_region(cv, sample_start as u64, sample_len);
            let cols = compute_stft_columns(
                &samples,
                audio.sample_rate,
                fft_size,
                hop_size,
                0,
                TILE_COLS,
            );
            if cols.is_empty() {
                CHROMA_IN_FLIGHT.with(|s| s.borrow_mut().remove(&key));
                return;
            }

            // Cache in spectral_store for future use
            spectral_store::insert_columns(file_idx, col_start, &cols);
            cols
        };

        // Compute global normalization max (needed for consistent brightness)
        let global_max = CHROMA_GLOBAL_MAX.with(|m| m.borrow().get(&file_idx).copied());
        let (max_class, max_note) = if let Some(gm) = global_max {
            gm
        } else {
            let from_store = spectral_store::compute_chroma_global_max(
                file_idx,
                freq_res,
                min_octave,
                num_octaves,
            );
            let gm = from_store.unwrap_or_else(|| {
                chromagram::compute_chroma_max(&stft_cols, freq_res, min_octave, num_octaves)
            });
            if gm.0 > 0.0 {
                CHROMA_GLOBAL_MAX.with(|m| m.borrow_mut().insert(file_idx, gm));
            }
            gm
        };

        let rendered = chromagram::pre_render_chromagram_columns(
            &stft_cols,
            freq_res,
            max_class,
            max_note,
            min_octave,
            num_octaves,
            gain_db,
            adapt,
            floor_db,
        );

        CHROMA_CACHE.with(|c| c.borrow_mut().insert(file_idx, tile_idx, rendered));
        CHROMA_IN_FLIGHT.with(|s| s.borrow_mut().remove(&key));
        state
            .viewmode
            .tile_ready_signal()
            .update(|n| *n = n.wrapping_add(1));
    });
}

/// Resonator-sourced chromagram tile: one constant-Q resonator per note.
///
/// Pre-pads with warm-up samples derived from the lowest in-range note's
/// bandwidth so the EMA has converged by the first emitted column (same
/// pattern as [`schedule_resonator_tile`]). Warm-up is capped at 0.25 s of
/// audio so the deepest octaves don't make every tile prohibitively
/// expensive — they remain usable, just not fully converged.
///
/// The global normalisation max is tracked progressively: each new tile may
/// raise it, and a large jump (≥1.5×) invalidates previously rendered tiles
/// so they get re-normalised to the new ceiling. Convergence is fast because
/// the max only grows.
fn schedule_chroma_tile_resonator(
    state: AppState,
    file_idx: usize,
    tile_idx: usize,
    key: ChromaKey,
) {
    use crate::dsp::chromagram::{
        self, compute_chroma_columns_resonators, compute_chroma_max_from_columns,
        CHROMA_RESONATOR_Q, CHROMA_RESONATOR_TAU_MAX_SECS,
    };
    use crate::dsp::resonators::warmup_samples;

    spawn_local(async move {
        yield_to_browser().await;

        let is_current = is_current_file(&state, file_idx);
        if !is_current {
            for _ in 0..3 {
                yield_to_browser().await;
            }
        }

        let audio = state
            .library
            .files()
            .with_untracked(|files| files.get(file_idx).map(|f| f.audio.clone()));
        let Some(audio) = audio else {
            CHROMA_IN_FLIGHT.with(|s| s.borrow_mut().remove(&key));
            return;
        };

        let (min_octave, num_octaves) = state.chroma.range().get_untracked().octave_params();
        let gain_db = state.chroma.gain().get_untracked();
        let adapt = state.chroma.adapt().get_untracked();
        let floor_db = state.chroma.floor_db().get_untracked();
        let cv = state.viewmode.channel_view().get_untracked();
        let sr = audio.sample_rate;
        let hop_size = BASELINE_HOP;

        // Warm-up: dominated by the slowest resonator in the bank, which is
        // the lowest in-range note — but its bandwidth is floored by the τ
        // cap in `compute_chroma_columns_resonators`, so warm-up follows the
        // floored bandwidth (not the unbounded constant-Q value). This keeps
        // every tile a sane size regardless of how low the range goes.
        let low_note_freq = {
            let midi = ((min_octave + 1) * 12) as f32; // pitch class 0 = C
            440.0_f32 * 2.0_f32.powf((midi - 69.0) / 12.0)
        }
        .max(1.0);
        let bw_floor = 1.0 / (std::f32::consts::TAU * CHROMA_RESONATOR_TAU_MAX_SECS);
        let low_bw = (low_note_freq / CHROMA_RESONATOR_Q).max(bw_floor);
        let warmup = warmup_samples(sr, low_bw);
        let warmup_cols = warmup.div_ceil(hop_size);
        let warmup_samples_aligned = warmup_cols * hop_size;

        let tile_sample_start = tile_idx * TILE_COLS * hop_size;
        let padded_start = tile_sample_start.saturating_sub(warmup_samples_aligned);
        let pre_pad_cols = (tile_sample_start - padded_start) / hop_size;
        let padded_len = (pre_pad_cols + TILE_COLS) * hop_size;

        streaming_source::prefetch_streaming(
            audio.source.as_ref(),
            padded_start as u64,
            padded_len,
        )
        .await;

        let still_loaded = state
            .library
            .files()
            .with_untracked(|files| file_idx < files.len());
        if !still_loaded {
            CHROMA_IN_FLIGHT.with(|s| s.borrow_mut().remove(&key));
            return;
        }

        let samples = audio
            .source
            .read_region(cv, padded_start as u64, padded_len);
        if samples.is_empty() {
            CHROMA_IN_FLIGHT.with(|s| s.borrow_mut().remove(&key));
            return;
        }

        yield_to_browser().await;

        let chromas = compute_chroma_columns_resonators(
            &samples,
            sr,
            hop_size,
            pre_pad_cols,
            TILE_COLS,
            min_octave,
            num_octaves,
            CHROMA_RESONATOR_Q,
        );

        if chromas.is_empty() {
            CHROMA_IN_FLIGHT.with(|s| s.borrow_mut().remove(&key));
            return;
        }

        // Progressive global max: merge this tile's local max into the
        // per-file running max. The tile is always rendered against the
        // most-recent (largest) max so it's never over-bright; if the
        // running max jumped notably, evict earlier (over-bright) tiles
        // so they get redone with the new ceiling.
        let local = compute_chroma_max_from_columns(&chromas, min_octave, num_octaves);
        let prev = CHROMA_GLOBAL_MAX.with(|m| m.borrow().get(&file_idx).copied());
        let merged_class = prev.map(|p| p.0.max(local.0)).unwrap_or(local.0);
        let merged_note = prev.map(|p| p.1.max(local.1)).unwrap_or(local.1);
        let merged = (merged_class, merged_note);

        let grew_significantly = match prev {
            None => merged_class > 0.0,
            Some((pc, pn)) => {
                let pc = pc.max(f32::MIN_POSITIVE);
                let pn = pn.max(f32::MIN_POSITIVE);
                (merged_class / pc) >= 1.5 || (merged_note / pn) >= 1.5
            }
        };

        if merged_class > 0.0 {
            CHROMA_GLOBAL_MAX.with(|m| m.borrow_mut().insert(file_idx, merged));
        }
        if grew_significantly && prev.is_some() {
            clear_chroma_tiles_for_file_keep_max(file_idx);
        }

        let rendered = chromagram::pre_render_chroma_from_columns(
            &chromas,
            merged.0,
            merged.1,
            min_octave,
            num_octaves,
            gain_db,
            adapt,
            floor_db,
        );

        CHROMA_CACHE.with(|c| c.borrow_mut().insert(file_idx, tile_idx, rendered));
        CHROMA_IN_FLIGHT.with(|s| s.borrow_mut().remove(&key));
        state
            .viewmode
            .tile_ready_signal()
            .update(|n| *n = n.wrapping_add(1));
    });
}

// ── Background preload ───────────────────────────────────────────────────────

struct BgPreloadState {
    file_idx: usize,
    lod: u8,
    center_tile: usize,
    max_tiles: usize,
    next_distance: usize,
    generation: u32,
    batch_timer: Option<i32>,
}

thread_local! {
    static BG_PRELOAD: RefCell<Option<BgPreloadState>> = const { RefCell::new(None) };
}

/// Start (or restart) background preloading of tiles at the given LOD,
/// expanding outward from `center_tile`. Tiles are scheduled in small batches
/// with 50ms delays to avoid blocking the UI. Stops at 90% cache capacity.
/// The `generation` counter is checked each step to cancel stale jobs.
pub fn start_background_preload(
    state: AppState,
    file_idx: usize,
    lod: u8,
    center_tile: usize,
    max_tiles: usize,
    generation: u32,
) {
    // Cancel any existing preload
    BG_PRELOAD.with(|bg| {
        let mut bg = bg.borrow_mut();
        if let Some(old) = bg.as_ref() {
            if let Some(h) = old.batch_timer {
                web_sys::window().unwrap().clear_timeout_with_handle(h);
            }
        }
        *bg = Some(BgPreloadState {
            file_idx,
            lod,
            center_tile,
            max_tiles,
            next_distance: 0,
            generation,
            batch_timer: None,
        });
    });

    schedule_preload_batch(state, generation);
}

/// Stop any in-progress background preload.
pub fn stop_background_preload() {
    BG_PRELOAD.with(|bg| {
        let mut bg = bg.borrow_mut();
        if let Some(ref s) = *bg {
            if let Some(h) = s.batch_timer {
                web_sys::window().unwrap().clear_timeout_with_handle(h);
            }
        }
        *bg = None;
    });
}

fn schedule_preload_batch(state: AppState, generation: u32) {
    let cb = Closure::once(move || {
        run_preload_batch(state, generation);
    });
    let h = web_sys::window()
        .unwrap()
        .set_timeout_with_callback_and_timeout_and_arguments_0(
            cb.as_ref().unchecked_ref(),
            50, // 50ms between batches (low priority)
        )
        .unwrap_or(0);
    cb.forget();

    BG_PRELOAD.with(|bg| {
        if let Some(ref mut s) = *bg.borrow_mut() {
            if s.generation == generation {
                s.batch_timer = Some(h);
            }
        }
    });
}

fn run_preload_batch(state: AppState, generation: u32) {
    // Check generation (cancel if stale)
    let current_gen = state.viewmode.bg_preload_gen().get_untracked();
    if current_gen != generation {
        return;
    }

    let batch = BG_PRELOAD.with(|bg| {
        let mut bg = bg.borrow_mut();
        let s = match bg.as_mut() {
            Some(s) if s.generation == generation => s,
            _ => return None,
        };

        // Check if cache is near capacity (stop at 90%)
        let cache_full = CACHE.with(|c| {
            let cache = c.borrow();
            cache.total_bytes() >= cache.max_bytes() / 10 * 9
        });
        if cache_full {
            return None;
        }

        let mut tiles_to_schedule = Vec::new();
        let batch_size = 4;

        while tiles_to_schedule.len() < batch_size {
            if s.next_distance > s.max_tiles {
                return None; // all distances covered
            }

            let dist = s.next_distance;
            s.next_distance += 1;

            // Expand outward from center
            let candidates = if dist == 0 {
                vec![s.center_tile]
            } else {
                let mut v = Vec::new();
                if s.center_tile + dist < s.max_tiles {
                    v.push(s.center_tile + dist);
                }
                if let Some(idx) = s.center_tile.checked_sub(dist) {
                    if idx < s.max_tiles {
                        v.push(idx);
                    }
                }
                v
            };

            for t in candidates {
                let key = TileKey {
                    file_idx: s.file_idx,
                    lod: s.lod,
                    tile_idx: t,
                };
                if CACHE.with(|c| c.borrow().contains_key(&key)) {
                    continue; // already cached
                }
                tiles_to_schedule.push(key);
            }
        }

        Some(tiles_to_schedule)
    });

    if let Some(tiles) = batch {
        if tiles.is_empty() {
            // All tiles in this batch were already cached; keep going
            schedule_preload_batch(state, generation);
            return;
        }
        for &k in &tiles {
            schedule_tile_lod(state, k.file_idx, k.lod, k.tile_idx);
        }
        // Schedule next batch
        schedule_preload_batch(state, generation);
    }
    // else: cache full or all tiles done — preload stops naturally
}

// ── Internal helpers ─────────────────────────────────────────────────────────

/// Yield once to the browser event loop via a zero-duration setTimeout.
pub async fn yield_to_browser() {
    // On wasm, yield a macrotask so the browser can paint / handle input.
    // Off-wasm (host tests of the streaming sources) there is nothing to yield
    // to, so this is a no-op.
    #[cfg(target_arch = "wasm32")]
    {
        let promise = js_sys::Promise::new(&mut |resolve, _reject| {
            let win = web_sys::window().unwrap();
            let cb = Closure::once_into_js(move || {
                let _ = resolve.call0(&JsValue::NULL);
            });
            let _ =
                win.set_timeout_with_callback_and_timeout_and_arguments_0(cb.unchecked_ref(), 0);
        });
        let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
    }
}

/// Apply the current playback DSP mode transform to samples for display.
/// Used when display_transform is true (Transform "Same" mode).
fn apply_display_transform(samples: &[f32], sample_rate: u32, state: AppState) -> Vec<f32> {
    let mode = state.playback.mode().get_untracked();
    match mode {
        PlaybackMode::Normal => samples.to_vec(),
        PlaybackMode::TimeExpansion => {
            let factor = crate::dsp::pitch_shift::PitchFactor::from_signed(
                state.transform.te_factor().get_untracked(),
            );
            if factor.is_bypass() {
                samples.to_vec()
            } else {
                crate::dsp::pitch_shift::pitch_shift_realtime(samples, factor)
            }
        }
        PlaybackMode::Heterodyne => {
            let lo = state.transform.het_frequency().get_untracked();
            let cutoff = state.transform.het_cutoff().get_untracked();
            crate::dsp::heterodyne::heterodyne_mix(samples, sample_rate, lo, cutoff)
        }
        PlaybackMode::PitchShift => {
            let factor = state.transform.ps_factor().get_untracked();
            crate::dsp::pitch_shift::pitch_shift_realtime(samples, factor)
        }
        PlaybackMode::PhaseVocoder => {
            let factor = state.transform.pv_factor().get_untracked();
            crate::dsp::phase_vocoder::phase_vocoder_pitch_shift(samples, factor)
        }
        PlaybackMode::ZeroCrossing => {
            let factor = state.transform.zc_factor().get_untracked() as u32;
            crate::dsp::zc_divide::zc_divide(samples, sample_rate, factor, false)
        }
    }
}

// ── Resonator tile cache ─────────────────────────────────────────────────────

pub fn get_resonator_tile(file_idx: usize, lod: u8, tile_idx: usize) -> Option<()> {
    resonator_ctx().get(file_idx, lod, tile_idx)
}

pub fn borrow_resonator_tile<R>(
    file_idx: usize,
    lod: u8,
    tile_idx: usize,
    f: impl FnOnce(&Tile) -> R,
) -> Option<R> {
    resonator_ctx().borrow_tile(file_idx, lod, tile_idx, f)
}

pub fn clear_resonator_cache() {
    resonator_ctx().clear_all();
    // Offscreen tile canvases are keyed by (file_idx, lod, tile_idx) without
    // an underlying-tile-dimension component, so resizing bin counts would
    // otherwise reuse old canvases (rendered at the previous height) and draw
    // them squished into the new destination rect.
    crate::canvas::spectrogram_renderer::clear_tile_canvas_cache();
}

pub fn clear_resonator_file(file_idx: usize) {
    resonator_ctx().clear_file(file_idx);
}

pub fn resonator_tile_active(file_idx: usize, lod: u8, tile_idx: usize) -> bool {
    resonator_ctx().tile_active(file_idx, lod, tile_idx)
}

pub fn resonator_debug_stats(
    file_idx: usize,
    lod: u8,
    first_tile: usize,
    last_tile: usize,
) -> TileDebugStats {
    resonator_ctx().debug_stats(file_idx, lod, first_tile, last_tile)
}

/// Schedule a resonator tile for background generation at any LOD.
///
/// Produces a spectral tile using the Resonate algorithm instead of STFT.
/// Output layout is identical to magnitude tiles (linear frequency axis,
/// `num_bins = fft_size/2+1`), so standard dB-blit / colormap code applies.
///
/// The algorithm is stateful, so this reads extra pre-padding samples to let
/// the resonator EMA converge before the tile's first column.
pub fn schedule_resonator_tile(state: AppState, file_idx: usize, lod: u8, tile_idx: usize) {
    use crate::dsp::resonators::{compute_resonator_hybrid_columns, warmup_samples};

    let key = TileKey {
        file_idx,
        lod,
        tile_idx,
    };
    if RESONATOR_CACHE.with(|c| c.borrow().contains_key(&key)) {
        return;
    }
    if RESONATOR_IN_FLIGHT.with(|s| has_active_in_flight(&mut s.borrow_mut(), &key)) {
        return;
    }
    if at_spawn_limit() {
        return;
    }

    let total_samples = state.library.files().with_untracked(|files| {
        files
            .get(file_idx)
            .map(|f| f.audio.source.total_samples() as usize)
            .unwrap_or(0)
    });
    let max_tiles = tile_count_for_samples(total_samples, lod);
    if tile_idx >= max_tiles {
        return;
    }

    RESONATOR_IN_FLIGHT.with(|s| s.borrow_mut().insert(key, js_sys::Date::now()));

    let gen = RESONATOR_CACHE_GENERATION.with(|g| *g.borrow());

    let config_hop = LOD_CONFIGS[lod as usize].hop_size;
    let reson_fft = state
        .resonator
        .fft_mode()
        .get_untracked()
        .fft_for_lod(lod)
        .max(16);
    let bandwidth_hz = state.resonator.bandwidth_hz().get_untracked().max(1.0);
    let alpha_mode = state.resonator.alpha_mode().get_untracked();
    let q = state.resonator.q().get_untracked();
    let hybrid_mode = state.resonator.hybrid_mode().get_untracked();
    let layout = state.resonator.layout().get_untracked();
    // Viewport-zoom mode: concentrate bins on the committed viewport range
    // (the debouncer in tile_scheduler updates this signal). `None` keeps
    // the default full-Nyquist (or full-log) range.
    let freq_range = state
        .resonator
        .viewport_range()
        .get_untracked()
        .map(|(lo, hi)| (lo as f32, hi as f32));

    spawn_local(async move {
        yield_to_browser().await;

        if !resonator_request_still_active(&key) {
            return;
        }

        // Resonators are O(samples * num_bins) — yield extra for expensive LODs.
        if lod >= 2 {
            yield_to_browser().await;
            if !resonator_request_still_active(&key) {
                return;
            }
        }
        let is_current = is_current_file(&state, file_idx);
        if !is_current {
            for _ in 0..3 {
                yield_to_browser().await;
                if !resonator_request_still_active(&key) {
                    return;
                }
            }
        }

        let audio = state
            .library
            .files()
            .with_untracked(|files| files.get(file_idx).map(|f| f.audio.clone()));
        let Some(audio) = audio else {
            RESONATOR_IN_FLIGHT.with(|s| s.borrow_mut().remove(&key));
            return;
        };

        let cv = state.viewmode.channel_view().get_untracked();

        // Warmup samples before the tile's first column so the EMA state has
        // converged by the time we emit output.
        let warmup = warmup_samples(audio.sample_rate, bandwidth_hz);
        let warmup_cols = warmup.div_ceil(config_hop);
        let warmup_samples_aligned = warmup_cols * config_hop;

        let tile_sample_start = tile_idx * TILE_COLS * config_hop;
        let padded_start = tile_sample_start.saturating_sub(warmup_samples_aligned);
        let pre_pad_cols = (tile_sample_start - padded_start) / config_hop;
        let padded_len = (pre_pad_cols + TILE_COLS) * config_hop;

        streaming_source::prefetch_streaming(
            audio.source.as_ref(),
            padded_start as u64,
            padded_len,
        )
        .await;

        if !resonator_request_still_active(&key) {
            return;
        }

        let samples = audio
            .source
            .read_region(cv, padded_start as u64, padded_len);
        if samples.is_empty() {
            RESONATOR_IN_FLIGHT.with(|s| s.borrow_mut().remove(&key));
            return;
        }

        yield_to_browser().await;
        if !resonator_request_still_active(&key) {
            return;
        }

        let cols = compute_resonator_hybrid_columns(
            &samples,
            audio.sample_rate,
            reson_fft,
            config_hop,
            pre_pad_cols,
            TILE_COLS,
            bandwidth_hz,
            alpha_mode,
            q,
            layout,
            freq_range,
            hybrid_mode,
        );

        RESONATOR_IN_FLIGHT.with(|s| s.borrow_mut().remove(&key));

        let current_gen = RESONATOR_CACHE_GENERATION.with(|g| *g.borrow());
        if current_gen != gen {
            state
                .viewmode
                .tile_ready_signal()
                .update(|n| *n = n.wrapping_add(1));
            return;
        }

        if cols.is_empty() {
            state
                .viewmode
                .tile_ready_signal()
                .update(|n| *n = n.wrapping_add(1));
            return;
        }

        let rendered = spectrogram_renderer::pre_render_columns(&cols);
        RESONATOR_CACHE.with(|c| c.borrow_mut().insert(file_idx, lod, tile_idx, rendered));
        state
            .viewmode
            .tile_ready_signal()
            .update(|n| *n = n.wrapping_add(1));
    });
}
