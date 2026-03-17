use leptos::prelude::*;
use crate::annotations::{AnnotationId, AnnotationKind, AnnotationSet};
use crate::canvas::spectrogram_renderer;
use crate::state::{AppState, PlaybackMode, ResizeHandlePosition, SpectrogramHandle};

/// Half-width of the FF handle interaction zone (pixels from center).
pub const FF_HANDLE_HALF_WIDTH: f64 = 50.0;

/// Hit-test all spectrogram overlay handles (FF + HET).
/// Returns the closest handle within `threshold` pixels of mouse_y, or None.
/// HET handles take priority over FF when they overlap and HET is manual.
/// FF hover is full-width; drag zone is checked separately via `is_in_ff_drag_zone`.
pub fn hit_test_spec_handles(
    state: &AppState,
    mouse_y: f64,
    min_freq: f64,
    max_freq: f64,
    canvas_height: f64,
    threshold: f64,
) -> Option<SpectrogramHandle> {
    let mut candidates: Vec<(SpectrogramHandle, f64)> = Vec::new();

    // FF handles — hover across full line width, drag only in center zone
    let ff_lo = state.ff_freq_lo.get_untracked();
    let ff_hi = state.ff_freq_hi.get_untracked();
    if ff_hi > ff_lo {
        let y_upper = spectrogram_renderer::freq_to_y(ff_hi.min(max_freq), min_freq, max_freq, canvas_height);
        let y_lower = spectrogram_renderer::freq_to_y(ff_lo.max(min_freq), min_freq, max_freq, canvas_height);
        let d_upper = (mouse_y - y_upper).abs();
        let d_lower = (mouse_y - y_lower).abs();
        if d_upper <= threshold { candidates.push((SpectrogramHandle::FfUpper, d_upper)); }
        if d_lower <= threshold { candidates.push((SpectrogramHandle::FfLower, d_lower)); }
        // Middle handle (midpoint between boundaries)
        let mid_freq = (ff_lo + ff_hi) / 2.0;
        let y_mid = spectrogram_renderer::freq_to_y(mid_freq.clamp(min_freq, max_freq), min_freq, max_freq, canvas_height);
        let d_mid = (mouse_y - y_mid).abs();
        if d_mid <= threshold { candidates.push((SpectrogramHandle::FfMiddle, d_mid)); }
    }

    // HET handles (only when in HET mode and parameter is manual)
    if state.playback_mode.get_untracked() == PlaybackMode::Heterodyne {
        let het_freq = state.het_frequency.get_untracked();
        let het_cutoff = state.het_cutoff.get_untracked();

        if !state.het_freq_auto.get_untracked() {
            let y_center = spectrogram_renderer::freq_to_y(het_freq, min_freq, max_freq, canvas_height);
            let d = (mouse_y - y_center).abs();
            if d <= threshold { candidates.push((SpectrogramHandle::HetCenter, d)); }
        }
        if !state.het_cutoff_auto.get_untracked() {
            let y_upper = spectrogram_renderer::freq_to_y(
                (het_freq + het_cutoff).min(max_freq), min_freq, max_freq, canvas_height,
            );
            let y_lower = spectrogram_renderer::freq_to_y(
                (het_freq - het_cutoff).max(min_freq), min_freq, max_freq, canvas_height,
            );
            let d_upper = (mouse_y - y_upper).abs();
            let d_lower = (mouse_y - y_lower).abs();
            if d_upper <= threshold { candidates.push((SpectrogramHandle::HetBandUpper, d_upper)); }
            if d_lower <= threshold { candidates.push((SpectrogramHandle::HetBandLower, d_lower)); }
        }
    }

    if candidates.is_empty() { return None; }

    // Sort by distance, then prefer HET over FF when tied
    candidates.sort_by(|a, b| {
        a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                let a_het = matches!(a.0, SpectrogramHandle::HetCenter | SpectrogramHandle::HetBandUpper | SpectrogramHandle::HetBandLower);
                let b_het = matches!(b.0, SpectrogramHandle::HetCenter | SpectrogramHandle::HetBandUpper | SpectrogramHandle::HetBandLower);
                b_het.cmp(&a_het) // HET first
            })
    });

    Some(candidates[0].0)
}

