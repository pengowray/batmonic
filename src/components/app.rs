use crate::audio::microphone;
use crate::audio::playback;
use crate::components::analysis_panel::AnalysisPanel;
use crate::components::annotation_label_editor::AnnotationLabelEditor;
use crate::components::bat_book_ref_panel::BatBookRefPanel;
use crate::components::bat_book_strip::BatBookStrip;
use crate::components::bottom_toolbar::BottomToolbar;
use crate::components::chromagram_view::ChromagramView;
use crate::components::file_sidebar::FileSidebar;
use crate::components::file_sidebar::{fetch_demo_index, load_single_demo};
use crate::components::hearing_bar::HearingBar;
use crate::components::icons::Icon;
use crate::components::overflow_menu::CanvasOverflowMenus;
use crate::components::overview::{OverviewPanel, OverviewToolbar};
use crate::components::play_controls::{BookmarkPopup, ToastDisplay};
use crate::components::right_sidebar::RightSidebar;
use crate::components::spectrogram::Spectrogram;
use crate::components::toolbar::Toolbar;
use crate::components::view_bar::ViewBar;
use crate::components::waveform::Waveform;
use crate::components::wiki_browser::WikiBrowser;
use crate::components::xc_browser::XcBrowser;
use crate::components::zc_chart::ZcDotChart;
use crate::state::store_fields::*;
use crate::state::{
    resonator_bw_to_slider, resonator_slider_to_bw, AppState, ChromaColormap, ChromaRange,
    ChromaSource, DisplayFilterMode, FftMode, FileSettings, FlowColorScheme, GainMode, LayerPanel,
    MainView, MicStrategy, PlayStartMode, PlaybackMode, ResonatorAlphaMode, ResonatorDensity,
    ResonatorFftMode, ResonatorHybridMode, ResonatorLayout, SpectrogramDisplay, WaveformView,
    RESONATOR_BW_SLIDER_MAX,
};
use crate::viewport;
use crate::web_util::sleep_ms;
use leptos::prelude::*;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

/// Recognise the Wikimedia forms of the URL hash: `#commons:File:Foo.wav`,
/// `#wiki:<any wikimedia url>`, or a bare `#File:Foo.wav`. Returns the
/// percent-decoded target for `wikimedia::load_from_input`.
fn wikimedia_hash_target(hash: &str) -> Option<String> {
    const PREFIXES: &[&str] = &["commons:", "wiki:", "wikipedia:"];
    // `get` rather than slicing: the hash is arbitrary user text and a byte
    // index landing mid-character would panic.
    let rest = PREFIXES
        .iter()
        .find(|p| {
            hash.get(..p.len())
                .is_some_and(|h| h.eq_ignore_ascii_case(p))
        })
        .map(|p| &hash[p.len()..])
        .filter(|rest| !rest.is_empty())
        .or_else(|| {
            hash.get(..5)
                .is_some_and(|h| h.eq_ignore_ascii_case("File:"))
                .then_some(hash)
                .filter(|h| h.len() > 5)
        })?;
    // A URL carries its own percent-encoding that `parse_input` handles; only
    // bare titles need decoding here.
    Some(
        if rest.starts_with("http://") || rest.starts_with("https://") {
            rest.to_string()
        } else {
            crate::wikimedia::decode_component(rest)
        },
    )
}

