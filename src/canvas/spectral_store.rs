//! Thread-local store for lazily-computed STFT columns.
//!
//! During file loading, spectrogram columns are inserted incrementally as
//! they are computed.  The tile cache reads completed tile-ranges from here
//! to render RGBA tiles progressively, instead of waiting for the entire
//! spectrogram to finish.
//!
//! Supports multiple concurrent file loads — each file_idx gets its own store.
//!
//! For large files, the store persists after loading and uses a memory budget
//! with LRU-by-distance eviction.  Evicted columns can be recomputed from
//! `AudioData.samples` via `tile_cache::schedule_tile_on_demand()`.

use std::cell::RefCell;
use std::collections::HashMap;
use crate::types::SpectrogramColumn;

/// Memory budget for all spectral stores combined (~200 MB).
const MAX_STORE_BYTES: usize = 200 * 1024 * 1024;

/// Approximate bytes per column (1025 f32 magnitudes + f64 time_offset + Vec overhead).
const APPROX_BYTES_PER_COL: usize = 1025 * 4 + 8 + 24;

struct SpectralColumnStore {
    /// Columns indexed by spectrogram column number.  `None` = not yet computed / evicted.
    columns: Vec<Option<SpectrogramColumn>>,
    /// FFT size used to compute these columns.
    fft_size: usize,
    /// Running maximum magnitude across all inserted columns.
    max_magnitude: f32,
    /// Number of `Some` columns currently stored.
    present_count: usize,
}

thread_local! {
    /// Keyed by file_idx so multiple files can load concurrently.
    static STORES: RefCell<HashMap<usize, SpectralColumnStore>> =
        RefCell::new(HashMap::new());
}

/// Total approximate bytes used across all stores.
fn total_bytes(stores: &HashMap<usize, SpectralColumnStore>) -> usize {
    stores.values().map(|s| s.present_count * APPROX_BYTES_PER_COL).sum()
}

/// Initialise (or reset) the store for a file.
pub fn init(file_idx: usize, total_cols: usize, fft_size: usize) {
    STORES.with(|s| {
        s.borrow_mut().insert(file_idx, SpectralColumnStore {
            columns: (0..total_cols).map(|_| None).collect(),
            fft_size,
            max_magnitude: 0.0,
            present_count: 0,
        });
    });
}

/// Insert computed columns into the store, starting at `start_col`.
/// Updates the running max magnitude.  If the global memory budget is
/// exceeded, evicts columns from the store furthest from `start_col`.
pub fn insert_columns(file_idx: usize, start_col: usize, cols: &[SpectrogramColumn]) {
    STORES.with(|s| {
        let mut stores = s.borrow_mut();
        let Some(store) = stores.get_mut(&file_idx) else { return };
        for (i, col) in cols.iter().enumerate() {
            let idx = start_col + i;
            if idx < store.columns.len() {
                for &mag in &col.magnitudes {
                    if mag > store.max_magnitude {
                        store.max_magnitude = mag;
                    }
                }
                if store.columns[idx].is_none() {
                    store.present_count += 1;
                }
                store.columns[idx] = Some(col.clone());
            }
        }

        // Enforce memory budget by evicting columns far from the insertion center
        let center = start_col + cols.len() / 2;
        evict_over_budget(&mut stores, file_idx, center);
    });
}

/// Evict columns across all stores until total memory is under budget.
/// Prefers evicting columns in other files first, then columns far from `center`
/// in the given `primary_file`.
fn evict_over_budget(
    stores: &mut HashMap<usize, SpectralColumnStore>,
    primary_file: usize,
    center: usize,
) {
    while total_bytes(stores) > MAX_STORE_BYTES {
        // Try to evict from non-primary files first
        let mut evicted = false;
        for (&fi, store) in stores.iter_mut() {
            if fi == primary_file { continue; }
            if store.present_count > 0 {
                // Evict the column furthest from the middle of this store
                let mid = store.columns.len() / 2;
                if let Some(idx) = farthest_present(&store.columns, mid) {
                    store.columns[idx] = None;
                    store.present_count -= 1;
                    evicted = true;
                    break;
                }
            }
        }
        if evicted { continue; }

        // Evict from primary file: column furthest from `center`
        if let Some(store) = stores.get_mut(&primary_file) {
            if store.present_count == 0 { break; }
            if let Some(idx) = farthest_present(&store.columns, center) {
                store.columns[idx] = None;
                store.present_count -= 1;
            } else {
                break;
            }
        } else {
            break;
        }
    }
}

/// Find the index of the present column furthest from `center`.
fn farthest_present(columns: &[Option<SpectrogramColumn>], center: usize) -> Option<usize> {
    let mut best: Option<(usize, usize)> = None; // (index, distance)
    for (i, col) in columns.iter().enumerate() {
        if col.is_some() {
            let dist = i.abs_diff(center);
            if best.map_or(true, |(_, d)| dist > d) {
                best = Some((i, dist));
            }
        }
    }
    best.map(|(i, _)| i)
}

