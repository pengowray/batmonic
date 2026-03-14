use crate::canvas::tile_cache::{self, TILE_COLS};

/// Viewport geometry for tile-based rendering.
/// Computed once per frame, shared across blit functions.
pub struct ViewportGeometry {
    pub cw: f64,
    pub ch: f64,
    pub ideal_lod: u8,
    pub ratio: f64,
    pub vis_start: f64,
    pub vis_end: f64,
    pub first_tile: usize,
    pub last_tile: usize,
    pub fc_lo: f64,
    pub fc_hi: f64,
    pub zoom: f64,
}

impl ViewportGeometry {
    /// Compute viewport geometry from scroll/zoom/canvas state.
    /// Returns None if nothing is visible.
    pub fn new(
        cw: f64,
        ch: f64,
        total_cols: usize,
        scroll_col: f64,
        zoom: f64,
        freq_crop_lo: f64,
        freq_crop_hi: f64,
    ) -> Option<Self> {
        if total_cols == 0 || zoom <= 0.0 {
            return None;
        }

        let ideal_lod = tile_cache::select_lod(zoom);
        let ratio = tile_cache::lod_ratio(ideal_lod);

        let vis_start = scroll_col.max(0.0).min((total_cols as f64 - 1.0).max(0.0));
        let vis_end = (vis_start + cw / zoom).min(total_cols as f64);

        let vis_start_lod = vis_start * ratio;
        let vis_end_lod = vis_end * ratio;

        let first_tile = (vis_start_lod / TILE_COLS as f64).floor() as usize;
        let last_tile = ((vis_end_lod - 0.001).max(0.0) / TILE_COLS as f64).floor() as usize;

        let fc_lo = freq_crop_lo.max(0.0);
        let fc_hi = freq_crop_hi.max(0.01);

        Some(Self {
            cw, ch, ideal_lod, ratio, vis_start, vis_end,
            first_tile, last_tile, fc_lo, fc_hi, zoom,
        })
    }

    /// Compute the LOD1 clip range for a tile at the ideal LOD.
    pub fn tile_clip_range(&self, tile_idx: usize) -> Option<(f64, f64)> {
        let tile_lod1_start = tile_idx as f64 * TILE_COLS as f64 / self.ratio;
        let tile_lod1_end = tile_lod1_start + TILE_COLS as f64 / self.ratio;
        let clip_start = self.vis_start.max(tile_lod1_start);
        let clip_end = self.vis_end.min(tile_lod1_end);
        if clip_end <= clip_start { None } else { Some((clip_start, clip_end)) }
    }
}

/// Source and destination rectangles for drawing a tile onto the canvas.
pub struct TileBlitCoords {
    pub src_x: f64,
    pub src_y: f64,
    pub src_w: f64,
    pub src_h: f64,
    pub dst_x: f64,
    pub dst_y: f64,
    pub dst_w: f64,
    pub dst_h: f64,
}

/// Compute source/destination rectangles for blitting a tile at a given LOD.
///
/// This is the core geometry shared by all tile blit functions (normal, flow, chromagram).
/// Returns None if the tile has zero size or the clip range doesn't intersect.
pub fn compute_tile_blit_coords(
    vg: &ViewportGeometry,
    tile_width: f64,
    tile_height: f64,
    tile_lod: u8,
    tile_idx: usize,
    clip_lod1_start: f64,
    clip_lod1_end: f64,
) -> Option<TileBlitCoords> {
    if tile_width == 0.0 || tile_height == 0.0 { return None; }

    let tile_ratio = tile_cache::lod_ratio(tile_lod);

    // Tile's LOD1 column range
    let tile_lod1_start = tile_idx as f64 * TILE_COLS as f64 / tile_ratio;
    let tile_lod1_end = tile_lod1_start + TILE_COLS as f64 / tile_ratio;

    // Clip to requested range
    let c_start = clip_lod1_start.max(tile_lod1_start);
    let c_end = clip_lod1_end.min(tile_lod1_end);
    if c_end <= c_start { return None; }

    // Source coordinates in tile pixel space
    let pixel_scale = tile_width * tile_ratio / TILE_COLS as f64;
    let src_x = ((c_start - tile_lod1_start) * pixel_scale).max(0.0);
    let src_x_end = ((c_end - tile_lod1_start) * pixel_scale).min(tile_width);
    let src_w = (src_x_end - src_x).max(0.0);
    if src_w <= 0.0 { return None; }

    // Vertical crop
    let th = tile_height;
    let ch = vg.ch;
    let (src_y, src_h, dst_y, dst_h) = if vg.fc_hi <= 1.0 {
        let sy = th * (1.0 - vg.fc_hi);
        let sh = th * (vg.fc_hi - vg.fc_lo).max(0.001);
        (sy, sh, 0.0, ch)
    } else {
        let fc_range = (vg.fc_hi - vg.fc_lo).max(0.001);
        let data_frac = (1.0 - vg.fc_lo) / fc_range;
        let sh = th * (1.0 - vg.fc_lo);
        (0.0, sh, ch * (1.0 - data_frac), ch * data_frac)
    };

    // Destination on canvas
    let dst_x_raw = (c_start - vg.vis_start) * vg.zoom;
    let dst_x_end_raw = (c_end - vg.vis_start) * vg.zoom;
    let dst_x = dst_x_raw.floor();
    let dst_w = (dst_x_end_raw.ceil() - dst_x).max(1.0);

    Some(TileBlitCoords {
        src_x, src_y, src_w, src_h,
        dst_x, dst_y, dst_w, dst_h,
    })
}

/// Iterate visible tiles with LOD fallback. Calls `draw_fn` for each visible tile,
/// trying the ideal LOD first, then falling back to coarser LODs.
///
/// `borrow_ideal` and `borrow_fallback` are called to access tiles from the cache.
/// They should call `draw_fn` with the tile if found.
/// Returns true if any tile was drawn.
pub fn for_each_visible_tile<F, G>(
    vg: &ViewportGeometry,
    mut borrow_ideal: F,
    mut borrow_fallback: G,
) -> bool
where
    F: FnMut(usize, f64, f64) -> bool,  // (tile_idx, clip_start, clip_end) -> drawn
    G: FnMut(usize, u8, f64, f64) -> bool,  // (tile_idx, fb_lod, clip_start, clip_end) -> drawn
{
    let mut any_drawn = false;

    for tile_idx in vg.first_tile..=vg.last_tile {
        let Some((clip_start, clip_end)) = vg.tile_clip_range(tile_idx) else { continue };

        let mut tile_drawn = false;

        // Try ideal LOD first
        if borrow_ideal(tile_idx, clip_start, clip_end) {
            tile_drawn = true;
        }

        // Fallback to coarser LODs
        if !tile_drawn {
            for fb_lod in (0..vg.ideal_lod).rev() {
                let (fb_tile, _, _) = tile_cache::fallback_tile_info(vg.ideal_lod, tile_idx, fb_lod);
                if borrow_fallback(fb_tile, fb_lod, clip_start, clip_end) {
                    tile_drawn = true;
                    break;
                }
            }
        }

        if tile_drawn { any_drawn = true; }
    }

    any_drawn
}
