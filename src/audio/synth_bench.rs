//! Automated live-render benchmark. Drives the synthetic mic (see
//! `synthetic_mic`) through a matrix of {sample rate × view × signal}, measures
//! the browser frame cadence (rAF intervals) + column throughput for each combo
//! with no user intervention, and emits a copy/downloadable report tagged with
//! the app version — so runs can be compared across code versions and devices.
//!
//! What actually varies the workload during *live* capture: the sample rate
//! (FFTs/s), and the view's column generation (Resonators runs a stateful EMA;
//! Spectrogram/Flow share the plain STFT path). Signal content does not change
//! cost (FFT cost is content-independent) but is swept for visual coverage and
//! to catch combos that fail to render.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use leptos::prelude::*;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use crate::audio::synthetic_mic::{self, SynthSignal};
use crate::state::store_fields::*;
use crate::state::{AppState, MainView, ResonatorDensity, ResonatorFftMode};
use crate::web_util::sleep_ms;

// ── Benchmark matrix (tunable) ───────────────────────────────────────────────
const RATES: [u32; 2] = [192_000, 384_000];
const SIGNALS: [SynthSignal; 3] = [SynthSignal::Noise, SynthSignal::Chirp, SynthSignal::Pulses];
/// Settle time before measuring (lets the waterfall start scrolling).
const SETTLE_MS: f64 = 600.0;
/// Frame-cadence measurement window per combo.
const MEASURE_MS: f64 = 1_800.0;

thread_local! {
    static BENCH_GEN: Cell<u64> = const { Cell::new(0) };
    static BENCH_ACTIVE: Cell<bool> = const { Cell::new(false) };
}

/// Whether a benchmark run is in progress.
pub fn is_running() -> bool {
    BENCH_ACTIVE.with(|a| a.get())
}

/// Cancel a running benchmark (e.g. the user pressed Stop).
pub fn cancel() {
    BENCH_GEN.with(|g| g.set(g.get().wrapping_add(1)));
    BENCH_ACTIVE.with(|a| a.set(false));
}

#[derive(Clone, Copy)]
struct Combo {
    rate: u32,
    view_label: &'static str,
    view: MainView,
    signal: SynthSignal,
    /// Multiplier on the default live (auto-fit) zoom. 1.0 = whole window fits
    /// and follow-scroll is gentle; >1 zooms in so the waterfall scrolls fast,
    /// exercising the per-frame scroll/redraw path.
    zoom_mult: f64,
    /// Resonator density to force for this combo (Resonators view only). `None`
    /// leaves the user's current setting untouched.
    density: Option<ResonatorDensity>,
}

fn density_label(d: Option<ResonatorDensity>) -> &'static str {
    match d {
        None => "—",
        Some(d) => d.percent(),
    }
}

fn build_combos() -> Vec<Combo> {
    let mut combos = Vec::new();
    // Spectrogram + Flow: rate × signal at fit zoom (density N/A).
    for &rate in RATES.iter() {
        for &(view_label, view) in &[
            ("Spectrogram", MainView::Spectrogram),
            ("Flow", MainView::Flow),
        ] {
            for &signal in SIGNALS.iter() {
                combos.push(Combo {
                    rate,
                    view_label,
                    view,
                    signal,
                    zoom_mult: 1.0,
                    density: None,
                });
            }
        }
    }
    // Resonators: sweep the density axis (100/50/25%) so the bank-cost lever is
    // visible directly. Signal content doesn't change resonator cost, so fix it.
    for &rate in RATES.iter() {
        for &density in &[
            ResonatorDensity::Full,
            ResonatorDensity::Half,
            ResonatorDensity::Quarter,
        ] {
            combos.push(Combo {
                rate,
                view_label: "Resonators",
                view: MainView::Resonators,
                signal: SynthSignal::Pulses,
                zoom_mult: 1.0,
                density: Some(density),
            });
        }
    }
    // Waveform main view: unlike the tile-cached spectrogram, it re-reads + does
    // a per-pixel min/max scan of the visible window every scroll frame, so cost
    // scales with samples-per-pixel. Fit zoom (max spp = whole window) is the
    // scan-stress case; ×8 is fast scroll at low spp. Signal content is irrelevant
    // to waveform cost, so fix it.
    for &rate in RATES.iter() {
        combos.push(Combo {
            rate,
            view_label: "Waveform",
            view: MainView::Waveform,
            signal: SynthSignal::Pulses,
            zoom_mult: 1.0,
            density: None,
        });
    }
    combos.push(Combo {
        rate: 384_000,
        view_label: "Waveform",
        view: MainView::Waveform,
        signal: SynthSignal::Pulses,
        zoom_mult: 8.0,
        density: None,
    });
    // Scroll stress (zoom ×8): heaviest cases. Resonators at Quarter (the usable
    // density) so the scrolled-resonator case reflects real use.
    combos.push(Combo {
        rate: 384_000,
        view_label: "Spectrogram",
        view: MainView::Spectrogram,
        signal: SynthSignal::Pulses,
        zoom_mult: 8.0,
        density: None,
    });
    combos.push(Combo {
        rate: 384_000,
        view_label: "Resonators",
        view: MainView::Resonators,
        signal: SynthSignal::Pulses,
        zoom_mult: 8.0,
        density: Some(ResonatorDensity::Quarter),
    });
    combos
}

