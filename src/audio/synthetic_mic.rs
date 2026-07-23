//! Synthetic "mic" test mode — drives the *real* live-listen pipeline with
//! generated audio (noise / tones / chirps / pulses) so the live waterfall,
//! waveform, overview and auto-scroll can be exercised visually and profiled
//! without any hardware. Started/stopped from the Debug panel.
//!
//! It deliberately reuses `start_live_listening` + `spawn_live_processing_loop`
//! + `spawn_smooth_scroll_animation` rather than re-implementing them, so what's
//! under test is the genuine capture-to-render path. The only fake part is a
//! feeder loop that appends generated samples to the live sample buffer at the
//! true sample rate, exactly as a real backend's audio callback would.

use std::cell::Cell;
use std::f64::consts::TAU;

use crate::audio::live_recording::{
    cleanup_listen_file, spawn_live_processing_loop, spawn_smooth_scroll_animation,
    start_live_listening,
};
use crate::audio::mic_backend::with_live_samples_mut;
use crate::state::store_fields::*;
use crate::state::{AppState, MainView};
use leptos::prelude::*;

/// Which synthetic signal to generate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SynthSignal {
    /// Broadband white noise (fills the whole spectrogram).
    Noise,
    /// Steady 40 kHz sine — a single horizontal line for axis calibration.
    Tone,
    /// Linear chirp sweeping 20 kHz → near-Nyquist over 2 s, repeating.
    Chirp,
    /// Three steady tones (20 / 45 / 80 kHz, clamped) — calibration grid.
    MultiTone,
    /// Bat-like downward FM pulses (~3 ms, 90→35 kHz) every 80 ms over noise.
    Pulses,
}

impl SynthSignal {
    pub fn label(self) -> &'static str {
        match self {
            SynthSignal::Noise => "Noise",
            SynthSignal::Tone => "Tone 40k",
            SynthSignal::Chirp => "Chirp",
            SynthSignal::MultiTone => "Multi-tone",
            SynthSignal::Pulses => "Pulses",
        }
    }
}

thread_local! {
    /// Generation counter — bumped on every start/stop so a stale feeder loop
    /// exits when a new session begins or the user stops.
    static SYNTH_GEN: Cell<u64> = const { Cell::new(0) };
    static SYNTH_ACTIVE: Cell<bool> = const { Cell::new(false) };
    /// The signal of the currently-running synthetic session (for status UI).
    static SYNTH_SIGNAL: Cell<Option<SynthSignal>> = const { Cell::new(None) };
}

/// Whether a synthetic test session is currently running.
pub fn is_active() -> bool {
    SYNTH_ACTIVE.with(|a| a.get())
}

/// Label of the currently-running synthetic signal, or `None` when no synth
/// session is active. Used to title the live status as a test signal.
pub fn active_label() -> Option<&'static str> {
    SYNTH_SIGNAL.with(|s| s.get()).map(SynthSignal::label)
}

/// Start a synthetic live session with `signal` at `sample_rate` Hz.
/// Tears down any prior synthetic session first.
pub fn start(state: AppState, signal: SynthSignal, sample_rate: u32) {
    // Never clobber a real capture session — synth clears the live buffer.
    if state.mic.recording().get_untracked()
        || (state.mic.listening().get_untracked() && !is_active())
    {
        state.show_error_toast("Stop the live mic session before running a test signal");
        return;
    }
    stop(&state);

    // The live waterfall only renders under a spectrogram-family view.
    if !state.viewmode.main_view().get_untracked().is_spectrogram() {
        state.viewmode.main_view().set(MainView::Spectrogram);
    }

    let gen = SYNTH_GEN.with(|g| {
        let next = g.get().wrapping_add(1);
        g.set(next);
        next
    });
    SYNTH_ACTIVE.with(|a| a.set(true));
    SYNTH_SIGNAL.with(|s| s.set(Some(signal)));

    state.mic.sample_rate().set(sample_rate);
    with_live_samples_mut(state.is_tauri, |b| b.clear());
    state.mic.listening().set(true);

    let file_idx = start_live_listening(&state, sample_rate);
    spawn_live_processing_loop(state, file_idx, sample_rate);
    spawn_smooth_scroll_animation(state);
    spawn_feeder(state, signal, sample_rate, gen);

    state.log_debug(
        "info",
        format!("Synth test: {} @ {} Hz", signal.label(), sample_rate),
    );
}

