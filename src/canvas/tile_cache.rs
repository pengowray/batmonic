//! Progressive tile cache for spectrogram rendering.
//!
//! Google Maps-style LOD system: each LOD level has its own tile index space.
//! All tiles have the same number of columns (`TILE_COLS = 256`), but different
//! LODs cover different time ranges:
//!
//! - LOD 0: hop=2048, covers ~524K samples/tile (wide, blurry)
//! - LOD 1: hop=512, covers ~131K samples/tile (normal)
//! - LOD 2: hop=128, covers ~33K samples/tile (zoomed in)
//! - LOD 3: hop=32, covers ~8K samples/tile (deep zoom)
//!
//! Each level is 4× finer than the previous. The renderer picks the ideal LOD
//! for the current zoom and falls back to lower LODs when tiles aren't cached.
//!
//! The cache uses an LRU eviction policy capped at `MAX_BYTES` total pixel storage.

use std::cell::RefCell;
use std::collections::HashMap;
use leptos::prelude::*;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::spawn_local;
use crate::canvas::spectrogram_renderer::{self, PreRendered, FlowAlgo};
use crate::state::{AppState, LoadedFile, PlaybackMode};
use crate::audio::streaming_playback::PV_HQ_OVERLAP;
use crate::audio::streaming_source;

/// Number of spectrogram columns per tile (constant across all LODs).
pub const TILE_COLS: usize = 256;

/// ~120 MB cap for tile pixel data.
const MAX_BYTES: usize = 120 * 1024 * 1024;

/// Maximum time (ms) a tile can be in-flight before being considered stuck.
const IN_FLIGHT_TIMEOUT_MS: f64 = 10_000.0;

// ── LOD configuration ────────────────────────────────────────────────────────

pub struct LodConfig {
    pub fft_size: usize,
    pub hop_size: usize,
}

pub const NUM_LODS: usize = 4;

pub const LOD_CONFIGS: [LodConfig; NUM_LODS] = [
    LodConfig { fft_size: 256, hop_size: 2048 }, // LOD 0 — wide overview
    LodConfig { fft_size: 256, hop_size: 512 },  // LOD 1 — normal resolution
    LodConfig { fft_size: 256, hop_size: 128 },  // LOD 2 — zoomed in
    LodConfig { fft_size: 256, hop_size: 32 },   // LOD 3 — deep zoom
];

/// Select the ideal LOD level for the current zoom.
/// `zoom` is pixels per LOD1 column.
pub fn select_lod(zoom: f64) -> u8 {
    if zoom >= 8.0 { 3 }
    else if zoom >= 2.0 { 2 }
    else if zoom >= 0.5 { 1 }
    else { 0 }
}

/// Ratio of LOD1 columns to LOD_L columns (how many LOD_L cols per LOD1 col).
/// LOD0: 0.25, LOD1: 1.0, LOD2: 4.0, LOD3: 16.0
pub fn lod_ratio(lod: u8) -> f64 {
    LOD_CONFIGS[1].hop_size as f64 / LOD_CONFIGS[lod as usize].hop_size as f64
}