struct ComboResult {
    rate: u32,
    view: &'static str,
    signal: &'static str,
    density: &'static str,
    zoom_mult: f64,
    avg_fps: f64,
    min_fps: f64,
    p95_ms: f64,
    frames: usize,
    cols_per_s: f64,
    /// Mean `render_viewport` time per call (ms) and the `put_image_data`
    /// (upload) portion of it — to locate the live-render bottleneck.
    render_ms: f64,
    upload_ms: f64,
}

/// Fetch native OS/arch from the Tauri backend for the report (desktop UA is
/// uninformative; Android model + OS version already ride in the UA).
async fn fetch_device_info(state: &AppState) -> Option<String> {
    if !state.is_tauri {
        return None;
    }
    #[derive(serde::Deserialize)]
    struct DeviceInfo {
        os: String,
        arch: String,
        family: String,
    }
    match crate::tauri_bridge::tauri_invoke_typed_no_args::<DeviceInfo>("device_info").await {
        Ok(d) => Some(format!("{} {} ({})", d.os, d.arch, d.family)),
        Err(_) => None,
    }
}

/// Run the full benchmark matrix. Restores the original view + stops the synth
/// when done, then emits the report. No-op if a run is already in progress.
pub fn run(state: AppState) {
    if BENCH_ACTIVE.with(|a| a.get()) {
        return;
    }
    let gen = BENCH_GEN.with(|g| {
        let next = g.get().wrapping_add(1);
        g.set(next);
        next
    });
    BENCH_ACTIVE.with(|a| a.set(true));

    let orig_view = state.viewmode.main_view().get_untracked();
    let orig_zoom = state.view.zoom_level().get_untracked();
    let orig_fft_mode = state.resonator.fft_mode().get_untracked();
    let combos = build_combos();
    let total = combos.len();
    state.show_info_toast(format!(
        "Benchmark started: {} combos (~{}s)",
        total,
        ((total as f64) * (SETTLE_MS + MEASURE_MS) / 1000.0).round() as i64,
    ));

    wasm_bindgen_futures::spawn_local(async move {
        let device = fetch_device_info(&state).await;
        let mut results: Vec<ComboResult> = Vec::new();
        'outer: for (i, combo) in combos.iter().enumerate() {
            if BENCH_GEN.with(|g| g.get()) != gen {
                break 'outer; // cancelled
            }
            let mut note = String::new();
            if let Some(d) = combo.density {
                note.push_str(&format!(" [{}]", d.percent()));
            }
            if combo.zoom_mult != 1.0 {
                note.push_str(&format!(" (zoom ×{:.0})", combo.zoom_mult));
            }
            state.log_debug(
                "info",
                format!(
                    "Bench {}/{}: {} / {} @ {} kHz{}",
                    i + 1,
                    total,
                    combo.view_label,
                    combo.signal.label(),
                    combo.rate / 1000,
                    note
                ),
            );

            // Force the resonator density for this combo (Resonators only).
            if let Some(d) = combo.density {
                state
                    .resonator
                    .fft_mode()
                    .set(ResonatorFftMode::Adaptive(d));
            }
            // Set the view, then (re)start the synth for this combo.
            state.viewmode.main_view().set(combo.view);
            synthetic_mic::start(state, combo.signal, combo.rate);
            // `start()` force-resets non-spectrogram views (e.g. Waveform) to
            // Spectrogram so its waterfall renders — re-assert the combo's view
            // afterward so Waveform combos actually measure the Waveform path.
            if state.viewmode.main_view().get_untracked() != combo.view {
                state.viewmode.main_view().set(combo.view);
            }
            // Zoom in for scroll-stress combos (start resets to auto-fit zoom).
            if combo.zoom_mult != 1.0 {
                let z = state.view.zoom_level().get_untracked() * combo.zoom_mult;
                state.view.zoom_level().set(z);
            }

            sleep_ms(SETTLE_MS as i32).await;
            if BENCH_GEN.with(|g| g.get()) != gen {
                break 'outer;
            }

            let cols0 = crate::canvas::live_waterfall::total_columns();
            crate::canvas::live_waterfall::take_render_timing(); // reset (discard settle)
            crate::canvas::waveform_renderer::take_wf_diag(); // reset waveform diag
            crate::components::spectrogram::take_spec_diag(); // reset spectrogram diag
            let frame_ts = measure_frames(MEASURE_MS, gen).await;
            let cols1 = crate::canvas::live_waterfall::total_columns();
            let (rcalls, rtotal_ms, rupload_ms) =
                crate::canvas::live_waterfall::take_render_timing();
            // Spectrogram-family combos: split per-frame cost into the whole render
            // Effect body vs the waterfall render_viewport (rtotal_ms) — so we can
            // see if the ×8 scroll cost is the Effect (overlays/markers/signal
            // reads) or external (scroll cascade / compositing).
            if combo.view != MainView::Waveform {
                let (seffn, seff_ms) = crate::components::spectrogram::take_spec_diag();
                if seffn > 0 {
                    let render_avg = if rcalls > 0 {
                        rtotal_ms / rcalls as f64
                    } else {
                        0.0
                    };
                    state.log_debug("spec-diag", format!(
                        "{} @ {}k z×{:.0}: {} effect runs avg {:.2}ms (of which render_viewport ~{:.2}ms, {} renders)",
                        combo.view_label, combo.rate / 1000, combo.zoom_mult,
                        seffn, seff_ms / seffn as f64, render_avg, rcalls,
                    ));
                }
            }
            // Waveform combos: surface where the per-frame cost actually is —
            // is the mip path taken, how long is the draw, how often does it
            // rebuild? (render_ms above is the WATERFALL, which is idle here.)
            if combo.view == MainView::Waveform {
                let (wc, wms, wread, weff, weffn, wmip, wrb, wspp) =
                    crate::canvas::waveform_renderer::take_wf_diag();
                if weffn > 0 {
                    let n = weffn.max(1) as f64;
                    state.log_debug("wf-diag", format!(
                        "{} @ {}k z×{:.0}: {} effect runs avg {:.2}ms (draw {:.2}, read {:.2}); {} draws, mip {}/{}, {} rebuilds; spp~{:.0}",
                        combo.view_label, combo.rate / 1000, combo.zoom_mult,
                        weffn, weff / n, wms / n, wread / n, wc, wmip, wc, wrb, wspp,
                    ));
                }
            }
            // Stop now if cancelled during the measurement window — don't
            // record a partial combo or start the next one.
            if BENCH_GEN.with(|g| g.get()) != gen {
                break 'outer;
            }

            let (avg_fps, min_fps, p95_ms, frames) = frame_stats(&frame_ts);
            let cols_per_s = (cols1.saturating_sub(cols0)) as f64 / (MEASURE_MS / 1000.0);
            let (render_ms, upload_ms) = if rcalls > 0 {
                (rtotal_ms / rcalls as f64, rupload_ms / rcalls as f64)
            } else {
                (0.0, 0.0)
            };

            results.push(ComboResult {
                rate: combo.rate,
                view: combo.view_label,
                signal: combo.signal.label(),
                density: density_label(combo.density),
                zoom_mult: combo.zoom_mult,
                avg_fps,
                min_fps,
                p95_ms,
                frames,
                cols_per_s,
                render_ms,
                upload_ms,
            });
        }

        let cancelled = BENCH_GEN.with(|g| g.get()) != gen;
        synthetic_mic::stop(&state);
        state.viewmode.main_view().set(orig_view);
        state.view.zoom_level().set(orig_zoom);
        state.resonator.fft_mode().set(orig_fft_mode);
        BENCH_ACTIVE.with(|a| a.set(false));

        if results.is_empty() {
            state.show_info_toast("Benchmark cancelled");
            return;
        }
        // Isolated resonator-SIMD micro-bench (skipped on cancel — don't keep
        // working after the user pressed Stop).
        let reso = if cancelled {
            (cfg!(target_feature = "simd128"), Vec::new())
        } else {
            run_resonator_microbench().await
        };
        let report = build_report(&results, cancelled, device.as_deref(), &reso);
        emit_report(&state, report, cancelled);
    });
}