/// Stop the synthetic session and clean up the transient listen file.
pub fn stop(state: &AppState) {
    if !SYNTH_ACTIVE.with(|a| a.get()) {
        return;
    }
    SYNTH_ACTIVE.with(|a| a.set(false));
    SYNTH_SIGNAL.with(|s| s.set(None));
    SYNTH_GEN.with(|g| g.set(g.get().wrapping_add(1)));

    state.mic.listening().set(false);
    crate::canvas::live_waterfall::clear();
    with_live_samples_mut(state.is_tauri, |b| b.clear());
    cleanup_listen_file(state);
    state.log_debug("info", "Synth test: stopped");
}

pub fn signal_from_str(s: &str) -> SynthSignal {
    match s {
        "noise" => SynthSignal::Noise,
        "tone" => SynthSignal::Tone,
        "multi" | "multitone" => SynthSignal::MultiTone,
        "pulses" | "pulse" => SynthSignal::Pulses,
        _ => SynthSignal::Chirp,
    }
}

/// Install `window.__synthStart(signal, rateHz?)` / `window.__synthStop()` so
/// the e2e harness can drive the synth (incl. the restart race) deterministically
/// without navigating the Debug-panel UI. The synth is a dev/test feature; these
/// hooks just call the same `start`/`stop` the buttons do.
pub fn install_test_hooks(state: AppState) {
    use wasm_bindgen::prelude::*;
    let Some(window) = web_sys::window() else {
        return;
    };

    let start_cb = Closure::wrap(Box::new(move |sig: JsValue, rate: JsValue| {
        let s = sig.as_string().unwrap_or_else(|| "chirp".to_string());
        let r = rate
            .as_f64()
            .map(|v| v as u32)
            .filter(|&v| v >= 8_000)
            .unwrap_or(256_000);
        start(state, signal_from_str(&s), r);
    }) as Box<dyn Fn(JsValue, JsValue)>);
    let _ = js_sys::Reflect::set(
        &window,
        &JsValue::from_str("__synthStart"),
        start_cb.as_ref().unchecked_ref(),
    );
    start_cb.forget();

    let stop_cb = Closure::wrap(Box::new(move || {
        stop(&state);
    }) as Box<dyn Fn()>);
    let _ = js_sys::Reflect::set(
        &window,
        &JsValue::from_str("__synthStop"),
        stop_cb.as_ref().unchecked_ref(),
    );
    stop_cb.forget();
}

/// Per-signal generator state carried across feeder ticks for phase continuity.
struct GenState {
    phase: f64,
    phases: [f64; 3],
    chirp_t: f64,
    pulse_t: f64,
    rng: u32,
}

impl GenState {
    fn new() -> Self {
        GenState {
            phase: 0.0,
            phases: [0.0; 3],
            chirp_t: 0.0,
            pulse_t: 0.0,
            rng: 0x9E3779B9,
        }
    }

    #[inline]
    fn noise(&mut self) -> f64 {
        // xorshift32 → [-1, 1)
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        (x as f64 / u32::MAX as f64) * 2.0 - 1.0
    }
}