/// Check whether a given x position is within the FF handle drag zone (center strip).
pub fn is_in_ff_drag_zone(mouse_x: f64, canvas_width: f64) -> bool {
    let center_x = canvas_width / 2.0;
    (mouse_x - center_x).abs() <= FF_HANDLE_HALF_WIDTH
}

/// Pixel radius for annotation resize handle hit detection (mouse).
pub const ANNOTATION_HANDLE_HIT_RADIUS: f64 = 8.0;
/// Pixel radius for annotation resize handle hit detection (touch/mobile).
pub const ANNOTATION_HANDLE_HIT_RADIUS_TOUCH: f64 = 22.0;

/// Compute the 8 (or 2 for segments) resize handle positions in pixel space for an annotation.
/// Returns a list of (handle_position, px_x, px_y).
fn annotation_handle_positions(
    time_start: f64,
    time_end: f64,
    freq_low: Option<f64>,
    freq_high: Option<f64>,
    _scroll_offset: f64,
    px_per_sec: f64,
    start_time: f64,
    min_freq: f64,
    max_freq: f64,
    canvas_height: f64,
) -> Vec<(ResizeHandlePosition, f64, f64)> {
    let x0 = (time_start - start_time) * px_per_sec;
    let x1 = (time_end - start_time) * px_per_sec;
    let mx = (x0 + x1) / 2.0;

    match (freq_high, freq_low) {
        (Some(fh), Some(fl)) => {
            let y0 = spectrogram_renderer::freq_to_y(fh, min_freq, max_freq, canvas_height);
            let y1 = spectrogram_renderer::freq_to_y(fl, min_freq, max_freq, canvas_height);
            let my = (y0 + y1) / 2.0;
            vec![
                (ResizeHandlePosition::TopLeft, x0, y0),
                (ResizeHandlePosition::Top, mx, y0),
                (ResizeHandlePosition::TopRight, x1, y0),
                (ResizeHandlePosition::Left, x0, my),
                (ResizeHandlePosition::Right, x1, my),
                (ResizeHandlePosition::BottomLeft, x0, y1),
                (ResizeHandlePosition::Bottom, mx, y1),
                (ResizeHandlePosition::BottomRight, x1, y1),
            ]
        }
        _ => {
            // Segment (time-only): only left/right handles at vertical midpoint
            let my = canvas_height / 2.0;
            vec![
                (ResizeHandlePosition::Left, x0, my),
                (ResizeHandlePosition::Right, x1, my),
            ]
        }
    }
}

/// Hit-test annotation resize handles for currently selected annotations.
/// Returns the closest handle within `hit_radius` pixels of (px_x, px_y), or None.
pub fn hit_test_annotation_handles(
    annotation_set: &AnnotationSet,
    selected_ids: &[AnnotationId],
    px_x: f64,
    px_y: f64,
    min_freq: f64,
    max_freq: f64,
    scroll_offset: f64,
    time_resolution: f64,
    zoom: f64,
    canvas_width: f64,
    canvas_height: f64,
    hit_radius: f64,
) -> Option<(AnnotationId, ResizeHandlePosition)> {
    if selected_ids.is_empty() {
        return None;
    }
    let visible_time = (canvas_width / zoom) * time_resolution;
    let start_time = scroll_offset;
    let px_per_sec = canvas_width / visible_time;

    let mut best: Option<(AnnotationId, ResizeHandlePosition, f64)> = None;

    for ann in &annotation_set.annotations {
        if !selected_ids.contains(&ann.id) {
            continue;
        }
        let region = match &ann.kind {
            AnnotationKind::Region(r) => r,
            _ => continue,
        };

        let handles = annotation_handle_positions(
            region.time_start, region.time_end,
            region.freq_low, region.freq_high,
            scroll_offset, px_per_sec, start_time,
            min_freq, max_freq, canvas_height,
        );

        for (pos, hx, hy) in &handles {
            let d = ((px_x - hx).powi(2) + (px_y - hy).powi(2)).sqrt();
            if d <= hit_radius {
                if best.as_ref().map_or(true, |(_, _, bd)| d < *bd) {
                    best = Some((ann.id.clone(), *pos, d));
                }
            }
        }
    }

    best.map(|(id, pos, _)| (id, pos))
}

