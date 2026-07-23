//! Platform-independent core of the spectrogram tile cache.
//!
//! This holds the pure, host-testable pieces shared by every tile cache in the
//! WASM frontend: the LRU + byte-capped [`TileCache`] container and the
//! in-flight request bookkeeping ([`in_flight_is_active`] /
//! [`in_flight_active_count`]). The wasm-coupled machinery — thread-locals,
//! `spawn_local` scheduling, generation counters wired to the leptos
//! `tile_ready_signal`, and `js_sys::Date` time — stays in the frontend
//! (`src/canvas/tile_cache.rs`), which re-exports the types from here.
//!
//! Keeping this layer pure lets the eviction / byte-accounting / timeout logic
//! be exercised by `cargo test` (the frontend crate can't be host-compiled).

use crate::types::PreRendered;
use std::collections::{HashMap, VecDeque};
use std::hash::Hash;

/// Multi-LOD tile key: identifies a tile by file, level-of-detail, and tile index.
///
/// A distinct struct (rather than a bare `(usize, u8, usize)` tuple) so the
/// compiler prevents transposing `file_idx`/`tile_idx` (both `usize`) and
/// prevents mixing it with the structurally-similar [`ChromaKey`]. Shared by the
/// magnitude / flow / reassign / resonator caches, their in-flight maps, and the
/// renderer's tile-canvas LRU.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct TileKey {
    pub file_idx: usize,
    pub lod: u8,
    pub tile_idx: usize,
}

impl TileKey {
    pub fn new(file_idx: usize, lod: u8, tile_idx: usize) -> Self {
        Self {
            file_idx,
            lod,
            tile_idx,
        }
    }
}

/// Baseline-LOD chroma tile key: identifies a chroma tile by file and tile index.
/// Chroma is computed at the baseline LOD only, so it carries no `lod` — a
/// distinct type from [`TileKey`] so the two can never be confused.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ChromaKey {
    pub file_idx: usize,
    pub tile_idx: usize,
}

impl ChromaKey {
    pub fn new(file_idx: usize, tile_idx: usize) -> Self {
        Self { file_idx, tile_idx }
    }
}

/// A rendered spectrogram tile plus its cache bookkeeping.
pub struct Tile {
    pub tile_idx: usize,
    pub file_idx: usize,
    pub lod: u8,
    pub rendered: PreRendered,
    /// LRU recency stamp; managed by [`TileCache`].
    lru_stamp: u64,
}

/// LRU + byte-capped tile container, generic over key shape.
///
/// Magnitude / flow / reassign / resonator tiles key by [`TileKey`]; chroma
/// tiles key by [`ChromaKey`]. They share the same stamp-based lazy LRU and
/// byte budget — only the key arity and the LRU-compaction floor differ.
/// Key-shaped `insert`/`get` helpers live in the per-key `impl` blocks.
pub struct TileCache<K: Eq + Hash + Copy = TileKey> {
    tiles: HashMap<K, Tile>,
    /// LRU order with lazy stale-entry skipping.
    lru: VecDeque<(K, u64)>,
    total_bytes: usize,
    max_bytes: usize,
    next_stamp: u64,
    /// Lower bound on the LRU-compaction threshold (see `maybe_compact_lru`).
    compact_floor: usize,
}

impl<K: Eq + Hash + Copy> TileCache<K> {
    pub fn new(max_bytes: usize) -> Self {
        Self::with_floor(max_bytes, 1024)
    }

    pub fn with_floor(max_bytes: usize, compact_floor: usize) -> Self {
        Self {
            tiles: HashMap::new(),
            lru: VecDeque::new(),
            total_bytes: 0,
            max_bytes,
            next_stamp: 0,
            compact_floor,
        }
    }

    /// Current cached-byte total (for diagnostics/tests).
    pub fn total_bytes(&self) -> usize {
        self.total_bytes
    }

    /// Configured byte cap.
    pub fn max_bytes(&self) -> usize {
        self.max_bytes
    }

    /// Number of cached tiles (for diagnostics/tests).
    pub fn len(&self) -> usize {
        self.tiles.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tiles.is_empty()
    }

    pub fn contains_key(&self, key: &K) -> bool {
        self.tiles.contains_key(key)
    }

    fn allocate_stamp(&mut self) -> u64 {
        self.next_stamp = self.next_stamp.wrapping_add(1);
        self.next_stamp
    }

