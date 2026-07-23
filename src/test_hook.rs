//! Read-only state snapshot exposed on `window.__oversample_test()` for end-to-
//! end (Playwright) assertions. Installed once at startup.
//!
//! This is intentionally always-on (the e2e harness runs the `--release`
//! build), but it is harmless in production: it only *reads* signals via
//! `get_untracked` and returns a plain JS object — there are no setters and no
//! reactive subscriptions, so it can't affect app behavior.

use crate::state::store_fields::*;
use crate::state::AppState;
use leptos::prelude::*;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

fn set(obj: &js_sys::Object, key: &str, val: JsValue) {
    let _ = js_sys::Reflect::set(obj, &JsValue::from_str(key), &val);
}

fn opt_f64(v: Option<f64>) -> JsValue {
    match v {
        Some(x) => JsValue::from_f64(x),
        None => JsValue::NULL,
    }
}

/// Install `window.__oversample_test`, a zero-arg function returning a snapshot
/// of view + live-capture state used by the e2e specs.
pub fn install(state: AppState) {
    let Some(window) = web_sys::window() else {
        return;
    };

    let cb = Closure::wrap(Box::new(move || -> JsValue {
        use crate::canvas::live_waterfall as wf;
        let o = js_sys::Object::new();
        // View
        set(
            &o,
            "scrollOffset",
            JsValue::from_f64(state.view.scroll_offset().get_untracked()),
        );
        set(
            &o,
            "zoom",
            JsValue::from_f64(state.view.zoom_level().get_untracked()),
        );
        set(
            &o,
            "minFreq",
            opt_f64(state.view.min_display_freq().get_untracked()),
        );
        set(
            &o,
            "maxFreq",
            opt_f64(state.view.max_display_freq().get_untracked()),
        );
        // Live capture
        let listening = state.mic.listening().get_untracked();
        let recording = state.mic.recording().get_untracked();
        let live_active = wf::is_active();
        set(&o, "listening", JsValue::from_bool(listening));
        set(&o, "recording", JsValue::from_bool(recording));
        set(&o, "liveActive", JsValue::from_bool(live_active));
        set(&o, "liveTotalTime", JsValue::from_f64(wf::total_time()));
        set(
            &o,
            "liveDataCols",
            JsValue::from_f64(state.mic.live_data_cols().get_untracked() as f64),
        );
        set(
            &o,
            "recordingTargetScroll",
            JsValue::from_f64(state.mic.recording_target_scroll().get_untracked()),
        );
        let pan_until = state.mic.scroll_user_pan_until().get_untracked();
        set(&o, "scrollUserPanUntil", JsValue::from_f64(pan_until));
        // Derived: is the live waterfall auto-follow currently engaged?
        let now = js_sys::Date::now();
        let following = (listening || recording) && live_active && now >= pan_until;
        set(&o, "following", JsValue::from_bool(following));
        // The overview's displayed window [axisStart, axisStart+span] (live only).
        if let Some((axis_start, span)) = crate::components::overview::live_overview_window(&state)
        {
            set(&o, "overviewAxisStart", JsValue::from_f64(axis_start));
            set(&o, "overviewSpan", JsValue::from_f64(span));
        }
        o.into()
    }) as Box<dyn Fn() -> JsValue>);

    let _ = js_sys::Reflect::set(
        &window,
        &JsValue::from_str("__oversample_test"),
        cb.as_ref().unchecked_ref(),
    );
    cb.forget(); // keep the closure alive for the page lifetime
}