/// Collect requestAnimationFrame timestamps for `window_ms`, returning the raw
/// per-frame DOMHighResTimeStamps (ms). The rAF cadence reflects how much main-
/// thread time the live render is taking: a saturated frame budget → fewer,
/// further-spaced frames → lower measured fps.
///
/// Always resolves, even if rAF stalls (a backgrounded tab suspends rAF): a
/// `setTimeout` watchdog resolves the Promise after the window regardless, so
/// the benchmark can never hang. The rAF chain also stops early if the run is
/// cancelled (gen bump) so Stop is responsive and the closure cycle is freed.
async fn measure_frames(window_ms: f64, gen: u64) -> Vec<f64> {
    let times: Rc<RefCell<Vec<f64>>> = Rc::new(RefCell::new(Vec::new()));
    let times_out = times.clone();

    let promise = js_sys::Promise::new(&mut |resolve, _reject| {
        let Some(win) = web_sys::window() else {
            let _ = resolve.call0(&JsValue::NULL);
            return;
        };

        // Watchdog: resolve after the window even if rAF never fires again.
        {
            let resolve_w = resolve.clone();
            let watchdog = Closure::once_into_js(move || {
                let _ = resolve_w.call0(&JsValue::NULL);
            });
            let _ = win.set_timeout_with_callback_and_timeout_and_arguments_0(
                watchdog.as_ref().unchecked_ref(),
                (window_ms as i32) + 300,
            );
        }

        let cb: Rc<RefCell<Option<Closure<dyn FnMut(f64)>>>> = Rc::new(RefCell::new(None));
        let cb_inner = cb.clone();
        let times = times.clone();
        let start = Rc::new(Cell::new(f64::NAN));

        *cb.borrow_mut() = Some(Closure::wrap(Box::new(move |ts: f64| {
            if start.get().is_nan() {
                start.set(ts);
            }
            times.borrow_mut().push(ts);
            let done = ts - start.get() >= window_ms || BENCH_GEN.with(|g| g.get()) != gen; // cancelled → stop early
            if done {
                let _ = resolve.call0(&JsValue::NULL);
                // Break the self-reference cycle so the closure chain frees.
                let _ = cb_inner.borrow_mut().take();
            } else if let Some(w) = web_sys::window() {
                if let Some(ref c) = *cb_inner.borrow() {
                    let _ = w.request_animation_frame(c.as_ref().unchecked_ref());
                }
            }
        }) as Box<dyn FnMut(f64)>));

        // Schedule the first frame (scope the borrow so the `Ref` drops before
        // `cb` does at the end of this executor closure).
        {
            let b = cb.borrow();
            if let Some(c) = b.as_ref() {
                let _ = win.request_animation_frame(c.as_ref().unchecked_ref());
            }
        }
        // The closure captures `cb_inner` (a clone of `cb`), so the Rc cycle keeps
        // it alive across frames; it's freed by the `take()` above on completion.
    });

    let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
    let v = times_out.borrow().clone();
    v
}