/// Tile count at a given LOD for a file with `total_samples` audio samples.
pub fn tile_count_for_samples(total_samples: usize, lod: u8) -> usize {
    let config = &LOD_CONFIGS[lod as usize];
    if total_samples < config.fft_size { return 0; }
    let total_cols = (total_samples - config.fft_size) / config.hop_size + 1;
    (total_cols + TILE_COLS - 1) / TILE_COLS
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

// ── Cache data structures ────────────────────────────────────────────────────

/// Cache key: (file_idx, lod, tile_idx)
type CacheKey = (usize, u8, usize);

pub struct Tile {
    pub tile_idx: usize,
    pub file_idx: usize,
    pub lod: u8,
    pub rendered: PreRendered,
}

struct TileCache {
    tiles: HashMap<CacheKey, Tile>,
    /// LRU order: front = oldest, back = most recently used
    lru: Vec<CacheKey>,
    total_bytes: usize,
}

impl TileCache {
    fn new() -> Self {
        Self { tiles: HashMap::new(), lru: Vec::new(), total_bytes: 0 }
    }

    fn insert(&mut self, file_idx: usize, lod: u8, tile_idx: usize, rendered: PreRendered) {
        let key = (file_idx, lod, tile_idx);
        let bytes = rendered.byte_len();
        // Remove old entry if replacing
        if let Some(old) = self.tiles.remove(&key) {
            self.total_bytes = self.total_bytes.saturating_sub(old.rendered.byte_len());
            self.lru.retain(|k| k != &key);
        }
        // Evict until under cap
        while self.total_bytes + bytes > MAX_BYTES && !self.lru.is_empty() {
            let oldest = self.lru.remove(0);
            if let Some(evicted) = self.tiles.remove(&oldest) {
                self.total_bytes = self.total_bytes.saturating_sub(evicted.rendered.byte_len());
            }
        }
        self.total_bytes += bytes;
        self.tiles.insert(key, Tile { tile_idx, file_idx, lod, rendered });
        self.lru.push(key);
    }

    fn get(&self, file_idx: usize, lod: u8, tile_idx: usize) -> Option<&Tile> {
        self.tiles.get(&(file_idx, lod, tile_idx))
    }

    fn touch(&mut self, key: CacheKey) {
        self.lru.retain(|k| k != &key);
        self.lru.push(key);
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
                self.lru.retain(|k| k != &key);
            }
        }
    }

    fn clear_for_file(&mut self, file_idx: usize) {
        let keys: Vec<_> = self.tiles.keys().copied().filter(|k| k.0 == file_idx).collect();
        for key in keys {
            if let Some(evicted) = self.tiles.remove(&key) {
                self.total_bytes = self.total_bytes.saturating_sub(evicted.rendered.byte_len());
                self.lru.retain(|k| k != &key);
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
    lru: Vec<ChromaKey>,
    total_bytes: usize,
}

impl ChromaTileCache {
    fn new() -> Self {
        Self { tiles: HashMap::new(), lru: Vec::new(), total_bytes: 0 }
    }

    fn insert(&mut self, file_idx: usize, tile_idx: usize, rendered: PreRendered) {
        let key = (file_idx, tile_idx);
        let bytes = rendered.byte_len();
        if let Some(old) = self.tiles.remove(&key) {
            self.total_bytes = self.total_bytes.saturating_sub(old.rendered.byte_len());
            self.lru.retain(|k| k != &key);
        }
        while self.total_bytes + bytes > MAX_BYTES && !self.lru.is_empty() {
            let oldest = self.lru.remove(0);
            if let Some(evicted) = self.tiles.remove(&oldest) {
                self.total_bytes = self.total_bytes.saturating_sub(evicted.rendered.byte_len());
            }
        }
        self.total_bytes += bytes;
        self.tiles.insert(key, Tile { tile_idx, file_idx, lod: 1, rendered });
        self.lru.push(key);
    }

    fn get(&self, file_idx: usize, tile_idx: usize) -> Option<&Tile> {
        self.tiles.get(&(file_idx, tile_idx))
    }

    fn touch(&mut self, key: ChromaKey) {
        self.lru.retain(|k| k != &key);
        self.lru.push(key);
    }
}

thread_local! {
    /// Unified magnitude tile cache — all LOD levels in one cache.
    static CACHE: RefCell<TileCache> = RefCell::new(TileCache::new());
    /// Map of (file_idx, lod, tile_idx) → timestamp (ms) for tiles currently being generated.
    /// Entries older than IN_FLIGHT_TIMEOUT_MS are considered stuck and can be re-scheduled.
    static IN_FLIGHT: RefCell<HashMap<CacheKey, f64>> =
        RefCell::new(HashMap::new());
    /// Generation counter — incremented on every clear_all_tiles(). In-flight
    /// async tasks capture the generation at spawn time and discard their result
    /// if a clear happened while they were computing (prevents stale tiles).
    static CACHE_GENERATION: RefCell<u64> = RefCell::new(0);

    /// Flow-mode tile cache — multi-LOD, same CacheKey as magnitude tiles.
    static FLOW_CACHE: RefCell<TileCache> = RefCell::new(TileCache::new());
    static FLOW_IN_FLIGHT: RefCell<HashMap<CacheKey, f64>> =
        RefCell::new(HashMap::new());

    /// Reassignment spectrogram tile cache — multi-LOD, same CacheKey as magnitude tiles.
    static REASSIGN_CACHE: RefCell<TileCache> = RefCell::new(TileCache::new());
    static REASSIGN_IN_FLIGHT: RefCell<HashMap<CacheKey, f64>> =
        RefCell::new(HashMap::new());

    /// Chromagram tile cache (LOD1-only).
    static CHROMA_CACHE: RefCell<ChromaTileCache> = RefCell::new(ChromaTileCache::new());
    static CHROMA_IN_FLIGHT: RefCell<HashMap<ChromaKey, f64>> =
        RefCell::new(HashMap::new());

    /// Cached per-file global chromagram normalisation maxima (max_class, max_note).
    static CHROMA_GLOBAL_MAX: RefCell<HashMap<usize, (f32, f32)>> =
        RefCell::new(HashMap::new());
}

// ── IN_FLIGHT helpers ────────────────────────────────────────────────────────

/// Check if a key is actively in-flight (not stale). Returns true if the
/// tile is being computed and should not be re-scheduled.
fn is_in_flight_active<K: Eq + std::hash::Hash>(map: &HashMap<K, f64>, key: &K) -> bool {
    match map.get(key) {
        None => false,
        Some(&ts) => js_sys::Date::now() - ts <= IN_FLIGHT_TIMEOUT_MS,
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
                c.borrow().tiles.get(&key).map(|t| f(t))
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
}

/// Clear all tile caches (main, flow, reassign, chroma). Used when a global
/// parameter like channel_view changes and all tiles need recomputation.
pub fn clear_all_caches() {
    clear_all_tiles();
    clear_flow_cache();
    clear_reassign_cache();
    clear_chroma_cache();
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
                cache.lru.retain(|k| k != &key);
            }
        }
    });
}

