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

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use leptos::prelude::*;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::spawn_local;
use crate::canvas::spectrogram_renderer::{self, PreRendered, FlowAlgo};
use crate::state::{AppState, LoadedFile, PlaybackMode};
use crate::audio::streaming_playback::PV_HQ_OVERLAP;
use crate::audio::streaming_source;
use crate::viewport;

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
    let files = state.files.get_untracked();
    let file = files.get(file_idx)?;
    let file_time_res = file.spectrogram.time_resolution;
    let file_duration = file.audio.duration_secs;
    let scroll = state.scroll_offset.get_untracked();
    let zoom = state.zoom_level.get_untracked();
    let canvas_w = state.spectrogram_canvas_width.get_untracked();

    if state.current_file_index.get_untracked() == Some(file_idx) {
        let visible_time = viewport::visible_time(canvas_w, zoom, file_time_res);
        return viewport::data_window(scroll, visible_time, file_duration);
    }

    let timeline = state.active_timeline.get_untracked();
    let tl = timeline.as_ref()?;
    let global_time_res = tl.segments.first()
        .and_then(|s| files.get(s.file_index))
        .map(|f| f.spectrogram.time_resolution)
        .unwrap_or(file_time_res);
    let visible_time = viewport::visible_time(canvas_w, zoom, global_time_res);
    if visible_time <= 0.0 {
        return None;
    }
    let visible_start = scroll;
    let visible_end = scroll + visible_time;

    tl.segments.iter()
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
    Some((first_tile, last_tile, center_tile.clamp(first_tile, last_tile)))
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
    LodConfig { fft_size: 256, hop_size: 8192 }, // LOD 0 — ultra-wide overview (quality downscaled)
    LodConfig { fft_size: 256, hop_size: 2048 }, // LOD 1 — wide overview (quality downscaled)
    LodConfig { fft_size: 256, hop_size: 512 },  // LOD 2 — normal resolution (baseline)
    LodConfig { fft_size: 256, hop_size: 128 },  // LOD 3 — zoomed in
    LodConfig { fft_size: 256, hop_size: 32 },   // LOD 4 — deep zoom
    LodConfig { fft_size: 256, hop_size: 8 },    // LOD 5 — extreme zoom
    LodConfig { fft_size: 256, hop_size: 2 },    // LOD 6 — sample-level zoom
    LodConfig { fft_size: 128, hop_size: 1 },    // LOD 7 — per-sample zoom (finest possible)
];

/// Select the ideal LOD level for the current zoom.
/// `zoom` is pixels per baseline (LOD2) column.
pub fn select_lod(zoom: f64) -> u8 {
    // LOD 7 is only 2× finer than LOD 6 (hop 1 vs 2), so its threshold is 2×.
    if zoom >= 256.0 { 7 }
    else if zoom >= 128.0 { 6 }
    else if zoom >= 32.0 { 5 }
    else if zoom >= 8.0 { 4 }
    else if zoom >= 2.0 { 3 }
    else if zoom >= 0.5 { 2 }
    else if zoom >= 0.125 { 1 }
    else { 0 }
}

/// Ratio of baseline (LOD2) columns to LOD_L columns (how many LOD_L cols per baseline col).
/// LOD0: 0.0625, LOD1: 0.25, LOD2: 1.0, LOD3: 4.0, LOD4: 16.0, LOD5: 64.0, LOD6: 256.0, LOD7: 512.0
pub fn lod_ratio(lod: u8) -> f64 {
    BASELINE_HOP as f64 / LOD_CONFIGS[lod as usize].hop_size as f64
}

/// Tile count at a given LOD for a file with `total_samples` audio samples.
pub fn tile_count_for_samples(total_samples: usize, lod: u8) -> usize {
    let config = &LOD_CONFIGS[lod as usize];
    if total_samples < config.fft_size { return 0; }
    let total_cols = (total_samples - config.fft_size) / config.hop_size + 1;
    total_cols.div_ceil(TILE_COLS)
}