#[component]
pub fn App() -> impl IntoView {
    let state = AppState::new();
    provide_context(state);

    // Expose a read-only state snapshot on window for e2e (Playwright) specs.
    crate::test_hook::install(state);
    // Expose synth-test start/stop on window for e2e (dev/test feature).
    crate::audio::synthetic_mic::install_test_hooks(state);

    // Detect browser's default audio output sample rate
    {
        use web_sys::AudioContext;
        if let Ok(ctx) = AudioContext::new() {
            let rate = ctx.sample_rate() as u32;
            if rate > 0 {
                state.display.browser_sample_rate().set(rate);
            }
            let _ = ctx.close();
        }
    }

    // Auto-load demo sound from URL hash (e.g. #XC928094)
    if let Some(window) = web_sys::window() {
        if let Ok(hash) = window.location().hash() {
            let trimmed = hash.trim_start_matches('#');
            // Synthetic live-waterfall test signal, e.g. `#synthtest`,
            // `#synthtest-chirp`, `#synthtest-pulses-384` (signal[-rateKHz]).
            // No mic needed — drives the real listen pipeline for visual/perf
            // testing (see audio::synthetic_mic).
            if trimmed.len() >= 9 && trimmed[..9].eq_ignore_ascii_case("synthtest") {
                use crate::audio::synthetic_mic::{self, signal_from_str};
                let rest = trimmed[9..].trim_start_matches('-').to_ascii_lowercase();
                let mut parts = rest.split('-').filter(|s| !s.is_empty());
                let signal = signal_from_str(parts.next().unwrap_or("chirp"));
                let rate = parts
                    .next()
                    .and_then(|s| s.parse::<u32>().ok())
                    .map(|khz| khz.clamp(8, 1000) * 1000)
                    .unwrap_or(256_000);
                wasm_bindgen_futures::spawn_local(async move {
                    // Let layout settle so the canvas width (→ zoom) is known.
                    sleep_ms(400).await;
                    synthetic_mic::start(state, signal, rate);
                });
            } else if trimmed.len() >= 3
                && trimmed[..2].eq_ignore_ascii_case("XC")
                && trimmed[2..].chars().all(|c| c.is_ascii_digit())
            {
                let xc_id = trimmed.to_uppercase();
                let load_id = state.loading_start(&xc_id);
                wasm_bindgen_futures::spawn_local(async move {
                    match fetch_demo_index().await {
                        Ok(entries) => {
                            let found = entries
                                .iter()
                                .find(|e| e.filename.to_uppercase().contains(&xc_id));
                            if let Some(entry) = found {
                                if let Err(e) = load_single_demo(entry, state, load_id).await {
                                    log::error!("Failed to load {}: {}", xc_id, e);
                                    state.show_error_toast(format!("Failed to load {}", xc_id));
                                }
                            } else {
                                // The live XC API is unavailable on the web build
                                // (no CORS / proxy — see xc_browser.rs), so point
                                // the user at the original recording instead.
                                let numeric = xc_id.trim_start_matches("XC");
                                state.show_info_toast(format!(
                                    "{} isn't in the included demo audio — view it at xeno-canto.org/{}",
                                    xc_id, numeric
                                ));
                            }
                        }
                        Err(e) => {
                            log::error!("Failed to fetch demo index: {e}");
                            state.show_error_toast("Could not load demo sounds index");
                        }
                    }
                    state.loading_done(load_id);
                });
            } else if let Some(target) = wikimedia_hash_target(trimmed) {
                // Wikimedia deep links: `#commons:File:Foo.wav`, `#wiki:<url>`,
                // or a bare `#File:Foo.wav`.
                wasm_bindgen_futures::spawn_local(async move {
                    crate::wikimedia::load_from_input(state, &target).await;
                });
            }
        }
    }

    // Startup: scan for orphaned crash-recovery recordings and finalize them.
    // Cheap no-op if the recovery dir is empty or missing.
    if state.is_tauri {
        wasm_bindgen_futures::spawn_local(async move {
            use crate::tauri_bridge::tauri_invoke_typed_no_args;
            let Ok(recordings) = tauri_invoke_typed_no_args::<
                Vec<oversample_ipc::mic::RecoveredRecording>,
            >("mic_recover_recordings")
            .await
            else {
                return;
            };
            let count = recordings.len();
            if count > 0 {
                log::info!(
                    "Recovered {} crashed recording(s) from previous session",
                    count
                );
                let first_path = recordings
                    .first()
                    .map(|r| r.path.clone())
                    .unwrap_or_default();
                state.show_info_toast(format!(
                    "Recovered {} recording{} from previous session ({})",
                    count,
                    if count == 1 { "" } else { "s" },
                    first_path,
                ));
            }

            // Android-only: also sweep stale IS_PENDING=1 MediaStore rows left
            // over from a crashed recording. The plugin call is a no-op on
            // non-Android and on pre-Q; a failed invoke (e.g. plugin missing
            // on desktop) is dropped silently. Quick call regardless of
            // whether any .wav.part files were found — the MediaStore row can
            // outlive the internal partial file.
            if let Ok(result) =
                tauri_invoke_typed_no_args::<oversample_ipc::plugins::CleanupResult>(
                    "plugin:media-store|cleanupPendingEntries",
                )
                .await
            {
                if result.deleted > 0 {
                    log::info!(
                        "Cleaned up {} orphaned MediaStore pending entries",
                        result.deleted
                    );
                }
            }
        });
    }

    // Startup: check for USB device (delayed to ensure Tauri internals are ready)
    if state.is_tauri && state.mic.strategy().get_untracked() == MicStrategy::Ask {
        wasm_bindgen_futures::spawn_local(async move {
            // Wait 500ms for Tauri plugin system to initialize
            sleep_ms(500).await;
            // Check USB status without requesting permission
            microphone::check_usb_status(&state).await;
            microphone::query_mic_info(&state).await;

            // If no USB found on first try, retry after 2s (device may enumerate slowly)
            if !state.mic.usb_connected().get_untracked() {
                sleep_ms(2000).await;
                microphone::check_usb_status(&state).await;
                microphone::query_mic_info(&state).await;
            }
        });
    }

    // USB hotplug (Tauri/Android): the plugin PUSHES attach/detach into the
    // `window.__oversampleUsbHotplug` global registered below — no polling (see
    // lows #35; the old 3s checkUsbStatus poll was unregistered/permission-
    // rejected and silently did nothing, and coalesced events besides).
    if state.is_tauri {
        // Seed usb_connected once at startup (the plugin only pushes on *changes*).
        wasm_bindgen_futures::spawn_local(async move {
            microphone::query_mic_info(&state).await;
        });

        // Clear the mic-button "new device" badge once the user opens the chooser.
        Effect::new(move |_| {
            if state.mic.show_chooser().get() {
                state.mic.new_device_available().set(false);
            }
        });

        // Global the Android USB plugin calls (via webView.evaluateJavascript) on
        // hot-plug, with a `{event, product, deviceName}` object.
        let state_usb = state;
        let usb_cb = wasm_bindgen::closure::Closure::<dyn FnMut(wasm_bindgen::JsValue)>::new(
            move |detail: wasm_bindgen::JsValue| {
                let field = |k: &str| {
                    js_sys::Reflect::get(&detail, &wasm_bindgen::JsValue::from_str(k))
                        .ok()
                        .and_then(|v| v.as_string())
                        .unwrap_or_default()
                };
                let event = field("event");
                let product = field("product");
                let device_name = field("deviceName");
                wasm_bindgen_futures::spawn_local(async move {
                    microphone::handle_usb_hotplug(&state_usb, &event, &product, &device_name)
                        .await;
                });
            },
        );
        if let Some(win) = web_sys::window() {
            let _ = js_sys::Reflect::set(
                &win,
                &wasm_bindgen::JsValue::from_str("__oversampleUsbHotplug"),
                usb_cb.as_ref().unchecked_ref(),
            );
        }
        usb_cb.forget();
    }

    // Live playback parameter switching: when any playback-relevant signal
    // changes while audio is playing, restart the stream from the current
    // playhead position with fresh parameters.
    {
        let first_run = std::cell::Cell::new(true);
        Effect::new(move |_| {
            // Subscribe to every playback-relevant signal (one place: state.rs).
            state.track_replay_params();

            if first_run.get() {
                first_run.set(false);
                return;
            }

            if state.playback.is_playing().get_untracked() {
                playback::schedule_replay_live(&state);
            }
        });
    }

    // Live-mode safety: when a playback-mode or filter parameter changes
    // while listening or recording, clear the live DSP state so PS/PV overlap
    // buffers, HET filter delay lines, and the IIR bandpass warmup tail don't
    // carry artefacts from the previous mode into the new one. Also warns the
    // user when TimeExpansion is selected during live audio — TE falls through
    // to passthrough for the mic since it relies on AudioContext sample-rate
    // tricks that can't work for an unbounded live stream.
    {
        let first_run = std::cell::Cell::new(true);
        let prev_mode = std::cell::Cell::new(state.playback.mode().get_untracked());
        Effect::new(move |_| {
            // Subscribe to the params that require clearing live DSP buffers.
            let mode = state.track_live_reset_params();

            if first_run.get() {
                first_run.set(false);
                prev_mode.set(mode);
                return;
            }

            let live =
                state.mic.listening().get_untracked() || state.mic.recording().get_untracked();
            if live {
                crate::audio::microphone::clear_live_dsp_state(&state);

                if mode == crate::state::PlaybackMode::TimeExpansion && prev_mode.get() != mode {
                    state.show_info_toast(
                        "Time-expansion isn't applicable to live audio — playing back at 1:1.",
                    );
                }
            }

            prev_mode.set(mode);
        });
    }

    // Auto-save project when dirty (debounced 2s)
    {
        use std::cell::RefCell;
        thread_local! {
            static AUTOSAVE_TIMER: RefCell<Option<i32>> = const { RefCell::new(None) };
        }
        fn cancel_autosave_timer() {
            AUTOSAVE_TIMER.with(|t| {
                if let Some(handle) = t.borrow_mut().take() {
                    web_sys::window().unwrap().clear_timeout_with_handle(handle);
                }
            });
        }
        Effect::new(move |_| {
            let dirty = state.project.dirty().get();
            let has_project = state.project.current().with(|p| p.is_some());
            if !dirty || !has_project {
                cancel_autosave_timer();
                return;
            }
            cancel_autosave_timer();
            let cb = wasm_bindgen::closure::Closure::once(move || {
                if state.project.dirty().get_untracked()
                    && state.project.current().with_untracked(|p| p.is_some())
                {
                    crate::components::file_sidebar::save_project_async(state);
                }
            });
            let handle = web_sys::window()
                .unwrap()
                .set_timeout_with_callback_and_timeout_and_arguments_0(
                    cb.as_ref().unchecked_ref(),
                    2000,
                )
                .unwrap_or(0);
            cb.forget();
            AUTOSAVE_TIMER.with(|t| {
                *t.borrow_mut() = Some(handle);
            });
        });
    }

    // Sync flow_enabled with main_view (Flow view → enabled, anything else → disabled)
    Effect::new(move |_| {
        let is_flow = state.viewmode.main_view().get() == MainView::Flow;
        state.flow.enabled().set(is_flow);
    });

    // When the user navigates onto a .zc file while sitting on a view that
    // would run DSP on the synthesised reconstruction (Resonators, Flow,
    // Chromagram, transformed spec), bounce back to the dot plot. The
    // initial-load path in loading.rs only catches first-load; this covers
    // file-list switching, project navigation, etc.
    Effect::new(move |_| {
        if state.current_is_zc() && !state.viewmode.main_view().get().is_sensible_for_zc() {
            state.viewmode.main_view().set(MainView::ZcChart);
        }
    });

    // Toast when a USB mic gets plugged in (transition false → true). Pair this
    // with re-showing the file-panel "Mic detected" chip — the user might have
    // dismissed a previous one, but a fresh connect is worth surfacing again.
    {
        let prev_usb: StoredValue<bool> = StoredValue::new(false);
        Effect::new(move |_| {
            let now = state.mic.usb_connected().get();
            let was = prev_usb.get_value();
            prev_usb.set_value(now);
            if now && !was {
                state.mic.chip_dismissed().set(false);
                let name = state
                    .mic
                    .device_name()
                    .get_untracked()
                    .unwrap_or_else(|| "USB mic".to_string());
                state.show_info_toast(format!("Mic detected: {name}"));
            }
        });
    }

    // Guarantee the Android foreground-audio notification can't outlive capture:
    // stop it whenever neither listening nor recording is active, no matter which
    // path cleared the state (normal stop, a mic-stream error, a USB disconnect…).
    // Starting it stays tied to the user gesture in microphone::do_start_* (Android
    // 14+ forbids starting a mic foreground service from the background), so this
    // effect only ever STOPS. The start paths set the new flag before clearing the
    // old, so a listen<->record transition never momentarily reads "both off".
    if state.is_tauri {
        Effect::new(move |_| {
            let active = state.mic.listening().get() || state.mic.recording().get();
            if !active {
                wasm_bindgen_futures::spawn_local(async move {
                    crate::audio::microphone::stop_foreground_service(&state).await;
                });
            }
        });
    }

    // Keep scroll valid for the active file/timeline when the viewport, zoom,
    // or target duration changes. Without this, switching to a shorter file or
    // resizing while a non-spectrogram view is active can leave scroll outside
    // the valid data window and the waveform view renders a blank canvas.
    Effect::new(move |_| {
        let scroll = state.view.scroll_offset().get();
        let zoom = state.view.zoom_level().get();
        let canvas_w = state.viewmode.spectrogram_canvas_width().get();
        let from_here_mode = state.playback.start_mode().get().uses_from_here();
        let timeline = state.timeline.active().get();
        let files = state.library.files().get();
        let idx = state.library.current_index().get();

        let (time_res, duration) = if let Some(ref tl) = timeline {
            let time_res = tl
                .segments
                .first()
                .and_then(|seg| files.get(seg.file_index))
                .map(|file| file.spectrogram.time_resolution)
                .unwrap_or(1.0);
            (time_res, tl.total_duration_secs)
        } else {
            idx.and_then(|i| files.get(i))
                .map(|file| (file.spectrogram.time_resolution, file.audio.duration_secs))
                .unwrap_or((1.0, 0.0))
        };

        let visible_time = viewport::visible_time(canvas_w, zoom, time_res);
        if visible_time <= 0.0 {
            return;
        }

        // During live recording/listening, use standard scroll bounds (no negative
        // lead-in) so the waterfall grows right from the left edge instead of
        // sliding leftward from a from-here offset.
        // Also use the waterfall's total duration (which grows indefinitely) instead
        // of the file's duration (which is capped at ~10s by the circular buffer trim).
        let is_live =
            state.mic.recording().get_untracked() || state.mic.listening().get_untracked();
        let effective_duration = if is_live && crate::canvas::live_waterfall::is_active() {
            crate::canvas::live_waterfall::total_columns() as f64 * time_res
        } else {
            duration
        };
        let effective_from_here = from_here_mode && !is_live;
        let clamped = viewport::clamp_scroll_for_mode(
            scroll,
            effective_duration,
            visible_time,
            effective_from_here,
        );
        if (clamped - scroll).abs() > f64::EPSILON {
            state.view.scroll_offset().set(clamped);
        }
    });

    // Sync focus_stack → band_ff_freq_lo/hi + hfr_enabled output signals.
    // The output signals are clamped to the active Nyquist (mic SR/2 when
    // listening/recording, current file's max_freq otherwise) so the band
    // can never exceed what the source can resolve. This keeps downstream
    // Effects (B, C, D in hfr_button) working unchanged.
    //
    // We re-run when any input to the clamp changes: focus_stack itself,
    // current_file_index (file's Nyquist), mic_sample_rate (mic's Nyquist),
    // and mic_listening / mic_recording (which selects between them).
    Effect::new(move |_| {
        let _ = state.viewmode.focus_stack().get();
        let _ = state.library.current_index().get();
        let _ = state.mic.sample_rate().get();
        let _ = state.mic.listening().get();
        let _ = state.mic.recording().get();
        state.resync_focus_outputs();
    });

    // Keep annotation-driven BandFF in sync regardless of whether selection happened
    // from the sidebar or directly on the canvas.
    Effect::new(move |_| {
        let _ = state.library.current_index().get();
        let _ = state.annotations.auto_focus().get();
        let _ = state.annotations.selected_ids().get();
        let _ = state.annotations.store().get();
        state.sync_annotation_auto_focus();
    });

    // Per-file vertical zoom: on file switch, load the new file's stored
    // min/max_display_freq into the global signals. New files have None,
    // so they default to full range rather than inheriting the previous file.
    Effect::new(move |_| {
        let idx = state.library.current_index().get();
        let (min, max) = if let Some(i) = idx {
            state.library.files().with_untracked(|files| {
                files
                    .get(i)
                    .map(|f| (f.min_display_freq, f.max_display_freq))
                    .unwrap_or((None, None))
            })
        } else {
            (None, None)
        };
        if state.view.min_display_freq().get_untracked() != min {
            state.view.min_display_freq().set(min);
        }
        if state.view.max_display_freq().get_untracked() != max {
            state.view.max_display_freq().set(max);
        }
    });

    // Persist vertical zoom back to the current file whenever it changes.
    Effect::new(move |_| {
        let min = state.view.min_display_freq().get();
        let max = state.view.max_display_freq().get();
        let idx = state.library.current_index().get_untracked();
        if let Some(i) = idx {
            let needs_write = state.library.files().with_untracked(|files| {
                files
                    .get(i)
                    .map(|f| f.min_display_freq != min || f.max_display_freq != max)
                    .unwrap_or(false)
            });
            if needs_write {
                // Vertical zoom is a VIEWPORT setting: it follows the MULTITRACK
                // group (simultaneous channels of one recording → same frequency
                // content of interest), so switching tracks keeps the same window.
                // [cross-file state-scoping model]
                let mt_members = state.library.files().with_untracked(|files| {
                    let names: Vec<String> = files.iter().map(|f| f.name.clone()).collect();
                    let groups = crate::components::file_sidebar::file_groups::compute_all_groups(
                        &names, files,
                    );
                    crate::scope::share_members(crate::scope::Setting::VerticalRange, &groups, i)
                });
                state.library.files().update(|files| {
                    for &j in &mt_members {
                        if let Some(f) = files.get_mut(j) {
                            f.min_display_freq = min;
                            f.max_display_freq = max;
                        }
                    }
                });
            }
        }
    });

    // Resolve display filter modes → effective display_* booleans.
    // When the DSP panel is enabled, the per-stage modes drive the existing
    // display_auto_gain / display_eq / display_noise_filter signals.
    Effect::new(move |_| {
        let enabled = state.display.xform_enabled().get();
        if !enabled {
            // Reset all display processing signals when DSP is off
            state.display.transform().set(false);
            state.display.eq().set(false);
            state.display.noise_filter().set(false);
            state.display.auto_gain().set(false);
            state.display.gain_boost().set(0.0);
            state.display.decimate_effective().set(0);
            return;
        }

        // EQ
        let eq_on = match state.display.filter_eq().get() {
            DisplayFilterMode::Off => false,
            DisplayFilterMode::Auto => state.filter.enabled().get(), // auto = show if playback EQ is on
            DisplayFilterMode::Same => state.filter.enabled().get(),
            DisplayFilterMode::Custom => false, // not yet implemented
        };
        state.display.eq().set(eq_on);

        // Noise (notch + spectral subtraction)
        let nr_on = match state.display.filter_nr().get() {
            DisplayFilterMode::Off => false,
            DisplayFilterMode::Auto | DisplayFilterMode::Custom => true,
            DisplayFilterMode::Same => state.noise_reduce.enabled().get(),
        };
        // Also consider notch
        let notch_on = match state.display.filter_notch().get() {
            DisplayFilterMode::Off => false,
            DisplayFilterMode::Auto => state.notch.enabled().get(), // auto = show if playback notch is on
            DisplayFilterMode::Same => state.notch.enabled().get(),
            DisplayFilterMode::Custom => false,
        };
        state.display.noise_filter().set(nr_on || notch_on);

        // Transform
        let xform_on = match state.display.filter_transform().get() {
            DisplayFilterMode::Off => false,
            DisplayFilterMode::Auto => false, // auto = off for transform
            DisplayFilterMode::Same => state.playback.mode().get() != PlaybackMode::Normal,
            DisplayFilterMode::Custom => false, // not yet implemented
        };
        state.display.transform().set(xform_on);

        // Gain — compute display_gain_boost (dB) and display_auto_gain
        let gain_filter = state.display.filter_gain().get();
        let (gain_auto, boost) = match gain_filter {
            DisplayFilterMode::Off => (false, 0.0),
            DisplayFilterMode::Auto => {
                // Peak-normalize: boost quiet files so peak → −3 dBFS
                let auto_db = state.compute_auto_gain() as f32;
                (true, auto_db)
            }
            DisplayFilterMode::Same => {
                // Mirror whatever the playback gain pipeline does
                let manual = state.gain.db().get() as f32;
                match state.gain.mode().get() {
                    GainMode::Off => (false, 0.0),
                    GainMode::Manual => (false, manual),
                    GainMode::AutoPeak => {
                        let auto_db = state.compute_auto_gain() as f32;
                        (true, auto_db + manual)
                    }
                    GainMode::Adaptive => {
                        let auto_db = state.compute_auto_gain() as f32;
                        (true, auto_db + manual)
                    }
                }
            }
            DisplayFilterMode::Custom => (false, 0.0), // manual slider control
        };
        state.display.auto_gain().set(gain_auto);
        state.display.gain_boost().set(boost);

        // When Gain is Off, zero out the display gain offset
        if gain_filter == DisplayFilterMode::Off {
            state.spect.gain_db().set(0.0);
        }

        // Decimation — resolve effective target rate (0 = no decimation)
        let decim_rate = match state.display.filter_decimate().get() {
            DisplayFilterMode::Off => 0,
            DisplayFilterMode::Auto => {
                // Only decimate when xform display is active
                if xform_on {
                    44100
                } else {
                    0
                }
            }
            DisplayFilterMode::Same => {
                // Decimate to browser's native output sample rate so Web Audio doesn't resample
                let bsr = state.display.browser_sample_rate().get();
                if bsr > 0 {
                    bsr
                } else {
                    0
                }
            }
            DisplayFilterMode::Custom => state.display.decimate_rate().get(),
        };
        state.display.decimate_effective().set(decim_rate);
    });

    // (Auto-zoom Effect removed — decimation now controls the frequency axis via sample rate)

    // Auto-learn display noise floor when NR is Auto/Custom and a file is loaded.
    // Re-triggers when file changes or NR mode changes to Auto/Custom.
    {
        let learning: RwSignal<bool> = RwSignal::new(false);
        Effect::new(move |_| {
            let nr_mode = state.display.filter_nr().get();
            let enabled = state.display.xform_enabled().get();
            let file_idx = state.library.current_index().get();
            // Only auto-learn when DSP is enabled and NR is Auto or Custom
            if !enabled || !matches!(nr_mode, DisplayFilterMode::Auto | DisplayFilterMode::Custom) {
                return;
            }
            // Already have a floor for this file? Skip.
            if state.display.auto_noise_floor().get_untracked().is_some() {
                return;
            }
            if learning.get_untracked() {
                return;
            }
            let files = state.library.files().get_untracked();
            let Some(idx) = file_idx else {
                return;
            };
            let Some(file) = files.get(idx).cloned() else {
                return;
            };

            learning.set(true);
            let sample_rate = file.audio.sample_rate;
            // Noise-floor learning only looks at the first 500ms — no need to
            // clone the entire file. Reuse audio.samples (already in-memory
            // mono-mix) and slice the head out of it.
            let needed = (0.5 * sample_rate as f64).ceil() as usize;
            let slice_len = needed.min(file.audio.samples.len());
            let samples = std::sync::Arc::new(file.audio.samples[..slice_len].to_vec());

            wasm_bindgen_futures::spawn_local(async move {
                crate::canvas::tile_cache::yield_to_browser().await;
                let floor = crate::dsp::spectral_sub::learn_noise_floor_async(
                    &samples,
                    sample_rate,
                    0.5, // 500ms from file start
                    crate::canvas::tile_cache::yield_to_browser,
                )
                .await;
                if let Some(f) = floor {
                    state.display.auto_noise_floor().set(Some(f));
                }
                learning.set(false);
            });
        });
    }

    // Clear display auto noise floor when file changes.
    {
        let prev_file: std::cell::Cell<Option<usize>> = std::cell::Cell::new(None);
        Effect::new(move |_| {
            let idx = state.library.current_index().get();
            if idx != prev_file.get() {
                prev_file.set(idx);
                state.display.auto_noise_floor().set(None);
            }
        });
    }

    // Save/restore per-file settings (gain, noise filter) when switching files.
    // Files in the same sequence group share settings.
    // Also resets HFR to OFF for each new file.
    //
    // We pin the "previous file" to its STABLE `LoadedFile.id`, not its
    // positional index: `remove_file_at` shifts indices (and so changes
    // `current_index`'s value) without changing WHICH file is current. Keying on
    // the index would mistake such a reindex for a switch and (a) save the
    // current globals onto a now-shifted neighbour, (b) discard the current
    // file's unsaved edits, and (c) reset its HFR/focus/annotation selection.
    {
        let prev_id: std::cell::Cell<Option<u64>> = std::cell::Cell::new(None);
        Effect::new(move |_| {
            let new_idx = state.library.current_index().get();
            let new_id = new_idx.and_then(|i| {
                state
                    .library
                    .files()
                    .with_untracked(|f| f.get(i).map(|lf| lf.id))
            });

            let old_id = prev_id.get();
            prev_id.set(new_id);

            // Same file (incl. a pure reindex from removing some OTHER file):
            // nothing to save or restore.
            if old_id == new_id {
                return;
            }

            // (No gain swap here anymore: HFR is global and the gain is stored in
            // a fixed off/hfr orientation, so from_state/apply_to_state keep it
            // ear-safe under any HFR state.)

            // Resolve the outgoing file's CURRENT index by id (it may have
            // shifted; `None` means it was removed → nothing to save).
            let old_oi = old_id.and_then(|oid| {
                state
                    .library
                    .files()
                    .with_untracked(|f| f.iter().position(|lf| lf.id == oid))
            });

            // Save current settings (gain + denoise) to the outgoing file and its
            // SEQUENTIAL group. These are AUDIO-CHARACTER settings: they follow
            // consecutive recordings from the same mic/session, NOT the
            // simultaneous channels of a multitrack recording (which have their
            // own levels/noise). [cross-file state-scoping model]
            if let Some(oi) = old_oi {
                let settings = FileSettings::from_state(&state);
                let seq_members = state.library.files().with_untracked(|files| {
                    let names: Vec<String> = files.iter().map(|f| f.name.clone()).collect();
                    let groups = crate::components::file_sidebar::file_groups::compute_all_groups(
                        &names, files,
                    );
                    crate::scope::share_members(crate::scope::Setting::AudioCharacter, &groups, oi)
                });
                state.library.files().update(|files| {
                    for &i in &seq_members {
                        if let Some(f) = files.get_mut(i) {
                            f.settings = settings.clone();
                        }
                    }
                });
            }

            // Genuine switch: clear the annotation selection (its focus override
            // is cleared below by the focus reset — note we do NOT call
            // pop_annotation_ff(), which would disable the now-global HFR), and
            // save the outgoing file's sidecar.
            state.annotations.selected_ids().set(Vec::new());
            if let Some(oi) = old_oi {
                crate::opfs::save_annotations(state, oi);
            }

            // The user's MANUAL focus RANGE is PerFile (shares None): save the
            // outgoing file's, restore the incoming file's. Override layers
            // (bat-book / annotation) are transient — they follow the live
            // selection — so they're cleared on switch, not persisted. The HFR
            // MODE (on/off + saved playback/bandpass) is GLOBAL and persists.
            // [state-scoping: focus + HFR-global]
            if let Some(oid) = old_id {
                let user_range = state
                    .viewmode
                    .focus_stack()
                    .with_untracked(|s| s.user_range());
                state.library.per_file_view().update(|m| {
                    m.entry(oid).or_default().focus = Some(crate::state::FocusSnapshot {
                        user_range,
                        overrides: Vec::new(),
                    });
                });
            }
            let incoming_range = new_id.and_then(|nid| {
                state.library.per_file_view().with_untracked(|m| {
                    m.get(&nid)
                        .and_then(|s| s.focus.as_ref().map(|f| f.user_range))
                })
            });
            // Keep the HFR mode (global); set the per-file user range and clear
            // the transient override layers.
            state.viewmode.focus_stack().update(|s| {
                s.set_focus_selections(
                    incoming_range.unwrap_or(crate::focus_stack::FocusRange::inactive()),
                    Vec::new(),
                );
            });
            // Re-derive the effective BandFF band for the new file's focus (and
            // keep the viewmode `hfr_enabled` mirror in sync).
            state.sync_focus_outputs();

            // Horizontal scroll is a VIEWPORT setting: PerFile, shared across the
            // MULTITRACK group (switch channels → stay at the same place). Save the
            // outgoing file's position to it + its multitrack members; restore the
            // incoming file's, or reset to the start for a fresh/unrelated file.
            // (zoom_level is snapshotted too but kept global for now — deferred.)
            // [cross-file state-scoping: view position]
            if let Some(oi) = old_oi {
                let pos = crate::state::ViewPos {
                    scroll_offset: state.view.scroll_offset().get_untracked(),
                    zoom_level: state.view.zoom_level().get_untracked(),
                };
                let mt_ids = state.library.files().with_untracked(|files| {
                    let names: Vec<String> = files.iter().map(|f| f.name.clone()).collect();
                    let groups = crate::components::file_sidebar::file_groups::compute_all_groups(
                        &names, files,
                    );
                    crate::scope::share_members(
                        crate::scope::Setting::HorizontalScroll,
                        &groups,
                        oi,
                    )
                    .iter()
                    .filter_map(|&j| files.get(j).map(|f| f.id))
                    .collect::<Vec<u64>>()
                });
                state.library.per_file_view().update(|m| {
                    for id in mt_ids {
                        m.entry(id).or_default().view = Some(pos);
                    }
                });
            }
            let incoming_view = new_id.and_then(|nid| {
                state
                    .library
                    .per_file_view()
                    .with_untracked(|m| m.get(&nid).and_then(|s| s.view))
            });
            state
                .view
                .scroll_offset()
                .set(incoming_view.map(|v| v.scroll_offset).unwrap_or(0.0));

            // (No HFR-exit reset on switch anymore: HFR is global, so playback
            // mode + bandpass persist; the per-file vertical-range Effect restores
            // min/max_display_freq.)

            // Restore settings from the incoming file (single restore boundary).
            if let Some(ni) = new_idx {
                let settings = state
                    .library
                    .files()
                    .with_untracked(|files| files.get(ni).map(|f| f.settings.clone()));
                if let Some(s) = settings {
                    s.apply_to_state(&state);
                }
            }
        });
    }

    // Auto-save annotations to OPFS (browser) or central store (Tauri) when dirty.
    Effect::new(move |_| {
        let dirty = state.annotations.dirty().get();
        if !dirty {
            return;
        }
        state.annotations.dirty().set(false);
        let idx = match state.library.current_index().get_untracked() {
            Some(i) => i,
            None => return,
        };
        crate::opfs::save_annotations(state, idx);
    });

    // Global keyboard shortcut: Space = play/stop
    let state_kb = state;
    let handler =
        Closure::<dyn Fn(web_sys::KeyboardEvent)>::new(move |ev: web_sys::KeyboardEvent| {
            // Ignore if focus is on an input/select/textarea
            if let Some(target) = ev.target() {
                if let Ok(el) = target.dyn_into::<web_sys::HtmlElement>() {
                    let tag = el.tag_name();
                    if tag == "INPUT" || tag == "SELECT" || tag == "TEXTAREA" {
                        return;
                    }
                }
            }
            if ev.key() == " " {
                ev.prevent_default();
                if state_kb.library.current_index().get_untracked().is_some() {
                    if state_kb.playback.is_playing().get_untracked() {
                        playback::stop(&state_kb);
                    } else {
                        match state_kb.playback.start_mode().get_untracked() {
                            PlayStartMode::All => playback::play_from_start(&state_kb),
                            PlayStartMode::FromHere => playback::play_from_here(&state_kb),
                            PlayStartMode::Selected => {
                                if playback::effective_selection(&state_kb).is_some() {
                                    playback::play(&state_kb);
                                } else {
                                    playback::play_from_start(&state_kb);
                                }
                            }
                            PlayStartMode::Auto => {
                                if let Some(sel) = playback::effective_selection(&state_kb) {
                                    if playback::is_selection_in_viewport(&state_kb, &sel) {
                                        playback::play(&state_kb);
                                    } else if state_kb.view.scroll_offset().get_untracked() <= 0.0 {
                                        playback::play_from_start(&state_kb);
                                    } else {
                                        playback::play_from_here(&state_kb);
                                    }
                                } else if state_kb.view.scroll_offset().get_untracked() <= 0.0 {
                                    playback::play_from_start(&state_kb);
                                } else {
                                    playback::play_from_here(&state_kb);
                                }
                            }
                        }
                    }
                }
            }
            if (ev.key() == "l" || ev.key() == "L")
                && !ev.ctrl_key()
                && !ev.meta_key()
                && !ev.alt_key()
            {
                ev.prevent_default();
                let st = state_kb;
                wasm_bindgen_futures::spawn_local(async move {
                    microphone::toggle_listen(&st).await;
                });
            }
            if (ev.key() == "r" || ev.key() == "R")
                && !ev.ctrl_key()
                && !ev.meta_key()
                && !ev.alt_key()
            {
                ev.prevent_default();
                let st = state_kb;
                wasm_bindgen_futures::spawn_local(async move {
                    microphone::toggle_record(&st).await;
                });
            }
            if (ev.key() == "h" || ev.key() == "H")
                && !ev.ctrl_key()
                && !ev.meta_key()
                && !ev.alt_key()
            {
                ev.prevent_default();
                state_kb.viewmode.hfr_enabled().update(|v| *v = !*v);
            }
            if (ev.key() == "b" || ev.key() == "B")
                && !ev.ctrl_key()
                && !ev.meta_key()
                && !ev.alt_key()
            {
                ev.prevent_default();
                state_kb.bat_book.open().update(|v| *v = !*v);
            }
            // M = drop a marker annotation at the current playhead position.
            if (ev.key() == "m" || ev.key() == "M")
                && !ev.ctrl_key()
                && !ev.meta_key()
                && !ev.alt_key()
            {
                // If something else wants the key (label editor, etc.), skip.
                if state_kb.annotations.editing().get_untracked() {
                    return;
                }
                ev.prevent_default();
                let t = state_kb.playback.playhead_time().get_untracked();
                crate::components::overflow_menu::add_marker_at_time(&state_kb, t);
            }
            // Q = toggle frequency bounds on current selection or selected annotations (region ↔ segment)
            if (ev.key() == "q" || ev.key() == "Q")
                && !ev.ctrl_key()
                && !ev.meta_key()
                && !ev.alt_key()
            {
                if state_kb.toggle_q_freq_bounds() {
                    ev.prevent_default();
                }
            }
            // Ctrl+Z / Cmd+Z = Undo, Ctrl+Shift+Z / Cmd+Shift+Z / Ctrl+Y = Redo
            if (ev.key() == "z" || ev.key() == "Z")
                && (ev.ctrl_key() || ev.meta_key())
                && !ev.alt_key()
            {
                ev.prevent_default();
                if ev.shift_key() {
                    state_kb.redo_annotations();
                } else {
                    state_kb.undo_annotations();
                }
            }
            if ev.key() == "y"
                && (ev.ctrl_key() || ev.meta_key())
                && !ev.shift_key()
                && !ev.alt_key()
            {
                ev.prevent_default();
                state_kb.redo_annotations();
            }
            // Navigation: arrow keys, PgUp/PgDn, Ctrl+Home/End
            let is_ctrl = ev.ctrl_key() || ev.meta_key();
            let nav_action = match ev.key().as_str() {
                "ArrowLeft" | "ArrowRight" => Some(ev.key()),
                "PageUp" | "PageDown" => Some(ev.key()),
                "Home" | "End" if is_ctrl => Some(ev.key()),
                _ => None,
            };
            if let Some(key) = nav_action {
                ev.prevent_default();
                let files = state_kb.library.files().get_untracked();
                let timeline = state_kb.timeline.active().get_untracked();
                let (time_res, duration) = if let Some(ref tl) = timeline {
                    let tr = tl
                        .segments
                        .first()
                        .and_then(|s| files.get(s.file_index))
                        .map(|f| f.spectrogram.time_resolution)
                        .unwrap_or(1.0);
                    (tr, tl.total_duration_secs)
                } else {
                    let idx = state_kb
                        .library
                        .current_index()
                        .get_untracked()
                        .unwrap_or(0);
                    match files.get(idx) {
                        Some(file) => (file.spectrogram.time_resolution, file.audio.duration_secs),
                        None => (1.0, 0.0),
                    }
                };
                {
                    let zoom = state_kb.view.zoom_level().get_untracked();
                    let canvas_w = state_kb.viewmode.spectrogram_canvas_width().get_untracked();
                    let visible_time = viewport::visible_time(canvas_w, zoom, time_res);
                    let from_here_mode = state_kb
                        .playback
                        .start_mode()
                        .get_untracked()
                        .uses_from_here();
                    let (_min_scroll, max_scroll) =
                        viewport::scroll_bounds_for_mode(duration, visible_time, from_here_mode);
                    let new_scroll = match key.as_str() {
                        "Home" => viewport::clamp_scroll_for_mode(
                            0.0,
                            duration,
                            visible_time,
                            from_here_mode,
                        ),
                        "End" => max_scroll,
                        "ArrowLeft" => viewport::clamp_scroll_for_mode(
                            state_kb.view.scroll_offset().get_untracked() - visible_time * 0.2,
                            duration,
                            visible_time,
                            from_here_mode,
                        ),
                        "ArrowRight" => viewport::clamp_scroll_for_mode(
                            state_kb.view.scroll_offset().get_untracked() + visible_time * 0.2,
                            duration,
                            visible_time,
                            from_here_mode,
                        ),
                        "PageUp" => viewport::clamp_scroll_for_mode(
                            state_kb.view.scroll_offset().get_untracked() - visible_time * 0.8,
                            duration,
                            visible_time,
                            from_here_mode,
                        ),
                        "PageDown" => viewport::clamp_scroll_for_mode(
                            state_kb.view.scroll_offset().get_untracked() + visible_time * 0.8,
                            duration,
                            visible_time,
                            from_here_mode,
                        ),
                        _ => state_kb.view.scroll_offset().get_untracked(),
                    };
                    state_kb.suspend_follow();
                    state_kb.view.scroll_offset().set(new_scroll);
                }
            }
            if ev.key() == "Escape" {
                if state_kb.bat_book.ref_open().get_untracked() {
                    state_kb.bat_book.ref_open().set(false);
                    return;
                }
                if state_kb.dialogs.xc_browser_open().get_untracked() {
                    state_kb.dialogs.xc_browser_open().set(false);
                    return;
                }
                if state_kb.dialogs.wiki_browser_open().get_untracked() {
                    state_kb.dialogs.wiki_browser_open().set(false);
                    return;
                }
                if state_kb.mic.listening().get_untracked()
                    || state_kb.mic.recording().get_untracked()
                {
                    microphone::stop_all(&state_kb);
                }
            }
            // Backtick: activate clean view (hide overlays)
            if ev.key() == "`"
                && !ev.ctrl_key()
                && !ev.meta_key()
                && !ev.alt_key()
                && !state_kb.viewmode.clean_view().get_untracked()
            {
                state_kb.viewmode.clean_view().set(true);
            }
        });
    let window = web_sys::window().unwrap();
    let _ = window.add_event_listener_with_callback("keydown", handler.as_ref().unchecked_ref());
    handler.forget();

    // Keyup handler: release clean view on backtick release
    let state_ku = state;
    let keyup_handler =
        Closure::<dyn Fn(web_sys::KeyboardEvent)>::new(move |ev: web_sys::KeyboardEvent| {
            if ev.key() == "`" {
                state_ku.viewmode.clean_view().set(false);
            }
        });
    let _ =
        window.add_event_listener_with_callback("keyup", keyup_handler.as_ref().unchecked_ref());
    keyup_handler.forget();

    // Reset clean view if window loses focus (so it doesn't stick)
    let state_blur = state;
    let blur_handler = Closure::<dyn Fn()>::new(move || {
        state_blur.viewmode.clean_view().set(false);
    });
    let _ = window.add_event_listener_with_callback("blur", blur_handler.as_ref().unchecked_ref());
    blur_handler.forget();

    // Update is_mobile reactively on window resize.
    // On resize, layout is driven purely by viewport width — the UA-based mobile
    // detection only matters for the initial load (before any resize events).
    {
        let state_resize = state;
        let on_resize = Closure::<dyn Fn()>::new(move || {
            let mobile = crate::state::is_mobile_viewport();
            let was_mobile = state_resize.status.is_mobile().get_untracked();
            if mobile != was_mobile {
                state_resize.status.is_mobile().set(mobile);
                if mobile {
                    // Switching to mobile: collapse sidebars (they become overlays)
                    state_resize.panels.left_collapsed().set(true);
                    state_resize.panels.right_collapsed().set(true);
                } else {
                    // Switching to desktop: show left sidebar by default
                    state_resize.panels.left_collapsed().set(false);
                }
            }
        });
        let _ =
            window.add_event_listener_with_callback("resize", on_resize.as_ref().unchecked_ref());
        on_resize.forget();
    }

    let grid_style = move || {
        if state.status.is_mobile().get() {
            // Sidebars are position:fixed overlays, so single column for main content
            "grid-template-columns: 1fr; grid-template-rows: auto 1fr".to_string()
        } else {
            let left = if state.panels.left_collapsed().get() {
                0
            } else {
                state.panels.left_width().get() as i32
            };
            let right = if state.panels.right_collapsed().get() {
                0
            } else {
                state.panels.right_width().get() as i32
            };
            format!(
                "grid-template-columns: {}px 1fr {}px; grid-template-rows: auto 1fr",
                left, right
            )
        }
    };

    // Prevent browser from opening dropped files (navigating away from the app).
    // This also ensures the sidebar drop zone receives drop events reliably.
    {
        let doc = web_sys::window().unwrap().document().unwrap();
        let on_dragover = Closure::<dyn Fn(web_sys::Event)>::new(|ev: web_sys::Event| {
            ev.prevent_default();
        });
        let on_drop = Closure::<dyn Fn(web_sys::Event)>::new(|ev: web_sys::Event| {
            ev.prevent_default();
        });
        let _ =
            doc.add_event_listener_with_callback("dragover", on_dragover.as_ref().unchecked_ref());
        let _ = doc.add_event_listener_with_callback("drop", on_drop.as_ref().unchecked_ref());
        on_dragover.forget();
        on_drop.forget();
    }

    // Live-audio lifecycle across app background/foreground (Page Visibility API).
    // On hide during a live session we snapshot wall-clock + capture + audio-clock
    // state. On show we resume the (possibly OS-suspended) playback context and
    // reset the schedule cursor so listening jumps back to "now" rather than
    // replaying a multi-second backlog — then check whether capture / audible
    // output actually kept flowing while hidden and, if not, surface a one-time
    // battery-optimization hint.
    {
        use std::cell::Cell;
        use std::rc::Rc;

        /// Raise the one-time background-audio guidance hint when capture
        /// (recording) or audible output (listening) demonstrably stalled while
        /// the app was hidden. `het_now`/`het_hide` are the HET AudioContext clock
        /// at show/hide; a frozen clock means audible monitoring was suspended.
        fn maybe_warn_background_throttle(
            state: &AppState,
            wall_hide: f64,
            samples_hide: usize,
            het_now: Option<f64>,
            het_hide: f64,
        ) {
            // Only meaningful on mobile Tauri, and only nag once.
            if !state.is_tauri || !state.status.is_mobile().get_untracked() {
                return;
            }
            if state.dialogs.background_hint_dismissed().get_untracked() {
                return;
            }
            if state.dialogs.background_audio_hint().get_untracked() {
                return;
            }

            let wall_elapsed = (js_sys::Date::now() - wall_hide) / 1000.0;
            // Need a non-trivial interval to judge — avoids false positives on a
            // quick app-switch where throttling never had time to bite.
            if wall_elapsed < 5.0 {
                return;
            }

            let sr = state.mic.sample_rate().get_untracked().max(1) as f64;
            let mut throttled = false;

            // Capture check (recording only — mic_samples_recorded isn't advanced
            // during listen-only): did samples keep pace with wall time?
            if state.mic.recording().get_untracked() {
                let actual = state
                    .mic
                    .samples_recorded()
                    .get_untracked()
                    .saturating_sub(samples_hide) as f64;
                if actual < wall_elapsed * sr * 0.5 {
                    throttled = true;
                }
            }

            // Audible-output check (listening): did the audio clock advance?
            if state.mic.listening().get_untracked() {
                if let Some(now) = het_now {
                    if now - het_hide < wall_elapsed * 0.5 {
                        throttled = true;
                    }
                }
            }

            if throttled {
                state.dialogs.background_audio_hint().set(true);
            }
        }

        let state_vis = state;
        // (wall_ms_at_hide, recorded_samples_at_hide, het_clock_at_hide)
        let snapshot: Rc<Cell<Option<(f64, usize, f64)>>> = Rc::new(Cell::new(None));
        let doc_vis = web_sys::window().unwrap().document().unwrap();
        let on_visibility = Closure::<dyn Fn()>::new(move || {
            let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
                return;
            };
            let live = state_vis.mic.listening().get_untracked()
                || state_vis.mic.recording().get_untracked();
            if doc.hidden() {
                if live {
                    let het = crate::audio::mic_backend::het_context_time().unwrap_or(0.0);
                    snapshot.set(Some((
                        js_sys::Date::now(),
                        state_vis.mic.samples_recorded().get_untracked(),
                        het,
                    )));
                } else {
                    snapshot.set(None);
                }
            } else {
                // Capture the audio clock BEFORE resume so it reflects the hidden
                // interval (resume is async and won't have advanced it yet).
                let het_now = crate::audio::mic_backend::het_context_time();
                crate::audio::mic_backend::resume_playback_context();
                if state_vis.mic.listening().get_untracked() {
                    crate::audio::mic_backend::stop_het_playback();
                }
                if let Some((wall_hide, samples_hide, het_hide)) = snapshot.take() {
                    maybe_warn_background_throttle(
                        &state_vis,
                        wall_hide,
                        samples_hide,
                        het_now,
                        het_hide,
                    );
                }
            }
        });
        let _ = doc_vis.add_event_listener_with_callback(
            "visibilitychange",
            on_visibility.as_ref().unchecked_ref(),
        );
        on_visibility.forget();
    }

    // Tauri: listen for native file drag-drop events (provides real filesystem paths)
    if state.is_tauri {
        let state_drop = state;
        #[derive(serde::Deserialize)]
        struct DragDropPayload {
            paths: Vec<String>,
        }
        #[derive(serde::Deserialize)]
        struct DragDropEvent {
            payload: DragDropPayload,
        }
        let callback = wasm_bindgen::closure::Closure::<dyn FnMut(wasm_bindgen::JsValue)>::new(
            move |ev: wasm_bindgen::JsValue| {
                // Payload shape: { event, payload: { paths: [...], position: {x, y} } }
                let Ok(event) = serde_wasm_bindgen::from_value::<DragDropEvent>(ev) else {
                    return;
                };
                let file_paths = event.payload.paths;
                if file_paths.is_empty() {
                    return;
                }

                log::info!("Tauri drag-drop: {} file(s)", file_paths.len());
                for path in file_paths {
                    let name = path.rsplit(['/', '\\']).next().unwrap_or(&path).to_string();
                    // Filter to audio-ish extensions
                    let ext = name.rsplit('.').next().unwrap_or("").to_lowercase();
                    if !matches!(
                        ext.as_str(),
                        "wav" | "w4v" | "flac" | "ogg" | "mp3" | "m4a" | "m4b"
                    ) {
                        log::info!("Skipping non-audio drop: {name}");
                        continue;
                    }
                    let state = state_drop;
                    let load_id = state.loading_start(&name);
                    let name_for_err = name.clone();
                    leptos::task::spawn_local(async move {
                        match crate::components::file_sidebar::load_native_file(
                            path, state, load_id,
                        )
                        .await
                        {
                            Ok(()) => {}
                            Err(e) => {
                                log::error!("Failed to load {}: {}", name_for_err, e);
                                state.show_error_toast(&format!(
                                    "Couldn't open {name_for_err}: {e}"
                                ));
                            }
                        }
                        state.loading_done(load_id);
                    });
                }
            },
        );
        crate::tauri_bridge::tauri_listen("tauri://drag-drop", callback);

        // Expose a global the Android notification "Stop" action calls (via
        // webView.evaluateJavascript from AudioServicePlugin) to tear down live
        // capture cleanly — finalizing/saving the recording and stopping the
        // foreground service. A native push is used (not a JS-timer poll) because
        // the user typically hits this while the app is backgrounded, where JS
        // timers are throttled but evaluateJavascript still runs.
        let state_stop = state;
        let stop_cb = wasm_bindgen::closure::Closure::<dyn FnMut()>::new(move || {
            log::info!("Native notification: stop capture requested");
            crate::audio::microphone::stop_all(&state_stop);
        });
        if let Some(win) = web_sys::window() {
            let _ = js_sys::Reflect::set(
                &win,
                &wasm_bindgen::JsValue::from_str("__oversampleStopCapture"),
                stop_cb.as_ref().unchecked_ref(),
            );
        }
        stop_cb.forget();
    }

    // Back button (Android/browser): close sidebar when open.
    // Registered unconditionally — harmless on desktop, needed if layout switches to mobile.
    {
        let state_back = state;
        let on_popstate = wasm_bindgen::closure::Closure::<dyn Fn(web_sys::Event)>::new(
            move |_: web_sys::Event| {
                if !state_back.status.is_mobile().get_untracked() {
                    return;
                }
                if !state_back.panels.right_collapsed().get_untracked() {
                    state_back.panels.right_collapsed().set(true);
                    let _ = web_sys::window()
                        .unwrap()
                        .history()
                        .unwrap()
                        .push_state_with_url(&wasm_bindgen::JsValue::NULL, "", None);
                } else if !state_back.panels.left_collapsed().get_untracked() {
                    state_back.panels.left_collapsed().set(true);
                    let _ = web_sys::window()
                        .unwrap()
                        .history()
                        .unwrap()
                        .push_state_with_url(&wasm_bindgen::JsValue::NULL, "", None);
                }
            },
        );
        let _ = window
            .add_event_listener_with_callback("popstate", on_popstate.as_ref().unchecked_ref());
        on_popstate.forget();
        // Push initial history entry so back button has something to pop
        let _ =
            window
                .history()
                .unwrap()
                .push_state_with_url(&wasm_bindgen::JsValue::NULL, "", None);
    }

    // Monitor visualViewport.scale to detect pinch-zoom on mobile browsers.
    // When the user is zoomed in, we set viewport_zoomed so the UI can show a
    // zoom-out button and disable our custom pinch handler.
    {
        let state_vp = state;
        let check_zoom = wasm_bindgen::closure::Closure::<dyn Fn()>::new(move || {
            let vv_obj = js_sys::Reflect::get(
                &js_sys::global(),
                &wasm_bindgen::JsValue::from_str("visualViewport"),
            )
            .ok()
            .filter(|v| !v.is_undefined() && !v.is_null());
            let scale = vv_obj
                .as_ref()
                .and_then(|vv| {
                    js_sys::Reflect::get(vv, &wasm_bindgen::JsValue::from_str("scale")).ok()
                })
                .and_then(|s| s.as_f64())
                .unwrap_or(1.0);
            let zoomed = scale > 1.05;
            // Track visual viewport position for button placement
            if let Some(ref vv) = vv_obj {
                let offset_top =
                    js_sys::Reflect::get(vv, &wasm_bindgen::JsValue::from_str("offsetTop"))
                        .ok()
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0);
                let offset_left =
                    js_sys::Reflect::get(vv, &wasm_bindgen::JsValue::from_str("offsetLeft"))
                        .ok()
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0);
                let vp_width = js_sys::Reflect::get(vv, &wasm_bindgen::JsValue::from_str("width"))
                    .ok()
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                state_vp.status.visual_viewport_rect().set((
                    offset_top,
                    offset_left,
                    vp_width,
                    scale,
                ));
            }
            let prev = state_vp.status.viewport_zoomed().get_untracked();
            state_vp.status.viewport_zoomed().set(zoomed);
            // Toggle body class so CSS can override touch-action on canvas areas
            if let Some(body) = web_sys::window()
                .and_then(|w| w.document())
                .and_then(|d| d.body())
            {
                if zoomed {
                    let _ = body.class_list().add_1("viewport-zoomed");
                } else {
                    let _ = body.class_list().remove_1("viewport-zoomed");
                }
            }
            // When zooming back out, clear any stale drag/interaction state so
            // a single touch doesn't get "stuck" acting like a pinch.
            if prev && !zoomed {
                state_vp.interaction.is_dragging().set(false);
                state_vp.interaction.spec_drag_handle().set(None);
                state_vp.annotations.drag_handle().set(None);
            }
        });
        let window = web_sys::window().unwrap();
        if let Ok(vv) =
            js_sys::Reflect::get(&window, &wasm_bindgen::JsValue::from_str("visualViewport"))
        {
            if !vv.is_undefined() && !vv.is_null() {
                // Listen to both "resize" and "scroll" — different platforms fire
                // different events during pinch-zoom gestures.
                let add_fn_val =
                    js_sys::Reflect::get(&vv, &wasm_bindgen::JsValue::from_str("addEventListener"))
                        .ok();
                if let Some(add_fn_val) = add_fn_val {
                    if let Ok(add_fn) = add_fn_val.dyn_into::<js_sys::Function>() {
                        let _ = add_fn.call2(
                            &vv,
                            &wasm_bindgen::JsValue::from_str("resize"),
                            check_zoom.as_ref(),
                        );
                        let _ = add_fn.call2(
                            &vv,
                            &wasm_bindgen::JsValue::from_str("scroll"),
                            check_zoom.as_ref(),
                        );
                    }
                }
            }
        }
        // Also re-check on every touchend — catches cases where visualViewport
        // events are delayed or don't fire (e.g. some Android WebViews).
        {
            let check_ref = check_zoom.as_ref().clone();
            let touchend_cb = wasm_bindgen::closure::Closure::<dyn Fn()>::new(move || {
                // Defer to next frame so the viewport scale has settled
                let check_ref2 = check_ref.clone();
                let _ = web_sys::window()
                    .unwrap()
                    .request_animation_frame(check_ref2.unchecked_ref());
            });
            let _ = window
                .add_event_listener_with_callback("touchend", touchend_cb.as_ref().unchecked_ref());
            touchend_cb.forget();
        }
        check_zoom.forget();
    }

    let app_class = move || {
        if state.status.is_mobile().get() {
            "app mobile"
        } else {
            "app"
        }
    };

    view! {
        <div class=app_class style=grid_style>
            <Toolbar />
            <FileSidebar />
            {move || state.status.is_mobile().get().then(|| view! {
                <div
                    class=move || if !state.panels.left_collapsed().get() || !state.panels.right_collapsed().get() { "sidebar-backdrop open" } else { "sidebar-backdrop" }
                    on:click=move |_| {
                        state.panels.left_collapsed().set(true);
                        state.panels.right_collapsed().set(true);
                    }
                ></div>
            })}
            <MainArea />
            <RightSidebar />
            {move || state.dialogs.xc_browser_open().get().then(|| view! { <XcBrowser /> })}
            {move || state.dialogs.wiki_browser_open().get().then(|| view! { <WikiBrowser /> })}
            {cfg!(debug_assertions).then(|| view! {
                <div class="debug-build-banner"
                    title="This is an unoptimised WASM build. It runs slower and can hit spurious panics that don't happen in release builds. Rebuild with `trunk serve --release`."
                >
                    "\u{26A0} DEBUG WASM \u{2014} slow & unstable. Use "
                    <code>"trunk serve --release"</code>
                </div>
            })}
        </div>
    }
}

