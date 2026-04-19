// Gutter components — dedicated drag surfaces for range selection that
// live alongside (not on top of) the main view canvases.
//
// `BandGutter` is a narrow vertical strip on the right of a view, owning
// frequency-band (HFR) selection. `TimeGutter` is a thin horizontal
// strip below a view, owning time-range selection and rendering the
// time axis labels that previously sat inside the main canvas.

use leptos::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement};
use crate::canvas::gutter_renderer;
use crate::components::spectrogram_events::{
    apply_axis_drag, finalize_axis_drag, freq_snap, select_all_frequencies,
    select_all_time,
};
use crate::state::{ActiveFocus, AppState, Selection};

/// Vertical band-selection gutter. Interactions mirror the spectrogram's
/// left y-axis so the two feel like one control surface: single tap
/// toggles HFR off, drag paints a new band (auto-enabling HFR), shift+
/// drag extends the existing band from its far edge, and double-click
/// selects the full Nyquist range. All three gestures route through the
/// shared `apply_axis_drag` / `finalize_axis_drag` helpers so snapping,
/// focus, and selection-upgrade behaviour stay in lockstep with the axis.
#[component]
pub fn BandGutter() -> impl IntoView {
    let state = expect_context::<AppState>();
    let canvas_ref = NodeRef::<leptos::html::Canvas>::new();
    // Start-of-drag anchor in Hz; None when not dragging.
    let drag_anchor: StoredValue<Option<f64>> = StoredValue::new(None);
    // Tooltip position (canvas-local y, in px) — drives the drag tooltip.
    // None while not dragging.
    let tooltip_y = RwSignal::new_local(Option::<f64>::None);

    // Resolve the visible frequency window for the gutter. On the
    // spectrogram this tracks min/max_display_freq so the gutter ticks
    // line up 1:1 with the spectrogram's y-axis; on views that don't set
    // those signals it falls back to 0..Nyquist.
    let display_range = move || -> (f64, f64) {
        let files = state.files.get();
        let idx = state.current_file_index.get();
        let nyquist = idx
            .and_then(|i| files.get(i))
            .map(|f| f.audio.sample_rate as f64 / 2.0)
            .unwrap_or(0.0);
        let lo = state.min_display_freq.get().unwrap_or(0.0);
        let hi = state.max_display_freq.get().unwrap_or(nyquist);
        (lo, hi)
    };

    // Redraw when any relevant signal changes.
    Effect::new(move |_| {
        let band_lo = state.band_ff_freq_lo.get();
        let band_hi = state.band_ff_freq_hi.get();
        let hfr_on = state.hfr_enabled.get();
        let shield_style = state.shield_style.get();
        // Live drag range from either this gutter or the spectrogram's
        // y-axis — when Some, overrides the stored band so the shield
        // lights up mid-drag even before the band has been committed.
        let drag_range = match (
            state.axis_drag_start_freq.get(),
            state.axis_drag_current_freq.get(),
        ) {
            (Some(s), Some(c)) => Some((s, c)),
            _ => None,
        };
        let (min_freq, max_freq) = display_range();
        let _sidebar = state.sidebar_collapsed.get();
        let _sidebar_width = state.sidebar_width.get();
        let _rsidebar = state.right_sidebar_collapsed.get();
        let _rsidebar_width = state.right_sidebar_width.get();
        let _tile_ready = state.tile_ready_signal.get();

        let Some(canvas_el) = canvas_ref.get() else { return };
        let canvas: &HtmlCanvasElement = canvas_el.as_ref();
        let rect = canvas.get_bounding_client_rect();
        let display_w = rect.width() as u32;
        let display_h = rect.height() as u32;
        if display_w == 0 || display_h == 0 { return; }
        if canvas.width() != display_w || canvas.height() != display_h {
            canvas.set_width(display_w);
            canvas.set_height(display_h);
        }

        let Ok(Some(obj)) = canvas.get_context("2d") else { return };
        let Ok(ctx) = obj.dyn_into::<CanvasRenderingContext2d>() else { return };

        gutter_renderer::draw_band_gutter(
            &ctx,
            display_w as f64,
            display_h as f64,
            min_freq,
            max_freq,
            band_lo,
            band_hi,
            hfr_on,
            shield_style,
            drag_range,
        );
    });

    // Resolve (local_y, canvas_height, min_freq, max_freq) for a pointer
    // event — frequency bounds reflect the host view's current display
    // range so drag math uses the same mapping the gutter renders with.
    let pointer_context = move |ev: &web_sys::PointerEvent| -> Option<(f64, f64, f64, f64)> {
        let canvas_el = canvas_ref.get()?;
        let canvas: &HtmlCanvasElement = canvas_el.as_ref();
        let rect = canvas.get_bounding_client_rect();
        let h = rect.height();
        if h <= 0.0 { return None; }
        let y = ev.client_y() as f64 - rect.top();
        let (min_freq, max_freq) = display_range();
        if max_freq <= min_freq { return None; }
        Some((y, h, min_freq, max_freq))
    };

    let on_pointerdown = move |ev: web_sys::PointerEvent| {
        if ev.button() != 0 { return; }
        let Some((y, h, min_freq, max_freq)) = pointer_context(&ev) else { return };
        ev.prevent_default();

        let freq = gutter_renderer::y_to_freq(y, min_freq, max_freq, h);
        let shift = ev.shift_key();
        let band_lo = state.band_ff_freq_lo.get_untracked();
        let band_hi = state.band_ff_freq_hi.get_untracked();
        let has_range = band_hi > band_lo;

        // Shift+click extend: anchor at the edge of the existing range
        // farthest from the click, so dragging grows the band from there.
        let raw_start = if shift && has_range {
            if (freq - band_lo).abs() < (freq - band_hi).abs() { band_hi } else { band_lo }
        } else {
            freq
        };

        drag_anchor.set_value(Some(raw_start));
        tooltip_y.set(Some(y));
        // Flag the drag so heavy consumers (waveform band-split) can cache.
        state.band_ff_dragging.set(true);

        // Seed the shared axis-drag state so the spectrogram's y-axis
        // shields light up in sync, and so finalize_axis_drag can detect
        // a tap (start ≈ current).
        let snap_s = freq_snap(raw_start, shift);
        let snap_e = freq_snap(freq, shift);
        state.axis_drag_start_freq.set(Some((raw_start / snap_s).round() * snap_s));
        state.axis_drag_current_freq.set(Some((freq / snap_e).round() * snap_e));
        state.is_dragging.set(true);

        // Shift-extend should update the band immediately; a fresh drag
        // waits for pointermove so a pure tap leaves the existing band
        // intact (tap = toggle HFR, handled in finalize_axis_drag).
        if shift && has_range {
            let lo = raw_start.min(freq);
            let hi = raw_start.max(freq);
            if hi - lo > 500.0 {
                state.set_band_ff_range(lo, hi);
            }
        }

        if let Some(target) = ev.target() {
            if let Ok(el) = target.dyn_into::<web_sys::Element>() {
                let _ = el.set_pointer_capture(ev.pointer_id());
            }
        }
    };

    let on_pointermove = move |ev: web_sys::PointerEvent| {
        let Some(raw_start) = drag_anchor.get_value() else { return };
        let Some((y, h, min_freq, max_freq)) = pointer_context(&ev) else { return };
        tooltip_y.set(Some(y.clamp(0.0, h)));
        let freq = gutter_renderer::y_to_freq(y, min_freq, max_freq, h);
        apply_axis_drag(state, raw_start, freq, ev.shift_key());
    };

    let on_pointerup = move |_ev: web_sys::PointerEvent| {
        if drag_anchor.get_value().is_some() {
            drag_anchor.set_value(None);
            tooltip_y.set(None);
            state.band_ff_dragging.set(false);
            // Shared finalize: taps toggle HFR off, meaningful drags
            // auto-enable HFR and promote focus to FrequencyFocus.
            finalize_axis_drag(state);
        }
    };

    let on_dblclick = move |_ev: web_sys::MouseEvent| {
        select_all_frequencies(state);
    };

    // Format "40.0 – 72.5 kHz" for the drag tooltip.
    let format_range = move || {
        let lo = state.band_ff_freq_lo.get();
        let hi = state.band_ff_freq_hi.get();
        if hi <= lo { return String::new(); }
        format!("{:.1} – {:.1} kHz", lo / 1000.0, hi / 1000.0)
    };

    view! {
        <div class="band-gutter">
            <canvas
                node_ref=canvas_ref
                on:pointerdown=on_pointerdown
                on:pointermove=on_pointermove
                on:pointerup=on_pointerup
                on:dblclick=on_dblclick
            />
            // Drag tooltip: floats next to the pointer while dragging, shows the
            // current lo–hi range. Hidden when not dragging.
            <div
                class="band-gutter-tooltip"
                style:top=move || tooltip_y.get().map(|y| format!("{:.0}px", y)).unwrap_or_default()
                style:display=move || if tooltip_y.get().is_some() && !format_range().is_empty() { "block" } else { "none" }
            >
                {format_range}
            </div>
        </div>
    }
}