/// Remove IN_FLIGHT entries far from the current viewport center.
/// Prevents old in-flight computations from blocking cache resources
/// when the user scrolls fast past them.
pub fn cancel_far_in_flight(file_idx: usize, lod: u8, center_tile: usize, keep_radius: usize) {
    IN_FLIGHT.with(|s| {
        s.borrow_mut().retain(|&(fi, l, ti), _| {
            fi != file_idx || l != lod || ti.abs_diff(center_tile) <= keep_radius
        });
    });
}

/// Cancel in-flight entries far from ALL given centers.
pub fn cancel_far_in_flight_multi(file_idx: usize, lod: u8, centers: &[(usize, usize)]) {
    IN_FLIGHT.with(|s| {
        s.borrow_mut().retain(|&(fi, l, ti), _| {
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
            let inflight = s.borrow();
            (first_tile..=last_tile).filter(|&t| {
                let key = (file_idx, lod, t);
                !cache.tiles.contains_key(&key) && !inflight.contains_key(&key)
            }).count()
        })
    })
}

/// Returns (used_bytes, max_bytes) for the magnitude tile cache.
pub fn cache_usage() -> (usize, usize) {
    CACHE.with(|c| (c.borrow().total_bytes, MAX_BYTES))
}

/// Returns the number of complete LOD1 tiles for a file currently in the cache.
pub fn tiles_ready(file_idx: usize, n_tiles: usize) -> usize {
    CACHE.with(|c| {
        let cache = c.borrow();
        (0..n_tiles).filter(|&i| cache.tiles.contains_key(&(file_idx, 1, i))).count()
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
    if IN_FLIGHT.with(|s| is_in_flight_active(&s.borrow(), &key)) { return; }

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
    let actual_fft = fft_mode.fft_for_lod(config_hop);

    spawn_local(async move {
        yield_to_browser().await;

        // Extra yields for expensive LODs (LOD2, LOD3) and non-current files
        if lod >= 2 {
            yield_to_browser().await;
        }
        let is_current = state.current_file_index.get_untracked() == Some(file_idx);
        if !is_current {
            for _ in 0..3 {
                yield_to_browser().await;
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
        streaming_source::prefetch_streaming(audio.source.as_ref(), padded_start as u64, padded_len).await;

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

        let cols = compute_stft_columns(&samples, effective_rate, actual_fft, config_hop, 0, TILE_COLS);
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

// ── LOD 1-specific scheduling (from in-memory columns / spectral store) ─────

/// Schedule generation of a LOD1 tile from in-memory spectrogram columns.
/// Used during initial file loading when LoadedFile.spectrogram.columns is available.
pub fn schedule_tile(state: AppState, file: LoadedFile, file_idx: usize, tile_idx: usize) {
    let key: CacheKey = (file_idx, 1, tile_idx);
    if CACHE.with(|c| c.borrow().tiles.contains_key(&key)) { return; }
    if IN_FLIGHT.with(|s| is_in_flight_active(&s.borrow(), &key)) { return; }
    IN_FLIGHT.with(|s| s.borrow_mut().insert(key, js_sys::Date::now()));

    spawn_local(async move {
        yield_to_browser().await;

        let is_current = state.current_file_index.get_untracked() == Some(file_idx);
        if !is_current {
            for _ in 0..3 {
                yield_to_browser().await;
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

        CACHE.with(|c| c.borrow_mut().insert(file_idx, 1, tile_idx, rendered));
        IN_FLIGHT.with(|s| s.borrow_mut().remove(&key));
        state.tile_ready_signal.update(|n| *n = n.wrapping_add(1));
    });
}

/// Schedule generation of all LOD1 tiles for a file (called after file load).
pub fn schedule_all_tiles(state: AppState, file: LoadedFile, file_idx: usize) {
    let total_cols = if file.spectrogram.total_columns > 0 {
        file.spectrogram.total_columns
    } else {
        file.spectrogram.columns.len()
    };
    if total_cols == 0 { return; }
    let n_tiles = (total_cols + TILE_COLS - 1) / TILE_COLS;

    for tile_idx in 0..n_tiles {
        schedule_tile(state.clone(), file.clone(), file_idx, tile_idx);
    }
}

/// Render a LOD1 tile synchronously from the spectral column store.
pub fn render_tile_from_store_sync(file_idx: usize, tile_idx: usize) -> bool {
    use crate::canvas::spectral_store;

    let key: CacheKey = (file_idx, 1, tile_idx);
    if CACHE.with(|c| c.borrow().tiles.contains_key(&key)) { return true; }

    let col_start = tile_idx * TILE_COLS;
    let col_end = col_start + TILE_COLS;

    let rendered = spectral_store::with_columns(file_idx, col_start, col_end, |cols, _max_mag| {
        spectrogram_renderer::pre_render_columns(cols)
    });

    if let Some(rendered) = rendered {
        CACHE.with(|c| c.borrow_mut().insert(file_idx, 1, tile_idx, rendered));
        true
    } else {
        false
    }
}

/// Render a partial (live) LOD1 tile from the spectral store.
pub fn render_live_tile_sync(file_idx: usize, tile_idx: usize, col_start: usize, available_cols: usize) -> bool {
    use crate::canvas::spectral_store;

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
        CACHE.with(|c| c.borrow_mut().insert(file_idx, 1, tile_idx, rendered));
        true
    } else {
        false
    }
}

/// Schedule LOD1 tile generation from the spectral column store.
pub fn schedule_tile_from_store(state: AppState, file_idx: usize, tile_idx: usize) {
    use crate::canvas::spectral_store;

    let key: CacheKey = (file_idx, 1, tile_idx);
    if CACHE.with(|c| c.borrow().tiles.contains_key(&key)) { return; }
    if IN_FLIGHT.with(|s| is_in_flight_active(&s.borrow(), &key)) { return; }
    IN_FLIGHT.with(|s| s.borrow_mut().insert(key, js_sys::Date::now()));

    spawn_local(async move {
        yield_to_browser().await;

        // Defer while audio is playing — playback chunk processing needs
        // uncontested CPU and I/O to avoid gaps
        if state.is_playing.get_untracked() {
            IN_FLIGHT.with(|s| s.borrow_mut().remove(&key));
            return;
        }

        let is_current = state.current_file_index.get_untracked() == Some(file_idx);
        if !is_current {
            for _ in 0..3 {
                yield_to_browser().await;
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

        if let Some(rendered) = rendered {
            CACHE.with(|c| c.borrow_mut().insert(file_idx, 1, tile_idx, rendered));
        }
        // Always bump signal so render effect retries even if store had no data
        state.tile_ready_signal.update(|n| *n = n.wrapping_add(1));
    });
}

/// Schedule visible LOD1 tiles from the spectral store.
pub fn schedule_visible_tiles_from_store(state: AppState, file_idx: usize, total_cols: usize) {
    if total_cols == 0 { return; }
    let n_tiles = (total_cols + TILE_COLS - 1) / TILE_COLS;

    let time_res = state.files.with_untracked(|files| {
        files.get(file_idx).map(|f| f.spectrogram.time_resolution).unwrap_or(0.01)
    });
    let scroll = state.scroll_offset.get_untracked();
    let zoom = state.zoom_level.get_untracked();
    let canvas_w = state.spectrogram_canvas_width.get_untracked();
    let visible_time = if zoom > 0.0 { canvas_w / zoom * time_res } else { 1.0 };
    let center_col = ((scroll + visible_time / 2.0) / time_res) as usize;
    let center_tile = center_col / TILE_COLS;

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
            schedule_tile_from_store(state.clone(), file_idx, t);
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

        if reassign && lod > 0 {
            schedule_reassign_tile(state, file_idx, lod, t);
        }
    }
}

/// Schedule LOD1 on-demand tile computation from audio samples.
pub fn schedule_tile_on_demand(
    state: AppState,
    file_idx: usize,
    tile_idx: usize,
) {
    use crate::canvas::spectral_store;
    use crate::dsp::fft::compute_stft_columns;

    let key: CacheKey = (file_idx, 1, tile_idx);
    if CACHE.with(|c| c.borrow().tiles.contains_key(&key)) { return; }
    if IN_FLIGHT.with(|s| is_in_flight_active(&s.borrow(), &key)) { return; }

    // Bounds check: reject tiles past the audio data
    let total_samples = state.files.with_untracked(|files| {
        files.get(file_idx).map(|f| f.audio.source.total_samples() as usize).unwrap_or(0)
    });
    let max_tiles = tile_count_for_samples(total_samples, 1);
    if tile_idx >= max_tiles { return; }

    IN_FLIGHT.with(|s| s.borrow_mut().insert(key, js_sys::Date::now()));

    spawn_local(async move {
        yield_to_browser().await;

        // Defer while audio is playing — playback needs uncontested I/O and CPU
        if state.is_playing.get_untracked() {
            IN_FLIGHT.with(|s| s.borrow_mut().remove(&key));
            return;
        }

        let is_current = state.current_file_index.get_untracked() == Some(file_idx);
        if !is_current {
            for _ in 0..3 {
                yield_to_browser().await;
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
        let hop_size = 512usize;
        let fft_size = 2048usize;

        // Read only the sample region needed for this tile
        let sample_start = col_start * hop_size;
        let sample_len = TILE_COLS * hop_size + fft_size;

        // Prefetch for streaming sources
        streaming_source::prefetch_streaming(audio.source.as_ref(), sample_start as u64, sample_len).await;

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

        CACHE.with(|c| c.borrow_mut().insert(file_idx, 1, tile_idx, rendered));
        IN_FLIGHT.with(|s| s.borrow_mut().remove(&key));
        state.tile_ready_signal.update(|n| *n = n.wrapping_add(1));
    });
}

// ── Flow tile cache (LOD1-only) ──────────────────────────────────────────────

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
                c.borrow().tiles.get(&key).map(|t| f(t))
            })
        } else {
            None
        }
    })
}

pub fn clear_flow_cache() {
    FLOW_CACHE.with(|c| c.borrow_mut().clear_all());
    FLOW_IN_FLIGHT.with(|s| s.borrow_mut().clear());
}

pub fn clear_flow_file(file_idx: usize) {
    FLOW_CACHE.with(|c| c.borrow_mut().clear_for_file(file_idx));
    FLOW_IN_FLIGHT.with(|s| s.borrow_mut().retain(|k, _| k.0 != file_idx));
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
    if FLOW_IN_FLIGHT.with(|s| is_in_flight_active(&s.borrow(), &key)) { return; }

    let total_samples = state.files.with_untracked(|files| {
        files.get(file_idx).map(|f| f.audio.source.total_samples() as usize).unwrap_or(0)
    });
    let max_tiles = tile_count_for_samples(total_samples, lod);
    if tile_idx >= max_tiles { return; }

    FLOW_IN_FLIGHT.with(|s| s.borrow_mut().insert(key, js_sys::Date::now()));

    let config_hop = LOD_CONFIGS[lod as usize].hop_size;
    let actual_fft = state.spect_fft_mode.get_untracked().fft_for_lod(config_hop);

    spawn_local(async move {
        yield_to_browser().await;

        // Extra yields for expensive LODs and non-current files
        if lod >= 2 {
            yield_to_browser().await;
        }
        let is_current = state.current_file_index.get_untracked() == Some(file_idx);
        if !is_current {
            for _ in 0..3 { yield_to_browser().await; }
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

                let total = audio.source.total_samples() as usize;
                let sample_end = (sample_start + sample_len).min(total);
                if sample_start >= total || sample_start >= sample_end {
                    FLOW_IN_FLIGHT.with(|s| s.borrow_mut().remove(&key));
                    return;
                }

                let ch_samples = audio.source.read_region(cv, sample_start as u64, sample_end - sample_start);
                let samples = &ch_samples[..];

                yield_to_browser().await;

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
                c.borrow().tiles.get(&key).map(|t| f(t))
            })
        } else {
            None
        }
    })
}

pub fn clear_reassign_cache() {
    REASSIGN_CACHE.with(|c| c.borrow_mut().clear_all());
    REASSIGN_IN_FLIGHT.with(|s| s.borrow_mut().clear());
}

pub fn clear_reassign_file(file_idx: usize) {
    REASSIGN_CACHE.with(|c| c.borrow_mut().clear_for_file(file_idx));
    REASSIGN_IN_FLIGHT.with(|s| s.borrow_mut().retain(|k, _| k.0 != file_idx));
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
    if REASSIGN_IN_FLIGHT.with(|s| is_in_flight_active(&s.borrow(), &key)) { return; }

    let total_samples = state.files.with_untracked(|files| {
        files.get(file_idx).map(|f| f.audio.source.total_samples() as usize).unwrap_or(0)
    });
    let max_tiles = tile_count_for_samples(total_samples, lod);
    if tile_idx >= max_tiles { return; }

    REASSIGN_IN_FLIGHT.with(|s| s.borrow_mut().insert(key, js_sys::Date::now()));

    let config_hop = LOD_CONFIGS[lod as usize].hop_size;
    let actual_fft = state.spect_fft_mode.get_untracked().fft_for_lod(config_hop);

    spawn_local(async move {
        yield_to_browser().await;

        // Extra yields: 3x FFT cost + expensive LODs
        if lod >= 2 { yield_to_browser().await; }
        yield_to_browser().await;

        let is_current = state.current_file_index.get_untracked() == Some(file_idx);
        if !is_current {
            for _ in 0..3 { yield_to_browser().await; }
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

        let total = audio.source.total_samples() as usize;
        let sample_end = (sample_start + sample_len).min(total);
        if sample_start >= total || sample_start >= sample_end {
            REASSIGN_IN_FLIGHT.with(|s| s.borrow_mut().remove(&key));
            return;
        }

        let ch_samples = audio.source.read_region(cv, sample_start as u64, sample_end - sample_start);
        let samples = &ch_samples[..];

        yield_to_browser().await;

        let rendered = compute_reassigned_tile(
            samples, TILE_COLS, actual_fft, config_hop, -60.0,
        );

        REASSIGN_CACHE.with(|c| c.borrow_mut().insert(file_idx, lod, tile_idx, rendered));
        REASSIGN_IN_FLIGHT.with(|s| s.borrow_mut().remove(&key));
        state.tile_ready_signal.update(|n| *n = n.wrapping_add(1));
    });
}

// ── Chromagram tile cache (LOD1-only) ────────────────────────────────────────

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
                c.borrow().tiles.get(&key).map(|t| f(t))
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

/// Schedule a chromagram tile for background generation (LOD1).
pub fn schedule_chroma_tile(
    state: AppState,
    file_idx: usize,
    tile_idx: usize,
) {
    use crate::canvas::spectral_store;
    use crate::dsp::chromagram;

    let key = (file_idx, tile_idx);
    if CHROMA_CACHE.with(|c| c.borrow().tiles.contains_key(&key)) { return; }
    if CHROMA_IN_FLIGHT.with(|s| is_in_flight_active(&s.borrow(), &key)) { return; }
    CHROMA_IN_FLIGHT.with(|s| s.borrow_mut().insert(key, js_sys::Date::now()));

    spawn_local(async move {
        yield_to_browser().await;

        let is_current = state.current_file_index.get_untracked() == Some(file_idx);
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

        let global_max = CHROMA_GLOBAL_MAX.with(|m| m.borrow().get(&file_idx).copied());
        let (max_class, max_note) = if let Some(gm) = global_max {
            gm
        } else {
            let from_store = spectral_store::compute_chroma_global_max(file_idx, freq_res);
            let gm = from_store.unwrap_or_else(|| {
                state.files.with_untracked(|files| {
                    files.get(file_idx)
                        .filter(|f| !f.spectrogram.columns.is_empty())
                        .map(|f| chromagram::compute_chroma_max(&f.spectrogram.columns, freq_res))
                        .unwrap_or((0.0, 0.0))
                })
            });
            if gm.0 > 0.0 {
                CHROMA_GLOBAL_MAX.with(|m| m.borrow_mut().insert(file_idx, gm));
            }
            gm
        };

        let result = spectral_store::with_columns(file_idx, col_start, col_start + TILE_COLS, |cols, _max_mag| {
            chromagram::pre_render_chromagram_columns(cols, freq_res, max_class, max_note)
        });

        let rendered = if let Some(r) = result {
            r
        } else {
            let fallback = state.files.with_untracked(|files| {
                files.get(file_idx).and_then(|f| {
                    if f.spectrogram.columns.is_empty() { return None; }
                    let end = (col_start + TILE_COLS).min(f.spectrogram.columns.len());
                    if col_start >= end { return None; }
                    Some(chromagram::pre_render_chromagram_columns(
                        &f.spectrogram.columns[col_start..end],
                        freq_res,
                        max_class,
                        max_note,
                    ))
                })
            });
            match fallback {
                Some(r) => r,
                None => {
                    CHROMA_IN_FLIGHT.with(|s| s.borrow_mut().remove(&key));
                    return;
                }
            }
        };

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
    static BG_PRELOAD: RefCell<Option<BgPreloadState>> = RefCell::new(None);
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
                let _ = web_sys::window().unwrap().clear_timeout_with_handle(h);
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
                let _ = web_sys::window().unwrap().clear_timeout_with_handle(h);
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
            c.borrow().total_bytes >= MAX_BYTES * 9 / 10
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