#[component]
fn MainArea() -> impl IntoView {
    let state = expect_context::<AppState>();
    let has_file = move || {
        state.library.current_index().get().is_some() || state.timeline.active().get().is_some()
    };

    // Click/tap anywhere in the main area closes open layer panels (and sidebar on mobile)
    let on_main_click = move |_: web_sys::MouseEvent| {
        state.panels.layer_panel_open().set(None);
        if state.status.is_mobile().get_untracked() {
            state.panels.left_collapsed().set(true);
            state.panels.right_collapsed().set(true);
        }
    };
    // touchstart also closes menus — needed because mobile touch handlers often
    // call preventDefault() which suppresses the synthetic click event
    let on_main_touchstart = move |_: web_sys::TouchEvent| {
        state.panels.layer_panel_open().set(None);
        if state.status.is_mobile().get_untracked() {
            state.panels.left_collapsed().set(true);
            state.panels.right_collapsed().set(true);
        }
    };

    view! {
        <div class="main" on:click=on_main_click on:touchstart=on_main_touchstart>
            <ToastDisplay />
            {move || {
                if has_file() {
                    view! {
                        // Overview strip (top)
                        <OverviewPanel />
                        // Thin toolbar tied to the overview — owns the
                        // spectrogram/waveform toggle.
                        <OverviewToolbar />

                        // View Bar — visualization-layer controls (which view,
                        // which overlays, canvas tool)
                        <ViewBar />

                        // Hearing Bar — sound-output / DSP row above the main view
                        <HearingBar />

                        // Main view (takes remaining space)
                        <div class="main-view">
                            // Show the selected main view
                            {move || match state.viewmode.main_view().get() {
                                MainView::Spectrogram | MainView::Flow | MainView::Resonators => view! { <Spectrogram /> }.into_any(),
                                MainView::Waveform => view! {
                                    <div class="main-waveform-full">
                                        <Waveform />
                                    </div>
                                }.into_any(),
                                MainView::ZcChart => view! {
                                    <div class="main-waveform-full">
                                        <ZcDotChart />
                                    </div>
                                }.into_any(),
                                MainView::Chromagram => view! { <ChromagramView /> }.into_any(),
                            }}

                            // VU meter — red line on right edge during recording/listening
                            {move || {
                                let recording = state.mic.recording().get();
                                let listening = state.mic.listening().get();
                                if !recording && !listening { return None; }
                                let level = state.mic.peak_level().get();
                                let height_pct = (level * 100.0).clamp(0.0, 100.0);
                                Some(view! {
                                    <div
                                        class="vu-meter"
                                        style:height=format!("{}%", height_pct)
                                    ></div>
                                })
                            }}

                            // Floating overlay layer
                            <div class="main-overlays"
                                style:display=move || if state.viewmode.clean_view().get() { "none" } else { "" }
                            >
                                // Unsaved recording banner (web only)
                                {move || {
                                    if state.is_tauri { return None; }
                                    let files = state.library.files().get();
                                    let idx = state.library.current_index().get()?;
                                    let file = files.get(idx)?;
                                    if !file.is_recording { return None; }
                                    let name = file.name.clone();
                                    Some(view! {
                                        <div
                                            class="unsaved-banner"
                                            on:click=move |_| {
                                                let files = state.library.files().get_untracked();
                                                let idx = state.library.current_index().get_untracked();
                                                if let Some(i) = idx {
                                                    if let Some(f) = files.get(i) {
                                                        let total = f.audio.source.total_samples() as usize;
                                                        let samples = f.audio.source.read_region(crate::audio::source::ChannelView::MonoMix, 0, total);
                                                        crate::audio::wav_encoder::download_recording_wav(
                                                            &samples, f.audio.sample_rate, &name,
                                                            f.audio.metadata.guano.as_ref(), &f.wav_markers,
                                                        );
                                                    }
                                                }
                                            }
                                        >
                                            "Unsaved recording \u{2014} click to download"
                                        </div>
                                    })
                                }}
                                <BookmarkPopup />
                                <CanvasOverflowMenus />
                                <AnnotationLabelEditor />
                            </div>

                            // Bat book reference panel (floating overlay, right side)
                            {move || (state.bat_book.ref_open().get() && !state.viewmode.clean_view().get()).then(|| view! { <BatBookRefPanel /> })}
                        </div>

                        // Bat book strip (between main view and bottom toolbar)
                        {move || state.bat_book.open().get().then(|| view! { <BatBookStrip /> })}

                        {move || state.panels.show_status_bar().get().then(|| view! { <AnalysisPanel /> })}
                    }.into_any()
                } else {
                    // Mobile: show the very icon the sidebar toggle uses.
                    let empty_msg = if state.status.is_mobile().get() {
                        view! { "Tap " <Icon kind=Icon::SidebarLeft /> " to load audio files" }.into_any()
                    } else {
                        view! { "Drop WAV, FLAC or MP3 files into the sidebar" }.into_any()
                    };
                    view! {
                        <div class="empty-state">
                            {empty_msg}
                        </div>
                        {move || state.bat_book.open().get().then(|| view! { <BatBookStrip /> })}
                        {move || state.bat_book.ref_open().get().then(|| view! { <BatBookRefPanel /> })}
                    }.into_any()
                }
            }}
            <BottomToolbar />

            // Zoom-out button — appears when mobile viewport is pinch-zoomed in.
            // Uses absolute positioning relative to the visual viewport (not
            // position:fixed which anchors to the layout viewport and goes off-screen).
            {move || state.status.viewport_zoomed().get().then(|| {
                let btn_size = 44.0_f64;
                let margin = 10.0_f64;
                view! {
                    <button
                        class="zoom-out-btn"
                        title="Reset zoom"
                        style=move || {
                            let (off_top, off_left, vp_w, _scale) = state.status.visual_viewport_rect().get();
                            let top = off_top + margin;
                            let left = off_left + vp_w - btn_size - margin;
                            format!("top:{top}px;left:{left}px;")
                        }
                        on:click={
                            let state = state.clone();
                            move |_| {
                                // On Tauri (Android), call the native zoom plugin —
                                // it's the only thing that actually works.
                                // Android WebView ignores all JS-side meta viewport /
                                // documentElement.style.zoom tricks for resetting
                                // the visual viewport scale after page load.
                                if state.is_tauri {
                                    wasm_bindgen_futures::spawn_local(async {
                                        use crate::tauri_bridge::tauri_invoke_no_args;
                                        if let Err(e) = tauri_invoke_no_args("plugin:zoom|reset").await {
                                            log::warn!("plugin:zoom|reset failed: {}", e);
                                        }
                                    });
                                    return;
                                }
                                // Web fallback: try meta viewport replace + Chromium
                                // documentElement.style.zoom. These work on desktop
                                // browsers some of the time but are unreliable on
                                // mobile web. (Most users hit this code path through
                                // the Android app, which uses the native plugin above.)
                                let Some(window) = web_sys::window() else { return };
                                let Some(doc) = window.document() else { return };
                                if let Some(html_el) = doc
                                    .document_element()
                                    .and_then(|h| h.dyn_into::<web_sys::HtmlElement>().ok())
                                {
                                    let _ = html_el.style().set_property("zoom", "1");
                                }
                                if let Some(meta) = doc.query_selector("meta[name=viewport]")
                                    .ok().flatten()
                                {
                                    let original = meta.get_attribute("content").unwrap_or_default();
                                    let _ = meta.set_attribute(
                                        "content",
                                        "width=device-width, initial-scale=1, \
                                         maximum-scale=1, user-scalable=no",
                                    );
                                    let meta_clone = meta.clone();
                                    let doc_clone = doc.clone();
                                    let cb = wasm_bindgen::closure::Closure::once(move || {
                                        let _ = meta_clone.set_attribute("content", &original);
                                        if let Some(html_el) = doc_clone
                                            .document_element()
                                            .and_then(|h| h.dyn_into::<web_sys::HtmlElement>().ok())
                                        {
                                            let _ = html_el.style().remove_property("zoom");
                                        }
                                    });
                                    let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(
                                        cb.as_ref().unchecked_ref(), 120,
                                    );
                                    cb.forget();
                                }
                            }
                        }
                    >
                        <svg viewBox="0 0 24 24" width="22" height="22" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
                            <circle cx="11" cy="11" r="8"/>
                            <line x1="21" y1="21" x2="16.65" y2="16.65"/>
                            <line x1="8" y1="11" x2="14" y2="11"/>
                        </svg>
                    </button>
                }
            })}

            // Mic chooser modal (position:fixed, shown when show_mic_chooser is true)
            {move || state.mic.show_chooser().get().then(|| view! {
                <crate::components::file_sidebar::mic_chooser::MicChooserModal />
            })}

            // Privacy settings modal
            {move || state.dialogs.privacy_settings().get().then(|| view! {
                <crate::components::file_sidebar::privacy_settings::PrivacySettingsModal />
            })}

            // "Ready to record" modal
            {move || (state.mic.record_ready_state().get() == crate::state::RecordReadyState::AwaitingConfirmation).then(|| {
                let on_ok = move |_: web_sys::MouseEvent| {
                    let st = state;
                    wasm_bindgen_futures::spawn_local(async move {
                        crate::audio::microphone::confirm_record_start(&st).await;
                    });
                };
                let on_cancel = move |_: web_sys::MouseEvent| {
                    crate::audio::microphone::cancel_record_start(&state);
                };
                view! {
                    <div class="xc-modal-overlay" on:click=on_cancel>
                        <div class="xc-modal" style="width: min(90vw, 340px); text-align: center;" on:click=move |ev: web_sys::MouseEvent| ev.stop_propagation()>
                            <div style="padding: 24px 16px 8px;">
                                <div style="font-size: 16px; font-weight: 600; margin-bottom: 8px;">"Ready to record"</div>
                                <div style="font-size: 13px; color: #aaa; margin-bottom: 16px;">
                                    {move || state.mic.device_info().get().map(|info| info.name.clone()).unwrap_or_else(|| "Microphone".to_string())}
                                    " is ready"
                                </div>
                            </div>
                            <div style="display: flex; gap: 8px; justify-content: center; padding: 8px 16px 16px;">
                                <button class="setting-btn" style="padding: 6px 20px;" on:click=on_cancel>"Cancel"</button>
                                <button class="setting-btn" style="padding: 6px 20px; background: #c44; color: #fff; font-weight: 600;" on:click=on_ok>"Record"</button>
                            </div>
                        </div>
                    </div>
                }
            })}

            // Background-audio throttling guidance (one-time; raised by the
            // visibility watchdog when capture/monitoring stalled while hidden).
            {move || state.dialogs.background_audio_hint().get().then(|| {
                let on_settings = move |_: web_sys::MouseEvent| {
                    wasm_bindgen_futures::spawn_local(async move {
                        let _ = crate::tauri_bridge::tauri_invoke_no_args(
                            "plugin:audio-service|requestDisableBatteryOptimization",
                        ).await;
                    });
                    dismiss_background_hint(&state);
                };
                view! {
                    <div class="xc-modal-overlay" on:click=move |_: web_sys::MouseEvent| dismiss_background_hint(&state)>
                        <div class="xc-modal" style="width: min(92vw, 380px);" on:click=move |ev: web_sys::MouseEvent| ev.stop_propagation()>
                            <div style="padding: 20px 18px 8px;">
                                <div style="font-size: 16px; font-weight: 600; margin-bottom: 8px;">"Background audio was interrupted"</div>
                                <div style="font-size: 13px; color: #bbb; line-height: 1.5;">
                                    "Android paused audio while the app was in the background. To keep listening and recording running when you switch apps or the screen turns off, allow unrestricted background activity for Oversample."
                                </div>
                            </div>
                            <div style="display: flex; gap: 8px; justify-content: flex-end; padding: 8px 16px 16px;">
                                <button class="setting-btn" style="padding: 6px 16px;" on:click=move |_: web_sys::MouseEvent| dismiss_background_hint(&state)>"Not now"</button>
                                <button class="setting-btn" style="padding: 6px 16px; background: #46c; color: #fff; font-weight: 600;" on:click=on_settings>"Open settings"</button>
                            </div>
                        </div>
                    </div>
                }
            })}

            // Notification-permission rationale (Android; shown during mic setup
            // before the OS POST_NOTIFICATIONS prompt so the user understands why).
            {move || state.dialogs.notif_rationale().get().then(|| {
                view! {
                    <div class="xc-modal-overlay" on:click=move |_: web_sys::MouseEvent| dismiss_notif_rationale(&state, false)>
                        <div class="xc-modal" style="width: min(92vw, 380px);" on:click=move |ev: web_sys::MouseEvent| ev.stop_propagation()>
                            <div style="padding: 20px 18px 8px;">
                                <div style="font-size: 16px; font-weight: 600; margin-bottom: 8px;">"Allow notifications?"</div>
                                <div style="font-size: 13px; color: #bbb; line-height: 1.5;">
                                    "While listening or recording, Oversample shows an ongoing notification so Android keeps capturing audio when the app is in the background or the screen is off. Next, Android will ask permission to show it — this app never sends promotional messages."
                                </div>
                            </div>
                            <div style="display: flex; gap: 8px; justify-content: flex-end; padding: 8px 16px 16px;">
                                <button class="setting-btn" style="padding: 6px 16px;" on:click=move |_: web_sys::MouseEvent| dismiss_notif_rationale(&state, false)>"Not now"</button>
                                <button class="setting-btn" style="padding: 6px 16px; background: #46c; color: #fff; font-weight: 600;" on:click=move |_: web_sys::MouseEvent| dismiss_notif_rationale(&state, true)>"Continue"</button>
                            </div>
                        </div>
                    </div>
                }
            })}
        </div>
    }
}