/// (avg_fps, min_fps, p95_frame_ms, frame_count) from rAF timestamps.
fn frame_stats(times: &[f64]) -> (f64, f64, f64, usize) {
    let n = times.len();
    if n < 2 {
        return (0.0, 0.0, 0.0, n);
    }
    let mut deltas: Vec<f64> = times
        .windows(2)
        .map(|w| w[1] - w[0])
        .filter(|d| *d > 0.0)
        .collect();
    if deltas.is_empty() {
        return (0.0, 0.0, 0.0, n);
    }
    let mean = deltas.iter().sum::<f64>() / deltas.len() as f64;
    let avg_fps = if mean > 0.0 { 1000.0 / mean } else { 0.0 };
    deltas.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let max_delta = *deltas.last().unwrap();
    let min_fps = if max_delta > 0.0 {
        1000.0 / max_delta
    } else {
        0.0
    };
    let p95_idx = (((deltas.len() as f64) * 0.95).floor() as usize).min(deltas.len() - 1);
    let p95_ms = deltas[p95_idx];
    (avg_fps, min_fps, p95_ms, n)
}

/// One bank size's result from the isolated resonator hot-loop micro-bench.
struct ResoBenchRow {
    num_bins: usize,
    total_ms: f64,
    /// Million resonator-sample updates per second (num_bins × samples / time) —
    /// the SIMD-utilisation figure of merit; higher is better, comparable across
    /// devices and code versions.
    mupdates_per_s: f64,
}