/// Map a tile index from one LOD to the corresponding tile at a lower (coarser) LOD.
/// Returns (fallback_tile_idx, sub_col_start, sub_col_end) — the sub-region within
/// the fallback tile that covers the same time range.
pub fn fallback_tile_info(target_lod: u8, target_tile: usize, fallback_lod: u8) -> (usize, f64, f64) {
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

/// Cache key: (file_idx, lod, tile_idx)
type CacheKey = (usize, u8, usize);

pub struct Tile {
    pub tile_idx: usize,
    pub file_idx: usize,
    pub lod: u8,
    pub rendered: PreRendered,
    lru_stamp: u64,
}

struct TileCache {
    tiles: HashMap<CacheKey, Tile>,
    /// LRU order with lazy stale-entry skipping.
    lru: VecDeque<(CacheKey, u64)>,
    total_bytes: usize,
    max_bytes: usize,
    next_stamp: u64,
}

impl TileCache {
    fn new(max_bytes: usize) -> Self {
        Self {
            tiles: HashMap::new(),
            lru: VecDeque::new(),
            total_bytes: 0,
            max_bytes,
            next_stamp: 0,
        }
    }

    fn allocate_stamp(&mut self) -> u64 {
        self.next_stamp = self.next_stamp.wrapping_add(1);
        self.next_stamp
    }

    fn maybe_compact_lru(&mut self) {
        let threshold = self.tiles.len().saturating_mul(8).max(1024);
        if self.lru.len() <= threshold {
            return;
        }

        let mut entries: Vec<(u64, CacheKey)> = self.tiles
            .iter()
            .map(|(&key, tile)| (tile.lru_stamp, key))
            .collect();
        entries.sort_by_key(|(stamp, _)| *stamp);
        self.lru = entries.into_iter().map(|(stamp, key)| (key, stamp)).collect();
    }

    fn evict_to_fit(&mut self, incoming_bytes: usize) {
        while self.total_bytes + incoming_bytes > self.max_bytes {
            let Some((oldest, stamp)) = self.lru.pop_front() else { break };
            let should_evict = self.tiles
                .get(&oldest)
                .map(|tile| tile.lru_stamp == stamp)
                .unwrap_or(false);
            if should_evict {
                if let Some(evicted) = self.tiles.remove(&oldest) {
                    self.total_bytes = self.total_bytes.saturating_sub(evicted.rendered.byte_len());
                }
            }
        }
    }

    fn insert(&mut self, file_idx: usize, lod: u8, tile_idx: usize, rendered: PreRendered) {
        let key = (file_idx, lod, tile_idx);
        let bytes = rendered.byte_len();
        if let Some(old) = self.tiles.remove(&key) {
            self.total_bytes = self.total_bytes.saturating_sub(old.rendered.byte_len());
        }
        self.evict_to_fit(bytes);
        let stamp = self.allocate_stamp();
        self.total_bytes += bytes;
        self.tiles.insert(key, Tile { tile_idx, file_idx, lod, rendered, lru_stamp: stamp });
        self.lru.push_back((key, stamp));
        self.maybe_compact_lru();
    }

    fn get(&self, file_idx: usize, lod: u8, tile_idx: usize) -> Option<&Tile> {
        self.tiles.get(&(file_idx, lod, tile_idx))
    }

    fn touch(&mut self, key: CacheKey) {
        let stamp = self.allocate_stamp();
        if let Some(tile) = self.tiles.get_mut(&key) {
            tile.lru_stamp = stamp;
            self.lru.push_back((key, stamp));
            self.maybe_compact_lru();
        }
    }

    fn evict_far_from(&mut self, file_idx: usize, lod: u8, center_tile: usize, keep_radius: usize) {
        let keys_to_evict: Vec<CacheKey> = self.tiles.keys().copied()
            .filter(|&(fi, l, ti)| {
                fi == file_idx && l == lod && ti.abs_diff(center_tile) > keep_radius
            })
            .collect();
        for key in keys_to_evict {
            if let Some(evicted) = self.tiles.remove(&key) {
                self.total_bytes = self.total_bytes.saturating_sub(evicted.rendered.byte_len());
            }
        }
    }

    fn clear_for_file(&mut self, file_idx: usize) {
        let keys: Vec<_> = self.tiles.keys().copied().filter(|k| k.0 == file_idx).collect();
        for key in keys {
            if let Some(evicted) = self.tiles.remove(&key) {
                self.total_bytes = self.total_bytes.saturating_sub(evicted.rendered.byte_len());
            }
        }
    }

    fn clear_all(&mut self) {
        self.tiles.clear();
        self.lru.clear();
        self.total_bytes = 0;
    }
}

// ── Flow cache (multi-LOD, same CacheKey as magnitude tiles) ─────────────────

// Reuse FlowTileCache type for chroma too (same key shape)
type ChromaKey = (usize, usize);

struct ChromaTileCache {
    tiles: HashMap<ChromaKey, Tile>,
    lru: VecDeque<(ChromaKey, u64)>,
    total_bytes: usize,
    max_bytes: usize,
    next_stamp: u64,
}

impl ChromaTileCache {
    fn new(max_bytes: usize) -> Self {
        Self {
            tiles: HashMap::new(),
            lru: VecDeque::new(),
            total_bytes: 0,
            max_bytes,
            next_stamp: 0,
        }
    }

    fn allocate_stamp(&mut self) -> u64 {
        self.next_stamp = self.next_stamp.wrapping_add(1);
        self.next_stamp
    }

    fn maybe_compact_lru(&mut self) {
        let threshold = self.tiles.len().saturating_mul(8).max(512);
        if self.lru.len() <= threshold {
            return;
        }

        let mut entries: Vec<(u64, ChromaKey)> = self.tiles
            .iter()
            .map(|(&key, tile)| (tile.lru_stamp, key))
            .collect();
        entries.sort_by_key(|(stamp, _)| *stamp);
        self.lru = entries.into_iter().map(|(stamp, key)| (key, stamp)).collect();
    }

    fn evict_to_fit(&mut self, incoming_bytes: usize) {
        while self.total_bytes + incoming_bytes > self.max_bytes {
            let Some((oldest, stamp)) = self.lru.pop_front() else { break };
            let should_evict = self.tiles
                .get(&oldest)
                .map(|tile| tile.lru_stamp == stamp)
                .unwrap_or(false);
            if should_evict {
                if let Some(evicted) = self.tiles.remove(&oldest) {
                    self.total_bytes = self.total_bytes.saturating_sub(evicted.rendered.byte_len());
                }
            }
        }
    }

    fn insert(&mut self, file_idx: usize, tile_idx: usize, rendered: PreRendered) {
        let key = (file_idx, tile_idx);
        let bytes = rendered.byte_len();
        if let Some(old) = self.tiles.remove(&key) {
            self.total_bytes = self.total_bytes.saturating_sub(old.rendered.byte_len());
        }
        self.evict_to_fit(bytes);
        let stamp = self.allocate_stamp();
        self.total_bytes += bytes;
        self.tiles.insert(key, Tile { tile_idx, file_idx, lod: 1, rendered, lru_stamp: stamp });
        self.lru.push_back((key, stamp));
        self.maybe_compact_lru();
    }

    fn get(&self, file_idx: usize, tile_idx: usize) -> Option<&Tile> {
        self.tiles.get(&(file_idx, tile_idx))
    }

    fn touch(&mut self, key: ChromaKey) {
        let stamp = self.allocate_stamp();
        if let Some(tile) = self.tiles.get_mut(&key) {
            tile.lru_stamp = stamp;
            self.lru.push_back((key, stamp));
            self.maybe_compact_lru();
        }
    }
}

thread_local! {
    /// Unified magnitude tile cache — all LOD levels in one cache.
    static CACHE: RefCell<TileCache> = RefCell::new(TileCache::new(MAGNITUDE_MAX_BYTES));
    /// Map of (file_idx, lod, tile_idx) → timestamp (ms) for tiles currently being generated.
    /// Entries older than IN_FLIGHT_TIMEOUT_MS are considered stuck and can be re-scheduled.
    static IN_FLIGHT: RefCell<HashMap<CacheKey, f64>> =
        RefCell::new(HashMap::new());
    /// Generation counter — incremented on every clear_all_tiles(). In-flight
    /// async tasks capture the generation at spawn time and discard their result
    /// if a clear happened while they were computing (prevents stale tiles).
    static CACHE_GENERATION: RefCell<u64> = const { RefCell::new(0) };

    /// Flow-mode tile cache — multi-LOD, same CacheKey as magnitude tiles.
    static FLOW_CACHE: RefCell<TileCache> = RefCell::new(TileCache::new(FLOW_MAX_BYTES));
    static FLOW_IN_FLIGHT: RefCell<HashMap<CacheKey, f64>> =
        RefCell::new(HashMap::new());
    static FLOW_CACHE_GENERATION: RefCell<u64> = const { RefCell::new(0) };

    /// Reassignment spectrogram tile cache — multi-LOD, same CacheKey as magnitude tiles.
    static REASSIGN_CACHE: RefCell<TileCache> = RefCell::new(TileCache::new(REASSIGN_MAX_BYTES));
    static REASSIGN_IN_FLIGHT: RefCell<HashMap<CacheKey, f64>> =
        RefCell::new(HashMap::new());
    static REASSIGN_CACHE_GENERATION: RefCell<u64> = const { RefCell::new(0) };

    /// Chromagram tile cache (baseline-LOD only).
    static CHROMA_CACHE: RefCell<ChromaTileCache> = RefCell::new(ChromaTileCache::new(CHROMA_MAX_BYTES));
    static CHROMA_IN_FLIGHT: RefCell<HashMap<ChromaKey, f64>> =
        RefCell::new(HashMap::new());

    /// Cached per-file global chromagram normalisation maxima (max_class, max_note).
    static CHROMA_GLOBAL_MAX: RefCell<HashMap<usize, (f32, f32)>> =
        RefCell::new(HashMap::new());

    /// Resonator tile cache — multi-LOD, same CacheKey as magnitude tiles.
    /// Stores absolute dB values per pixel (identical layout to magnitude tiles).
    static RESONATOR_CACHE: RefCell<TileCache> = RefCell::new(TileCache::new(RESONATOR_MAX_BYTES));
    static RESONATOR_IN_FLIGHT: RefCell<HashMap<CacheKey, f64>> =
        RefCell::new(HashMap::new());
    static RESONATOR_CACHE_GENERATION: RefCell<u64> = const { RefCell::new(0) };
}

// ── IN_FLIGHT helpers ────────────────────────────────────────────────────────

/// Returns true if the total number of active in-flight tasks across all
/// categories has reached `MAX_CONCURRENT_SPAWNS`.
fn at_spawn_limit() -> bool {
    let now = js_sys::Date::now();
    let mag = IN_FLIGHT.with(|s| s.borrow().values().filter(|&&ts| now - ts <= IN_FLIGHT_TIMEOUT_MS).count());
    let flow = FLOW_IN_FLIGHT.with(|s| s.borrow().values().filter(|&&ts| now - ts <= IN_FLIGHT_TIMEOUT_MS).count());
    let reassign = REASSIGN_IN_FLIGHT.with(|s| s.borrow().values().filter(|&&ts| now - ts <= IN_FLIGHT_TIMEOUT_MS).count());
    let chroma = CHROMA_IN_FLIGHT.with(|s| s.borrow().values().filter(|&&ts| now - ts <= IN_FLIGHT_TIMEOUT_MS).count());
    let reson = RESONATOR_IN_FLIGHT.with(|s| s.borrow().values().filter(|&&ts| now - ts <= IN_FLIGHT_TIMEOUT_MS).count());
    mag + flow + reassign + chroma + reson >= MAX_CONCURRENT_SPAWNS
}

fn has_active_in_flight<K: Eq + std::hash::Hash>(map: &mut HashMap<K, f64>, key: &K) -> bool {
    match map.get(key).copied() {
        None => false,
        Some(ts) if js_sys::Date::now() - ts <= IN_FLIGHT_TIMEOUT_MS => true,
        Some(_) => {
            map.remove(key);
            false
        }
    }
}

// ── Public API: magnitude tile cache ─────────────────────────────────────────

pub fn get_tile(file_idx: usize, lod: u8, tile_idx: usize) -> Option<()> {
    CACHE.with(|c| c.borrow().get(file_idx, lod, tile_idx).map(|_| ()))
}

pub fn borrow_tile<R>(file_idx: usize, lod: u8, tile_idx: usize, f: impl FnOnce(&Tile) -> R) -> Option<R> {
    CACHE.with(|c| {
        let mut cache = c.borrow_mut();
        let key = (file_idx, lod, tile_idx);
        if cache.tiles.contains_key(&key) {
            cache.touch(key);
            drop(cache);
            CACHE.with(|c| {
                c.borrow().tiles.get(&key).map(f)
            })
        } else {
            None
        }
    })
}

pub fn clear_file(file_idx: usize) {
    CACHE.with(|c| c.borrow_mut().clear_for_file(file_idx));
    IN_FLIGHT.with(|s| s.borrow_mut().retain(|k, _| k.0 != file_idx));
    CACHE_GENERATION.with(|g| *g.borrow_mut() += 1);
}

/// Clear all magnitude tiles (all files, all LODs). Used when global
/// settings like FFT size change and all cached tiles become stale.
pub fn clear_all_tiles() {
    CACHE.with(|c| c.borrow_mut().clear_all());
    IN_FLIGHT.with(|s| s.borrow_mut().clear());
    CACHE_GENERATION.with(|g| *g.borrow_mut() += 1);
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
    CACHE.with(|c| c.borrow_mut().evict_far_from(file_idx, lod, center_tile, keep_radius));
}

/// Evict tiles far from ALL given centers. A tile is only evicted if it falls
/// outside every (center, radius) zone — tiles near any center are kept.
pub fn evict_far_multi(file_idx: usize, lod: u8, centers: &[(usize, usize)]) {
    CACHE.with(|c| {
        let mut cache = c.borrow_mut();
        let to_evict: Vec<CacheKey> = cache.tiles.keys().copied()
            .filter(|&(fi, l, ti)| {
                fi == file_idx && l == lod
                    && centers.iter().all(|&(center, radius)| ti.abs_diff(center) > radius)
            })
            .collect();
        for key in to_evict {
            if let Some(evicted) = cache.tiles.remove(&key) {
                cache.total_bytes = cache.total_bytes.saturating_sub(evicted.rendered.byte_len());
            }
        }
    });
}

/// Remove IN_FLIGHT entries far from the current viewport center.
/// Prevents old in-flight computations from blocking cache resources
/// when the user scrolls fast past them.
pub fn cancel_far_in_flight(file_idx: usize, lod: u8, center_tile: usize, keep_radius: usize) {
    IN_FLIGHT.with(|s| {
        let mut map = s.borrow_mut();
        if map.is_empty() { return; }
        map.retain(|&(fi, l, ti), _| {
            fi != file_idx || l != lod || ti.abs_diff(center_tile) <= keep_radius
        });
    });
}

/// Cancel in-flight entries far from ALL given centers.
pub fn cancel_far_in_flight_multi(file_idx: usize, lod: u8, centers: &[(usize, usize)]) {
    IN_FLIGHT.with(|s| {
        let mut map = s.borrow_mut();
        if map.is_empty() { return; }
        map.retain(|&(fi, l, ti), _| {
            fi != file_idx || l != lod
                || centers.iter().any(|&(center, radius)| ti.abs_diff(center) <= radius)
        });
    });
}

/// Returns the count of visible tiles that are neither cached nor in-flight.
/// Used by the render effect to detect stuck states needing recovery.
pub fn count_missing_visible(file_idx: usize, lod: u8, first_tile: usize, last_tile: usize) -> usize {
    CACHE.with(|c| {
        let cache = c.borrow();
        IN_FLIGHT.with(|s| {
            let mut inflight = s.borrow_mut();
            (first_tile..=last_tile).filter(|&t| {
                let key = (file_idx, lod, t);
                !cache.tiles.contains_key(&key) && !has_active_in_flight(&mut inflight, &key)
            }).count()
        })
    })
}

/// Returns (used_bytes, max_bytes) for the magnitude tile cache.
pub fn cache_usage() -> (usize, usize) {
    CACHE.with(|c| {
        let cache = c.borrow();
        (cache.total_bytes, cache.max_bytes)
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

pub fn magnitude_debug_stats(file_idx: usize, lod: u8, first_tile: usize, last_tile: usize) -> TileDebugStats {
    CACHE.with(|c| {
        let cache = c.borrow();
        IN_FLIGHT.with(|s| {
            let mut inflight = s.borrow_mut();
            let mut stats = collect_debug_stats(
                cache.tiles.len(),
                &mut inflight,
                (first_tile..=last_tile).map(|tile_idx| (file_idx, lod, tile_idx)),
                |key| cache.tiles.contains_key(key),
            );
            stats.used_bytes = cache.total_bytes;
            stats.max_bytes = cache.max_bytes;
            stats
        })
    })
}

pub fn flow_debug_stats(file_idx: usize, lod: u8, first_tile: usize, last_tile: usize) -> TileDebugStats {
    FLOW_CACHE.with(|c| {
        let cache = c.borrow();
        FLOW_IN_FLIGHT.with(|s| {
            let mut inflight = s.borrow_mut();
            let mut stats = collect_debug_stats(
                cache.tiles.len(),
                &mut inflight,
                (first_tile..=last_tile).map(|tile_idx| (file_idx, lod, tile_idx)),
                |key| cache.tiles.contains_key(key),
            );
            stats.used_bytes = cache.total_bytes;
            stats.max_bytes = cache.max_bytes;
            stats
        })
    })
}

pub fn magnitude_tile_active(file_idx: usize, lod: u8, tile_idx: usize) -> bool {
    let key = (file_idx, lod, tile_idx);
    IN_FLIGHT.with(|s| has_active_in_flight(&mut s.borrow_mut(), &key))
}

pub fn flow_tile_active(file_idx: usize, lod: u8, tile_idx: usize) -> bool {
    let key = (file_idx, lod, tile_idx);
    FLOW_IN_FLIGHT.with(|s| has_active_in_flight(&mut s.borrow_mut(), &key))
}

fn magnitude_request_still_active(key: &CacheKey) -> bool {
    IN_FLIGHT.with(|s| has_active_in_flight(&mut s.borrow_mut(), key))
}

fn flow_request_still_active(key: &CacheKey) -> bool {
    FLOW_IN_FLIGHT.with(|s| has_active_in_flight(&mut s.borrow_mut(), key))
}

fn reassign_request_still_active(key: &CacheKey) -> bool {
    REASSIGN_IN_FLIGHT.with(|s| has_active_in_flight(&mut s.borrow_mut(), key))
}

fn resonator_request_still_active(key: &CacheKey) -> bool {
    RESONATOR_IN_FLIGHT.with(|s| has_active_in_flight(&mut s.borrow_mut(), key))
}

fn active_baseline_fft(state: AppState) -> usize {
    state.spect_fft_mode.get_untracked().fft_for_lod(LOD_BASELINE)
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
        (0..n_tiles).filter(|&i| cache.tiles.contains_key(&(file_idx, LOD_BASELINE, i))).count()
    })
}

// ── Generic LOD tile scheduling ──────────────────────────────────────────────

/// Schedule a tile at any LOD level. Computes STFT from audio samples.
/// Uses the user's chosen FFT mode (from `state.spect_fft_mode`).
/// For single-FFT mode, the size is clamped to at least the LOD's hop size.
/// For multi-resolution mode, each band uses its own FFT size.
pub fn schedule_tile_lod(state: AppState, file_idx: usize, lod: u8, tile_idx: usize) {
    use crate::dsp::fft::compute_stft_columns;

    let key: CacheKey = (file_idx, lod, tile_idx);
    if CACHE.with(|c| c.borrow().tiles.contains_key(&key)) { return; }
    if IN_FLIGHT.with(|s| has_active_in_flight(&mut s.borrow_mut(), &key)) { return; }
    if at_spawn_limit() { return; }

    // Bounds check: reject tiles that are entirely past the audio data.
    // This prevents futile async work and IN_FLIGHT entries that never resolve.
    let total_samples = state.files.with_untracked(|files| {
        files.get(file_idx).map(|f| f.audio.source.total_samples() as usize).unwrap_or(0)
    });
    let max_tiles = tile_count_for_samples(total_samples, lod);
    if tile_idx >= max_tiles { return; }

    IN_FLIGHT.with(|s| s.borrow_mut().insert(key, js_sys::Date::now()));

    // Capture generation so we can discard the result if a clear happened mid-compute
    let gen = CACHE_GENERATION.with(|g| *g.borrow());

    let config_hop = LOD_CONFIGS[lod as usize].hop_size;
    let fft_mode = state.spect_fft_mode.get_untracked();
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

        let audio = state.files.with_untracked(|files| {
            files.get(file_idx).map(|f| f.audio.clone())
        });
        let Some(audio) = audio else {
            IN_FLIGHT.with(|s| s.borrow_mut().remove(&key));
            return;
        };

        // Compute STFT columns for this tile using channel-aware samples
        let cv = state.channel_view.get_untracked();
        let col_start = tile_idx * TILE_COLS;

        // Read only the sample region needed for this tile
        let sample_start = col_start * config_hop;
        let sample_len = TILE_COLS * config_hop + actual_fft;

        // When xform is active with a DSP mode that has per-chunk edge artifacts
        // (phase vocoder, pitch shift), read extra pre-padding samples so the
        // transform's onset fade/warmup falls on discarded samples rather than
        // visible tile content.
        let xform_on = state.display_transform.get_untracked();
        let needs_padding = xform_on && matches!(
            state.playback_mode.get_untracked(),
            PlaybackMode::PhaseVocoder | PlaybackMode::PitchShift | PlaybackMode::TimeExpansion
        );
        // Pad by PV_HQ_OVERLAP to ensure complete overlap-add warmup
        // before the tile's actual samples begin.
        let xform_pad = if needs_padding { PV_HQ_OVERLAP } else { 0 };
        let padded_start = sample_start.saturating_sub(xform_pad);
        let pre_pad_used = sample_start - padded_start;
        let padded_len = pre_pad_used + sample_len;

        // Prefetch for streaming sources
        let (did_seek, is_vbr) = streaming_source::prefetch_streaming(audio.source.as_ref(), padded_start as u64, padded_len).await;
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

        let raw_samples = audio.source.read_region(cv, padded_start as u64, padded_len);

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
        let decim_target = state.display_decimate_effective.get_untracked();
        let (samples, effective_rate) = if decim_target > 0 && decim_target < audio.sample_rate {
            let decimated = crate::dsp::filters::decimate(&samples, audio.sample_rate, decim_target);
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
                &samples, effective_rate, compute_fft, BASELINE_HOP, 0, TILE_COLS * oversample,
            );
            aggregate_columns_max(&dense_cols, oversample)
        } else {
            compute_stft_columns(&samples, effective_rate, actual_fft, config_hop, 0, TILE_COLS)
        };
        IN_FLIGHT.with(|s| s.borrow_mut().remove(&key));

        // Discard result if the cache was cleared while we were computing
        // (e.g. user toggled xform mode — these tiles are stale)
        let current_gen = CACHE_GENERATION.with(|g| *g.borrow());
        if current_gen != gen {
            state.tile_ready_signal.update(|n| *n = n.wrapping_add(1));
            return;
        }

        if cols.is_empty() {
            // Still bump the signal so the render effect re-evaluates
            // (e.g. to schedule tiles at clamped positions after fast scrolling)
            state.tile_ready_signal.update(|n| *n = n.wrapping_add(1));
            return;
        }

        let rendered = spectrogram_renderer::pre_render_columns(&cols);
        CACHE.with(|c| c.borrow_mut().insert(file_idx, lod, tile_idx, rendered));
        state.tile_ready_signal.update(|n| *n = n.wrapping_add(1));
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

    let key: CacheKey = (file_idx, LOD_BASELINE, tile_idx);
    if CACHE.with(|c| c.borrow().tiles.contains_key(&key)) { return; }
    if IN_FLIGHT.with(|s| has_active_in_flight(&mut s.borrow_mut(), &key)) { return; }
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

        let still_loaded = state.files.with_untracked(|files| {
            files.get(file_idx).map(|f| f.name == file.name).unwrap_or(false)
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

        let rendered = spectrogram_renderer::pre_render_columns(
            &file.spectrogram.columns[col_start..col_end],
        );

        let current_gen = CACHE_GENERATION.with(|g| *g.borrow());
        if current_gen != gen {
            IN_FLIGHT.with(|s| s.borrow_mut().remove(&key));
            state.tile_ready_signal.update(|n| *n = n.wrapping_add(1));
            return;
        }

        CACHE.with(|c| c.borrow_mut().insert(file_idx, LOD_BASELINE, tile_idx, rendered));
        IN_FLIGHT.with(|s| s.borrow_mut().remove(&key));
        state.tile_ready_signal.update(|n| *n = n.wrapping_add(1));
    });
}

/// Schedule generation of all baseline-LOD tiles for a file (called after file load).
pub fn schedule_all_tiles(state: AppState, file: LoadedFile, file_idx: usize) {
    let total_cols = if file.spectrogram.total_columns > 0 {
        file.spectrogram.total_columns
    } else {
        file.spectrogram.columns.len()
    };
    if total_cols == 0 { return; }
    let n_tiles = total_cols.div_ceil(TILE_COLS);

    let tile_order = visible_tile_focus_for_file(&state, file_idx, total_cols, file.spectrogram.time_resolution)
        .map(|(_, _, center_tile)| full_tile_order(n_tiles, center_tile))
        .unwrap_or_else(|| (0..n_tiles).collect());

    for tile_idx in tile_order {
        schedule_tile(state, file.clone(), file_idx, tile_idx);
    }
}

/// Render a baseline-LOD tile synchronously from the spectral column store.
pub fn render_tile_from_store_sync(file_idx: usize, tile_idx: usize, expected_fft: usize) -> bool {
    use crate::canvas::spectral_store;

    let key: CacheKey = (file_idx, LOD_BASELINE, tile_idx);
    if CACHE.with(|c| c.borrow().tiles.contains_key(&key)) { return true; }
    if !spectral_store::fft_matches(file_idx, expected_fft) { return false; }

    let col_start = tile_idx * TILE_COLS;
    let col_end = col_start + TILE_COLS;

    let rendered = spectral_store::with_columns(file_idx, col_start, col_end, |cols, _max_mag| {
        spectrogram_renderer::pre_render_columns(cols)
    });

    if let Some(rendered) = rendered {
        CACHE.with(|c| c.borrow_mut().insert(file_idx, LOD_BASELINE, tile_idx, rendered));
        true
    } else {
        false
    }
}

/// Render a partial (live) baseline-LOD tile from the spectral store.
pub fn render_live_tile_sync(file_idx: usize, tile_idx: usize, col_start: usize, available_cols: usize, expected_fft: usize) -> bool {
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
        CACHE.with(|c| c.borrow_mut().insert(file_idx, LOD_BASELINE, tile_idx, rendered));
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

    let key: CacheKey = (file_idx, LOD_BASELINE, tile_idx);
    if CACHE.with(|c| c.borrow().tiles.contains_key(&key)) { return; }
    if IN_FLIGHT.with(|s| has_active_in_flight(&mut s.borrow_mut(), &key)) { return; }
    if at_spawn_limit() { return; }
    IN_FLIGHT.with(|s| s.borrow_mut().insert(key, js_sys::Date::now()));

    let gen = CACHE_GENERATION.with(|g| *g.borrow());

    spawn_local(async move {
        yield_to_browser().await;

        if !magnitude_request_still_active(&key) {
            return;
        }

        // Defer while audio is playing — playback chunk processing needs
        // uncontested CPU and I/O to avoid gaps
        if state.is_playing.get_untracked() {
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

        let still_loaded = state.files.with_untracked(|files| {
            file_idx < files.len()
        });
        if !still_loaded {
            IN_FLIGHT.with(|s| s.borrow_mut().remove(&key));
            return;
        }

        let col_start = tile_idx * TILE_COLS;
        let col_end = col_start + TILE_COLS;

        let rendered = spectral_store::with_columns(file_idx, col_start, col_end, |cols, _max_mag| {
            spectrogram_renderer::pre_render_columns(cols)
        });

        IN_FLIGHT.with(|s| s.borrow_mut().remove(&key));

        let current_gen = CACHE_GENERATION.with(|g| *g.borrow());
        if current_gen != gen {
            state.tile_ready_signal.update(|n| *n = n.wrapping_add(1));
            return;
        }

        if let Some(rendered) = rendered {
            CACHE.with(|c| c.borrow_mut().insert(file_idx, LOD_BASELINE, tile_idx, rendered));
        }
        // Always bump signal so render effect retries even if store had no data
        state.tile_ready_signal.update(|n| *n = n.wrapping_add(1));
    });
}

/// Schedule visible baseline-LOD tiles from the spectral store.
pub fn schedule_visible_tiles_from_store(state: AppState, file_idx: usize, total_cols: usize) {
    if total_cols == 0 { return; }
    let n_tiles = total_cols.div_ceil(TILE_COLS);

    let time_res = state.files.with_untracked(|files| {
        files.get(file_idx).map(|f| f.spectrogram.time_resolution).unwrap_or(0.01)
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
                if l < n_tiles { v.push(l); }
            }
            if center_tile + dist < n_tiles {
                v.push(center_tile + dist);
            }
            v
        };
        if tiles.is_empty() { break; }
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
    if max_tiles == 0 { return; }

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
        if tiles.len() >= max_prefetch { break; }
        tiles.push(t);
    }

    // Region 2: first initial_secs from file start
    let initial_end = time_to_tile(initial_secs).min(max_tiles.saturating_sub(1));
    for t in 0..=initial_end {
        if tiles.len() >= max_prefetch { break; }
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
pub fn schedule_tile_on_demand(
    state: AppState,
    file_idx: usize,
    tile_idx: usize,
) {
    use crate::canvas::spectral_store;
    use crate::dsp::fft::compute_stft_columns;

    let key: CacheKey = (file_idx, LOD_BASELINE, tile_idx);
    if CACHE.with(|c| c.borrow().tiles.contains_key(&key)) { return; }
    if IN_FLIGHT.with(|s| has_active_in_flight(&mut s.borrow_mut(), &key)) { return; }
    if at_spawn_limit() { return; }

    // Bounds check: reject tiles past the audio data
    let total_samples = state.files.with_untracked(|files| {
        files.get(file_idx).map(|f| f.audio.source.total_samples() as usize).unwrap_or(0)
    });
    let max_tiles = tile_count_for_samples(total_samples, LOD_BASELINE);
    if tile_idx >= max_tiles { return; }

    IN_FLIGHT.with(|s| s.borrow_mut().insert(key, js_sys::Date::now()));

    let gen = CACHE_GENERATION.with(|g| *g.borrow());

    spawn_local(async move {
        yield_to_browser().await;

        if !magnitude_request_still_active(&key) {
            return;
        }

        // Defer while audio is playing — playback needs uncontested I/O and CPU
        if state.is_playing.get_untracked() {
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

        let audio = state.files.with_untracked(|files| {
            files.get(file_idx).map(|f| f.audio.clone())
        });
        let Some(audio) = audio else {
            IN_FLIGHT.with(|s| s.borrow_mut().remove(&key));
            return;
        };

        let cv = state.channel_view.get_untracked();
        let col_start = tile_idx * TILE_COLS;
        let hop_size = BASELINE_HOP;
        let fft_size = state.spect_fft_mode.get_untracked().fft_for_lod(LOD_BASELINE);

        // Read only the sample region needed for this tile
        let sample_start = col_start * hop_size;
        let sample_len = TILE_COLS * hop_size + fft_size;

        // Prefetch for streaming sources
        let (did_seek, is_vbr) = streaming_source::prefetch_streaming(audio.source.as_ref(), sample_start as u64, sample_len).await;
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

        let samples = audio.source.read_region(cv, sample_start as u64, sample_len);
        let cols = compute_stft_columns(&samples, audio.sample_rate, fft_size, hop_size, 0, TILE_COLS);
        if cols.is_empty() {
            IN_FLIGHT.with(|s| s.borrow_mut().remove(&key));
            // Bump signal so render effect retries (e.g. after fast scroll clamping)
            state.tile_ready_signal.update(|n| *n = n.wrapping_add(1));
            return;
        }

        spectral_store::insert_columns(file_idx, col_start, &cols);

        let rendered = spectrogram_renderer::pre_render_columns(&cols);

        let current_gen = CACHE_GENERATION.with(|g| *g.borrow());
        if current_gen != gen {
            IN_FLIGHT.with(|s| s.borrow_mut().remove(&key));
            state.tile_ready_signal.update(|n| *n = n.wrapping_add(1));
            return;
        }

        CACHE.with(|c| c.borrow_mut().insert(file_idx, LOD_BASELINE, tile_idx, rendered));
        IN_FLIGHT.with(|s| s.borrow_mut().remove(&key));
        state.tile_ready_signal.update(|n| *n = n.wrapping_add(1));
    });
}

// ── Flow tile cache ─────────────────────────────────────────────────────────

pub fn get_flow_tile(file_idx: usize, lod: u8, tile_idx: usize) -> Option<()> {
    FLOW_CACHE.with(|c| c.borrow().get(file_idx, lod, tile_idx).map(|_| ()))
}

pub fn borrow_flow_tile<R>(file_idx: usize, lod: u8, tile_idx: usize, f: impl FnOnce(&Tile) -> R) -> Option<R> {
    FLOW_CACHE.with(|c| {
        let mut cache = c.borrow_mut();
        let key = (file_idx, lod, tile_idx);
        if cache.tiles.contains_key(&key) {
            cache.touch(key);
            drop(cache);
            FLOW_CACHE.with(|c| {
                c.borrow().tiles.get(&key).map(f)
            })
        } else {
            None
        }
    })
}

pub fn clear_flow_cache() {
    FLOW_CACHE.with(|c| c.borrow_mut().clear_all());
    FLOW_IN_FLIGHT.with(|s| s.borrow_mut().clear());
    FLOW_CACHE_GENERATION.with(|g| *g.borrow_mut() += 1);
}

pub fn clear_flow_file(file_idx: usize) {
    FLOW_CACHE.with(|c| c.borrow_mut().clear_for_file(file_idx));
    FLOW_IN_FLIGHT.with(|s| s.borrow_mut().retain(|k, _| k.0 != file_idx));
    FLOW_CACHE_GENERATION.with(|g| *g.borrow_mut() += 1);
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

    let key: CacheKey = (file_idx, lod, tile_idx);
    if FLOW_CACHE.with(|c| c.borrow().tiles.contains_key(&key)) { return; }
    if FLOW_IN_FLIGHT.with(|s| has_active_in_flight(&mut s.borrow_mut(), &key)) { return; }
    if at_spawn_limit() { return; }

    let total_samples = state.files.with_untracked(|files| {
        files.get(file_idx).map(|f| f.audio.source.total_samples() as usize).unwrap_or(0)
    });
    let max_tiles = tile_count_for_samples(total_samples, lod);
    if tile_idx >= max_tiles { return; }

    FLOW_IN_FLIGHT.with(|s| s.borrow_mut().insert(key, js_sys::Date::now()));

    let gen = FLOW_CACHE_GENERATION.with(|g| *g.borrow());

    let config_hop = LOD_CONFIGS[lod as usize].hop_size;
    let actual_fft = state.spect_fft_mode.get_untracked().fft_for_lod(lod);

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

        let audio = state.files.with_untracked(|files| {
            files.get(file_idx).map(|f| f.audio.clone())
        });
        let Some(audio) = audio else {
            FLOW_IN_FLIGHT.with(|s| s.borrow_mut().remove(&key));
            return;
        };

        let cv = state.channel_view.get_untracked();
        let col_start = tile_idx * TILE_COLS;

        let rendered = match algo {
            FlowAlgo::Phase | FlowAlgo::PhaseCoherence => {
                use crate::dsp::harmonics;

                let sample_start = col_start * config_hop;
                let extra = if algo == FlowAlgo::PhaseCoherence { TILE_COLS + 1 } else { TILE_COLS };
                let sample_len = extra * config_hop + actual_fft;

                // Prefetch for streaming sources
                streaming_source::prefetch_streaming(audio.source.as_ref(), sample_start as u64, sample_len).await;

                if !flow_request_still_active(&key) {
                    return;
                }

                let total = audio.source.total_samples() as usize;
                let sample_end = (sample_start + sample_len).min(total);
                if sample_start >= total || sample_start >= sample_end {
                    FLOW_IN_FLIGHT.with(|s| s.borrow_mut().remove(&key));
                    return;
                }

                let ch_samples = audio.source.read_region(cv, sample_start as u64, sample_end - sample_start);
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
                    harmonics::compute_tile_phase_data(
                        samples, TILE_COLS, actual_fft, config_hop,
                    )
                }
            }
            FlowAlgo::Optical | FlowAlgo::Centroid | FlowAlgo::Gradient => {
                // Read the sample region needed (including one previous column for flow diff)
                let extra_cols = if tile_idx > 0 { 1 } else { 0 };
                let region_col_start = col_start.saturating_sub(extra_cols);
                let region_sample_start = region_col_start * config_hop;
                let region_cols = TILE_COLS + extra_cols;
                let region_sample_len = region_cols * config_hop + actual_fft;

                streaming_source::prefetch_streaming(audio.source.as_ref(), region_sample_start as u64, region_sample_len).await;

                if !flow_request_still_active(&key) {
                    return;
                }

                let region_samples = audio.source.read_region(cv, region_sample_start as u64, region_sample_len);

                let prev_col = if extra_cols > 0 {
                    let prev_cols = compute_stft_columns(
                        &region_samples, audio.sample_rate, actual_fft, config_hop, 0, 1,
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
                    &region_samples, audio.sample_rate, actual_fft, config_hop, extra_cols, TILE_COLS,
                );
                if cols.is_empty() {
                    FLOW_IN_FLIGHT.with(|s| s.borrow_mut().remove(&key));
                    return;
                }

                spectrogram_renderer::pre_render_flow_columns(
                    &cols, prev_col.as_deref(), algo,
                )
            }
        };

        let current_gen = FLOW_CACHE_GENERATION.with(|g| *g.borrow());
        if current_gen != gen {
            FLOW_IN_FLIGHT.with(|s| s.borrow_mut().remove(&key));
            state.tile_ready_signal.update(|n| *n = n.wrapping_add(1));
            return;
        }

        FLOW_CACHE.with(|c| c.borrow_mut().insert(file_idx, lod, tile_idx, rendered));
        FLOW_IN_FLIGHT.with(|s| s.borrow_mut().remove(&key));
        state.tile_ready_signal.update(|n| *n = n.wrapping_add(1));
    });
}

// ── Reassignment spectrogram tile cache ──────────────────────────────────────

pub fn get_reassign_tile(file_idx: usize, lod: u8, tile_idx: usize) -> Option<()> {
    REASSIGN_CACHE.with(|c| c.borrow().get(file_idx, lod, tile_idx).map(|_| ()))
}

pub fn borrow_reassign_tile<R>(file_idx: usize, lod: u8, tile_idx: usize, f: impl FnOnce(&Tile) -> R) -> Option<R> {
    REASSIGN_CACHE.with(|c| {
        let mut cache = c.borrow_mut();
        let key = (file_idx, lod, tile_idx);
        if cache.tiles.contains_key(&key) {
            cache.touch(key);
            drop(cache);
            REASSIGN_CACHE.with(|c| {
                c.borrow().tiles.get(&key).map(f)
            })
        } else {
            None
        }
    })
}

pub fn clear_reassign_cache() {
    REASSIGN_CACHE.with(|c| c.borrow_mut().clear_all());
    REASSIGN_IN_FLIGHT.with(|s| s.borrow_mut().clear());
    REASSIGN_CACHE_GENERATION.with(|g| *g.borrow_mut() += 1);
}

pub fn clear_reassign_file(file_idx: usize) {
    REASSIGN_CACHE.with(|c| c.borrow_mut().clear_for_file(file_idx));
    REASSIGN_IN_FLIGHT.with(|s| s.borrow_mut().retain(|k, _| k.0 != file_idx));
    REASSIGN_CACHE_GENERATION.with(|g| *g.borrow_mut() += 1);
}

/// Schedule a reassignment spectrogram tile for background generation.
///
/// Performs 3 FFTs per frame (standard, time-ramped, derivative-windowed) to
/// compute corrected time-frequency positions, producing sharper spectrograms.
pub fn schedule_reassign_tile(
    state: AppState,
    file_idx: usize,
    lod: u8,
    tile_idx: usize,
) {
    use crate::dsp::fft::compute_reassigned_tile;

    let key: CacheKey = (file_idx, lod, tile_idx);
    if REASSIGN_CACHE.with(|c| c.borrow().tiles.contains_key(&key)) { return; }
    if REASSIGN_IN_FLIGHT.with(|s| has_active_in_flight(&mut s.borrow_mut(), &key)) { return; }
    if at_spawn_limit() { return; }

    let total_samples = state.files.with_untracked(|files| {
        files.get(file_idx).map(|f| f.audio.source.total_samples() as usize).unwrap_or(0)
    });
    let max_tiles = tile_count_for_samples(total_samples, lod);
    if tile_idx >= max_tiles { return; }

    REASSIGN_IN_FLIGHT.with(|s| s.borrow_mut().insert(key, js_sys::Date::now()));

    let gen = REASSIGN_CACHE_GENERATION.with(|g| *g.borrow());

    let config_hop = LOD_CONFIGS[lod as usize].hop_size;
    let actual_fft = state.spect_fft_mode.get_untracked().fft_for_lod(lod);

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

        let audio = state.files.with_untracked(|files| {
            files.get(file_idx).map(|f| f.audio.clone())
        });
        let Some(audio) = audio else {
            REASSIGN_IN_FLIGHT.with(|s| s.borrow_mut().remove(&key));
            return;
        };

        let cv = state.channel_view.get_untracked();
        let sample_start = tile_idx * TILE_COLS * config_hop;
        let sample_len = TILE_COLS * config_hop + actual_fft;

        // Prefetch for streaming sources
        streaming_source::prefetch_streaming(audio.source.as_ref(), sample_start as u64, sample_len).await;

        if !reassign_request_still_active(&key) {
            return;
        }

        let total = audio.source.total_samples() as usize;
        let sample_end = (sample_start + sample_len).min(total);
        if sample_start >= total || sample_start >= sample_end {
            REASSIGN_IN_FLIGHT.with(|s| s.borrow_mut().remove(&key));
            return;
        }

        let ch_samples = audio.source.read_region(cv, sample_start as u64, sample_end - sample_start);
        let samples = &ch_samples[..];

        yield_to_browser().await;

        if !reassign_request_still_active(&key) {
            return;
        }

        let rendered = compute_reassigned_tile(
            samples, TILE_COLS, actual_fft, config_hop, -60.0,
        );

        let current_gen = REASSIGN_CACHE_GENERATION.with(|g| *g.borrow());
        if current_gen != gen {
            REASSIGN_IN_FLIGHT.with(|s| s.borrow_mut().remove(&key));
            state.tile_ready_signal.update(|n| *n = n.wrapping_add(1));
            return;
        }

        REASSIGN_CACHE.with(|c| c.borrow_mut().insert(file_idx, lod, tile_idx, rendered));
        REASSIGN_IN_FLIGHT.with(|s| s.borrow_mut().remove(&key));
        state.tile_ready_signal.update(|n| *n = n.wrapping_add(1));
    });
}