/// Hit-test annotation bodies (click inside an annotation region).
/// Prioritizes: (1) label area clicks, (2) smallest-area annotation when overlapping.
/// Returns the annotation ID if the click is inside any annotation.
pub fn hit_test_annotation_body(
    annotation_set: &AnnotationSet,
    px_x: f64,
    px_y: f64,
    min_freq: f64,
    max_freq: f64,
    scroll_offset: f64,
    time_resolution: f64,
    zoom: f64,
    canvas_width: f64,
    canvas_height: f64,
) -> Option<AnnotationId> {
    let visible_time = (canvas_width / zoom) * time_resolution;
    let start_time = scroll_offset;
    let px_per_sec = canvas_width / visible_time;

    let mut label_hit: Option<AnnotationId> = None;
    let mut best_body: Option<(AnnotationId, f64)> = None; // (id, area)

    for ann in &annotation_set.annotations {
        let region = match &ann.kind {
            AnnotationKind::Region(r) => r,
            _ => continue,
        };

        let x0 = ((region.time_start - start_time) * px_per_sec).max(0.0);
        let x1 = ((region.time_end - start_time) * px_per_sec).min(canvas_width);
        if x1 <= x0 {
            continue;
        }

        let (y0, y1) = match (region.freq_high, region.freq_low) {
            (Some(fh), Some(fl)) => {
                let y0 = spectrogram_renderer::freq_to_y(fh, min_freq, max_freq, canvas_height).max(0.0);
                let y1 = spectrogram_renderer::freq_to_y(fl, min_freq, max_freq, canvas_height).min(canvas_height);
                if y1 <= y0 { continue; }
                (y0, y1)
            }
            _ => (0.0, canvas_height),
        };

        // Check if click is inside the annotation bounding box
        if px_x >= x0 && px_x <= x1 && px_y >= y0 && px_y <= y1 {
            // Check label area (top-left ~80x16px)
            if region.label.is_some() && px_x <= x0 + 80.0 && px_y <= y0 + 16.0 {
                // Label hit — highest priority
                label_hit = Some(ann.id.clone());
            }

            let area = (x1 - x0) * (y1 - y0);
            if best_body.as_ref().map_or(true, |(_, ba)| area < *ba) {
                best_body = Some((ann.id.clone(), area));
            }
        }
    }

    // Label hits take priority
    label_hit.or_else(|| best_body.map(|(id, _)| id))
}

/// Get the pixel positions of resize handles for a specific annotation.
/// Used by both hit-testing and rendering.
pub fn get_annotation_handle_positions(
    time_start: f64,
    time_end: f64,
    freq_low: Option<f64>,
    freq_high: Option<f64>,
    scroll_offset: f64,
    time_resolution: f64,
    zoom: f64,
    canvas_width: f64,
    min_freq: f64,
    max_freq: f64,
    canvas_height: f64,
) -> Vec<(ResizeHandlePosition, f64, f64)> {
    let visible_time = (canvas_width / zoom) * time_resolution;
    let start_time = scroll_offset;
    let px_per_sec = canvas_width / visible_time;
    annotation_handle_positions(
        time_start, time_end, freq_low, freq_high,
        scroll_offset, px_per_sec, start_time,
        min_freq, max_freq, canvas_height,
    )
}