/// Time the resonators crate hot loop (`process_samples`) in isolation — no
/// render, no STFT — across a few bank sizes, yielding to the browser between
/// iterations so the main thread isn't blocked. This is the SIMD sanity check:
/// `simd128` is reported, and the throughput tells you whether autovectorisation
/// is actually paying off on this device/build. Mirrors `run_resonator_bench`
/// (the Debug-panel button) but returns data for the report instead of logging.
async fn run_resonator_microbench() -> (bool, Vec<ResoBenchRow>) {
    let simd128 = cfg!(target_feature = "simd128");
    let Some(perf) = web_sys::window().and_then(|w| w.performance()) else {
        return (simd128, Vec::new());
    };
    let now_ms = {
        let perf = perf.clone();
        move || perf.now()
    };

    let sample_rate = 48_000u32;
    let samples_per_iter = sample_rate as usize; // ~1 s of audio per iteration
    let iterations = 8usize;
    let bandwidth_hz = 20.0f32;

    let mut rows = Vec::new();
    for &num_bins in &[129usize, 257, 513] {
        crate::canvas::tile_cache::yield_to_browser().await;
        // Sum per-iteration timings (each builds a fresh bank — negligible cost),
        // yielding between so individual main-thread blocks stay short.
        let mut total_ms = 0.0;
        for _ in 0..iterations {
            crate::canvas::tile_cache::yield_to_browser().await;
            let r = crate::dsp::resonators::bench_resonator_bank(
                num_bins,
                samples_per_iter,
                1,
                bandwidth_hz,
                sample_rate,
                now_ms.clone(),
            );
            total_ms += r.elapsed_ms;
        }
        let total_samples = (samples_per_iter * iterations) as f64;
        let mupdates_per_s = if total_ms > 0.0 {
            (num_bins as f64 * total_samples) / (total_ms / 1000.0) / 1.0e6
        } else {
            0.0
        };
        rows.push(ResoBenchRow {
            num_bins,
            total_ms,
            mupdates_per_s,
        });
    }
    (simd128, rows)
}