fn toggle_panel(state: &AppState, panel: LayerPanel) {
    state.panels.layer_panel_open().update(|p| {
        *p = if *p == Some(panel) { None } else { Some(panel) };
    });
}

/// Dismiss the background-audio guidance hint and persist that the user has seen
/// it (so it never auto-shows again). Shared by both modal buttons.
fn dismiss_background_hint(state: &AppState) {
    state.dialogs.background_audio_hint().set(false);
    state.dialogs.background_hint_dismissed().set(true);
    crate::settings::set_bool(crate::settings::keys::BG_AUDIO_HINT_DISMISSED, true);
}

/// Dismiss the notification-permission rationale (persisting that we've asked so
/// it never re-prompts). When `enable`, kick off the native POST_NOTIFICATIONS
/// request — the OS prompt now appears with the user already knowing why.
fn dismiss_notif_rationale(state: &AppState, enable: bool) {
    state.dialogs.notif_rationale().set(false);
    crate::audio::microphone::mark_notif_asked(state);
    if enable {
        let st = *state;
        wasm_bindgen_futures::spawn_local(async move {
            crate::audio::microphone::request_notification_permission(&st).await;
        });
    }
}

/// Current effective sample rate for the Resonators readouts — the live
/// waterfall's rate when mic is active, otherwise the current file's, with a
/// sensible default so the label stays populated with no file loaded.
fn resonator_quick_sample_rate(state: AppState) -> f64 {
    if crate::canvas::live_waterfall::is_active() {
        return crate::canvas::live_waterfall::max_freq() * 2.0;
    }
    let files = state.library.files().get();
    let idx = state.library.current_index().get();
    idx.and_then(|i| files.get(i))
        .map(|f| f.spectrogram.sample_rate as f64)
        .unwrap_or(192_000.0)
}