/// Horizontal time-range gutter. Mounts as the bottom strip of a main
/// view; the strip renders the time-axis labels that used to live inside
/// the host canvas (so low frequencies in the spectrogram stay readable)
/// and acts as the single drag surface for creating `state.selection`
/// time ranges. A tap clears the selection, a drag sets it, a double-
/// click selects the full file duration.
///
/// `data_left_offset` is the number of pixels on the left that the host
/// view reserves for its own y-axis labels (0 for spectrogram / waveform,
/// `LABEL_AREA_WIDTH` on ZcChart). The gutter leaves that strip blank and
/// maps pointer events to time only within the data region, so the ticks
/// line up 1:1 with the host canvas.
#[component]
pub fn TimeGutter(#[prop(default = 0.0)] data_left_offset: f64) -> impl IntoView {
    let state = expect_context::<AppState>();
    let canvas_ref = NodeRef::<leptos::html::Canvas>::new();
    // Anchor time (seconds) at pointerdown. None when no drag is active.
    let drag_anchor: StoredValue<Option<f64>> = StoredValue::new(None);
    // Client-space start so we can detect a "tap" (no meaningful drag).
    let drag_start_client: StoredValue<(f64, f64)> = StoredValue::new((0.0, 0.0));

    // Resolve (scroll, visible_time, total_duration, time_res, clock_cfg).
    // Mirrors the per-view bookkeeping the main Effect does so the gutter
    // paints the same ticks as the host would have.
    let time_window = move || -> Option<(f64, f64, f64, f64, Option<crate::canvas::time_markers::ClockTimeConfig>)> {
        let canvas_w = state.spectrogram_canvas_width.get();
        if canvas_w <= 0.0 { return None; }
        let zoom = state.zoom_level.get();
        let scroll = state.scroll_offset.get();
        // Timeline mode has its own time_res/duration/clock.
        if let Some(tl) = state.active_timeline.get() {
            let files = state.files.get();
            let time_res = tl.segments.first()
                .and_then(|s| files.get(s.file_index))
                .map(|f| f.spectrogram.time_resolution)
                .unwrap_or(1.0);
            let duration = tl.total_duration_secs;
            let clock = if tl.origin_epoch_ms > 0.0 {
                Some(crate::canvas::time_markers::ClockTimeConfig {
                    recording_start_epoch_ms: tl.origin_epoch_ms,
                })
            } else { None };
            let data_w = (canvas_w - data_left_offset).max(1.0);
            let visible_time = (data_w / zoom) * time_res;
            return Some((scroll, visible_time, duration, time_res, clock));
        }
        let files = state.files.get();
        let idx = state.current_file_index.get()?;
        let file = files.get(idx)?;
        let time_res = file.spectrogram.time_resolution;
        let data_w = (canvas_w - data_left_offset).max(1.0);
        let visible_time = (data_w / zoom) * time_res;
        // Live listen/record uses waterfall total time as the duration
        // ceiling so the x-axis reads real elapsed seconds.
        let is_live = (file.is_live_listen || file.is_recording)
            && crate::canvas::live_waterfall::is_active();
        let duration = if is_live {
            crate::canvas::live_waterfall::total_time()
        } else {
            file.audio.duration_secs
        };
        let clock = file.recording_start_epoch_ms().map(|ms| {
            crate::canvas::time_markers::ClockTimeConfig {
                recording_start_epoch_ms: ms,
            }
        });
        Some((scroll, visible_time, duration, time_res, clock))
    };

    // Redraw on any relevant signal change.
    Effect::new(move |_| {
        let selection = state.selection.get();
        let _sidebar = state.sidebar_collapsed.get();
        let _sidebar_width = state.sidebar_width.get();
        let _rsidebar = state.right_sidebar_collapsed.get();
        let _rsidebar_width = state.right_sidebar_width.get();
        let _main_view = state.main_view.get();
        let show_clock = state.show_clock_time.get();
        let Some((scroll, visible_time, duration, _time_res, clock)) = time_window() else { return };

        let Some(canvas_el) = canvas_ref.get() else { return };
        let canvas: &HtmlCanvasElement = canvas_el.as_ref();
        let rect = canvas.get_bounding_client_rect();
        let display_w = rect.width() as u32;
        let display_h = rect.height() as u32;
        if display_w == 0 || display_h == 0 { return; }
        if canvas.width() != display_w || canvas.height() != display_h {
            canvas.set_width(display_w);
            canvas.set_height(display_h);
        }

        let Ok(Some(obj)) = canvas.get_context("2d") else { return };
        let Ok(ctx) = obj.dyn_into::<CanvasRenderingContext2d>() else { return };

        let w = display_w as f64;
        let h = display_h as f64;
        let data_x = data_left_offset.clamp(0.0, w);
        let data_w = (w - data_x).max(0.0);

        // Blank + fog across the data strip; the left-offset area stays
        // solid black so it reads as "no data here" next to the host's
        // y-axis labels.
        ctx.set_fill_style_str("#0a0a0a");
        ctx.fill_rect(0.0, 0.0, w, h);
        gutter_renderer::draw_time_gutter_overlay(
            &ctx,
            data_x, 0.0, data_w, h,
            scroll, scroll + visible_time,
            selection.map(|s| (s.time_start, s.time_end)),
        );

        // Time tick labels — translate so (0, 0) is the data origin, then
        // call the shared renderer with the data-region width. That keeps
        // label positions aligned with whatever the host draws above.
        ctx.save();
        let _ = ctx.translate(data_x, 0.0);
        crate::canvas::time_markers::draw_time_markers(
            &ctx, scroll, visible_time, data_w, h,
            duration, clock, show_clock, 1.0,
        );
        ctx.restore();
    });

    // Map a client-x to a time value inside the data strip.
    let x_to_time = move |client_x: f64| -> Option<f64> {
        let canvas_el = canvas_ref.get()?;
        let canvas: &HtmlCanvasElement = canvas_el.as_ref();
        let rect = canvas.get_bounding_client_rect();
        let w = rect.width();
        let data_w = (w - data_left_offset).max(1.0);
        let (scroll, visible_time, _, _, _) = time_window()?;
        let local_x = client_x - rect.left() - data_left_offset;
        let frac = (local_x / data_w).clamp(0.0, 1.0);
        Some(scroll + frac * visible_time)
    };

    let on_pointerdown = move |ev: web_sys::PointerEvent| {
        if ev.button() != 0 { return; }
        let Some(t) = x_to_time(ev.client_x() as f64) else { return };
        ev.prevent_default();
        drag_anchor.set_value(Some(t));
        drag_start_client.set_value((ev.client_x() as f64, ev.client_y() as f64));
        // Seed a zero-width selection so the highlight starts drawing; the
        // range expands as the pointer moves.
        let ff = state.focus_stack.get_untracked().effective_range();
        let (fl, fh) = if ff.is_active() { (Some(ff.lo), Some(ff.hi)) } else { (None, None) };
        state.selection.set(Some(Selection {
            time_start: t, time_end: t,
            freq_low: fl, freq_high: fh,
        }));
        state.is_dragging.set(true);
        if let Some(target) = ev.target() {
            if let Ok(el) = target.dyn_into::<web_sys::Element>() {
                let _ = el.set_pointer_capture(ev.pointer_id());
            }
        }
    };

    let on_pointermove = move |ev: web_sys::PointerEvent| {
        let Some(anchor) = drag_anchor.get_value() else { return };
        let Some(t) = x_to_time(ev.client_x() as f64) else { return };
        let (ts, te) = if t < anchor { (t, anchor) } else { (anchor, t) };
        let ff = state.focus_stack.get_untracked().effective_range();
        let (fl, fh) = if ff.is_active() { (Some(ff.lo), Some(ff.hi)) } else { (None, None) };
        state.selection.set(Some(Selection {
            time_start: ts, time_end: te,
            freq_low: fl, freq_high: fh,
        }));
    };

    let on_pointerup = move |ev: web_sys::PointerEvent| {
        if drag_anchor.get_value().is_none() { return; }
        let (sx, sy) = drag_start_client.get_value();
        let dx = (ev.client_x() as f64 - sx).abs();
        let dy = (ev.client_y() as f64 - sy).abs();
        let was_tap = dx < 3.0 && dy < 3.0;
        drag_anchor.set_value(None);
        state.is_dragging.set(false);
        if was_tap {
            // Tap on the time gutter clears any existing selection — same
            // "fog returns" metaphor the waveform's old in-canvas strip had.
            if state.selection.get_untracked().is_some() {
                state.selection.set(None);
            }
            return;
        }
        // Real drag committed. Promote a time-only segment to a region when
        // HFR is on so the selection carries the active band.
        if let Some(sel) = state.selection.get_untracked() {
            if sel.time_end - sel.time_start < 1e-4 {
                state.selection.set(None);
            } else if sel.freq_low.is_none() {
                let ff = state.focus_stack.get_untracked().effective_range();
                if ff.is_active() {
                    state.selection.set(Some(Selection {
                        freq_low: Some(ff.lo),
                        freq_high: Some(ff.hi),
                        ..sel
                    }));
                }
            }
        }
        state.active_focus.set(Some(ActiveFocus::TransientSelection));
    };

    let on_dblclick = move |_ev: web_sys::MouseEvent| {
        select_all_time(state);
    };

    view! {
        <div class="time-gutter">
            <canvas
                node_ref=canvas_ref
                on:pointerdown=on_pointerdown
                on:pointermove=on_pointermove
                on:pointerup=on_pointerup
                on:dblclick=on_dblclick
            />
        </div>
    }
}