fn build_report(
    results: &[ComboResult],
    cancelled: bool,
    device: Option<&str>,
    reso: &(bool, Vec<ResoBenchRow>),
) -> String {
    let mut s = String::new();
    s.push_str("# Oversample live-render benchmark\n\n");
    s.push_str(&format!("- Version: {}\n", env!("CARGO_PKG_VERSION")));
    let date = js_sys::Date::new_0().to_iso_string();
    s.push_str(&format!("- Date: {}\n", String::from(date)));
    if let Some(dev) = device {
        s.push_str(&format!("- Device (Tauri): {}\n", dev));
    }
    if let Some(nav) = web_sys::window().map(|w| w.navigator()) {
        if let Ok(ua) = nav.user_agent() {
            s.push_str(&format!("- User agent: {}\n", ua));
        }
        let cores = nav.hardware_concurrency();
        s.push_str(&format!("- Logical cores: {}\n", cores as i64));
    }
    if let Some(win) = web_sys::window() {
        let dpr = win.device_pixel_ratio();
        s.push_str(&format!("- devicePixelRatio: {:.2}\n", dpr));
    }
    s.push_str(&format!(
        "- Per combo: {:.0}ms settle + {:.0}ms measure\n",
        SETTLE_MS, MEASURE_MS
    ));
    if cancelled {
        s.push_str("- NOTE: run was cancelled early (partial results)\n");
    }
    s.push('\n');

    s.push_str("| Rate | View | Signal | Dens | Zoom | avg fps | min fps | p95 ms | cols/s | frames | render ms | upload ms |\n");
    s.push_str("|------|------|--------|------|-----:|--------:|--------:|-------:|-------:|-------:|----------:|----------:|\n");
    for r in results {
        let zoom = if r.zoom_mult != 1.0 {
            format!("×{:.0}", r.zoom_mult)
        } else {
            "fit".to_string()
        };
        s.push_str(&format!(
            "| {}k | {} | {} | {} | {} | {:.1} | {:.1} | {:.1} | {:.0} | {} | {:.2} | {:.2} |\n",
            r.rate / 1000,
            r.view,
            r.signal,
            r.density,
            zoom,
            r.avg_fps,
            r.min_fps,
            r.p95_ms,
            r.cols_per_s,
            r.frames,
            r.render_ms,
            r.upload_ms,
        ));
    }

    // Summary: overall average + worst combo by avg fps.
    if !results.is_empty() {
        let mean_fps = results.iter().map(|r| r.avg_fps).sum::<f64>() / results.len() as f64;
        let worst = results.iter().min_by(|a, b| {
            a.avg_fps
                .partial_cmp(&b.avg_fps)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        s.push('\n');
        s.push_str(&format!("Overall avg fps: {:.1}\n", mean_fps));
        if let Some(w) = worst {
            s.push_str(&format!(
                "Worst combo: {} / {} @ {}k — {:.1} avg fps ({:.1} min)\n",
                w.view,
                w.signal,
                w.rate / 1000,
                w.avg_fps,
                w.min_fps,
            ));
        }
    }

    // ── Resonator hot-loop micro-bench (isolated SIMD sanity check) ──
    let (simd128, rows) = reso;
    if !rows.is_empty() {
        s.push('\n');
        s.push_str(&format!(
            "## Resonator hot loop — isolated (target-feature simd128={})\n\n",
            if *simd128 { "on" } else { "off" },
        ));
        s.push_str("Pure `resonators::process_samples`, no render/STFT — gauges SIMD\n");
        s.push_str("utilisation. Higher Mupd/s (num_bins × samples / s) is better.\n\n");
        s.push_str("| bins | total ms | Mupd/s |\n");
        s.push_str("|-----:|---------:|-------:|\n");
        for r in rows {
            s.push_str(&format!(
                "| {} | {:.2} | {:.0} |\n",
                r.num_bins, r.total_ms, r.mupdates_per_s,
            ));
        }
    }
    s
}

fn emit_report(state: &AppState, report: String, cancelled: bool) {
    // Echo the report into the debug log in ONE batched update (auto-scrolls;
    // Copy All includes it) rather than a reactive update per line.
    let ts = js_sys::Date::now();
    state.status.debug_log().update(|entries| {
        for line in report.lines() {
            entries.push((ts, "info".to_string(), line.to_string()));
        }
        if entries.len() > 500 {
            entries.drain(0..entries.len() - 500);
        }
    });
    let version = env!("CARGO_PKG_VERSION");
    download_text(&format!("oversample-bench-{}.md", version), &report);
    copy_to_clipboard(&report);
    state.show_info_toast(if cancelled {
        "Benchmark cancelled — partial report downloaded".to_string()
    } else {
        "Benchmark complete — report downloaded & copied".to_string()
    });
}

/// Download `content` as a file via a data-URL anchor (no Blob features needed).
/// The anchor is attached to the DOM before clicking and removed after, which
/// some browsers require for a programmatic download to fire.
fn download_text(filename: &str, content: &str) {
    let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    let encoded = String::from(js_sys::encode_uri_component(content));
    let href = format!("data:text/markdown;charset=utf-8,{}", encoded);
    let Ok(a) = doc.create_element("a") else {
        return;
    };
    let _ = a.set_attribute("href", &href);
    let _ = a.set_attribute("download", filename);
    let _ = a.set_attribute("style", "display:none");
    if let Some(body) = doc.body() {
        let _ = body.append_child(&a);
    }
    if let Some(html) = a.dyn_ref::<web_sys::HtmlElement>() {
        html.click();
    }
    let _ = a.remove();
}

/// Best-effort copy to the clipboard (same pattern the debug-log "Copy All" uses).
fn copy_to_clipboard(text: &str) {
    let Some(window) = web_sys::window() else {
        return;
    };
    if let Ok(nav) = js_sys::Reflect::get(&window, &JsValue::from_str("navigator")) {
        if let Ok(clip) = js_sys::Reflect::get(&nav, &JsValue::from_str("clipboard")) {
            if let Some(func) = js_sys::Reflect::get(&clip, &JsValue::from_str("writeText"))
                .ok()
                .and_then(|f| f.dyn_ref::<js_sys::Function>().cloned())
            {
                let _ = func.call1(&clip, &JsValue::from_str(text));
            }
        }
    }
}