fn layer_opt_class(active: bool) -> &'static str {
    if active {
        "layer-panel-opt sel"
    } else {
        "layer-panel-opt"
    }
}

/// Turn the XForm display-processing panel on: enable it with all per-stage
/// filters defaulting to "Same" (mirror current playback), and eagerly resolve
/// the display_* signals so the spectrogram render Effect sees them immediately
/// (rather than waiting a tick for the resolve Effect).
pub fn enable_xform(state: &AppState) {
    state.display.xform_enabled().set(true);
    state.display.filter_eq().set(DisplayFilterMode::Same);
    state.display.filter_notch().set(DisplayFilterMode::Same);
    state.display.filter_nr().set(DisplayFilterMode::Same);
    state
        .display
        .filter_transform()
        .set(DisplayFilterMode::Same);
    state.display.filter_gain().set(DisplayFilterMode::Same);
    state.display.filter_decimate().set(DisplayFilterMode::Same);
    // Eagerly resolve "Same" → mirror current playback state.
    state
        .display
        .eq()
        .set(state.filter.enabled().get_untracked());
    state
        .display
        .noise_filter()
        .set(state.noise_reduce.enabled().get_untracked() || state.notch.enabled().get_untracked());
    state
        .display
        .transform()
        .set(state.playback.mode().get_untracked() != PlaybackMode::Normal);
}

