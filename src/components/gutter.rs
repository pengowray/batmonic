// Gutter components — dedicated drag surfaces for range selection that
// live alongside (not on top of) the main view canvases.
//
// `BandGutter` is a narrow vertical strip on the right of a view, owning
// frequency-band (HFR) selection. `TimeGutter` is a thin horizontal
// strip below a view, owning time-range selection and rendering the
// time axis labels that previously sat inside the main canvas.

use crate::canvas::gutter_renderer;
use crate::components::axis_drag::{
    apply_axis_drag, finalize_axis_drag, freq_snap, select_all_frequencies, select_all_time,
};
use crate::components::pinch::{apply_freq_pinch, two_finger_y_geometry, FreqPinchState};
use crate::state::store_fields::*;
use crate::state::{ActiveFocus, AppState, Selection};
use js_sys;
use leptos::prelude::*;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement};

/// Slack (CSS px) before a pointer press on a gutter is promoted from
/// "tap" to "drag". Wider than the spectrogram's 3 px because touch
/// wobble on a narrow strip reliably exceeded 3 px, turning every tap
/// into a spurious 1 kHz band + HFR toggle-on.
const TAP_SLOP_PX: f64 = 10.0;

/// Max gap (ms) between taps for double-tap detection on touch devices,
/// where browsers sometimes suppress synthetic `dblclick` after
/// `touch-action: none`.
const DBLTAP_WINDOW_MS: f64 = 400.0;

/// File Nyquist (Hz) — ceiling for display-freq clamping. Mirrors
/// `spectrogram_events::file_nyquist` but duplicated here to keep the
/// gutter self-contained.
fn gutter_nyquist(state: AppState) -> f64 {
    let is_mic_active =
        state.mic.recording().get_untracked() || state.mic.listening().get_untracked();
    if is_mic_active && crate::canvas::live_waterfall::is_active() {
        crate::canvas::live_waterfall::max_freq()
    } else {
        let files = state.library.files().get_untracked();
        let idx = state.library.current_index().get_untracked();
        idx.and_then(|i| files.get(i))
            .map(|f| f.spectrogram.max_freq)
            .unwrap_or(96_000.0)
    }
}

