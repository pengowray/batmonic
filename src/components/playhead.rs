// DOM-based playhead overlay shared by every main chart view. Sits as a
// 2px absolutely-positioned vertical line on top of the host `.*-stage`,
// driven by a `translateX` that follows `state.playhead_time`. Using a
// DOM overlay (rather than re-drawing the canvas) lets the line track
// smoothly at 60fps without invalidating the expensive underlying image.

use leptos::prelude::*;
use crate::state::AppState;

/// Shared playhead line. Mount inside a position-relative stage whose
/// width matches the main canvas. `x_offset` inserts a left-side inset
/// for views that draw their own y-axis labels inside the canvas (e.g.
/// ZcChart reserves 60px), so the line stays aligned with the data
/// region rather than the full canvas width.
#[component]
pub fn Playhead(#[prop(default = 0.0)] x_offset: f64) -> impl IntoView {
    let state = expect_context::<AppState>();

    let transform = move || {
        let playhead = state.playhead_time.get();
        let scroll = state.scroll_offset.get();
        let zoom = state.zoom_level.get();
        let cw = state.spectrogram_canvas_width.get();
        let files = state.files.get_untracked();
        // Timeline mode borrows time_res from the first segment's file;
        // single-file mode uses the current file. Both stay None-safe
        // (unknown → 1.0) so the line renders at x=0 when a view mounts
        // before its data is ready.
        let time_res = if let Some(ref tl) = state.active_timeline.get_untracked() {
            tl.segments.first().and_then(|s| files.get(s.file_index))
                .map(|f| f.spectrogram.time_resolution).unwrap_or(1.0)
        } else {
            let idx = state.current_file_index.get_untracked();
            idx.and_then(|i| files.get(i))
                .map(|f| f.spectrogram.time_resolution)
                .unwrap_or(1.0)
        };
        let data_w = (cw - x_offset).max(0.0);
        let visible_time = (data_w / zoom) * time_res;
        let px_per_sec = if visible_time > 0.0 { data_w / visible_time } else { 0.0 };
        let x = x_offset + (playhead - scroll) * px_per_sec;
        format!("translateX({:.1}px)", x)
    };

    view! {
        <div
            class="playhead-line"
            style:transform=transform
            style:display=move || {
                if state.is_playing.get() && !state.clean_view.get() { "block" } else { "none" }
            }
        />
    }
}