/// Turn the XForm panel off and reset all derived display_* signals so the
/// spectrogram render Effect sees a consistent "no display processing" state.
pub fn disable_xform(state: &AppState) {
    state.display.xform_enabled().set(false);
    state.display.transform().set(false);
    state.display.eq().set(false);
    state.display.noise_filter().set(false);
    state.display.auto_gain().set(false);
    state.display.gain_boost().set(0.0);
    state.display.decimate_effective().set(0);
}

/// Split-button: click cycles Spec/Wave, down-arrow opens a dropdown with all
/// view modes + DSP settings. Rendered in the bottom toolbar.
#[component]
pub fn MainViewButton() -> impl IntoView {
    use crate::components::popup::{Align, PopupPanel, Side};
    let state = expect_context::<AppState>();
    let is_open =
        Signal::derive(move || state.panels.layer_panel_open().get() == Some(LayerPanel::MainView));
    let no_file = move || {
        state.library.current_index().get().is_none() && state.timeline.active().get().is_none()
    };

    // Helper: handle all side-effects of a view switch synchronously,
    // so the spectrogram render Effect always sees consistent state.
    let switch_view = move |new_view: MainView| {
        let old_view = state.viewmode.main_view().get_untracked();
        if new_view == old_view {
            return;
        }
        // XForm is an independent toggle now (see XformButton) — the view
        // switch only changes which renderer is shown. The xform_enabled
        // state persists across view changes; it's gated to spectro-like
        // views by the XForm button being inert elsewhere.
        state.viewmode.main_view().set(new_view);
    };

    // Click on a non-selected view: switch to it. Click on the
    // already-selected view: toggle its settings popup. Mirrors the
    // playback mode radio group's "click again to open settings".
    let select_view = move |new_view: MainView| {
        if no_file() {
            return;
        }
        let cur = state.viewmode.main_view().get_untracked();
        if cur == new_view {
            toggle_panel(&state, LayerPanel::MainView);
            return;
        }
        switch_view(new_view);
        state.panels.layer_panel_open().set(None);
    };

    let row_ref = NodeRef::<leptos::html::Div>::new();

    // Per-view-button class (radio-group look, reused from .mode-radio-*).
    let view_btn_class = move |view: MainView| {
        Signal::derive(move || {
            let is_sel = state.viewmode.main_view().get() == view;
            let mut s = String::from("layer-btn mode-radio-btn");
            if is_sel {
                s.push_str(" selected has-settings");
                if is_open.get() {
                    s.push_str(" open");
                }
            }
            if no_file() {
                s.push_str(" disabled");
            }
            s
        })
    };

    view! {
        // Reuses .mode-radio-group styling for visual consistency with the
        // playback Mode radio group on the Hearing Bar.
        <div node_ref=row_ref class="mode-radio-group view-radio-group"
            on:click=|ev: web_sys::MouseEvent| ev.stop_propagation()
            on:touchstart=|ev: web_sys::TouchEvent| ev.stop_propagation()
        >
            {move || {
                // ZC files store dot-plot data; the synthesised waveform
                // is only a reconstruction, so hide views whose DSP would
                // measure the synth rather than the recording.
                let is_zc = state.current_is_zc();
                MainView::ALL.iter()
                    .filter(move |m| !is_zc || m.is_sensible_for_zc())
                    .map(|&view| {
                        let class_sig = view_btn_class(view);
                        let title = view.label();
                        view! {
                            <button class=move || class_sig.get()
                                title=title
                                on:click=move |_: web_sys::MouseEvent| select_view(view)
                            >
                                <span class="mode-radio-label">{view.short_label()}</span>
                                <span class="mode-radio-corner">{"\u{25E2}"}</span>
                            </button>
                        }
                    }).collect_view()
            }}

            <PopupPanel
                is_open=is_open
                anchor=row_ref
                preferred_side=Side::Below
                preferred_align=Align::Start
                extra_style="min-width: 240px;"
            >
                <div class="layer-panel-title">{move || state.viewmode.main_view().get().label()}</div>

            // Waveform sub-view (when Waveform is the active main view)
            {move || (state.viewmode.main_view().get() == MainView::Waveform).then(|| {
                view! {
                    <hr />
                    <div class="layer-panel-title">"Waveform Mode"</div>
                    {WaveformView::ALL.iter().map(|&wv| {
                        view! {
                            <button
                                class=move || layer_opt_class(state.viewmode.waveform_view().get() == wv)
                                on:click=move |_: web_sys::MouseEvent| {
                                    state.viewmode.waveform_view().set(wv);
                                }
                            >
                                {wv.label()}
                            </button>
                        }
                    }).collect_view()}
                }
            })}

            // Reassignment checkbox (spectrogram view)
            {move || (state.viewmode.main_view().get() == MainView::Spectrogram).then(|| {
                view! {
                    <hr />
                    <label style="display:flex;align-items:center;gap:4px;cursor:pointer;padding:4px 8px;font-size:12px;"
                        title="Sharpen time-frequency localization using the reassignment method (3x FFT cost)">
                        <input
                            type="checkbox"
                            prop:checked=move || state.spect.reassign_enabled().get()
                            on:change=move |ev: web_sys::Event| {
                                let target = ev.target().unwrap();
                                let input: web_sys::HtmlInputElement = target.unchecked_into();
                                state.spect.reassign_enabled().set(input.checked());
                            }
                        />
                        "Reassignment"
                    </label>
                }
            })}

            // Resonator quick controls (shown only when Resonators view is active)
            {move || (state.viewmode.main_view().get() == MainView::Resonators).then(|| {
                view! {
                    <hr />
                    <div class="layer-panel-title">"Resonators"</div>
                    <div class="dsp-custom-section">
                        <div class="dsp-custom-title">{move || {
                            let bw = state.resonator.bandwidth_hz().get().max(0.001);
                            let tau_ms = 1000.0 / (2.0 * std::f32::consts::PI * bw);
                            let bw_str = if bw < 10.0 {
                                format!("{:.1}", bw)
                            } else {
                                format!("{:.0}", bw.round())
                            };
                            format!("Bandwidth: {} Hz (\u{03c4} \u{2248} {:.1} ms)", bw_str, tau_ms)
                        }}</div>
                        <div class="dsp-custom-slider-row">
                            // Log-scale slider: 0..RESONATOR_BW_SLIDER_MAX ↦ 1..2000 Hz.
                            <input
                                type="range"
                                class="setting-range"
                                min="0"
                                max=RESONATOR_BW_SLIDER_MAX.to_string()
                                step="1"
                                prop:value=move || {
                                    resonator_bw_to_slider(state.resonator.bandwidth_hz().get())
                                        .round()
                                        .to_string()
                                }
                                on:input=move |ev: web_sys::Event| {
                                    let target = ev.target().unwrap();
                                    let input: web_sys::HtmlInputElement = target.unchecked_into();
                                    if let Ok(pos) = input.value().parse::<f32>() {
                                        state.resonator.bandwidth_hz().set(resonator_slider_to_bw(pos));
                                    }
                                }
                                on:dblclick=move |_| state.resonator.bandwidth_hz().set(20.0)
                            />
                        </div>
                    </div>
                    <div class="setting-row" style="padding: 4px 8px;">
                        <span class="setting-label">{move || {
                            let mode = state.resonator.fft_mode().get();
                            let sr = resonator_quick_sample_rate(state);
                            let current_lod = crate::canvas::tile_cache::select_lod(
                                state.view.zoom_level().get(),
                            );
                            let f = mode.fft_for_lod(current_lod).max(2);
                            let spacing = sr / f as f64;
                            let spacing_str = if spacing >= 1000.0 {
                                format!("{:.2} kHz", spacing / 1000.0)
                            } else if spacing >= 100.0 {
                                format!("{:.0} Hz", spacing)
                            } else {
                                format!("{:.1} Hz", spacing)
                            };
                            format!("Bins ({}/bin)", spacing_str)
                        }}</span>
                        <select
                            class="setting-select"
                            on:change=move |ev: web_sys::Event| {
                                let target = ev.target().unwrap();
                                let select: web_sys::HtmlSelectElement = target.unchecked_into();
                                let v = select.value();
                                let new_mode = match v.as_str() {
                                    "adaptive" => ResonatorFftMode::Adaptive(ResonatorDensity::Full),
                                    "adaptive-50" => ResonatorFftMode::Adaptive(ResonatorDensity::Half),
                                    "adaptive-25" => ResonatorFftMode::Adaptive(ResonatorDensity::Quarter),
                                    _ => match v.parse::<usize>() {
                                        Ok(sz) => ResonatorFftMode::Single(sz.max(16)),
                                        Err(_) => return,
                                    },
                                };
                                state.resonator.fft_mode().set(new_mode);
                            }
                            prop:value=move || match state.resonator.fft_mode().get() {
                                ResonatorFftMode::Adaptive(d) => d.as_value().to_string(),
                                ResonatorFftMode::Single(sz) => sz.to_string(),
                            }
                        >
                            <option value="adaptive">"Adaptive 100%"</option>
                            <option value="adaptive-50">"Adaptive 50%"</option>
                            <option value="adaptive-25">"Adaptive 25%"</option>
                            <option value="64">"33"</option>
                            <option value="128">"65"</option>
                            <option value="256">"129"</option>
                            <option value="512">"257"</option>
                            <option value="1024">"513"</option>
                        </select>
                    </div>
                    <div class="setting-row" style="padding: 4px 8px;">
                        <span class="setting-label">"Layout"</span>
                        <select
                            class="setting-select"
                            on:change=move |ev: web_sys::Event| {
                                let target = ev.target().unwrap();
                                let select: web_sys::HtmlSelectElement = target.unchecked_into();
                                let new_layout = match select.value().as_str() {
                                    "log" => ResonatorLayout::Log,
                                    _ => ResonatorLayout::Linear,
                                };
                                state.resonator.layout().set(new_layout);
                            }
                            prop:value=move || match state.resonator.layout().get() {
                                ResonatorLayout::Linear => "linear",
                                ResonatorLayout::Log => "log",
                            }
                        >
                            <option value="linear">"Linear"</option>
                            <option value="log">"Log"</option>
                        </select>
                    </div>
                    <div class="setting-row" style="padding: 4px 8px;">
                        <span class="setting-label"
                            title="Const bandwidth: one bandwidth for every bin (uniform; uses the Bandwidth slider above). Const Q: bw = f / Q — sharp at low freq, fast at high freq (suits FM bat calls; uses the Q slider).">
                            "Alpha"
                        </span>
                        <select
                            class="setting-select"
                            on:change=move |ev: web_sys::Event| {
                                let target = ev.target().unwrap();
                                let select: web_sys::HtmlSelectElement = target.unchecked_into();
                                let new_mode = match select.value().as_str() {
                                    "constq" => ResonatorAlphaMode::ConstQ,
                                    _ => ResonatorAlphaMode::ConstBandwidth,
                                };
                                state.resonator.alpha_mode().set(new_mode);
                            }
                            prop:value=move || match state.resonator.alpha_mode().get() {
                                ResonatorAlphaMode::ConstBandwidth => "constbw",
                                ResonatorAlphaMode::ConstQ => "constq",
                            }
                        >
                            <option value="constbw">"Const bandwidth"</option>
                            <option value="constq">"Const Q"</option>
                        </select>
                    </div>
                    <div class="dsp-custom-section">
                        <div class="dsp-custom-title">{move || {
                            let q = state.resonator.q().get().max(0.1);
                            format!("Q: {:.0} (bw \u{2248} {:.0} Hz @ 40 kHz)", q, 40_000.0 / q)
                        }}</div>
                        <div class="dsp-custom-slider-row">
                            // Linear Q slider; only affects the Const Q schedule.
                            <input
                                type="range"
                                class="setting-range"
                                min="10"
                                max="1000"
                                step="5"
                                prop:value=move || state.resonator.q().get().round().to_string()
                                on:input=move |ev: web_sys::Event| {
                                    let target = ev.target().unwrap();
                                    let input: web_sys::HtmlInputElement = target.unchecked_into();
                                    if let Ok(v) = input.value().parse::<f32>() {
                                        state.resonator.q().set(v.max(1.0));
                                    }
                                }
                                on:dblclick=move |_| state.resonator.q().set(200.0)
                            />
                        </div>
                    </div>
                    <div class="setting-row" style="padding: 4px 8px;">
                        <span class="setting-label"
                            title="FFT-steered hybrid (files & live). Clean: gate the resonator with an FFT to remove its leakage skirts (crisp line, clean background). Adaptive: blend a sharp + fast bank by FFT tonalness — sharp on sustained tones, crisp on transients (≈2× cost live). Off: plain resonator.">
                            "Hybrid"
                        </span>
                        <select
                            class="setting-select"
                            on:change=move |ev: web_sys::Event| {
                                let target = ev.target().unwrap();
                                let select: web_sys::HtmlSelectElement = target.unchecked_into();
                                let m = match select.value().as_str() {
                                    "clean" => ResonatorHybridMode::Clean,
                                    "adaptive" => ResonatorHybridMode::Adaptive,
                                    _ => ResonatorHybridMode::Off,
                                };
                                state.resonator.hybrid_mode().set(m);
                            }
                            prop:value=move || match state.resonator.hybrid_mode().get() {
                                ResonatorHybridMode::Off => "off",
                                ResonatorHybridMode::Clean => "clean",
                                ResonatorHybridMode::Adaptive => "adaptive",
                            }
                        >
                            <option value="off">"Off"</option>
                            <option value="clean">"Clean (de-leak)"</option>
                            <option value="adaptive">"Adaptive (sharp+fast)"</option>
                        </select>
                    </div>
                    <label style="display:flex;align-items:center;gap:4px;cursor:pointer;padding:4px 8px;font-size:12px;"
                        title="Concentrate all bins on the visible freq range for finer vertical zoom. Rebuilds ~0.5s after you stop zooming vertically.">
                        <input
                            type="checkbox"
                            prop:checked=move || state.resonator.viewport_bins().get()
                            on:change=move |ev: web_sys::Event| {
                                let target = ev.target().unwrap();
                                let input: web_sys::HtmlInputElement = target.unchecked_into();
                                state.resonator.viewport_bins().set(input.checked());
                            }
                        />
                        "Viewport zoom"
                    </label>
                    <label style="display:flex;align-items:center;gap:4px;cursor:pointer;padding:4px 8px;font-size:12px;"
                        title="Live recording/listening only: automatically lower the resonator density when the device can't keep up (and raise it back when it can), capped at the density chosen above. Keeps the live waterfall smooth at high sample rates.">
                        <input
                            type="checkbox"
                            prop:checked=move || state.resonator.auto_quality().get()
                            on:change=move |ev: web_sys::Event| {
                                let target = ev.target().unwrap();
                                let input: web_sys::HtmlInputElement = target.unchecked_into();
                                state.resonator.auto_quality().set(input.checked());
                            }
                        />
                        "Auto quality (fps, live)"
                    </label>
                }
            })}

            // FFT size selector (when any spectrogram/flow view is active)
            {move || matches!(state.viewmode.main_view().get(), MainView::Spectrogram | MainView::Flow).then(|| {
                view! {
                    <hr />
                    <div class="layer-panel-title">"FFT Size"</div>
                    <select
                        class="setting-select"
                        style="margin: 4px 8px; width: calc(100% - 16px);"
                        on:change=move |ev: web_sys::Event| {
                            let target = ev.target().unwrap();
                            let select: web_sys::HtmlSelectElement = target.unchecked_into();
                            let val = select.value();
                            let mode = match val.as_str() {
                                "ax" => FftMode::AdaptiveXS,
                                "as" => FftMode::AdaptiveS,
                                "am" => FftMode::AdaptiveM,
                                "al" => FftMode::AdaptiveL,
                                _ => {
                                    if let Ok(v) = val.parse::<usize>() {
                                        FftMode::Single(v)
                                    } else {
                                        return;
                                    }
                                }
                            };
                            state.spect.fft_mode().set(mode);
                        }
                    >
                        {move || {
                            let current = state.spect.fft_mode().get();
                            let options: [(&str, &str); 13] = [
                                ("ax", "Adaptive XS"),
                                ("as", "Adaptive S"),
                                ("am", "Adaptive M"),
                                ("al", "Adaptive L"),
                                ("32", "32"),
                                ("64", "64"),
                                ("128", "128"),
                                ("256", "256"),
                                ("512", "512"),
                                ("1024", "1024"),
                                ("2048", "2048"),
                                ("4096", "4096"),
                                ("8192", "8192"),
                            ];
                            options.into_iter().map(|(value, label)| {
                                let is_selected = match (value, current) {
                                    ("ax", FftMode::AdaptiveXS) => true,
                                    ("as", FftMode::AdaptiveS) => true,
                                    ("am", FftMode::AdaptiveM) => true,
                                    ("al", FftMode::AdaptiveL) => true,
                                    (v, FftMode::Single(sz)) => v.parse::<usize>().ok() == Some(sz),
                                    _ => false,
                                };
                                let v = value.to_string();
                                let l = label.to_string();
                                view! { <option value={v} selected=move || is_selected>{l}</option> }
                            }).collect::<Vec<_>>()
                        }}
                    </select>
                }
            })}

            // Frequency range selector (for spectrogram/flow/resonator views)
            {move || matches!(state.viewmode.main_view().get(), MainView::Spectrogram | MainView::Flow | MainView::Resonators).then(|| {
                let file_max = move || {
                    let files = state.library.files().get();
                    let idx = state.library.current_index().get();
                    idx.and_then(|i| files.get(i))
                        .map(|f| f.spectrogram.max_freq)
                        .unwrap_or(96_000.0)
                };
                let is_range = move |lo: Option<f64>, hi: Option<f64>| -> bool {
                    let cur_min = state.view.min_display_freq().get();
                    let cur_max = state.view.max_display_freq().get();
                    match (lo, hi) {
                        (None, None) => {
                            let fm = file_max();
                            (cur_min.is_none() || cur_min == Some(0.0))
                                && (cur_max.is_none() || cur_max.is_some_and(|m| (m - fm).abs() < 100.0))
                        }
                        (_, Some(h)) => cur_max.is_some_and(|m| (m - h).abs() < 100.0)
                            && (lo.is_none() || cur_min.is_none() || cur_min == lo),
                        _ => false,
                    }
                };
                view! {
                    <hr />
                    <div class="layer-panel-title">"Freq Range"</div>
                    <select
                        class="setting-select"
                        style="margin: 4px 8px; width: calc(100% - 16px);"
                        on:change=move |ev: web_sys::Event| {
                            let target = ev.target().unwrap();
                            let select: web_sys::HtmlSelectElement = target.unchecked_into();
                            let val = select.value();
                            match val.as_str() {
                                "full" => {
                                    state.view.min_display_freq().set(None);
                                    state.view.max_display_freq().set(None);
                                }
                                "22k" => {
                                    state.view.min_display_freq().set(Some(0.0));
                                    state.view.max_display_freq().set(Some(22_000.0));
                                }
                                "50k" => {
                                    state.view.min_display_freq().set(Some(0.0));
                                    state.view.max_display_freq().set(Some(50_000.0));
                                }
                                "100k" => {
                                    state.view.min_display_freq().set(Some(0.0));
                                    state.view.max_display_freq().set(Some(100_000.0));
                                }
                                "192k" => {
                                    state.view.min_display_freq().set(Some(0.0));
                                    state.view.max_display_freq().set(Some(192_000.0));
                                }
                                _ => {}
                            }
                        }
                    >
                        {move || {
                            let options: [(&str, &str, Option<f64>, Option<f64>); 5] = [
                                ("full", "Full", None, None),
                                ("22k", "0 \u{2013} 22 kHz", Some(0.0), Some(22_000.0)),
                                ("50k", "0 \u{2013} 50 kHz", Some(0.0), Some(50_000.0)),
                                ("100k", "0 \u{2013} 100 kHz", Some(0.0), Some(100_000.0)),
                                ("192k", "0 \u{2013} 192 kHz", Some(0.0), Some(192_000.0)),
                            ];
                            options.into_iter().map(|(value, label, lo, hi)| {
                                let selected = is_range(lo, hi);
                                let v = value.to_string();
                                let l = label.to_string();
                                view! { <option value={v} selected=move || selected>{l}</option> }
                            }).collect::<Vec<_>>()
                        }}
                    </select>
                }
            })}

            // Intensity sliders (for Spectrogram, Flow, or Resonators). These
            // are spectrogram settings — they also drive the XForm view, which
            // shares the Spectrogram menu rather than duplicating these.
            {move || matches!(state.viewmode.main_view().get(), MainView::Spectrogram | MainView::Flow | MainView::Resonators).then(|| {
                let gain_get = move || state.spect.gain_db().get();
                let gain_set = move |v: f32| state.spect.gain_db().set(v);
                let range_get = move || state.spect.range_db().get();
                let range_set = move |v: f32| state.spect.range_db().set(v);
                let floor_set = move |v: f32| state.spect.floor_db().set(v);
                let gamma_get = move || state.spect.gamma().get();
                let gamma_set = move |v: f32| state.spect.gamma().set(v);
                view! {
                    <hr />
                    <div class="dsp-custom-section">
                        <div class="dsp-custom-title">"Intensity"</div>
                        <div class="dsp-custom-slider-row">
                            <span class="dsp-slider-label">"Gain"</span>
                            <input
                                type="range"
                                class="setting-range"
                                min="-40" max="40" step="1"
                                prop:value=move || gain_get().to_string()
                                on:input=move |ev: web_sys::Event| {
                                    let target = ev.target().unwrap();
                                    let input: web_sys::HtmlInputElement = target.unchecked_into();
                                    if let Ok(v) = input.value().parse::<f32>() {
                                        gain_set(v);
                                        state.display.auto_gain().set(false);
                                    }
                                }
                                on:dblclick=move |_| gain_set(0.0)
                            />
                            <span class="dsp-custom-value">{move || format!("{:+.0} dB", gain_get())}</span>
                        </div>
                        <div class="dsp-custom-slider-row">
                            <span class="dsp-slider-label">"Range"</span>
                            <input
                                type="range"
                                class="setting-range"
                                min="20" max="120" step="5"
                                prop:value=move || range_get().to_string()
                                on:input=move |ev: web_sys::Event| {
                                    let target = ev.target().unwrap();
                                    let input: web_sys::HtmlInputElement = target.unchecked_into();
                                    if let Ok(v) = input.value().parse::<f32>() {
                                        range_set(v);
                                        floor_set(-v);
                                    }
                                }
                                on:dblclick=move |_| {
                                    range_set(120.0);
                                    floor_set(-120.0);
                                }
                            />
                            <span class="dsp-custom-value">{move || format!("{:.0} dB", range_get())}</span>
                        </div>
                        <div class="dsp-custom-slider-row">
                            <span class="dsp-slider-label">"Contrast"</span>
                            <input
                                type="range"
                                class="setting-range"
                                min="0.2" max="3.0" step="0.05"
                                prop:value=move || gamma_get().to_string()
                                on:input=move |ev: web_sys::Event| {
                                    let target = ev.target().unwrap();
                                    let input: web_sys::HtmlInputElement = target.unchecked_into();
                                    if let Ok(v) = input.value().parse::<f32>() {
                                        gamma_set(v);
                                    }
                                }
                                on:dblclick=move |_| gamma_set(1.0)
                            />
                            <span class="dsp-custom-value">{move || {
                                let g = gamma_get();
                                if g == 1.0 { "linear".to_string() } else { format!("{:.2}", g) }
                            }}</span>
                        </div>
                        <div style="text-align: right; padding-top: 4px;">
                            <button
                                class="layer-panel-opt"
                                style="display: inline; width: auto; padding: 2px 8px; font-size: 9px;"
                                on:click=move |_| {
                                    gain_set(0.0);
                                    floor_set(-120.0);
                                    range_set(120.0);
                                    gamma_set(1.0);
                                    state.display.auto_gain().set(false);
                                }
                            >"Reset"</button>
                        </div>
                    </div>
                }
            })}

            // Waveform view gain (when Waveform is active)
            {move || (state.viewmode.main_view().get() == MainView::Waveform).then(|| {
                view! {
                    <hr />
                    <div class="dsp-custom-section">
                        <div class="dsp-custom-title">"Waveform Gain"</div>
                        <div class="dsp-custom-slider-row">
                            <button
                                class=move || if state.gain.wave_view_auto().get() {
                                    "layer-panel-opt selected"
                                } else {
                                    "layer-panel-opt"
                                }
                                style="font-size: 9px; padding: 2px 6px; width: auto; display: inline;"
                                on:click=move |_| {
                                    state.gain.wave_view_auto().set(!state.gain.wave_view_auto().get_untracked());
                                }
                            >"Auto"</button>
                        </div>
                        <div class="dsp-custom-slider-row">
                            <span class="dsp-slider-label">"Gain"</span>
                            <input
                                type="range"
                                class="setting-range"
                                min="-12" max="60" step="1"
                                prop:value=move || if state.gain.wave_view_auto().get() {
                                    state.compute_auto_gain().to_string()
                                } else {
                                    state.gain.wave_view_db().get().to_string()
                                }
                                on:input=move |ev: web_sys::Event| {
                                    let target = ev.target().unwrap();
                                    let input: web_sys::HtmlInputElement = target.unchecked_into();
                                    if let Ok(v) = input.value().parse::<f64>() {
                                        state.gain.wave_view_db().set(v);
                                        state.gain.wave_view_auto().set(false);
                                    }
                                }
                                on:dblclick=move |_| {
                                    state.gain.wave_view_db().set(0.0);
                                    state.gain.wave_view_auto().set(false);
                                }
                            />
                            <span class="dsp-custom-value">{move || {
                                if state.gain.wave_view_auto().get() {
                                    let db = state.compute_auto_gain();
                                    if db.abs() < 0.5 { "auto".to_string() }
                                    else { format!("a{:+.0}", db) }
                                } else {
                                    format!("{:+.0} dB", state.gain.wave_view_db().get())
                                }
                            }}</span>
                        </div>
                    </div>
                }
            })}

            // Flow algorithm options (when Flow is active)
            {move || (state.viewmode.main_view().get() == MainView::Flow).then(|| {
                view! {
                    <hr />
                    <div class="layer-panel-title">"Algorithm"</div>
                    <button
                        class=move || layer_opt_class(state.spect.display().get() == SpectrogramDisplay::FlowOptical)
                        on:click=move |_| state.spect.display().set(SpectrogramDisplay::FlowOptical)
                    >"Optical"</button>
                    <button
                        class=move || layer_opt_class(state.spect.display().get() == SpectrogramDisplay::PhaseCoherence)
                        on:click=move |_| state.spect.display().set(SpectrogramDisplay::PhaseCoherence)
                    >"Phase Coherence"</button>
                    <button
                        class=move || layer_opt_class(state.spect.display().get() == SpectrogramDisplay::FlowCentroid)
                        on:click=move |_| state.spect.display().set(SpectrogramDisplay::FlowCentroid)
                    >"Centroid"</button>
                    <button
                        class=move || layer_opt_class(state.spect.display().get() == SpectrogramDisplay::FlowGradient)
                        on:click=move |_| state.spect.display().set(SpectrogramDisplay::FlowGradient)
                    >"Gradient"</button>
                    <button
                        class=move || layer_opt_class(state.spect.display().get() == SpectrogramDisplay::Phase)
                        on:click=move |_| state.spect.display().set(SpectrogramDisplay::Phase)
                    >"Phase"</button>

                    // Color scheme (only for non-phase flow algorithms)
                    {move || {
                        let display = state.spect.display().get();
                        matches!(display,
                            SpectrogramDisplay::FlowOptical |
                            SpectrogramDisplay::FlowCentroid |
                            SpectrogramDisplay::FlowGradient
                        ).then(|| view! {
                            <hr />
                            <div class="layer-panel-title">"Color Scheme"</div>
                            <select
                                class="setting-select"
                                style="margin: 4px 8px; width: calc(100% - 16px);"
                                on:change=move |ev: web_sys::Event| {
                                    let target = ev.target().unwrap();
                                    let select: web_sys::HtmlSelectElement = target.unchecked_into();
                                    let scheme = match select.value().as_str() {
                                        "coolwarm" => FlowColorScheme::CoolWarm,
                                        "tealorange" => FlowColorScheme::TealOrange,
                                        "purplegreen" => FlowColorScheme::PurpleGreen,
                                        "spectral" => FlowColorScheme::Spectral,
                                        _ => FlowColorScheme::RedBlue,
                                    };
                                    state.flow.color_scheme().set(scheme);
                                }
                                prop:value=move || match state.flow.color_scheme().get() {
                                    FlowColorScheme::RedBlue => "redblue",
                                    FlowColorScheme::CoolWarm => "coolwarm",
                                    FlowColorScheme::TealOrange => "tealorange",
                                    FlowColorScheme::PurpleGreen => "purplegreen",
                                    FlowColorScheme::Spectral => "spectral",
                                }
                            >
                                <option value="redblue">"Red-Blue"</option>
                                <option value="coolwarm">"Cool-Warm"</option>
                                <option value="tealorange">"Teal-Orange"</option>
                                <option value="purplegreen">"Purple-Green"</option>
                                <option value="spectral">"Spectral"</option>
                            </select>
                        })
                    }}

                    // Flow-specific sliders
                    <hr />
                    <div class="dsp-custom-section">
                        <div class="dsp-custom-title">"Flow Controls"</div>
                        <div class="dsp-custom-slider-row">
                            <span class="dsp-slider-label">"Intensity gate"</span>
                            <input
                                type="range"
                                class="setting-range"
                                min="0" max="100" step="1"
                                prop:value=move || (state.flow.intensity_gate().get() * 100.0).round().to_string()
                                on:input=move |ev: web_sys::Event| {
                                    let target = ev.target().unwrap();
                                    let input: web_sys::HtmlInputElement = target.unchecked_into();
                                    if let Ok(v) = input.value().parse::<f32>() {
                                        state.flow.intensity_gate().set(v / 100.0);
                                    }
                                }
                                on:dblclick=move |_| state.flow.intensity_gate().set(0.0)
                            />
                            <span class="dsp-custom-value">{move || format!("{}%", (state.flow.intensity_gate().get() * 100.0).round() as u32)}</span>
                        </div>
                        <div class="dsp-custom-slider-row">
                            <span class="dsp-slider-label">"Flow gate"</span>
                            <input
                                type="range"
                                class="setting-range"
                                min="0" max="100" step="1"
                                prop:value=move || (state.flow.gate().get() * 100.0).round().to_string()
                                on:input=move |ev: web_sys::Event| {
                                    let target = ev.target().unwrap();
                                    let input: web_sys::HtmlInputElement = target.unchecked_into();
                                    if let Ok(v) = input.value().parse::<f32>() {
                                        state.flow.gate().set(v / 100.0);
                                    }
                                }
                                on:dblclick=move |_| state.flow.gate().set(0.0)
                            />
                            <span class="dsp-custom-value">{move || format!("{}%", (state.flow.gate().get() * 100.0).round() as u32)}</span>
                        </div>
                        <div class="dsp-custom-slider-row">
                            <span class="dsp-slider-label">"Color gain"</span>
                            <input
                                type="range"
                                class="setting-range"
                                min="0.5" max="10.0" step="0.5"
                                prop:value=move || state.flow.shift_gain().get().to_string()
                                on:input=move |ev: web_sys::Event| {
                                    let target = ev.target().unwrap();
                                    let input: web_sys::HtmlInputElement = target.unchecked_into();
                                    if let Ok(v) = input.value().parse::<f32>() {
                                        state.flow.shift_gain().set(v);
                                    }
                                }
                                on:dblclick=move |_| state.flow.shift_gain().set(3.0)
                            />
                            <span class="dsp-custom-value">{move || format!("{:.1}x", state.flow.shift_gain().get())}</span>
                        </div>
                        <div class="dsp-custom-slider-row">
                            <span class="dsp-slider-label">"Contrast"</span>
                            <input
                                type="range"
                                class="setting-range"
                                min="0.2" max="3.0" step="0.05"
                                prop:value=move || state.flow.color_gamma().get().to_string()
                                on:input=move |ev: web_sys::Event| {
                                    let target = ev.target().unwrap();
                                    let input: web_sys::HtmlInputElement = target.unchecked_into();
                                    if let Ok(v) = input.value().parse::<f32>() {
                                        state.flow.color_gamma().set(v);
                                    }
                                }
                                on:dblclick=move |_| state.flow.color_gamma().set(1.0)
                            />
                            <span class="dsp-custom-value">{move || {
                                let g = state.flow.color_gamma().get();
                                if g == 1.0 { "linear".to_string() } else { format!("{:.2}", g) }
                            }}</span>
                        </div>
                    </div>
                }
            })}

            // Chromagram options (when Chromagram is active)
            {move || (state.viewmode.main_view().get() == MainView::Chromagram).then(|| {
                view! {
                    <hr />
                    <div class="layer-panel-title">"Colormap"</div>
                    {ChromaColormap::ALL.iter().map(|&mode| {
                        view! {
                            <button
                                class=move || layer_opt_class(state.chroma.colormap().get() == mode)
                                on:click=move |_| state.chroma.colormap().set(mode)
                            >
                                {mode.label()}
                            </button>
                        }
                    }).collect_view()}

                    <hr />
                    <div class="layer-panel-title">"Freq Range"</div>
                    {ChromaRange::ALL.iter().map(|&range| {
                        view! {
                            <button
                                class=move || layer_opt_class(state.chroma.range().get() == range)
                                on:click=move |_| state.chroma.range().set(range)
                            >
                                {range.label()}
                            </button>
                        }
                    }).collect_view()}

                    <hr />
                    <div class="layer-panel-title">"Source"</div>
                    {ChromaSource::ALL.iter().map(|&src| {
                        view! {
                            <button
                                class=move || layer_opt_class(state.chroma.source().get() == src)
                                on:click=move |_| state.chroma.source().set(src)
                            >
                                {src.label()}
                            </button>
                        }
                    }).collect_view()}

                    <hr />
                    <div class="dsp-custom-section">
                        <div class="dsp-custom-title">"Chromagram Controls"</div>
                        <div class="dsp-custom-slider-row">
                            <span class="dsp-slider-label">"Gain"</span>
                            <input
                                type="range"
                                class="setting-range"
                                min="-20" max="60" step="1"
                                prop:value=move || state.chroma.gain().get().round().to_string()
                                on:input=move |ev: web_sys::Event| {
                                    let target = ev.target().unwrap();
                                    let input: web_sys::HtmlInputElement = target.unchecked_into();
                                    if let Ok(v) = input.value().parse::<f32>() {
                                        state.chroma.gain().set(v);
                                    }
                                }
                                on:dblclick=move |_| state.chroma.gain().set(0.0)
                            />
                            <span class="dsp-custom-value">{move || {
                                let db = state.chroma.gain().get();
                                if db == 0.0 { "0 dB".to_string() } else { format!("{:+.0} dB", db) }
                            }}</span>
                        </div>
                        <div class="dsp-custom-slider-row">
                            <span class="dsp-slider-label">"Contrast"</span>
                            <input
                                type="range"
                                class="setting-range"
                                min="0.2" max="3.0" step="0.05"
                                prop:value=move || state.chroma.gamma().get().to_string()
                                on:input=move |ev: web_sys::Event| {
                                    let target = ev.target().unwrap();
                                    let input: web_sys::HtmlInputElement = target.unchecked_into();
                                    if let Ok(v) = input.value().parse::<f32>() {
                                        state.chroma.gamma().set(v);
                                    }
                                }
                                on:dblclick=move |_| state.chroma.gamma().set(1.0)
                            />
                            <span class="dsp-custom-value">{move || {
                                let g = state.chroma.gamma().get();
                                if g == 1.0 { "linear".to_string() } else { format!("{:.2}", g) }
                            }}</span>
                        </div>
                        <div class="dsp-custom-slider-row">
                            <span class="dsp-slider-label">"Adapt"</span>
                            <input
                                type="range"
                                class="setting-range"
                                min="0" max="100" step="1"
                                prop:value=move || (state.chroma.adapt().get() * 100.0).round().to_string()
                                on:input=move |ev: web_sys::Event| {
                                    let target = ev.target().unwrap();
                                    let input: web_sys::HtmlInputElement = target.unchecked_into();
                                    if let Ok(v) = input.value().parse::<f32>() {
                                        state.chroma.adapt().set((v / 100.0).clamp(0.0, 1.0));
                                    }
                                }
                                on:dblclick=move |_| state.chroma.adapt().set(0.0)
                            />
                            <span class="dsp-custom-value">{move || {
                                let pct = (state.chroma.adapt().get() * 100.0).round() as i32;
                                if pct == 0 { "off".to_string() } else { format!("{}%", pct) }
                            }}</span>
                        </div>
                        <div class="dsp-custom-slider-row">
                            <span class="dsp-slider-label">"Floor"</span>
                            <input
                                type="range"
                                class="setting-range"
                                min="-80" max="0" step="1"
                                prop:value=move || state.chroma.floor_db().get().round().to_string()
                                on:input=move |ev: web_sys::Event| {
                                    let target = ev.target().unwrap();
                                    let input: web_sys::HtmlInputElement = target.unchecked_into();
                                    if let Ok(v) = input.value().parse::<f32>() {
                                        state.chroma.floor_db().set(v.clamp(-80.0, 0.0));
                                    }
                                }
                                on:dblclick=move |_| state.chroma.floor_db().set(-80.0)
                            />
                            <span class="dsp-custom-value">{move || {
                                let db = state.chroma.floor_db().get().round() as i32;
                                if db <= -80 { "off".to_string() } else { format!("{} dB", db) }
                            }}</span>
                        </div>
                    </div>
                }
            })}
            </PopupPanel>
        </div>
    }
}
