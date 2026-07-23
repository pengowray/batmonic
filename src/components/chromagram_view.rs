use crate::canvas::spectrogram_renderer;
use crate::canvas::tile_cache::{self, TILE_COLS};
use crate::components::gutter::TimeGutter;
use crate::components::playhead::Playhead;
use crate::dsp::chromagram::{NUM_PITCH_CLASSES, PITCH_CLASS_NAMES};
use crate::state::store_fields::*;
use crate::state::{AppState, CanvasTool};
use crate::viewport;
use leptos::ev::MouseEvent;
use leptos::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement};

#[component]
pub fn ChromagramView() -> impl IntoView {
    let state = expect_context::<AppState>();
    let canvas_ref = NodeRef::<leptos::html::Canvas>::new();
    let hand_drag_start = RwSignal::new((0.0f64, 0.0f64));
    let pinch_state: RwSignal<Option<crate::components::pinch::PinchState>> = RwSignal::new(None);

    // Clear chromagram cache when file, range, source, or any of the
    // pre-render baked controls change (gain / adapt / floor are all applied
    // before quantising to u8, so each must invalidate the tile cache).
    Effect::new(move || {
        state.library.files().track(); // subscribe without cloning the Vec
        let _idx = state.library.current_index().get();
        let _range = state.chroma.range().get();
        let _gain = state.chroma.gain().get();
        let _adapt = state.chroma.adapt().get();
        let _floor = state.chroma.floor_db().get();
        let _source = state.chroma.source().get();
        tile_cache::clear_chroma_cache();
    });

    // Main render effect
    Effect::new(move || {
        let _tile_ready = state.viewmode.tile_ready_signal().get();
        let scroll = state.view.scroll_offset().get();
        let zoom = state.view.zoom_level().get();
        let chroma_colormap = state.chroma.colormap().get();
        let _chroma_gain = state.chroma.gain().get(); // triggers re-render after cache clear
        let _chroma_source = state.chroma.source().get(); // same — re-render after source-swap cache clear
        let _chroma_adapt = state.chroma.adapt().get(); // same — re-render after AGC-change cache clear
        let _chroma_floor = state.chroma.floor_db().get(); // same — re-render after floor-change cache clear
        let chroma_gamma = state.chroma.gamma().get();
        let chroma_range = state.chroma.range().get();
        let (_min_octave, num_octaves) = chroma_range.octave_params();
        let files = state.library.files().get();
        let idx = state.library.current_index().get();
        let is_playing = state.playback.is_playing().get();
        let canvas_tool = state.interaction.canvas_tool().get();
        // Re-read canvas dimensions when sidebar layout changes
        let _sidebar = state.panels.left_collapsed().get();
        let _sidebar_width = state.panels.left_width().get();
        let _rsidebar = state.panels.right_collapsed().get();
        let _rsidebar_width = state.panels.right_width().get();

        let Some(canvas_el) = canvas_ref.get() else {
            return;
        };
        let canvas: &HtmlCanvasElement = canvas_el.as_ref();

        let rect = canvas.get_bounding_client_rect();
        let display_w = rect.width() as u32;
        let display_h = rect.height() as u32;
        if display_w == 0 || display_h == 0 {
            return;
        }
        if canvas.width() != display_w || canvas.height() != display_h {
            canvas.set_width(display_w);
            canvas.set_height(display_h);
        }
        state
            .viewmode
            .spectrogram_canvas_width()
            .set(display_w as f64);

        let ctx = canvas
            .get_context("2d")
            .unwrap()
            .unwrap()
            .dyn_into::<CanvasRenderingContext2d>()
            .unwrap();

        let Some(file) = idx.and_then(|i| files.get(i)) else {
            ctx.set_fill_style_str("#000");
            ctx.fill_rect(0.0, 0.0, display_w as f64, display_h as f64);
            return;
        };

        let time_res = file.spectrogram.time_resolution;
        let total_cols = if file.spectrogram.total_columns > 0 {
            file.spectrogram.total_columns
        } else {
            file.spectrogram.columns.len()
        };
        let file_idx = idx.unwrap_or(0);
        let scroll_col = scroll / time_res;

        // Blit chromagram tiles
        spectrogram_renderer::blit_chromagram_tiles_viewport(
            &ctx,
            canvas,
            file_idx,
            total_cols,
            scroll_col,
            zoom,
            chroma_colormap,
            chroma_gamma,
            num_octaves,
        );

        // Schedule missing chromagram tiles
        if total_cols > 0 {
            let visible_cols_f = display_w as f64 / zoom;
            let src_start = scroll_col.max(0.0);
            let src_end = (src_start + visible_cols_f).min(total_cols as f64);
            let first_tile = (src_start / TILE_COLS as f64).floor() as usize;
            let last_tile = ((src_end - 1.0).max(0.0) / TILE_COLS as f64).floor() as usize;
            let n_tiles = total_cols.div_ceil(TILE_COLS);

            for t in first_tile..=last_tile.min(n_tiles.saturating_sub(1)) {
                if tile_cache::get_chroma_tile(file_idx, t).is_none() {
                    tile_cache::schedule_chroma_tile(state, file_idx, t);
                }
            }
        }

        // Draw pitch class labels on left edge
        let ch = display_h as f64;
        let row_height = ch / (NUM_PITCH_CLASSES * num_octaves) as f64;
        ctx.set_font("10px monospace");
        ctx.set_text_baseline("middle");
        for (pc, &label) in PITCH_CLASS_NAMES.iter().enumerate().take(NUM_PITCH_CLASSES) {
            let band_bottom = ch - (pc * num_octaves) as f64 * row_height;
            let band_top = band_bottom - num_octaves as f64 * row_height;
            let band_center = (band_top + band_bottom) / 2.0;

            // Semi-transparent background for label
            ctx.set_fill_style_str("rgba(0, 0, 0, 0.6)");
            ctx.fill_rect(0.0, band_top, 24.0, band_bottom - band_top);

            // Label text
            ctx.set_fill_style_str("#aaa");
            let _ = ctx.fill_text(label, 2.0, band_center);

            // Separator line between pitch classes
            if pc > 0 {
                ctx.set_stroke_style_str("rgba(80, 80, 80, 0.4)");
                ctx.set_line_width(0.5);
                ctx.begin_path();
                ctx.move_to(0.0, band_bottom);
                ctx.line_to(display_w as f64, band_bottom);
                ctx.stroke();
            }
        }

        // Draw "play here" marker when not playing
        if state.playback.start_mode().get().uses_from_here()
            && !is_playing
            && canvas_tool == CanvasTool::Hand
        {
            let visible_time = viewport::visible_time(display_w as f64, zoom, time_res);
            let here_x = display_w as f64 * viewport::PLAY_FROM_HERE_FRACTION;
            let here_time = viewport::play_from_here_time(scroll, visible_time);
            state.playback.from_here_time().set(here_time);
            ctx.set_stroke_style_str("rgba(100, 160, 255, 0.35)");
            ctx.set_line_width(1.5);
            let _ = ctx.set_line_dash(&js_sys::Array::of2(
                &wasm_bindgen::JsValue::from_f64(4.0),
                &wasm_bindgen::JsValue::from_f64(3.0),
            ));
            ctx.begin_path();
            ctx.move_to(here_x, 0.0);
            ctx.line_to(here_x, display_h as f64);
            ctx.stroke();
            let _ = ctx.set_line_dash(&js_sys::Array::new());
        }
    });

    // Auto-scroll to follow playhead (with suspension support)
    Effect::new(move || {
        let playhead = state.playback.playhead_time().get();
        let is_playing = state.playback.is_playing().get();
        let follow = state.view.follow_cursor().get();
        let suspended = state.view.follow_suspended().get_untracked();

        if !follow {
            return;
        }
        if !is_playing {
            if suspended {
                state.view.follow_suspended().set(false);
                state.view.follow_visible_since().set(None);
            }
            return;
        }

        let Some(canvas_el) = canvas_ref.get() else {
            return;
        };
        let canvas: &HtmlCanvasElement = canvas_el.as_ref();
        let display_w = canvas.width() as f64;
        if display_w == 0.0 {
            return;
        }

        let files = state.library.files().get_untracked();
        let idx = state.library.current_index().get_untracked();
        let (time_res, duration) = idx
            .and_then(|i| files.get(i))
            .map(|f| (f.spectrogram.time_resolution, f.audio.duration_secs))
            .unwrap_or((1.0, 0.0));
        let zoom = state.view.zoom_level().get_untracked();
        let scroll = state.view.scroll_offset().get_untracked();
        let from_here_mode = state.playback.start_mode().get_untracked().uses_from_here();

        let visible_time = viewport::visible_time(display_w, zoom, time_res);
        let playhead_rel = playhead - scroll;

        if suspended {
            let playhead_visible = playhead_rel >= 0.0 && playhead_rel <= visible_time;
            if playhead_visible {
                let resume = match state.view.follow_visible_since().get_untracked() {
                    Some(since) => js_sys::Date::now() - since >= 200.0,
                    None => true,
                };
                if resume {
                    state.view.follow_suspended().set(false);
                    state.view.follow_visible_since().set(None);
                }
            }
            return;
        }

        if visible_time < viewport::FOLLOW_EXACT_THRESHOLD_SECS {
            let target_scroll = playhead - visible_time * viewport::FOLLOW_CURSOR_FRACTION;
            state
                .view
                .scroll_offset()
                .set(viewport::clamp_scroll_for_mode(
                    target_scroll,
                    duration,
                    visible_time,
                    from_here_mode,
                ));
        } else if playhead_rel > visible_time * viewport::FOLLOW_CURSOR_EDGE_FRACTION
            || playhead_rel < 0.0
        {
            let target_scroll = playhead - visible_time * viewport::FOLLOW_CURSOR_FRACTION;
            state
                .view
                .scroll_offset()
                .set(viewport::clamp_scroll_for_mode(
                    target_scroll,
                    duration,
                    visible_time,
                    from_here_mode,
                ));
        }
    });

    let on_wheel = move |ev: web_sys::WheelEvent| {
        ev.prevent_default();
        if ev.ctrl_key() {
            let delta = if ev.delta_y() > 0.0 { 0.9 } else { 1.1 };
            state
                .view
                .zoom_level()
                .update(|z| *z = (*z * delta).clamp(0.02, 100.0));
        } else {
            let raw_delta = ev.delta_y() + ev.delta_x();
            let files = state.library.files().get_untracked();
            let idx = state.library.current_index().get_untracked().unwrap_or(0);
            let (visible_time, duration) = if let Some(file) = files.get(idx) {
                let zoom = state.view.zoom_level().get_untracked();
                let canvas_w = state.viewmode.spectrogram_canvas_width().get_untracked();
                (
                    viewport::visible_time(canvas_w, zoom, file.spectrogram.time_resolution),
                    file.audio.duration_secs,
                )
            } else {
                return;
            };
            let delta =
                raw_delta.signum() * visible_time * 0.1 * (raw_delta.abs() / 100.0).min(3.0);
            state.suspend_follow();
            let from_here_mode = state.playback.start_mode().get_untracked().uses_from_here();
            state.view.scroll_offset().update(|s| {
                *s = viewport::clamp_scroll_for_mode(
                    *s + delta,
                    duration,
                    visible_time,
                    from_here_mode,
                )
            });
        }
    };

    let on_mousedown = move |ev: MouseEvent| {
        if ev.button() != 0 {
            return;
        }
        if state.status.viewport_zoomed().get_untracked() {
            return;
        }
        if state.interaction.canvas_tool().get_untracked() != CanvasTool::Hand {
            return;
        }
        state.interaction.is_dragging().set(true);
        hand_drag_start.set((
            ev.client_x() as f64,
            state.view.scroll_offset().get_untracked(),
        ));
    };

    let on_mousemove = move |ev: MouseEvent| {
        if !state.interaction.is_dragging().get_untracked() {
            return;
        }
        if state.interaction.canvas_tool().get_untracked() != CanvasTool::Hand {
            return;
        }
        let (start_client_x, start_scroll) = hand_drag_start.get_untracked();
        let dx = ev.client_x() as f64 - start_client_x;
        let cw = state.viewmode.spectrogram_canvas_width().get_untracked();
        if cw == 0.0 {
            return;
        }
        let files = state.library.files().get_untracked();
        let idx = state.library.current_index().get_untracked();
        let file = idx.and_then(|i| files.get(i));
        let time_res = file
            .as_ref()
            .map(|f| f.spectrogram.time_resolution)
            .unwrap_or(1.0);
        let zoom = state.view.zoom_level().get_untracked();
        let visible_time = viewport::visible_time(cw, zoom, time_res);
        let duration = file.as_ref().map(|f| f.audio.duration_secs).unwrap_or(0.0);
        let from_here_mode = state.playback.start_mode().get_untracked().uses_from_here();
        let dt = -(dx / cw) * visible_time;
        state.suspend_follow();
        state
            .view
            .scroll_offset()
            .set(viewport::clamp_scroll_for_mode(
                start_scroll + dt,
                duration,
                visible_time,
                from_here_mode,
            ));
    };

    let on_mouseup = move |ev: MouseEvent| {
        if state.interaction.is_dragging().get_untracked()
            && state.interaction.canvas_tool().get_untracked() == CanvasTool::Hand
        {
            let (start_x, _) = hand_drag_start.get_untracked();
            let dx = (ev.client_x() as f64 - start_x).abs();
            if dx < 3.0 && state.playback.is_playing().get_untracked() {
                let t = state.playback.playhead_time().get_untracked();
                state
                    .viewmode
                    .bookmarks()
                    .update(|bm| bm.push(crate::state::Bookmark { time: t }));
            }
        }
        state.interaction.is_dragging().set(false);
    };

    let on_mouseleave = move |_ev: MouseEvent| {
        state.interaction.is_dragging().set(false);
    };

    let on_touchstart = move |ev: web_sys::TouchEvent| {
        if state.status.viewport_zoomed().get_untracked() {
            return;
        }
        let touches = ev.touches();
        let n = touches.length();

        if n == 2 {
            ev.prevent_default();
            use crate::components::pinch::{two_finger_geometry, PinchState};
            if let Some((mid_x, dist)) = two_finger_geometry(&touches) {
                let files = state.library.files().get_untracked();
                let idx = state.library.current_index().get_untracked();
                let file = idx.and_then(|i| files.get(i));
                let time_res = file
                    .as_ref()
                    .map(|f| f.spectrogram.time_resolution)
                    .unwrap_or(1.0);
                let duration = file
                    .as_ref()
                    .map(|f| f.audio.duration_secs)
                    .unwrap_or(f64::MAX);
                pinch_state.set(Some(PinchState::horizontal(
                    dist,
                    state.view.zoom_level().get_untracked(),
                    state.view.scroll_offset().get_untracked(),
                    mid_x,
                    time_res,
                    duration,
                    state.playback.start_mode().get_untracked().uses_from_here(),
                )));
            }
            state.interaction.is_dragging().set(false);
            return;
        }

        if n != 1 {
            return;
        }
        pinch_state.set(None);

        let touch = touches.get(0).unwrap();
        if state.interaction.canvas_tool().get_untracked() != CanvasTool::Hand {
            return;
        }
        ev.prevent_default();
        state.interaction.is_dragging().set(true);
        hand_drag_start.set((
            touch.client_x() as f64,
            state.view.scroll_offset().get_untracked(),
        ));
    };

    let on_touchmove = move |ev: web_sys::TouchEvent| {
        if state.status.viewport_zoomed().get_untracked() {
            return;
        }
        let touches = ev.touches();
        let n = touches.length();

        if n == 2 {
            if let Some(ps) = pinch_state.get_untracked() {
                ev.prevent_default();
                use crate::components::pinch::{apply_pinch, two_finger_geometry};
                if let Some((mid_x, dist)) = two_finger_geometry(&touches) {
                    let Some(canvas_el) = canvas_ref.get() else {
                        return;
                    };
                    let canvas: &HtmlCanvasElement = canvas_el.as_ref();
                    let rect = canvas.get_bounding_client_rect();
                    let cw = canvas.width() as f64;
                    let (new_zoom, new_scroll) = apply_pinch(&ps, dist, mid_x, rect.left(), cw);
                    state.suspend_follow();
                    state.view.zoom_level().set(new_zoom);
                    state.view.scroll_offset().set(new_scroll);
                }
            }
            return;
        }

        if n != 1 {
            return;
        }
        let touch = touches.get(0).unwrap();
        if !state.interaction.is_dragging().get_untracked() {
            return;
        }
        if state.interaction.canvas_tool().get_untracked() != CanvasTool::Hand {
            return;
        }
        ev.prevent_default();
        let (start_client_x, start_scroll) = hand_drag_start.get_untracked();
        let dx = touch.client_x() as f64 - start_client_x;
        let cw = state.viewmode.spectrogram_canvas_width().get_untracked();
        if cw == 0.0 {
            return;
        }
        let files = state.library.files().get_untracked();
        let idx = state.library.current_index().get_untracked();
        let file = idx.and_then(|i| files.get(i));
        let time_res = file
            .as_ref()
            .map(|f| f.spectrogram.time_resolution)
            .unwrap_or(1.0);
        let zoom = state.view.zoom_level().get_untracked();
        let visible_time = viewport::visible_time(cw, zoom, time_res);
        let duration = file.as_ref().map(|f| f.audio.duration_secs).unwrap_or(0.0);
        let from_here_mode = state.playback.start_mode().get_untracked().uses_from_here();
        let dt = -(dx / cw) * visible_time;
        state.suspend_follow();
        state
            .view
            .scroll_offset()
            .set(viewport::clamp_scroll_for_mode(
                start_scroll + dt,
                duration,
                visible_time,
                from_here_mode,
            ));
    };

    let on_touchend = move |_ev: web_sys::TouchEvent| {
        let remaining = _ev.touches().length();
        if remaining < 2 {
            pinch_state.set(None);
        }
        if remaining == 1 {
            if let Some(touch) = _ev.touches().get(0) {
                hand_drag_start.set((
                    touch.client_x() as f64,
                    state.view.scroll_offset().get_untracked(),
                ));
                if state.interaction.canvas_tool().get_untracked() == CanvasTool::Hand {
                    state.interaction.is_dragging().set(true);
                }
            }
            return;
        }
        if remaining == 0 {
            if state.interaction.canvas_tool().get_untracked() == CanvasTool::Hand {
                if let Some(touch) = _ev.changed_touches().get(0) {
                    let (start_x, _) = hand_drag_start.get_untracked();
                    let dx = (touch.client_x() as f64 - start_x).abs();
                    if dx < 5.0 && state.playback.is_playing().get_untracked() {
                        let t = state.playback.playhead_time().get_untracked();
                        state
                            .viewmode
                            .bookmarks()
                            .update(|bm| bm.push(crate::state::Bookmark { time: t }));
                    }
                }
            }
            state.interaction.is_dragging().set(false);
        }
    };

    view! {
        <div class="spectrogram-container"
            style=move || {
                let ta = if state.status.viewport_zoomed().get() { "pinch-zoom" } else { "none" };
                match state.interaction.canvas_tool().get() {
                    CanvasTool::Hand => if state.interaction.is_dragging().get() {
                        format!("cursor: grabbing; touch-action: {ta};")
                    } else {
                        format!("cursor: grab; touch-action: {ta};")
                    },
                    CanvasTool::Selection => format!("cursor: crosshair; touch-action: {ta};"),
                }
            }
        >
            <div class="chart-row">
            <div class="chart-stage">
            <canvas
                node_ref=canvas_ref
                style:pointer-events=move || if state.status.viewport_zoomed().get() { "none" } else { "auto" }
                on:wheel=on_wheel
                on:mousedown=on_mousedown
                on:mousemove=on_mousemove
                on:mouseup=on_mouseup
                on:mouseleave=on_mouseleave
                on:touchstart=on_touchstart
                on:touchmove=on_touchmove
                on:touchend=on_touchend
            />
            <Playhead/>
            </div>
            </div>
            <div class="view-bottom-row">
                <TimeGutter/>
                <div class="view-bottom-corner"></div>
            </div>
        </div>
    }
}