    /// Rebuild the LRU deque from live stamps once it has accumulated too many
    /// stale (superseded) entries. The deque can grow past the live tile count
    /// because `touch`/`insert` push without removing the old entry; this keeps
    /// it bounded without an O(n) scan on every access.
    fn maybe_compact_lru(&mut self) {
        let threshold = self.tiles.len().saturating_mul(8).max(self.compact_floor);
        if self.lru.len() <= threshold {
            return;
        }

        let mut entries: Vec<(u64, K)> = self
            .tiles
            .iter()
            .map(|(&key, tile)| (tile.lru_stamp, key))
            .collect();
        entries.sort_by_key(|(stamp, _)| *stamp);
        self.lru = entries
            .into_iter()
            .map(|(stamp, key)| (key, stamp))
            .collect();
    }

    fn evict_to_fit(&mut self, incoming_bytes: usize) {
        while self.total_bytes + incoming_bytes > self.max_bytes {
            let Some((oldest, stamp)) = self.lru.pop_front() else {
                break;
            };
            // Skip stale LRU entries: only evict if this is still the tile's
            // current stamp (it may have been touched/re-inserted since).
            let should_evict = self
                .tiles
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

    /// Insert a pre-built tile under `key`, evicting to fit and stamping it for
    /// LRU. `tile.lru_stamp` is overwritten with the freshly allocated stamp.
    pub fn insert_keyed(&mut self, key: K, mut tile: Tile) {
        let bytes = tile.rendered.byte_len();
        if let Some(old) = self.tiles.remove(&key) {
            self.total_bytes = self.total_bytes.saturating_sub(old.rendered.byte_len());
        }
        self.evict_to_fit(bytes);
        let stamp = self.allocate_stamp();
        tile.lru_stamp = stamp;
        self.total_bytes += bytes;
        self.tiles.insert(key, tile);
        self.lru.push_back((key, stamp));
        self.maybe_compact_lru();
    }

    pub fn get_keyed(&self, key: &K) -> Option<&Tile> {
        self.tiles.get(key)
    }

    pub fn touch(&mut self, key: K) {
        let stamp = self.allocate_stamp();
        if let Some(tile) = self.tiles.get_mut(&key) {
            tile.lru_stamp = stamp;
            self.lru.push_back((key, stamp));
            self.maybe_compact_lru();
        }
    }

    pub fn clear_all(&mut self) {
        self.tiles.clear();
        self.lru.clear();
        self.total_bytes = 0;
    }

    /// Remove every tile whose key matches `pred`, adjusting the byte total and
    /// leaving stale LRU entries to be skipped lazily. Used for per-file clears.
    pub fn clear_keys(&mut self, pred: impl Fn(&K) -> bool) {
        let keys: Vec<K> = self.tiles.keys().copied().filter(|k| pred(k)).collect();
        for key in keys {
            if let Some(evicted) = self.tiles.remove(&key) {
                self.total_bytes = self.total_bytes.saturating_sub(evicted.rendered.byte_len());
            }
        }
    }

    /// Iterate the live keys (order unspecified). For diagnostics/scheduling.
    pub fn keys(&self) -> impl Iterator<Item = &K> {
        self.tiles.keys()
    }
}

// Convenience wrappers for the (file_idx, lod, tile_idx) multi-LOD caches.
impl TileCache<TileKey> {
    pub fn insert(&mut self, file_idx: usize, lod: u8, tile_idx: usize, rendered: PreRendered) {
        let key = TileKey {
            file_idx,
            lod,
            tile_idx,
        };
        self.insert_keyed(
            key,
            Tile {
                tile_idx,
                file_idx,
                lod,
                rendered,
                lru_stamp: 0,
            },
        );
    }

    pub fn get(&self, file_idx: usize, lod: u8, tile_idx: usize) -> Option<&Tile> {
        self.get_keyed(&TileKey {
            file_idx,
            lod,
            tile_idx,
        })
    }

    pub fn evict_far_from(
        &mut self,
        file_idx: usize,
        lod: u8,
        center_tile: usize,
        keep_radius: usize,
    ) {
        let keys_to_evict: Vec<TileKey> = self
            .tiles
            .keys()
            .copied()
            .filter(|k| {
                k.file_idx == file_idx
                    && k.lod == lod
                    && k.tile_idx.abs_diff(center_tile) > keep_radius
            })
            .collect();
        for key in keys_to_evict {
            if let Some(evicted) = self.tiles.remove(&key) {
                self.total_bytes = self.total_bytes.saturating_sub(evicted.rendered.byte_len());
            }
        }
    }

    pub fn clear_for_file(&mut self, file_idx: usize) {
        self.clear_keys(|k| k.file_idx == file_idx);
    }
}

// Convenience wrappers for the (file_idx, tile_idx) chroma cache.
impl TileCache<ChromaKey> {
    pub fn insert(&mut self, file_idx: usize, tile_idx: usize, rendered: PreRendered) {
        let key = ChromaKey { file_idx, tile_idx };
        self.insert_keyed(
            key,
            Tile {
                tile_idx,
                file_idx,
                lod: 1,
                rendered,
                lru_stamp: 0,
            },
        );
    }

    pub fn get(&self, file_idx: usize, tile_idx: usize) -> Option<&Tile> {
        self.get_keyed(&ChromaKey { file_idx, tile_idx })
    }
}

// ── In-flight request bookkeeping ────────────────────────────────────────────
//
// In-flight maps record `key -> spawn_timestamp_ms`. An entry older than the
// timeout is treated as a stuck/abandoned task and may be re-scheduled. The
// frontend passes `js_sys::Date::now()` and its `IN_FLIGHT_TIMEOUT_MS`; these
// functions are time-injected so they can be unit-tested.

/// True if `key` has an in-flight entry that hasn't timed out. Prunes a
/// timed-out entry as a side effect (so the next schedule attempt is allowed).
pub fn in_flight_is_active<K: Eq + Hash>(
    map: &mut HashMap<K, f64>,
    key: &K,
    now: f64,
    timeout_ms: f64,
) -> bool {
    match map.get(key).copied() {
        None => false,
        Some(ts) if now - ts <= timeout_ms => true,
        Some(_) => {
            map.remove(key);
            false
        }
    }
}

/// Count of in-flight entries that haven't timed out (does not prune).
pub fn in_flight_active_count<K>(map: &HashMap<K, f64>, now: f64, timeout_ms: f64) -> usize {
    map.values().filter(|&&ts| now - ts <= timeout_ms).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a `PreRendered` whose `byte_len()` is exactly `bytes` (via the
    /// pixels field, 1 byte/pixel).
    fn tile_of(bytes: usize) -> PreRendered {
        PreRendered {
            width: bytes as u32,
            height: 1,
            pixels: vec![0u8; bytes],
            db_data: Vec::new(),
            flow_shifts: Vec::new(),
        }
    }

    #[test]
    fn insert_and_get_roundtrips() {
        let mut c: TileCache<TileKey> = TileCache::new(1_000);
        c.insert(0, 0, 0, tile_of(100));
        assert_eq!(c.len(), 1);
        assert_eq!(c.total_bytes(), 100);
        let t = c.get(0, 0, 0).unwrap();
        assert_eq!(t.file_idx, 0);
        assert_eq!(t.tile_idx, 0);
        assert!(c.get(0, 0, 1).is_none());
    }

    #[test]
    fn reinsert_same_key_replaces_bytes_not_doubles() {
        let mut c: TileCache<TileKey> = TileCache::new(1_000);
        c.insert(0, 0, 0, tile_of(100));
        c.insert(0, 0, 0, tile_of(250));
        assert_eq!(c.len(), 1);
        assert_eq!(c.total_bytes(), 250);
    }

    #[test]
    fn evicts_oldest_to_stay_under_cap() {
        let mut c: TileCache<TileKey> = TileCache::new(300);
        c.insert(0, 0, 0, tile_of(100)); // stamp 1
        c.insert(0, 0, 1, tile_of(100)); // stamp 2
        c.insert(0, 0, 2, tile_of(100)); // stamp 3, total = 300 (== cap, fits)
        assert_eq!(c.total_bytes(), 300);
        assert_eq!(c.len(), 3);
        // Fourth insert (100) must evict the oldest (tile 0) to fit.
        c.insert(0, 0, 3, tile_of(100));
        assert_eq!(c.total_bytes(), 300);
        assert_eq!(c.len(), 3);
        assert!(
            c.get(0, 0, 0).is_none(),
            "oldest tile should have been evicted"
        );
        assert!(c.get(0, 0, 3).is_some());
    }

    #[test]
    fn touch_protects_from_eviction() {
        let mut c: TileCache<TileKey> = TileCache::new(300);
        c.insert(0, 0, 0, tile_of(100)); // oldest
        c.insert(0, 0, 1, tile_of(100));
        c.insert(0, 0, 2, tile_of(100));
        // Touch tile 0 so it's now most-recently-used; tile 1 becomes oldest.
        c.touch(TileKey::new(0, 0, 0));
        c.insert(0, 0, 3, tile_of(100)); // evicts the new oldest (tile 1)
        assert!(c.get(0, 0, 0).is_some(), "touched tile should survive");
        assert!(
            c.get(0, 0, 1).is_none(),
            "untouched oldest should be evicted"
        );
    }

    #[test]
    fn clear_for_file_only_removes_that_file() {
        let mut c: TileCache<TileKey> = TileCache::new(10_000);
        c.insert(0, 0, 0, tile_of(100));
        c.insert(0, 0, 1, tile_of(100));
        c.insert(1, 0, 0, tile_of(100));
        c.clear_for_file(0);
        assert_eq!(c.len(), 1);
        assert_eq!(c.total_bytes(), 100);
        assert!(c.get(1, 0, 0).is_some());
    }

    #[test]
    fn evict_far_keeps_window_around_center() {
        let mut c: TileCache<TileKey> = TileCache::new(100_000);
        for ti in 0..10 {
            c.insert(0, 0, ti, tile_of(100));
        }
        // Keep radius 2 around center tile 5 -> keep tiles 3..=7.
        c.evict_far_from(0, 0, 5, 2);
        for ti in 0..10 {
            let kept = (3..=7).contains(&ti);
            assert_eq!(c.get(0, 0, ti).is_some(), kept, "tile {ti}");
        }
    }

    #[test]
    fn clear_all_resets_everything() {
        let mut c: TileCache<TileKey> = TileCache::new(10_000);
        c.insert(0, 0, 0, tile_of(100));
        c.insert(0, 0, 1, tile_of(100));
        c.clear_all();
        assert_eq!(c.len(), 0);
        assert_eq!(c.total_bytes(), 0);
        assert!(c.is_empty());
    }

    #[test]
    fn chroma_cache_uses_two_field_key() {
        let mut c: TileCache<ChromaKey> = TileCache::with_floor(10_000, 512);
        c.insert(0, 7, tile_of(100));
        let t = c.get(0, 7).unwrap();
        assert_eq!(t.lod, 1, "chroma tiles are baseline LOD");
        assert_eq!(t.tile_idx, 7);
        c.clear_keys(|k| k.file_idx == 0);
        assert!(c.get(0, 7).is_none());
    }

    #[test]
    fn lru_compaction_bounds_the_deque_without_losing_tiles() {
        // floor 4 so compaction triggers quickly: threshold = max(len*8, 4).
        let mut c: TileCache<TileKey> = TileCache::with_floor(10_000, 4);
        c.insert(0, 0, 0, tile_of(100));
        // Repeatedly touch the same key: each push grows lru without growing tiles.
        for _ in 0..50 {
            c.touch(TileKey::new(0, 0, 0));
        }
        // Compaction must have rebuilt lru down to the live set; tile survives.
        assert_eq!(c.len(), 1);
        assert!(c.get(0, 0, 0).is_some());
    }

    #[test]
    fn in_flight_active_within_timeout_then_pruned() {
        let mut map: HashMap<TileKey, f64> = HashMap::new();
        let key = TileKey::new(0, 0, 0);
        map.insert(key, 1_000.0);
        // now=5000, timeout=10000 -> 4000 <= 10000 -> active, not pruned.
        assert!(in_flight_is_active(&mut map, &key, 5_000.0, 10_000.0));
        assert!(map.contains_key(&key));
        // now=12000 -> 11000 > 10000 -> stale, returns false AND prunes.
        assert!(!in_flight_is_active(&mut map, &key, 12_000.0, 10_000.0));
        assert!(!map.contains_key(&key), "timed-out entry should be pruned");
    }

    #[test]
    fn in_flight_absent_key_is_inactive() {
        let mut map: HashMap<TileKey, f64> = HashMap::new();
        assert!(!in_flight_is_active(
            &mut map,
            &TileKey::new(9, 9, 9),
            0.0,
            10_000.0
        ));
    }

    #[test]
    fn in_flight_active_count_ignores_timed_out() {
        let mut map: HashMap<TileKey, f64> = HashMap::new();
        map.insert(TileKey::new(0, 0, 0), 1_000.0); // age 4000 -> active
        map.insert(TileKey::new(0, 0, 1), 1_000.0); // active
        map.insert(TileKey::new(0, 0, 2), 0.0); //     age 5000 -> active (== timeout boundary not crossed)
        map.insert(TileKey::new(0, 0, 3), -20_000.0); // very old -> timed out
        let n = in_flight_active_count(&map, 5_000.0, 10_000.0);
        assert_eq!(n, 3);
    }
}
