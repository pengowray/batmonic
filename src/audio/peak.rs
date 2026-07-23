use crate::audio::source::ChannelView;
use crate::state::store_fields::*;
use crate::state::AppState;
use leptos::prelude::*;
use std::collections::HashMap;

const CHUNK_SIZE: usize = 256 * 1024; // 256K samples per chunk

/// Key for the selection peak cache: (file_index, start_sample, end_sample).
/// Using sample indices avoids floating-point comparison issues.
type SelectionPeakKey = (usize, u64, u64);

/// LRU-style cache for recently computed selection peaks.
/// Keeps the most recent entries so switching between selections is instant.
const MAX_CACHE_ENTRIES: usize = 16;

/// Shared cache for selection peak values. Stored as a signal so reactive
/// code can subscribe to updates.
#[derive(Clone, Debug, Default)]
pub struct PeakCache {
    entries: HashMap<SelectionPeakKey, Option<f64>>,
    /// Insertion order for LRU eviction.
    order: Vec<SelectionPeakKey>,
}

impl PeakCache {
    pub fn get(&self, key: &SelectionPeakKey) -> Option<&Option<f64>> {
        self.entries.get(key)
    }

    pub fn insert(&mut self, key: SelectionPeakKey, value: Option<f64>) {
        if !self.entries.contains_key(&key) {
            self.order.push(key);
            // Evict oldest if over capacity
            while self.order.len() > MAX_CACHE_ENTRIES {
                let old = self.order.remove(0);
                self.entries.remove(&old);
            }
        }
        self.entries.insert(key, value);
    }

    /// Invalidate all entries for a given file index (e.g. when file is replaced).
    pub fn invalidate_file(&mut self, file_index: usize) {
        self.order.retain(|k| k.0 != file_index);
        self.entries.retain(|k, _| k.0 != file_index);
    }
}

/// Schedule an async full-file peak scan. Updates `cached_full_peak_db` on the
/// LoadedFile at `file_index` when done. For files <= 30s the peak is already
/// known from the initial scan, so we just copy `cached_peak_db`.
pub fn start_full_peak_scan(state: AppState, file_index: usize) {
    let files = state.library.files().get_untracked();
    let Some(file) = files.get(file_index) else {
        return;
    };
    let duration = file.audio.duration_secs;
    let sr = file.audio.sample_rate;

    // For short files (<= 30s), the first-30s peak IS the full-file peak
    if duration <= 30.0 {
        let peak = file.cached_peak_db;
        drop(files);
        state.library.files().update(|files| {
            if let Some(f) = files.get_mut(file_index) {
                f.cached_full_peak_db = peak;
            }
        });
        return;
    }

    let source = file.audio.source.clone();
    let total_samples = source.total_samples() as usize;
    drop(files);

    leptos::task::spawn_local(async move {
        let mut peak: f32 = 0.0;
        let mut offset: u64 = 0;

        while (offset as usize) < total_samples {
            let remaining = total_samples - offset as usize;
            let chunk_len = remaining.min(CHUNK_SIZE);
            let samples = source.read_region(ChannelView::MonoMix, offset, chunk_len);
            let chunk_peak = samples.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
            if chunk_peak > peak {
                peak = chunk_peak;
            }
            offset += chunk_len as u64;

            // Yield to keep UI responsive
            let promise = js_sys::Promise::new(&mut |resolve, _| {
                web_sys::window()
                    .unwrap()
                    .set_timeout_with_callback(&resolve)
                    .unwrap();
            });
            let _ = wasm_bindgen_futures::JsFuture::from(promise).await;

            // Check file is still at same index (user may have removed it)
            let still_valid = state.library.files().with_untracked(|files| {
                files
                    .get(file_index)
                    .map(|f| {
                        f.audio.sample_rate == sr
                            && f.audio.source.total_samples() as usize == total_samples
                    })
                    .unwrap_or(false)
            });
            if !still_valid {
                return;
            }
        }

        let peak_db = if peak > 1e-10 {
            Some(20.0 * (peak as f64).log10())
        } else {
            None
        };

        state.library.files().update(|files| {
            if let Some(f) = files.get_mut(file_index) {
                f.cached_full_peak_db = peak_db;
            }
        });
    });
}

/// Start an async selection peak computation. The result is stored in the
/// selection peak cache signal. If already cached, does nothing.
/// Returns the cache key so callers can look up the result.
pub fn start_selection_peak_scan(
    state: AppState,
    file_index: usize,
    start_sample: u64,
    end_sample: u64,
) {
    let key: SelectionPeakKey = (file_index, start_sample, end_sample);

    // Already cached?
    if state
        .gain
        .selection_peak_cache()
        .with_untracked(|c| c.get(&key).is_some())
    {
        return;
    }

    let files = state.library.files().get_untracked();
    let Some(file) = files.get(file_index) else {
        return;
    };
    let sr = file.audio.sample_rate;
    let source = file.audio.source.clone();
    let total_file_samples = source.total_samples();
    drop(files);

    let len = (end_sample - start_sample) as usize;

    // For small selections (< 2 chunks), compute synchronously — fast enough
    if len <= CHUNK_SIZE * 2 {
        let samples = source.read_region(ChannelView::MonoMix, start_sample, len);
        let peak = samples.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        let peak_db = if peak > 1e-10 {
            Some(20.0 * (peak as f64).log10())
        } else {
            None
        };
        state
            .gain
            .selection_peak_cache()
            .update(|c| c.insert(key, peak_db));
        return;
    }

    // Large selection — compute async
    leptos::task::spawn_local(async move {
        let mut peak: f32 = 0.0;
        let mut offset = start_sample;

        while offset < end_sample {
            let remaining = (end_sample - offset) as usize;
            let chunk_len = remaining.min(CHUNK_SIZE);
            let samples = source.read_region(ChannelView::MonoMix, offset, chunk_len);
            let chunk_peak = samples.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
            if chunk_peak > peak {
                peak = chunk_peak;
            }
            offset += chunk_len as u64;

            // Yield to keep UI responsive
            let promise = js_sys::Promise::new(&mut |resolve, _| {
                web_sys::window()
                    .unwrap()
                    .set_timeout_with_callback(&resolve)
                    .unwrap();
            });
            let _ = wasm_bindgen_futures::JsFuture::from(promise).await;

            // Verify file is still valid
            let still_valid = state.library.files().with_untracked(|files| {
                files
                    .get(file_index)
                    .map(|f| {
                        f.audio.sample_rate == sr
                            && f.audio.source.total_samples() == total_file_samples
                    })
                    .unwrap_or(false)
            });
            if !still_valid {
                return;
            }
        }

        let peak_db = if peak > 1e-10 {
            Some(20.0 * (peak as f64).log10())
        } else {
            None
        };

        state
            .gain
            .selection_peak_cache()
            .update(|c| c.insert(key, peak_db));
    });
}