/// Append `n` generated samples for `signal` to `out`, advancing `gs`.
fn generate(
    signal: SynthSignal,
    sr: f64,
    nyq: f64,
    n: usize,
    gs: &mut GenState,
    out: &mut Vec<f32>,
) {
    match signal {
        SynthSignal::Noise => {
            for _ in 0..n {
                out.push((gs.noise() * 0.25) as f32);
            }
        }
        SynthSignal::Tone => {
            let f = 40_000.0_f64.min(nyq * 0.9);
            let dphi = TAU * f / sr;
            for _ in 0..n {
                gs.phase += dphi;
                if gs.phase > TAU {
                    gs.phase -= TAU;
                }
                out.push((gs.phase.sin() * 0.5) as f32);
            }
        }
        SynthSignal::MultiTone => {
            let freqs = [
                20_000.0_f64.min(nyq * 0.9),
                45_000.0_f64.min(nyq * 0.9),
                80_000.0_f64.min(nyq * 0.9),
            ];
            let dphi = [
                TAU * freqs[0] / sr,
                TAU * freqs[1] / sr,
                TAU * freqs[2] / sr,
            ];
            for _ in 0..n {
                let mut s = 0.0;
                for k in 0..3 {
                    gs.phases[k] += dphi[k];
                    if gs.phases[k] > TAU {
                        gs.phases[k] -= TAU;
                    }
                    s += gs.phases[k].sin();
                }
                out.push((s * 0.3 / 3.0) as f32);
            }
        }
        SynthSignal::Chirp => {
            let period = 2.0;
            let lo = 20_000.0_f64.min(nyq * 0.9);
            let hi = nyq * 0.95;
            for _ in 0..n {
                gs.chirp_t += 1.0 / sr;
                if gs.chirp_t >= period {
                    gs.chirp_t -= period;
                }
                let f = lo + (hi - lo) * (gs.chirp_t / period);
                gs.phase += TAU * f / sr; // continuous instantaneous phase
                if gs.phase > TAU {
                    gs.phase -= TAU;
                }
                out.push((gs.phase.sin() * 0.5) as f32);
            }
        }
        SynthSignal::Pulses => {
            let gap = 0.08; // pulse repetition period (s)
            let dur = 0.003; // pulse length (s)
            let f_start = (90_000.0_f64).min(nyq * 0.95);
            let f_end = (35_000.0_f64).min(nyq * 0.6);
            for _ in 0..n {
                gs.pulse_t += 1.0 / sr;
                if gs.pulse_t >= gap {
                    gs.pulse_t -= gap;
                    gs.phase = 0.0; // restart each pulse cleanly
                }
                let s = if gs.pulse_t < dur {
                    let frac = gs.pulse_t / dur; // 0..1 across the pulse
                    let f = f_start + (f_end - f_start) * frac; // downward sweep
                    gs.phase += TAU * f / sr;
                    if gs.phase > TAU {
                        gs.phase -= TAU;
                    }
                    // Raised-cosine envelope to avoid harsh edges.
                    let env = (std::f64::consts::PI * frac).sin();
                    gs.phase.sin() * env * 0.7
                } else {
                    gs.noise() * 0.02 // quiet noise floor between pulses
                };
                out.push(s as f32);
            }
        }
    }
}

/// Spawn the feeder loop: every ~30 ms append however many samples elapsed
/// (in wall-clock terms) at the true sample rate, so load matches a real mic.
fn spawn_feeder(state: AppState, signal: SynthSignal, sample_rate: u32, gen: u64) {
    let sr = sample_rate as f64;
    let nyq = sr / 2.0;

    wasm_bindgen_futures::spawn_local(async move {
        let mut gs = GenState::new();
        let mut last_ms = js_sys::Date::now();
        let mut buf: Vec<f32> = Vec::new();
        // Throughput accounting (rough perf readout).
        let mut report_ms = last_ms;
        let mut fed_since_report: usize = 0;

        loop {
            crate::web_util::sleep_ms(30).await;

            // Superseded or stopped?
            if SYNTH_GEN.with(|g| g.get()) != gen || !state.mic.listening().get_untracked() {
                break;
            }

            let now = js_sys::Date::now();
            let elapsed = (now - last_ms).max(0.0);
            last_ms = now;
            // Convert elapsed wall-clock to a sample count, capped so a long
            // stall (tab backgrounded) doesn't dump a multi-second burst.
            let mut n = ((elapsed / 1000.0) * sr) as usize;
            n = n.min((sr * 0.2) as usize);
            if n == 0 {
                continue;
            }

            buf.clear();
            buf.reserve(n);
            generate(signal, sr, nyq, n, &mut gs, &mut buf);
            with_live_samples_mut(state.is_tauri, |b| b.extend_from_slice(&buf));
            fed_since_report += n;

            if now - report_ms >= 2000.0 {
                let secs = (now - report_ms) / 1000.0;
                let ksps = (fed_since_report as f64 / secs) / 1000.0;
                state.log_debug(
                    "info",
                    format!(
                        "Synth feed: {:.0} kS/s ({} total cols)",
                        ksps,
                        crate::canvas::live_waterfall::total_columns()
                    ),
                );
                report_ms = now;
                fed_since_report = 0;
            }
        }
    });
}