/// True if the primary pointer is a finger — used to reserve the larger
/// slop / explicit-dbltap paths for touch only, so mouse precision
/// isn't degraded.
fn pointer_is_touch(ev: &web_sys::PointerEvent) -> bool {
    ev.pointer_type() == "touch" || ev.pointer_type() == "pen"
}

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
    // Canvas-local y at pointerdown — used with TAP_SLOP_PX to block
    // apply_axis_drag firings until the pointer has moved far enough for
    // the press to count as a drag (stops taps from creating tiny bands).
    let drag_start_y: StoredValue<Option<f64>> = StoredValue::new(None);
    // Flipped true once the pointer has moved past the slop — after that,
    // pointermove feeds apply_axis_drag and pointerup finalizes the drag
    // instead of treating it as a tap.
    let drag_active: StoredValue<bool> = StoredValue::new(false);
    // Last tap timestamp / canvas-y for explicit double-tap detection on
    // touch. Browsers synthesize `dblclick` unreliably with
    // `touch-action: none`, so we detect it ourselves from the pointer
    // stream to keep double-tap → "select all frequencies" working on
    // mobile.
    let last_tap_time: StoredValue<f64> = StoredValue::new(0.0);
    let last_tap_y: StoredValue<f64> = StoredValue::new(0.0);
    // Two-finger pinch state — Some while a pinch gesture is in progress.
    let pinch_state: StoredValue<Option<FreqPinchState>> = StoredValue::new(None);
    // Tooltip position (canvas-local y, in px) — drives the drag tooltip.
    // None while not dragging.
    let tooltip_y = RwSignal::new_local(Option::<f64>::None);
    // Bumped by a ResizeObserver so the draw Effect re-runs when the gutter's
    // box changes height (see Spectrogram for the same pattern and rationale).
    let canvas_size_tick: RwSignal<u32> = RwSignal::new(0);

    // Always paint the gutter at the file's full 0..Nyquist range so it
    // acts as a stable frequency reference / overview while the main
    // canvas independently V-zooms. The gutter is wide enough to drag-
    // select bands at file scale; v-zoom navigation lives on the main
    // canvas y-axis.
    let display_range = move || -> (f64, f64) {
        let files = state.library.files().get();
        let idx = state.library.current_index().get();
        let nyquist = idx
            .and_then(|i| files.get(i))
            .map(|f| f.audio.sample_rate as f64 / 2.0)
            .unwrap_or(0.0);
        (0.0, nyquist)
    };

    // Redraw when any relevant signal changes.
    Effect::new(move |_| {
        let band_lo = state.filter.band_ff_freq_lo().get();
        let band_hi = state.filter.band_ff_freq_hi().get();
        let hfr_on = state.viewmode.hfr_enabled().get();
        let shield_style = state.viewmode.shield_style().get();
        // Live drag range from either this gutter or the spectrogram's
        // y-axis — when Some, overrides the stored band so the shield
        // lights up mid-drag even before the band has been committed.
        let drag_range = match (
            state.interaction.axis_drag_start_freq().get(),
            state.interaction.axis_drag_current_freq().get(),
        ) {
            (Some(s), Some(c)) => Some((s, c)),
            _ => None,
        };
        let (min_freq, max_freq) = display_range();
        let _sidebar = state.panels.left_collapsed().get();
        let _sidebar_width = state.panels.left_width().get();
        let _rsidebar = state.panels.right_collapsed().get();
        let _rsidebar_width = state.panels.right_width().get();
        let _tile_ready = state.viewmode.tile_ready_signal().get();
        let _size_tick = canvas_size_tick.get();

        let Some(canvas_el) = canvas_ref.get() else {
            return;
        };
        let canvas: &HtmlCanvasElement = canvas_el.as_ref();
        // Measure the parent .band-gutter so the canvas pixel buffer tracks
        // the flex-sized container height even when `height: 100%` on the
        // canvas doesn't resolve through nested flex containers.
        let (display_w, display_h) = match canvas.parent_element() {
            Some(parent) => {
                let r = parent.get_bounding_client_rect();
                (r.width() as u32, r.height() as u32)
            }
            None => {
                let r = canvas.get_bounding_client_rect();
                (r.width() as u32, r.height() as u32)
            }
        };
        if display_w == 0 || display_h == 0 {
            return;
        }
        if canvas.width() != display_w || canvas.height() != display_h {
            canvas.set_width(display_w);
            canvas.set_height(display_h);
        }

        let Ok(Some(obj)) = canvas.get_context("2d") else {
            return;
        };
        let Ok(ctx) = obj.dyn_into::<CanvasRenderingContext2d>() else {
            return;
        };

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

    // ResizeObserver: observe the parent .band-gutter (not the canvas), so
    // flex-driven container resizes re-trigger a draw with the container's
    // real height even when the canvas itself is stuck at an intrinsic size.
    Effect::new(move |_| {
        let Some(el) = canvas_ref.get() else { return };
        let canvas: &HtmlCanvasElement = el.as_ref();
        let Some(parent) = canvas.parent_element() else {
            return;
        };
        let cb = Closure::<dyn Fn(js_sys::Array)>::new(move |_entries: js_sys::Array| {
            // Bail if the component (and this signal) was disposed between
            // the DOM mutation and the observer firing — otherwise
            // `get_untracked` on a disposed signal panics.
            let Some(cur) = canvas_size_tick.try_get_untracked() else {
                return;
            };
            canvas_size_tick.set(cur.wrapping_add(1));
        });
        if let Ok(observer) = web_sys::ResizeObserver::new(cb.as_ref().unchecked_ref()) {
            observer.observe(&parent);
            let _ =
                js_sys::Reflect::set(&parent, &JsValue::from_str("__band_resize_obs"), &observer);
        }
        cb.forget();
    });

    // Resolve (local_y, canvas_height, min_freq, max_freq) for a pointer
    // event — frequency bounds reflect the host view's current display
    // range so drag math uses the same mapping the gutter renders with.
    let pointer_context = move |ev: &web_sys::PointerEvent| -> Option<(f64, f64, f64, f64)> {
        let canvas_el = canvas_ref.get()?;
        let canvas: &HtmlCanvasElement = canvas_el.as_ref();
        let rect = canvas.get_bounding_client_rect();
        let h = rect.height();
        if h <= 0.0 {
            return None;
        }
        let y = ev.client_y() as f64 - rect.top();
        let (min_freq, max_freq) = display_range();
        if max_freq <= min_freq {
            return None;
        }
        Some((y, h, min_freq, max_freq))
    };

    let on_pointerdown = move |ev: web_sys::PointerEvent| {
        if ev.button() != 0 {
            return;
        }
        // Two-finger touch is handled by the touchstart path (pinch/pan).
        // Skip the pointer path so it doesn't race with the pinch state.
        if pinch_state.get_value().is_some() {
            return;
        }
        let Some((y, h, min_freq, max_freq)) = pointer_context(&ev) else {
            return;
        };
        ev.prevent_default();

        // Explicit double-tap on touch: browsers don't always fire
        // dblclick with `touch-action: none`, so we detect it ourselves
        // from the pointer stream and route to "select all frequencies".
        if pointer_is_touch(&ev) {
            let now = js_sys::Date::now();
            let last_t = last_tap_time.get_value();
            let last_y = last_tap_y.get_value();
            if now - last_t < DBLTAP_WINDOW_MS && (y - last_y).abs() < 30.0 {
                last_tap_time.set_value(0.0);
                // Clear any in-progress tap bookkeeping before the dbl-tap
                // swaps the selection out from under us.
                drag_anchor.set_value(None);
                drag_start_y.set_value(None);
                drag_active.set_value(false);
                state.filter.band_ff_dragging().set(false);
                state.interaction.axis_drag_start_freq().set(None);
                state.interaction.axis_drag_current_freq().set(None);
                state.interaction.is_dragging().set(false);
                select_all_frequencies(state);
                return;
            }
        }

        let freq = gutter_renderer::y_to_freq(y, min_freq, max_freq, h);
        let shift = ev.shift_key();
        let band_lo = state.filter.band_ff_freq_lo().get_untracked();
        let band_hi = state.filter.band_ff_freq_hi().get_untracked();
        let has_range = band_hi > band_lo;

        // Shift+click extend: anchor at the edge of the existing range
        // farthest from the click, so dragging grows the band from there.
        let raw_start = if shift && has_range {
            if (freq - band_lo).abs() < (freq - band_hi).abs() {
                band_hi
            } else {
                band_lo
            }
        } else {
            freq
        };

        drag_anchor.set_value(Some(raw_start));
        drag_start_y.set_value(Some(y));
        drag_active.set_value(false);
        tooltip_y.set(Some(y));
        // Flag the drag so heavy consumers (waveform band-split) can cache.
        state.filter.band_ff_dragging().set(true);

        // Seed the shared axis-drag state so the fog/shield glows in
        // response to the press. Both endpoints seed to the same snapped
        // freq so finalize_axis_drag will still detect a tap (start == end)
        // if the pointer never leaves the slop zone.
        let snap_s = freq_snap(raw_start, shift);
        let snap_e = freq_snap(freq, shift);
        state
            .interaction
            .axis_drag_start_freq()
            .set(Some((raw_start / snap_s).round() * snap_s));
        state
            .interaction
            .axis_drag_current_freq()
            .set(Some((freq / snap_e).round() * snap_e));
        state.interaction.is_dragging().set(true);

        // Shift-extend should update the band immediately; a fresh drag
        // waits for pointermove so a pure tap leaves the existing band
        // intact (tap = toggle HFR, handled in finalize_axis_drag).
        if shift && has_range {
            let lo = raw_start.min(freq);
            let hi = raw_start.max(freq);
            if hi - lo > 500.0 {
                state.set_band_ff_range(lo, hi);
            }
            drag_active.set_value(true);
        }

        if let Some(target) = ev.target() {
            if let Ok(el) = target.dyn_into::<web_sys::Element>() {
                let _ = el.set_pointer_capture(ev.pointer_id());
            }
        }
    };

    let on_pointermove = move |ev: web_sys::PointerEvent| {
        let Some(raw_start) = drag_anchor.get_value() else {
            return;
        };
        let Some((y, h, min_freq, max_freq)) = pointer_context(&ev) else {
            return;
        };
        tooltip_y.set(Some(y.clamp(0.0, h)));

        // Tap-zone gate: don't promote a press into a drag (which would
        // snap a 1 kHz range and auto-enable HFR) until the pointer has
        // moved past TAP_SLOP_PX vertically. Finger jitter on the narrow
        // gutter routinely exceeded 1–2 px on mobile, causing every tap
        // to register as a drag.
        if !drag_active.get_value() {
            let dy = drag_start_y
                .get_value()
                .map(|s0| (y - s0).abs())
                .unwrap_or(0.0);
            if dy < TAP_SLOP_PX {
                return;
            }
            drag_active.set_value(true);
        }

        let freq = gutter_renderer::y_to_freq(y, min_freq, max_freq, h);
        apply_axis_drag(state, raw_start, freq, ev.shift_key());
    };

    let on_pointerup = move |ev: web_sys::PointerEvent| {
        if drag_anchor.get_value().is_none() {
            return;
        }
        let was_active = drag_active.get_value();
        drag_anchor.set_value(None);
        drag_start_y.set_value(None);
        drag_active.set_value(false);
        tooltip_y.set(None);
        state.filter.band_ff_dragging().set(false);

        if was_active {
            // Shared finalize: meaningful drag auto-enables HFR and
            // promotes focus to FrequencyFocus.
            finalize_axis_drag(state);
        } else {
            // Never promoted to a drag — treat as a tap. Toggle HFR off
            // if on, otherwise leave band untouched. Record the tap time
            // so a follow-up tap within DBLTAP_WINDOW_MS is seen as a
            // double-tap ("select all frequencies").
            state.interaction.axis_drag_start_freq().set(None);
            state.interaction.axis_drag_current_freq().set(None);
            state.interaction.is_dragging().set(false);
            let stack = state.viewmode.focus_stack().get_untracked();
            if stack.hfr_enabled() {
                state.toggle_hfr();
            }
            if pointer_is_touch(&ev) {
                if let Some((y, _, _, _)) = pointer_context(&ev) {
                    last_tap_time.set_value(js_sys::Date::now());
                    last_tap_y.set_value(y);
                }
            }
        }
    };

    let on_dblclick = move |ev: web_sys::MouseEvent| {
        // Dedupe with the explicit touch double-tap path above: suppress
        // browser-synthesized dblclicks too close to the last touch tap,
        // otherwise we'd run select_all_frequencies twice.
        let now = js_sys::Date::now();
        if now - last_tap_time.get_value() < DBLTAP_WINDOW_MS + 50.0 {
            last_tap_time.set_value(0.0);
            ev.prevent_default();
            return;
        }
        select_all_frequencies(state);
    };

    // Two-finger pinch + pan on the gutter. Pinch-in/out changes the
    // spectrogram's visible frequency range (zoom on the host view, not
    // on the gutter). Two-finger parallel drag in y pans the range.
    let on_touchstart = move |ev: web_sys::TouchEvent| {
        let touches = ev.touches();
        if touches.length() == 2 {
            ev.prevent_default();
            // Kill any single-finger drag that may have started before the
            // second finger landed — otherwise the pointer path keeps
            // applying apply_axis_drag underneath the pinch.
            drag_anchor.set_value(None);
            drag_start_y.set_value(None);
            drag_active.set_value(false);
            tooltip_y.set(None);
            state.filter.band_ff_dragging().set(false);
            state.interaction.axis_drag_start_freq().set(None);
            state.interaction.axis_drag_current_freq().set(None);
            state.interaction.is_dragging().set(false);

            let Some(canvas_el) = canvas_ref.get() else {
                return;
            };
            let canvas: &HtmlCanvasElement = canvas_el.as_ref();
            let rect = canvas.get_bounding_client_rect();
            if rect.height() <= 0.0 {
                return;
            }
            let Some((mid_client_y, dist_y)) = two_finger_y_geometry(&touches) else {
                return;
            };

            let nyquist = gutter_nyquist(state);
            let initial_min_freq = state.view.min_display_freq().get_untracked().unwrap_or(0.0);
            let initial_max_freq = state
                .view
                .max_display_freq()
                .get_untracked()
                .unwrap_or(nyquist);
            pinch_state.set_value(Some(FreqPinchState {
                initial_dist_y: dist_y.max(1.0),
                initial_min_freq,
                initial_max_freq,
                initial_mid_canvas_y: mid_client_y - rect.top(),
                nyquist,
            }));
        }
    };

    let on_touchmove = move |ev: web_sys::TouchEvent| {
        let touches = ev.touches();
        if touches.length() != 2 {
            return;
        }
        let Some(ps) = pinch_state.get_value() else {
            return;
        };
        ev.prevent_default();

        let Some((mid_client_y, dist_y)) = two_finger_y_geometry(&touches) else {
            return;
        };
        let Some(canvas_el) = canvas_ref.get() else {
            return;
        };
        let canvas: &HtmlCanvasElement = canvas_el.as_ref();
        let rect = canvas.get_bounding_client_rect();
        let canvas_h = rect.height();
        if canvas_h <= 0.0 {
            return;
        }
        let current_mid_canvas_y = mid_client_y - rect.top();

        let (new_min, new_max) = apply_freq_pinch(&ps, dist_y, current_mid_canvas_y, canvas_h);
        state.view.min_display_freq().set(Some(new_min));
        state.view.max_display_freq().set(Some(new_max));
    };

    let on_touchend = move |ev: web_sys::TouchEvent| {
        if ev.touches().length() < 2 {
            pinch_state.set_value(None);
        }
    };

    // Mouse wheel over the gutter: plain wheel pans the visible frequency
    // range vertically (mirrors the two-finger pan on touch); modifier
    // keys zoom around the pointer so the gutter is a one-stop vertical
    // navigation control on desktop too.
    let on_wheel = move |ev: web_sys::WheelEvent| {
        ev.prevent_default();
        let Some(canvas_el) = canvas_ref.get() else {
            return;
        };
        let canvas: &HtmlCanvasElement = canvas_el.as_ref();
        let rect = canvas.get_bounding_client_rect();
        let h = rect.height();
        if h <= 0.0 {
            return;
        }
        let nyquist = gutter_nyquist(state);
        if nyquist <= 0.0 {
            return;
        }
        let cur_min = state.view.min_display_freq().get_untracked().unwrap_or(0.0);
        let cur_max = state
            .view
            .max_display_freq()
            .get_untracked()
            .unwrap_or(nyquist);
        let range = (cur_max - cur_min).max(1.0);

        if ev.shift_key() || ev.ctrl_key() || ev.meta_key() {
            // Zoom around pointer y. delta_y > 0 (wheel down) → zoom out.
            let local_y = ev.client_y() as f64 - rect.top();
            let anchor_frac = (local_y / h).clamp(0.0, 1.0);
            let anchor_freq = cur_max - anchor_frac * range;
            let factor = if ev.delta_y() > 0.0 { 1.15 } else { 1.0 / 1.15 };
            let new_range = (range * factor).clamp(500.0_f64.min(nyquist), nyquist);
            let mut new_max = anchor_freq + anchor_frac * new_range;
            let mut new_min = new_max - new_range;
            if new_min < 0.0 {
                new_min = 0.0;
                new_max = new_range.min(nyquist);
            }
            if new_max > nyquist {
                new_max = nyquist;
                new_min = (new_max - new_range).max(0.0);
            }
            state.view.min_display_freq().set(Some(new_min));
            state.view.max_display_freq().set(Some(new_max));
        } else {
            // Plain wheel: pan by ~10% of the visible range per tick.
            // delta_y > 0 (wheel down) → see lower freqs (max decreases).
            let raw = ev.delta_y() + ev.delta_x();
            let step = raw.signum() * range * 0.1 * (raw.abs() / 100.0).min(3.0);
            let mut new_max = cur_max - step;
            let mut new_min = cur_min - step;
            if new_min < 0.0 {
                new_min = 0.0;
                new_max = range.min(nyquist);
            }
            if new_max > nyquist {
                new_max = nyquist;
                new_min = (new_max - range).max(0.0);
            }
            state.view.min_display_freq().set(Some(new_min));
            state.view.max_display_freq().set(Some(new_max));
        }
    };

    // Format "40.0 – 72.5 kHz" for the drag tooltip.
    let format_range = move || {
        let lo = state.filter.band_ff_freq_lo().get();
        let hi = state.filter.band_ff_freq_hi().get();
        if hi <= lo {
            return String::new();
        }
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
                on:touchstart=on_touchstart
                on:touchmove=on_touchmove
                on:touchend=on_touchend
                on:touchcancel=on_touchend
                on:wheel=on_wheel
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
/// and acts as the single drag surface for creating `state.interaction.selection()`
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
    // Last tap timestamp / client-x for explicit double-tap on touch.
    let last_tap_time: StoredValue<f64> = StoredValue::new(0.0);
    let last_tap_x: StoredValue<f64> = StoredValue::new(0.0);
    // Bumped by a ResizeObserver so the draw Effect re-runs when the
    // parent's box changes height (see BandGutter for the same pattern).
    let canvas_size_tick: RwSignal<u32> = RwSignal::new(0);

    // Resolve (scroll, visible_time, total_duration, time_res, clock_cfg).
    // Mirrors the per-view bookkeeping the main Effect does so the gutter
    // paints the same ticks as the host would have.
    let time_window = move || -> Option<(f64, f64, f64, f64, Option<crate::canvas::time_markers::ClockTimeConfig>)> {
        let canvas_w = state.viewmode.spectrogram_canvas_width().get();
        if canvas_w <= 0.0 { return None; }
        let zoom = state.view.zoom_level().get();
        let scroll = state.view.scroll_offset().get();
        // Timeline mode has its own time_res/duration/clock.
        if let Some(tl) = state.timeline.active().get() {
            let files = state.library.files().get();
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
        let files = state.library.files().get();
        let idx = state.library.current_index().get()?;
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
        let selection = state.interaction.selection().get();
        let _sidebar = state.panels.left_collapsed().get();
        let _sidebar_width = state.panels.left_width().get();
        let _rsidebar = state.panels.right_collapsed().get();
        let _rsidebar_width = state.panels.right_width().get();
        let _main_view = state.viewmode.main_view().get();
        let show_clock = state.timeline.show_clock_time().get();
        let _size_tick = canvas_size_tick.get();
        let Some((scroll, visible_time, duration, _time_res, clock)) = time_window() else {
            return;
        };

        let Some(canvas_el) = canvas_ref.get() else {
            return;
        };
        let canvas: &HtmlCanvasElement = canvas_el.as_ref();
        // Measure the parent .time-gutter, not the canvas. The canvas'
        // `height: 100%` can fail to resolve through the flex chain and
        // fall back to the intrinsic 150px — writing that height back via
        // set_height() feedback-loops the .view-bottom-row past its 24px
        // flex-basis, swallowing most of the view.
        let (display_w, display_h) = match canvas.parent_element() {
            Some(parent) => {
                let r = parent.get_bounding_client_rect();
                (r.width() as u32, r.height() as u32)
            }
            None => {
                let r = canvas.get_bounding_client_rect();
                (r.width() as u32, r.height() as u32)
            }
        };
        if display_w == 0 || display_h == 0 {
            return;
        }
        if canvas.width() != display_w || canvas.height() != display_h {
            canvas.set_width(display_w);
            canvas.set_height(display_h);
        }

        let Ok(Some(obj)) = canvas.get_context("2d") else {
            return;
        };
        let Ok(ctx) = obj.dyn_into::<CanvasRenderingContext2d>() else {
            return;
        };

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
            data_x,
            0.0,
            data_w,
            h,
            scroll,
            scroll + visible_time,
            selection.map(|s| (s.time_start, s.time_end)),
        );

        // Time tick labels — translate so (0, 0) is the data origin, then
        // call the shared renderer with the data-region width. That keeps
        // label positions aligned with whatever the host draws above.
        ctx.save();
        let _ = ctx.translate(data_x, 0.0);
        crate::canvas::time_markers::draw_time_markers(
            &ctx,
            scroll,
            visible_time,
            data_w,
            h,
            duration,
            clock,
            show_clock,
            1.0,
        );
        ctx.restore();
    });

    // ResizeObserver: observe the parent .time-gutter (not the canvas), so
    // flex-driven container resizes re-trigger a draw with the container's
    // real height even when the canvas itself is stuck at an intrinsic size.
    Effect::new(move |_| {
        let Some(el) = canvas_ref.get() else { return };
        let canvas: &HtmlCanvasElement = el.as_ref();
        let Some(parent) = canvas.parent_element() else {
            return;
        };
        let cb = Closure::<dyn Fn(js_sys::Array)>::new(move |_entries: js_sys::Array| {
            // Bail if the component (and this signal) was disposed between
            // the DOM mutation and the observer firing — otherwise
            // `get_untracked` on a disposed signal panics.
            let Some(cur) = canvas_size_tick.try_get_untracked() else {
                return;
            };
            canvas_size_tick.set(cur.wrapping_add(1));
        });
        if let Ok(observer) = web_sys::ResizeObserver::new(cb.as_ref().unchecked_ref()) {
            observer.observe(&parent);
            let _ =
                js_sys::Reflect::set(&parent, &JsValue::from_str("__time_resize_obs"), &observer);
        }
        cb.forget();
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
        if ev.button() != 0 {
            return;
        }
        let Some(t) = x_to_time(ev.client_x() as f64) else {
            return;
        };
        ev.prevent_default();

        // Explicit double-tap on touch: browsers sometimes suppress
        // synthetic `dblclick` when `touch-action: none` is set, so we
        // detect the gesture ourselves and route it to select_all_time.
        if pointer_is_touch(&ev) {
            let now = js_sys::Date::now();
            let last_t = last_tap_time.get_value();
            let last_x = last_tap_x.get_value();
            if now - last_t < DBLTAP_WINDOW_MS && (ev.client_x() as f64 - last_x).abs() < 30.0 {
                last_tap_time.set_value(0.0);
                drag_anchor.set_value(None);
                state.interaction.is_dragging().set(false);
                select_all_time(state);
                return;
            }
        }

        drag_anchor.set_value(Some(t));
        drag_start_client.set_value((ev.client_x() as f64, ev.client_y() as f64));
        // Seed a zero-width selection so the highlight starts drawing; the
        // range expands as the pointer moves.
        let ff = state
            .viewmode
            .focus_stack()
            .get_untracked()
            .effective_range();
        let (fl, fh) = if ff.is_active() {
            (Some(ff.lo), Some(ff.hi))
        } else {
            (None, None)
        };
        state.interaction.selection().set(Some(Selection {
            time_start: t,
            time_end: t,
            freq_low: fl,
            freq_high: fh,
        }));
        state.interaction.is_dragging().set(true);
        if let Some(target) = ev.target() {
            if let Ok(el) = target.dyn_into::<web_sys::Element>() {
                let _ = el.set_pointer_capture(ev.pointer_id());
            }
        }
    };

    let on_pointermove = move |ev: web_sys::PointerEvent| {
        let Some(anchor) = drag_anchor.get_value() else {
            return;
        };
        let Some(t) = x_to_time(ev.client_x() as f64) else {
            return;
        };
        let (ts, te) = if t < anchor { (t, anchor) } else { (anchor, t) };
        let ff = state
            .viewmode
            .focus_stack()
            .get_untracked()
            .effective_range();
        let (fl, fh) = if ff.is_active() {
            (Some(ff.lo), Some(ff.hi))
        } else {
            (None, None)
        };
        state.interaction.selection().set(Some(Selection {
            time_start: ts,
            time_end: te,
            freq_low: fl,
            freq_high: fh,
        }));
    };

    let on_pointerup = move |ev: web_sys::PointerEvent| {
        if drag_anchor.get_value().is_none() {
            return;
        }
        let (sx, sy) = drag_start_client.get_value();
        let dx = (ev.client_x() as f64 - sx).abs();
        let dy = (ev.client_y() as f64 - sy).abs();
        // Tap slop: generous enough to ride through finger wobble on
        // touch; the tight 3 px threshold used to force a mobile tap to
        // carve out a half-second selection before the user could release.
        let slop = if pointer_is_touch(&ev) {
            TAP_SLOP_PX
        } else {
            3.0
        };
        let was_tap = dx < slop && dy < slop;
        drag_anchor.set_value(None);
        state.interaction.is_dragging().set(false);
        if was_tap {
            // Tap on the time gutter clears any existing selection — same
            // "fog returns" metaphor the waveform's old in-canvas strip had.
            if state.interaction.selection().get_untracked().is_some() {
                state.interaction.selection().set(None);
            }
            // Record this tap so a second one within DBLTAP_WINDOW_MS
            // still fires select_all_time even when the browser eats the
            // dblclick event.
            if pointer_is_touch(&ev) {
                last_tap_time.set_value(js_sys::Date::now());
                last_tap_x.set_value(ev.client_x() as f64);
            }
            return;
        }
        // Real drag committed. Promote a time-only segment to a region when
        // HFR is on so the selection carries the active band.
        if let Some(sel) = state.interaction.selection().get_untracked() {
            if sel.time_end - sel.time_start < 1e-4 {
                state.interaction.selection().set(None);
            } else if sel.freq_low.is_none() {
                let ff = state
                    .viewmode
                    .focus_stack()
                    .get_untracked()
                    .effective_range();
                if ff.is_active() {
                    state.interaction.selection().set(Some(Selection {
                        freq_low: Some(ff.lo),
                        freq_high: Some(ff.hi),
                        ..sel
                    }));
                }
            }
        }
        state
            .interaction
            .active_focus()
            .set(Some(ActiveFocus::TransientSelection));
    };

    let on_dblclick = move |ev: web_sys::MouseEvent| {
        // De-dupe with explicit touch double-tap detection: if a
        // synthetic dblclick arrives right after we already handled the
        // touch gesture, swallow it.
        let now = js_sys::Date::now();
        if now - last_tap_time.get_value() < DBLTAP_WINDOW_MS + 50.0 {
            last_tap_time.set_value(0.0);
            ev.prevent_default();
            return;
        }
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