// ── Chromagram tile cache (baseline-LOD only) ───────────────────────────────

pub fn get_chroma_tile(file_idx: usize, tile_idx: usize) -> Option<()> {
    CHROMA_CACHE.with(|c| c.borrow().get(file_idx, tile_idx).map(|_| ()))
}

pub fn borrow_chroma_tile<R>(file_idx: usize, tile_idx: usize, f: impl FnOnce(&Tile) -> R) -> Option<R> {
    CHROMA_CACHE.with(|c| {
        let mut cache = c.borrow_mut();
        let key = (file_idx, tile_idx);
        if cache.tiles.contains_key(&key) {
            cache.touch(key);
            drop(cache);
            CHROMA_CACHE.with(|c| {
                c.borrow().tiles.get(&key).map(f)
            })
        } else {
            None
        }
    })
}

pub fn clear_chroma_cache() {
    CHROMA_CACHE.with(|c| {
        let mut cache = c.borrow_mut();
        cache.tiles.clear();
        cache.lru.clear();
        cache.total_bytes = 0;
    });
    CHROMA_IN_FLIGHT.with(|s| s.borrow_mut().clear());
    CHROMA_GLOBAL_MAX.with(|m| m.borrow_mut().clear());
}

/// Schedule a chromagram tile for background generation (baseline LOD).
pub fn schedule_chroma_tile(
    state: AppState,
    file_idx: usize,
    tile_idx: usize,
) {
    use crate::canvas::spectral_store;
    use crate::dsp::chromagram;
    use crate::dsp::fft::compute_stft_columns;

    let key = (file_idx, tile_idx);
    if CHROMA_CACHE.with(|c| c.borrow().tiles.contains_key(&key)) { return; }
    if CHROMA_IN_FLIGHT.with(|s| has_active_in_flight(&mut s.borrow_mut(), &key)) { return; }
    if at_spawn_limit() { return; }
    CHROMA_IN_FLIGHT.with(|s| s.borrow_mut().insert(key, js_sys::Date::now()));

    spawn_local(async move {
        yield_to_browser().await;

        let is_current = is_current_file(&state, file_idx);
        if !is_current {
            for _ in 0..3 { yield_to_browser().await; }
        }

        let still_loaded = state.files.with_untracked(|files| file_idx < files.len());
        if !still_loaded {
            CHROMA_IN_FLIGHT.with(|s| s.borrow_mut().remove(&key));
            return;
        }

        let col_start = tile_idx * TILE_COLS;

        let freq_res = state.files.with_untracked(|files| {
            files.get(file_idx).map(|f| f.spectrogram.freq_resolution)
        }).unwrap_or(1.0);

        let (min_octave, num_octaves) = state.chroma_range.get_untracked().octave_params();
        let gain_db = state.chroma_gain.get_untracked();

        // Try spectral_store first, then file columns, then compute on-demand from audio
        let cols_from_store = spectral_store::with_columns(file_idx, col_start, col_start + TILE_COLS, |cols, _| {
            cols.to_vec()
        });
        let cols_from_file = if cols_from_store.is_some() {
            None
        } else {
            state.files.with_untracked(|files| {
                files.get(file_idx).and_then(|f| {
                    if f.spectrogram.columns.is_empty() { return None; }
                    let end = (col_start + TILE_COLS).min(f.spectrogram.columns.len());
                    if col_start >= end { return None; }
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
            let audio = state.files.with_untracked(|files| {
                files.get(file_idx).map(|f| f.audio.clone())
            });
            let Some(audio) = audio else {
                CHROMA_IN_FLIGHT.with(|s| s.borrow_mut().remove(&key));
                return;
            };

            let cv = state.channel_view.get_untracked();
            let hop_size = BASELINE_HOP;
            let fft_size = active_baseline_fft(state);
            let sample_start = col_start * hop_size;
            let sample_len = TILE_COLS * hop_size + fft_size;

            streaming_source::prefetch_streaming(
                audio.source.as_ref(),
                sample_start as u64,
                sample_len,
            ).await;

            let samples = audio.source.read_region(cv, sample_start as u64, sample_len);
            let cols = compute_stft_columns(&samples, audio.sample_rate, fft_size, hop_size, 0, TILE_COLS);
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
            let from_store = spectral_store::compute_chroma_global_max(file_idx, freq_res, min_octave, num_octaves);
            let gm = from_store.unwrap_or_else(|| {
                chromagram::compute_chroma_max(&stft_cols, freq_res, min_octave, num_octaves)
            });
            if gm.0 > 0.0 {
                CHROMA_GLOBAL_MAX.with(|m| m.borrow_mut().insert(file_idx, gm));
            }
            gm
        };

        let rendered = chromagram::pre_render_chromagram_columns(&stft_cols, freq_res, max_class, max_note, min_octave, num_octaves, gain_db);

        CHROMA_CACHE.with(|c| c.borrow_mut().insert(file_idx, tile_idx, rendered));
        CHROMA_IN_FLIGHT.with(|s| s.borrow_mut().remove(&key));
        state.tile_ready_signal.update(|n| *n = n.wrapping_add(1));
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
    let current_gen = state.bg_preload_gen.get_untracked();
    if current_gen != generation { return; }

    let batch = BG_PRELOAD.with(|bg| {
        let mut bg = bg.borrow_mut();
        let s = match bg.as_mut() {
            Some(s) if s.generation == generation => s,
            _ => return None,
        };

        // Check if cache is near capacity (stop at 90%)
        let cache_full = CACHE.with(|c| {
            let cache = c.borrow();
            cache.total_bytes >= cache.max_bytes / 10 * 9
        });
        if cache_full { return None; }

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
                    if idx < s.max_tiles { v.push(idx); }
                }
                v
            };

            for t in candidates {
                let key = (s.file_idx, s.lod, t);
                if CACHE.with(|c| c.borrow().tiles.contains_key(&key)) {
                    continue; // already cached
                }
                tiles_to_schedule.push((s.file_idx, s.lod, t));
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
        for &(fi, lod, ti) in &tiles {
            schedule_tile_lod(state, fi, lod, ti);
        }
        // Schedule next batch
        schedule_preload_batch(state, generation);
    }
    // else: cache full or all tiles done — preload stops naturally
}

// ── Internal helpers ─────────────────────────────────────────────────────────

/// Yield once to the browser event loop via a zero-duration setTimeout.
pub async fn yield_to_browser() {
    let promise = js_sys::Promise::new(&mut |resolve, _reject| {
        let win = web_sys::window().unwrap();
        let cb = Closure::once_into_js(move || {
            let _ = resolve.call0(&JsValue::NULL);
        });
        let _ = win.set_timeout_with_callback_and_timeout_and_arguments_0(
            cb.unchecked_ref(), 0,
        );
    });
    let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
}

/// Apply the current playback DSP mode transform to samples for display.
/// Used when display_transform is true (Transform "Same" mode).
fn apply_display_transform(samples: &[f32], sample_rate: u32, state: AppState) -> Vec<f32> {
    let mode = state.playback_mode.get_untracked();
    match mode {
        PlaybackMode::Normal => {
            samples.to_vec()
        }
        PlaybackMode::TimeExpansion => {
            let factor = state.te_factor.get_untracked();
            if factor.abs() > 1.0 {
                crate::dsp::pitch_shift::pitch_shift_realtime(samples, factor)
            } else {
                samples.to_vec()
            }
        }
        PlaybackMode::Heterodyne => {
            let lo = state.het_frequency.get_untracked();
            let cutoff = state.het_cutoff.get_untracked();
            crate::dsp::heterodyne::heterodyne_mix(samples, sample_rate, lo, cutoff)
        }
        PlaybackMode::PitchShift => {
            let factor = state.ps_factor.get_untracked();
            crate::dsp::pitch_shift::pitch_shift_realtime(samples, factor)
        }
        PlaybackMode::PhaseVocoder => {
            let factor = state.pv_factor.get_untracked();
            crate::dsp::phase_vocoder::phase_vocoder_pitch_shift(samples, factor)
        }
        PlaybackMode::ZeroCrossing => {
            let factor = state.zc_factor.get_untracked() as u32;
            crate::dsp::zc_divide::zc_divide(samples, sample_rate, factor, false)
        }
    }
}

// ── Resonator tile cache ─────────────────────────────────────────────────────

pub fn get_resonator_tile(file_idx: usize, lod: u8, tile_idx: usize) -> Option<()> {
    RESONATOR_CACHE.with(|c| c.borrow().get(file_idx, lod, tile_idx).map(|_| ()))
}

pub fn borrow_resonator_tile<R>(file_idx: usize, lod: u8, tile_idx: usize, f: impl FnOnce(&Tile) -> R) -> Option<R> {
    RESONATOR_CACHE.with(|c| {
        let mut cache = c.borrow_mut();
        let key = (file_idx, lod, tile_idx);
        if cache.tiles.contains_key(&key) {
            cache.touch(key);
            drop(cache);
            RESONATOR_CACHE.with(|c| {
                c.borrow().tiles.get(&key).map(f)
            })
        } else {
            None
        }
    })
}

pub fn clear_resonator_cache() {
    RESONATOR_CACHE.with(|c| c.borrow_mut().clear_all());
    RESONATOR_IN_FLIGHT.with(|s| s.borrow_mut().clear());
    RESONATOR_CACHE_GENERATION.with(|g| *g.borrow_mut() += 1);
    // Offscreen tile canvases are keyed by (file_idx, lod, tile_idx) without
    // an underlying-tile-dimension component, so resizing bin counts would
    // otherwise reuse old canvases (rendered at the previous height) and draw
    // them squished into the new destination rect.
    crate::canvas::spectrogram_renderer::clear_tile_canvas_cache();
}

pub fn clear_resonator_file(file_idx: usize) {
    RESONATOR_CACHE.with(|c| c.borrow_mut().clear_for_file(file_idx));
    RESONATOR_IN_FLIGHT.with(|s| s.borrow_mut().retain(|k, _| k.0 != file_idx));
    RESONATOR_CACHE_GENERATION.with(|g| *g.borrow_mut() += 1);
}

pub fn resonator_tile_active(file_idx: usize, lod: u8, tile_idx: usize) -> bool {
    let key = (file_idx, lod, tile_idx);
    RESONATOR_IN_FLIGHT.with(|s| has_active_in_flight(&mut s.borrow_mut(), &key))
}

pub fn resonator_debug_stats(file_idx: usize, lod: u8, first_tile: usize, last_tile: usize) -> TileDebugStats {
    RESONATOR_CACHE.with(|c| {
        let cache = c.borrow();
        RESONATOR_IN_FLIGHT.with(|s| {
            let mut inflight = s.borrow_mut();
            let mut stats = collect_debug_stats(
                cache.tiles.len(),
                &mut inflight,
                (first_tile..=last_tile).map(|tile_idx| (file_idx, lod, tile_idx)),
                |key| cache.tiles.contains_key(key),
            );
            stats.used_bytes = cache.total_bytes;
            stats.max_bytes = cache.max_bytes;
            stats
        })
    })
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
    use crate::dsp::resonators::{compute_resonator_columns, warmup_samples};

    let key: CacheKey = (file_idx, lod, tile_idx);
    if RESONATOR_CACHE.with(|c| c.borrow().tiles.contains_key(&key)) { return; }
    if RESONATOR_IN_FLIGHT.with(|s| has_active_in_flight(&mut s.borrow_mut(), &key)) { return; }
    if at_spawn_limit() { return; }

    let total_samples = state.files.with_untracked(|files| {
        files.get(file_idx).map(|f| f.audio.source.total_samples() as usize).unwrap_or(0)
    });
    let max_tiles = tile_count_for_samples(total_samples, lod);
    if tile_idx >= max_tiles { return; }

    RESONATOR_IN_FLIGHT.with(|s| s.borrow_mut().insert(key, js_sys::Date::now()));

    let gen = RESONATOR_CACHE_GENERATION.with(|g| *g.borrow());

    let config_hop = LOD_CONFIGS[lod as usize].hop_size;
    let reson_fft = state.resonator_fft_mode.get_untracked().fft_for_lod(lod).max(16);
    let bandwidth_hz = state.resonator_bandwidth_hz.get_untracked().max(1.0);

    spawn_local(async move {
        yield_to_browser().await;

        if !resonator_request_still_active(&key) {
            return;
        }

        // Resonators are O(samples * num_bins) — yield extra for expensive LODs.
        if lod >= 2 {
            yield_to_browser().await;
            if !resonator_request_still_active(&key) { return; }
        }
        let is_current = is_current_file(&state, file_idx);
        if !is_current {
            for _ in 0..3 {
                yield_to_browser().await;
                if !resonator_request_still_active(&key) { return; }
            }
        }

        let audio = state.files.with_untracked(|files| {
            files.get(file_idx).map(|f| f.audio.clone())
        });
        let Some(audio) = audio else {
            RESONATOR_IN_FLIGHT.with(|s| s.borrow_mut().remove(&key));
            return;
        };

        let cv = state.channel_view.get_untracked();

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
        ).await;

        if !resonator_request_still_active(&key) { return; }

        let samples = audio.source.read_region(cv, padded_start as u64, padded_len);
        if samples.is_empty() {
            RESONATOR_IN_FLIGHT.with(|s| s.borrow_mut().remove(&key));
            return;
        }

        yield_to_browser().await;
        if !resonator_request_still_active(&key) { return; }

        let cols = compute_resonator_columns(
            &samples,
            audio.sample_rate,
            reson_fft,
            config_hop,
            pre_pad_cols,
            TILE_COLS,
            bandwidth_hz,
        );

        RESONATOR_IN_FLIGHT.with(|s| s.borrow_mut().remove(&key));

        let current_gen = RESONATOR_CACHE_GENERATION.with(|g| *g.borrow());
        if current_gen != gen {
            state.tile_ready_signal.update(|n| *n = n.wrapping_add(1));
            return;
        }

        if cols.is_empty() {
            state.tile_ready_signal.update(|n| *n = n.wrapping_add(1));
            return;
        }

        let rendered = spectrogram_renderer::pre_render_columns(&cols);
        RESONATOR_CACHE.with(|c| c.borrow_mut().insert(file_idx, lod, tile_idx, rendered));
        state.tile_ready_signal.update(|n| *n = n.wrapping_add(1));
    });
}