/// Extend the store's column capacity to accommodate at least `new_total` columns.
/// Only grows, never shrinks. If the store doesn't exist, creates it.
/// Used during live recording where total column count grows incrementally.
pub fn ensure_capacity(file_idx: usize, new_total: usize) {
    STORES.with(|s| {
        let mut stores = s.borrow_mut();
        match stores.get_mut(&file_idx) {
            Some(store) => {
                if new_total > store.columns.len() {
                    store.columns.resize_with(new_total, || None);
                }
            }
            None => {
                stores.insert(file_idx, SpectralColumnStore {
                    columns: (0..new_total).map(|_| None).collect(),
                    fft_size: 0,
                    max_magnitude: 0.0,
                    present_count: 0,
                });
            }
        }
    });
}

/// Check whether all columns in the range `tile_start..tile_end` are present.
pub fn tile_complete(file_idx: usize, tile_start: usize, tile_end: usize) -> bool {
    STORES.with(|s| {
        let stores = s.borrow();
        let Some(store) = stores.get(&file_idx) else { return false };
        let end = tile_end.min(store.columns.len());
        (tile_start..end).all(|i| store.columns[i].is_some())
    })
}

/// Borrow a completed slice of columns from the store and call `f` with them.
/// Returns `None` if any column in the range is missing or the store doesn't
/// exist for `file_idx`.
pub fn with_columns<R>(
    file_idx: usize,
    start: usize,
    end: usize,
    f: impl FnOnce(&[SpectrogramColumn], f32) -> R,
) -> Option<R> {
    STORES.with(|s| {
        let stores = s.borrow();
        let store = stores.get(&file_idx)?;
        let end = end.min(store.columns.len());
        let cols: Vec<&SpectrogramColumn> = (start..end)
            .map(|i| store.columns.get(i)?.as_ref())
            .collect::<Option<Vec<_>>>()?;
        let owned: Vec<SpectrogramColumn> = cols.into_iter().cloned().collect();
        Some(f(&owned, store.max_magnitude))
    })
}

/// Get the current running max magnitude.
pub fn get_max_magnitude(file_idx: usize) -> f32 {
    STORES.with(|s| {
        s.borrow().get(&file_idx)
            .map(|st| st.max_magnitude)
            .unwrap_or(0.0)
    })
}

/// Compute the global chromagram normalisation maxima (max_class, max_note)
/// by iterating all present columns in the store **by reference** (no cloning).
/// Returns `None` if the store doesn't exist or has no data.
pub fn compute_chroma_global_max(file_idx: usize, freq_resolution: f64) -> Option<(f32, f32)> {
    use crate::dsp::chromagram::stft_to_chromagram;
    STORES.with(|s| {
        let stores = s.borrow();
        let store = stores.get(&file_idx)?;
        let mut max_class = 0.0f32;
        let mut max_note = 0.0f32;
        for col in store.columns.iter().flatten() {
            let ch = stft_to_chromagram(&col.magnitudes, freq_resolution);
            for &v in &ch.pitch_classes { max_class = max_class.max(v); }
            for octaves in &ch.octave_detail {
                for &v in octaves { max_note = max_note.max(v); }
            }
        }
        if max_class > 0.0 { Some((max_class, max_note)) } else { None }
    })
}

/// Check whether a store exists for a file (i.e. it's still alive for large-file mode).
pub fn has_store(file_idx: usize) -> bool {
    STORES.with(|s| s.borrow().contains_key(&file_idx))
}

/// Return the FFT size associated with a file's stored columns.
pub fn fft_size(file_idx: usize) -> Option<usize> {
    STORES.with(|s| s.borrow().get(&file_idx).map(|store| store.fft_size))
}

/// Check whether a file's stored columns were computed with the expected FFT size.
pub fn fft_matches(file_idx: usize, expected_fft_size: usize) -> bool {
    fft_size(file_idx) == Some(expected_fft_size)
}

/// Drain all columns from the store and return them as a contiguous Vec.
/// Missing columns are replaced with empty `SpectrogramColumn` structs.
/// Removes this file's store entry afterwards.
pub fn drain_columns(file_idx: usize) -> Option<Vec<SpectrogramColumn>> {
    STORES.with(|s| {
        let mut stores = s.borrow_mut();
        let store = stores.remove(&file_idx)?;
        let result: Vec<SpectrogramColumn> = store.columns.into_iter()
            .map(|opt| opt.unwrap_or_else(|| SpectrogramColumn {
                magnitudes: Vec::new(),
                time_offset: 0.0,
            }))
            .collect();
        Some(result)
    })
}

/// Remove a specific file's store (e.g. when a file is unloaded).
pub fn clear_file(file_idx: usize) {
    STORES.with(|s| { s.borrow_mut().remove(&file_idx); });
}

/// Clear all stores.
pub fn clear() {
    STORES.with(|s| s.borrow_mut().clear());
}
