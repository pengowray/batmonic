/// Inertia/momentum scrolling for touch pan gestures.
///
/// Tracks finger velocity during touchmove, then animates a decaying
/// scroll after touchend via requestAnimationFrame.

use leptos::prelude::*;
use wasm_bindgen::prelude::*;

// ── Tuning constants ────────────────────────────────────────────────────────

/// Minimum px/s velocity to trigger inertia (below this, pan stops immediately).
const MIN_VELOCITY: f64 = 100.0;
/// Animation stops when effective velocity decays below this (px/s).
const STOP_VELOCITY: f64 = 5.0;
/// Exponential decay rate. Higher = faster stop. 4.0 ≈ settles in ~1s.
const FRICTION: f64 = 4.0;
/// Ring-buffer capacity for velocity estimation.
const MAX_SAMPLES: usize = 4;
/// Samples older than this (ms) are discarded from velocity calculation.
const MAX_SAMPLE_AGE_MS: f64 = 100.0;

// ── Velocity tracker ────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
pub struct VelocityTracker {
    samples: [(f64, f64); MAX_SAMPLES], // (timestamp_ms, client_x)
    count: usize,
    index: usize,
}

impl VelocityTracker {
    pub fn new() -> Self {
        Self {
            samples: [(0.0, 0.0); MAX_SAMPLES],
            count: 0,
            index: 0,
        }
    }

    pub fn reset(&mut self) {
        self.count = 0;
        self.index = 0;
    }

    pub fn push(&mut self, timestamp_ms: f64, client_x: f64) {
        self.samples[self.index] = (timestamp_ms, client_x);
        self.index = (self.index + 1) % MAX_SAMPLES;
        if self.count < MAX_SAMPLES {
            self.count += 1;
        }
    }

    /// Compute velocity in px/sec from recent samples.
    /// Positive = finger moving right, negative = finger moving left.
    pub fn velocity_px_per_sec(&self) -> f64 {
        if self.count < 2 {
            return 0.0;
        }

        // Collect samples newest-first
        let mut ordered: Vec<(f64, f64)> = Vec::with_capacity(self.count);
        for i in 0..self.count {
            let idx = (self.index + MAX_SAMPLES - 1 - i) % MAX_SAMPLES;
            ordered.push(self.samples[idx]);
        }

        let newest_time = ordered[0].0;

        // Filter to recent samples only
        let recent: Vec<&(f64, f64)> = ordered
            .iter()
            .filter(|(t, _)| newest_time - t <= MAX_SAMPLE_AGE_MS)
            .collect();

        if recent.len() < 2 {
            return 0.0;
        }

        let first = recent.last().unwrap();
        let last = recent.first().unwrap();
        let dt_ms = last.0 - first.0;
        if dt_ms < 1.0 {
            return 0.0;
        }

        (last.1 - first.1) / (dt_ms / 1000.0)
    }
}

// ── Inertia animation ───────────────────────────────────────────────────────

/// Bump the generation counter, causing any in-flight rAF loop to exit.
pub fn cancel_inertia(generation: StoredValue<u32>) {
    generation.update_value(|g| *g = g.wrapping_add(1));
}

/// Launch an inertia animation after a flick gesture.
///
/// `velocity_px_per_sec` is signed (positive = finger moved right = scroll backward).
/// The function converts this to a time-domain velocity and animates scroll_offset
/// with exponential decay until it stops.
use crate::viewport;

pub fn start_inertia(
    state: crate::state::AppState,
    velocity_px_per_sec: f64,
    canvas_width: f64,
    time_resolution: f64,
    duration: f64,
    from_here_mode: bool,
    generation: StoredValue<u32>,
) {
    if velocity_px_per_sec.abs() < MIN_VELOCITY || canvas_width == 0.0 {
        return;
    }

    // Bump generation so any previous animation stops
    cancel_inertia(generation);
    let my_gen = generation.get_value();

    let zoom = state.zoom_level.get_untracked();
    let visible_time = viewport::visible_time(canvas_width, zoom, time_resolution);
    // Convert px velocity to time velocity (same sign convention as apply_hand_pan: negate)
    let v0_time = -(velocity_px_per_sec / canvas_width) * visible_time;
    let start_scroll = state.scroll_offset.get_untracked();
    let (min_scroll, max_scroll) = viewport::scroll_bounds_for_mode(duration, visible_time, from_here_mode);

    // Threshold in time-domain units
    let stop_threshold = (STOP_VELOCITY / canvas_width) * visible_time;

    let window = web_sys::window().unwrap();
    let start_ms = window.performance().unwrap().now();

    // Self-referencing rAF closure (same pattern as playback.rs)
    let cb: std::rc::Rc<std::cell::RefCell<Option<Closure<dyn Fn()>>>> =
        std::rc::Rc::new(std::cell::RefCell::new(None));
    let cb_clone = cb.clone();

    *cb.borrow_mut() = Some(Closure::new(move || {
        // Generation check — exit if cancelled
        if generation.get_value() != my_gen {
            return;
        }

        let window = web_sys::window().unwrap();
        let now = window.performance().unwrap().now();
        let elapsed = (now - start_ms) / 1000.0; // seconds

        // Exponential decay: v(t) = v0 * e^(-friction * t)
        let decay = (-FRICTION * elapsed).exp();
        let current_v = v0_time * decay;

        // Integrated position: x(t) = x0 + (v0/friction)(1 - e^(-friction*t))
        let scroll = start_scroll + (v0_time / FRICTION) * (1.0 - decay);
        let clamped = scroll.clamp(min_scroll, max_scroll);

        state.scroll_offset.set(clamped);
        state.suspend_follow();

        // Stop conditions: velocity too low, or hit bounds
        let hit_bound = (clamped <= min_scroll && current_v < 0.0)
            || (clamped >= max_scroll && current_v > 0.0);

        if current_v.abs() < stop_threshold || hit_bound {
            return; // animation done, closure dropped on next GC
        }

        // Request next frame
        let _ = window.request_animation_frame(
            cb_clone.borrow().as_ref().unwrap().as_ref().unchecked_ref(),
        );
    }));

    // Kick off the first frame
    let _ = window.request_animation_frame(
        cb.borrow().as_ref().unwrap().as_ref().unchecked_ref(),
    );
}
