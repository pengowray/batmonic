use leptos::prelude::*;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use crate::state::{
    AppState, ChromaColormap, ChromaRange, DisplayFilterMode, FftMode, FileSettings,
    FlowColorScheme, GainMode, LayerPanel, MainView, MicBackend, MicStrategy,
    MicAcquisitionState, PlayStartMode, PlaybackMode, ResonatorFftMode, ResonatorLayout,
    SpectrogramDisplay, WaveformView, RESONATOR_BW_SLIDER_MAX, resonator_bw_to_slider,
    resonator_slider_to_bw,
};
use crate::audio::playback;
use crate::audio::microphone;
use crate::components::file_sidebar::FileSidebar;
use crate::components::right_sidebar::RightSidebar;
use crate::components::spectrogram::Spectrogram;
use crate::components::waveform::Waveform;
use crate::components::toolbar::Toolbar;
use crate::components::analysis_panel::AnalysisPanel;
use crate::components::overview::OverviewPanel;
use crate::components::play_controls::{ToastDisplay, BookmarkPopup};
use crate::components::bottom_toolbar::BottomToolbar;
use crate::components::xc_browser::XcBrowser;
use crate::components::zc_chart::ZcDotChart;
use crate::components::chromagram_view::ChromagramView;
use crate::components::file_sidebar::{fetch_demo_index, load_single_demo};
use crate::components::bat_book_strip::BatBookStrip;
use crate::components::bat_book_ref_panel::BatBookRefPanel;
use crate::components::display_filter_button::DspFilterRow;
use crate::components::annotation_label_editor::AnnotationLabelEditor;
use crate::components::overflow_menu::CanvasOverflowMenus;
use crate::viewport;

#[component]
pub fn App() -> impl IntoView {
    let state = AppState::new();
    provide_context(state);

    // Detect browser's default audio output sample rate
    {
        use web_sys::AudioContext;
        if let Ok(ctx) = AudioContext::new() {
            let rate = ctx.sample_rate() as u32;
            if rate > 0 {
                state.browser_sample_rate.set(rate);
            }
            let _ = ctx.close();
        }
    }

    // Auto-load demo sound from URL hash (e.g. #XC928094)
    if let Some(window) = web_sys::window() {
        if let Ok(hash) = window.location().hash() {
            let trimmed = hash.trim_start_matches('#');
            if trimmed.len() >= 3 && trimmed[..2].eq_ignore_ascii_case("XC") && trimmed[2..].chars().all(|c| c.is_ascii_digit()) {
                let xc_id = trimmed.to_uppercase();
                let load_id = state.loading_start(&xc_id);
                wasm_bindgen_futures::spawn_local(async move {
                    match fetch_demo_index().await {
                        Ok(entries) => {
                            let found = entries.iter().find(|e| {
                                e.filename.to_uppercase().contains(&xc_id)
                            });
                            if let Some(entry) = found {
                                if let Err(e) = load_single_demo(entry, state, load_id).await {
                                    log::error!("Failed to load {}: {}", xc_id, e);
                                    state.show_error_toast(format!("Failed to load {}", xc_id));
                                }
                            } else {
                                state.show_info_toast(format!(
                                    "{} is not available in the demo audio. Only a small selection of recordings are included.",
                                    xc_id
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
            }
        }
    }

    // Startup: check for USB device (delayed to ensure Tauri internals are ready)
    if state.is_tauri && state.mic_strategy.get_untracked() == MicStrategy::Ask {
        wasm_bindgen_futures::spawn_local(async move {
            // Wait 500ms for Tauri plugin system to initialize
            let p = js_sys::Promise::new(&mut |resolve, _| {
                if let Some(w) = web_sys::window() {
                    let _ = w.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 500);
                }
            });
            let _ = wasm_bindgen_futures::JsFuture::from(p).await;
            // Check USB status without requesting permission
            microphone::check_usb_status(&state).await;
            microphone::query_mic_info(&state).await;

            // If no USB found on first try, retry after 2s (device may enumerate slowly)
            if !state.mic_usb_connected.get_untracked() {
                let p = js_sys::Promise::new(&mut |resolve, _| {
                    if let Some(w) = web_sys::window() {
                        let _ = w.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 2000);
                    }
                });
                let _ = wasm_bindgen_futures::JsFuture::from(p).await;
                microphone::check_usb_status(&state).await;
                microphone::query_mic_info(&state).await;
            }
        });
    }

    // Poll for USB device changes every 3 seconds (Tauri only)
    if state.is_tauri {
        wasm_bindgen_futures::spawn_local(async move {
            use crate::tauri_bridge::tauri_invoke;
            let mut was_connected = false;
            loop {
                // Sleep 3 seconds
                let p = js_sys::Promise::new(&mut |resolve, _| {
                    if let Some(w) = web_sys::window() {
                        let _ = w.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 3000);
                    }
                });
                let _ = wasm_bindgen_futures::JsFuture::from(p).await;

                // Skip polling when mic is active (recording/listening)
                if state.mic_listening.get_untracked() || state.mic_recording.get_untracked() {
                    continue;
                }

                // Poll USB status via Kotlin plugin
                let status = match tauri_invoke("plugin:usb-audio|checkUsbStatus",
                    &js_sys::Object::new().into()).await {
                    Ok(v) => v,
                    Err(_) => continue,
                };

                let is_connected = js_sys::Reflect::get(&status, &JsValue::from_str("audioDeviceAttached"))
                    .ok().and_then(|v| v.as_bool()).unwrap_or(false);
                let last_event = js_sys::Reflect::get(&status, &JsValue::from_str("lastEvent"))
                    .ok().and_then(|v| v.as_string());
                let _product_name = js_sys::Reflect::get(&status, &JsValue::from_str("productName"))
                    .ok().and_then(|v| v.as_string()).unwrap_or_else(|| "USB Audio".into());

                // Update USB connected state
                state.mic_usb_connected.set(is_connected);

                // Handle hotplug events
                if let Some(event) = last_event {
                    if event == "attached" && !was_connected {
                        // Wait 500ms for USB device to fully enumerate
                        let p = js_sys::Promise::new(&mut |resolve, _| {
                            if let Some(w) = web_sys::window() {
                                let _ = w.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 500);
                            }
                        });
                        let _ = wasm_bindgen_futures::JsFuture::from(p).await;
                        microphone::check_usb_status(&state).await;
                        microphone::query_mic_info(&state).await;
                    } else if event == "detached" && was_connected {
                        // If we were using USB, clear backend so user is re-prompted
                        if state.mic_backend.get_untracked() == Some(MicBackend::RawUsb) {
                            state.mic_backend.set(None);
                            state.mic_acquisition_state.set(MicAcquisitionState::Idle);
                        }
                        state.show_info_toast("USB mic disconnected");
                        microphone::query_mic_info(&state).await;
                    }
                }

                was_connected = is_connected;
            }
        });
    }

    // Live playback parameter switching: when any playback-relevant signal
    // changes while audio is playing, restart the stream from the current
    // playhead position with fresh parameters.
    {
        let first_run = std::cell::Cell::new(true);
        Effect::new(move |_| {
            // Track all playback-relevant signals (subscribes to changes)
            let _ = state.playback_mode.get();
            let _ = state.te_factor.get();
            let _ = state.ps_factor.get();
            let _ = state.pv_factor.get();
            let _ = state.pv_hq.get();
            let _ = state.zc_factor.get();
            let _ = state.het_frequency.get();
            let _ = state.het_cutoff.get();
            let _ = state.gain_db.get();
            let _ = state.auto_gain.get();
            let _ = state.gain_mode.get();
            let _ = state.filter_enabled.get();
            let _ = state.filter_freq_low.get();
            let _ = state.filter_freq_high.get();
            let _ = state.filter_db_below.get();
            let _ = state.filter_db_selected.get();
            let _ = state.filter_db_harmonics.get();
            let _ = state.filter_db_above.get();
            let _ = state.filter_band_mode.get();
            let _ = state.filter_quality.get();
            let _ = state.bandpass_mode.get();
            let _ = state.channel_view.get();
            let notch_on = state.notch_enabled.get();
            let _ = state.notch_bands.get();
            let noise_on = state.noise_reduce_enabled.get();
            let _ = state.noise_reduce_strength.get();
            let _ = state.noise_reduce_floor.get();
            // Only trigger replay for harmonic suppression when a noise system is active
            if notch_on || noise_on {
                let _ = state.notch_harmonic_suppression.get();
            }

            if first_run.get() {
                first_run.set(false);
                return;
            }

            if state.is_playing.get_untracked() {
                playback::schedule_replay_live(&state);
            }
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
            let dirty = state.project_dirty.get();
            let has_project = state.current_project.with(|p| p.is_some());
            if !dirty || !has_project {
                cancel_autosave_timer();
                return;
            }
            cancel_autosave_timer();
            let cb = wasm_bindgen::closure::Closure::once(move || {
                if state.project_dirty.get_untracked()
                    && state.current_project.with_untracked(|p| p.is_some())
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
        let is_flow = state.main_view.get() == MainView::Flow;
        state.flow_enabled.set(is_flow);
    });

    // Keep scroll valid for the active file/timeline when the viewport, zoom,
    // or target duration changes. Without this, switching to a shorter file or
    // resizing while a non-spectrogram view is active can leave scroll outside
    // the valid data window and the waveform view renders a blank canvas.
    Effect::new(move |_| {
        let scroll = state.scroll_offset.get();
        let zoom = state.zoom_level.get();
        let canvas_w = state.spectrogram_canvas_width.get();
        let from_here_mode = state.play_start_mode.get().uses_from_here();
        let timeline = state.active_timeline.get();
        let files = state.files.get();
        let idx = state.current_file_index.get();

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
        let is_live = state.mic_recording.get_untracked() || state.mic_listening.get_untracked();
        let effective_duration = if is_live && crate::canvas::live_waterfall::is_active() {
            crate::canvas::live_waterfall::total_columns() as f64 * time_res
        } else {
            duration
        };
        let effective_from_here = from_here_mode && !is_live;
        let clamped = viewport::clamp_scroll_for_mode(scroll, effective_duration, visible_time, effective_from_here);
        if (clamped - scroll).abs() > f64::EPSILON {
            state.scroll_offset.set(clamped);
        }
    });

    // Sync focus_stack → band_ff_freq_lo/hi + hfr_enabled output signals.
    // This keeps downstream Effects (B, C, D in hfr_button) working unchanged.
    Effect::new(move |_| {
        let stack = state.focus_stack.get();
        let eff = stack.effective_range();
        let hfr = stack.hfr_enabled();
        if state.band_ff_freq_lo.get_untracked() != eff.lo {
            state.band_ff_freq_lo.set(eff.lo);
        }
        if state.band_ff_freq_hi.get_untracked() != eff.hi {
            state.band_ff_freq_hi.set(eff.hi);
        }
        if state.hfr_enabled.get_untracked() != hfr {
            state.hfr_enabled.set(hfr);
        }
    });

    // Keep annotation-driven BandFF in sync regardless of whether selection happened
    // from the sidebar or directly on the canvas.
    Effect::new(move |_| {
        let _ = state.current_file_index.get();
        let _ = state.annotation_auto_focus.get();
        let _ = state.selected_annotation_ids.get();
        let _ = state.annotation_store.get();
        state.sync_annotation_auto_focus();
    });

    // Per-file vertical zoom: on file switch, load the new file's stored
    // min/max_display_freq into the global signals. New files have None,
    // so they default to full range rather than inheriting the previous file.
    Effect::new(move |_| {
        let idx = state.current_file_index.get();
        let (min, max) = if let Some(i) = idx {
            state.files.with_untracked(|files| {
                files.get(i)
                    .map(|f| (f.min_display_freq, f.max_display_freq))
                    .unwrap_or((None, None))
            })
        } else {
            (None, None)
        };
        if state.min_display_freq.get_untracked() != min {
            state.min_display_freq.set(min);
        }
        if state.max_display_freq.get_untracked() != max {
            state.max_display_freq.set(max);
        }
    });

    // Persist vertical zoom back to the current file whenever it changes.
    Effect::new(move |_| {
        let min = state.min_display_freq.get();
        let max = state.max_display_freq.get();
        let idx = state.current_file_index.get_untracked();
        if let Some(i) = idx {
            let needs_write = state.files.with_untracked(|files| {
                files.get(i)
                    .map(|f| f.min_display_freq != min || f.max_display_freq != max)
                    .unwrap_or(false)
            });
            if needs_write {
                state.files.update(|files| {
                    if let Some(f) = files.get_mut(i) {
                        f.min_display_freq = min;
                        f.max_display_freq = max;
                    }
                });
            }
        }
    });

    // Resolve display filter modes → effective display_* booleans.
    // When the DSP panel is enabled, the per-stage modes drive the existing
    // display_auto_gain / display_eq / display_noise_filter signals.
    Effect::new(move |_| {
        let enabled = state.display_filter_enabled.get();
        if !enabled {
            // Reset all display processing signals when DSP is off
            state.display_transform.set(false);
            state.display_eq.set(false);
            state.display_noise_filter.set(false);
            state.display_auto_gain.set(false);
            state.display_gain_boost.set(0.0);
            state.display_decimate_effective.set(0);
            return;
        }

        // EQ
        let eq_on = match state.display_filter_eq.get() {
            DisplayFilterMode::Off => false,
            DisplayFilterMode::Auto => state.filter_enabled.get(), // auto = show if playback EQ is on
            DisplayFilterMode::Same => state.filter_enabled.get(),
            DisplayFilterMode::Custom => false, // not yet implemented
        };
        state.display_eq.set(eq_on);

        // Noise (notch + spectral subtraction)
        let nr_on = match state.display_filter_nr.get() {
            DisplayFilterMode::Off => false,
            DisplayFilterMode::Auto | DisplayFilterMode::Custom => true,
            DisplayFilterMode::Same => state.noise_reduce_enabled.get(),
        };
        // Also consider notch
        let notch_on = match state.display_filter_notch.get() {
            DisplayFilterMode::Off => false,
            DisplayFilterMode::Auto => state.notch_enabled.get(), // auto = show if playback notch is on
            DisplayFilterMode::Same => state.notch_enabled.get(),
            DisplayFilterMode::Custom => false,
        };
        state.display_noise_filter.set(nr_on || notch_on);

        // Transform
        let xform_on = match state.display_filter_transform.get() {
            DisplayFilterMode::Off => false,
            DisplayFilterMode::Auto => false, // auto = off for transform
            DisplayFilterMode::Same => state.playback_mode.get() != PlaybackMode::Normal,
            DisplayFilterMode::Custom => false, // not yet implemented
        };
        state.display_transform.set(xform_on);

        // Gain — compute display_gain_boost (dB) and display_auto_gain
        let gain_filter = state.display_filter_gain.get();
        let (gain_auto, boost) = match gain_filter {
            DisplayFilterMode::Off => (false, 0.0),
            DisplayFilterMode::Auto => {
                // Peak-normalize: boost quiet files so peak → −3 dBFS
                let auto_db = state.compute_auto_gain() as f32;
                (true, auto_db)
            }
            DisplayFilterMode::Same => {
                // Mirror whatever the playback gain pipeline does
                let manual = state.gain_db.get() as f32;
                match state.gain_mode.get() {
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
        state.display_auto_gain.set(gain_auto);
        state.display_gain_boost.set(boost);

        // When Gain is Off, zero out the display gain offset
        if gain_filter == DisplayFilterMode::Off {
            state.spect_gain_db.set(0.0);
        }

        // Decimation — resolve effective target rate (0 = no decimation)
        let decim_rate = match state.display_filter_decimate.get() {
            DisplayFilterMode::Off => 0,
            DisplayFilterMode::Auto => {
                // Only decimate when xform display is active
                if xform_on { 44100 } else { 0 }
            }
            DisplayFilterMode::Same => {
                // Decimate to browser's native output sample rate so Web Audio doesn't resample
                let bsr = state.browser_sample_rate.get();
                if bsr > 0 { bsr } else { 0 }
            }
            DisplayFilterMode::Custom => state.display_decimate_rate.get(),
        };
        state.display_decimate_effective.set(decim_rate);
    });

    // (Auto-zoom Effect removed — decimation now controls the frequency axis via sample rate)

    // Auto-learn display noise floor when NR is Auto/Custom and a file is loaded.
    // Re-triggers when file changes or NR mode changes to Auto/Custom.
    {
        let learning: RwSignal<bool> = RwSignal::new(false);
        Effect::new(move |_| {
            let nr_mode = state.display_filter_nr.get();
            let enabled = state.display_filter_enabled.get();
            let file_idx = state.current_file_index.get();
            // Only auto-learn when DSP is enabled and NR is Auto or Custom
            if !enabled || !matches!(nr_mode, DisplayFilterMode::Auto | DisplayFilterMode::Custom) {
                return;
            }
            // Already have a floor for this file? Skip.
            if state.display_auto_noise_floor.get_untracked().is_some() {
                return;
            }
            if learning.get_untracked() {
                return;
            }
            let files = state.files.get_untracked();
            let Some(idx) = file_idx else { return; };
            let Some(file) = files.get(idx).cloned() else { return; };

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
                    &samples, sample_rate, 0.5, // 500ms from file start
                    crate::canvas::tile_cache::yield_to_browser,
                ).await;
                if let Some(f) = floor {
                    state.display_auto_noise_floor.set(Some(f));
                }
                learning.set(false);
            });
        });
    }

    // Clear display auto noise floor when file changes.
    {
        let prev_file: std::cell::Cell<Option<usize>> = std::cell::Cell::new(None);
        Effect::new(move |_| {
            let idx = state.current_file_index.get();
            if idx != prev_file.get() {
                prev_file.set(idx);
                state.display_auto_noise_floor.set(None);
            }
        });
    }

    // Save/restore per-file settings (gain, noise filter) when switching files.
    // Files in the same sequence group share settings.
    // Also resets HFR to OFF for each new file.
    {
        let prev_idx: std::cell::Cell<Option<usize>> = std::cell::Cell::new(None);
        Effect::new(move |_| {
            let new_idx = state.current_file_index.get();

            let old_idx = prev_idx.get();
            prev_idx.set(new_idx);

            // If HFR is on and we're switching files, swap gain back to
            // "normal" orientation before saving the outgoing file settings.
            let was_hfr = old_idx.is_some()
                && old_idx != new_idx
                && state.focus_stack.get_untracked().hfr_enabled();
            if was_hfr {
                let current_gain = state.gain_db.get_untracked();
                let stashed_gain = state.gain_db_stash.get_untracked();
                state.gain_db.set(stashed_gain);
                state.gain_db_stash.set(current_gain);
            }

            // Save current settings to the outgoing file
            if let Some(oi) = old_idx {
                let settings = FileSettings {
                    gain_mode: state.gain_mode.get_untracked(),
                    gain_db: state.gain_db.get_untracked(),
                    gain_db_stash: state.gain_db_stash.get_untracked(),
                    notch_enabled: state.notch_enabled.get_untracked(),
                    notch_bands: state.notch_bands.get_untracked(),
                    notch_profile_name: state.notch_profile_name.get_untracked(),
                    notch_harmonic_suppression: state.notch_harmonic_suppression.get_untracked(),
                    noise_reduce_enabled: state.noise_reduce_enabled.get_untracked(),
                    noise_reduce_strength: state.noise_reduce_strength.get_untracked(),
                    noise_reduce_floor: state.noise_reduce_floor.get_untracked(),
                };

                // Save to the outgoing file and all files in its sequence group
                let names: Vec<String> = state.files.get_untracked().iter().map(|f| f.name.clone()).collect();
                let groups = crate::components::file_sidebar::file_groups::compute_file_groups(&names);
                let group_key = groups.get(oi).and_then(|g| g.as_ref()).map(|ti| ti.group_key.clone());

                state.files.update(|files| {
                    for (i, file) in files.iter_mut().enumerate() {
                        let dominated = i == oi || group_key.as_ref().is_some_and(|gk| {
                            groups.get(i).and_then(|g| g.as_ref()).map(|ti| &ti.group_key) == Some(gk)
                        });
                        if dominated {
                            file.settings = settings.clone();
                        }
                    }
                });
            }

            // Clear annotation selection and save outgoing file's sidecar when switching
            if old_idx != new_idx {
                state.selected_annotation_ids.set(Vec::new());
                state.pop_annotation_ff();
                // Save outgoing file's annotations
                if let Some(oi) = old_idx {
                    crate::opfs::save_annotations(state, oi);
                }
                // Reset HFR and focus stack for the new file (HFR defaults to OFF).
                state.focus_stack.set(crate::focus_stack::FocusStack::new());
                if was_hfr {
                    state.playback_mode.set(PlaybackMode::Normal);
                    state.bandpass_mode.set(crate::state::BandpassMode::Off);
                    state.min_display_freq.set(None);
                    state.max_display_freq.set(None);
                }
            }

            // Restore settings from the incoming file
            if let Some(ni) = new_idx {
                let files = state.files.get_untracked();
                if let Some(file) = files.get(ni) {
                    let s = &file.settings;
                    state.gain_mode.set(s.gain_mode);
                    state.auto_gain.set(s.gain_mode.is_auto());
                    state.gain_db.set(s.gain_db);
                    state.gain_db_stash.set(s.gain_db_stash);
                    state.notch_enabled.set(s.notch_enabled);
                    state.notch_bands.set(s.notch_bands.clone());
                    state.notch_profile_name.set(s.notch_profile_name.clone());
                    state.notch_harmonic_suppression.set(s.notch_harmonic_suppression);
                    state.noise_reduce_enabled.set(s.noise_reduce_enabled);
                    state.noise_reduce_strength.set(s.noise_reduce_strength);
                    state.noise_reduce_floor.set(s.noise_reduce_floor.clone());
                }
            }
        });
    }

    // Auto-save annotations to OPFS (browser) or central store (Tauri) when dirty.
    Effect::new(move |_| {
        let dirty = state.annotations_dirty.get();
        if !dirty { return; }
        state.annotations_dirty.set(false);
        let idx = match state.current_file_index.get_untracked() {
            Some(i) => i,
            None => return,
        };
        crate::opfs::save_annotations(state, idx);
    });

    // Global keyboard shortcut: Space = play/stop
    let state_kb = state;
    let handler = Closure::<dyn Fn(web_sys::KeyboardEvent)>::new(move |ev: web_sys::KeyboardEvent| {
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
            if state_kb.current_file_index.get_untracked().is_some() {
                if state_kb.is_playing.get_untracked() {
                    playback::stop(&state_kb);
                } else {
                    match state_kb.play_start_mode.get_untracked() {
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
                                } else if state_kb.scroll_offset.get_untracked() <= 0.0 {
                                    playback::play_from_start(&state_kb);
                                } else {
                                    playback::play_from_here(&state_kb);
                                }
                            } else if state_kb.scroll_offset.get_untracked() <= 0.0 {
                                playback::play_from_start(&state_kb);
                            } else {
                                playback::play_from_here(&state_kb);
                            }
                        }
                    }
                }
            }
        }
        if (ev.key() == "l" || ev.key() == "L") && !ev.ctrl_key() && !ev.meta_key() && !ev.alt_key() {
            ev.prevent_default();
            let st = state_kb;
            wasm_bindgen_futures::spawn_local(async move {
                microphone::toggle_listen(&st).await;
            });
        }
        if (ev.key() == "r" || ev.key() == "R") && !ev.ctrl_key() && !ev.meta_key() && !ev.alt_key() {
            ev.prevent_default();
            let st = state_kb;
            wasm_bindgen_futures::spawn_local(async move {
                microphone::toggle_record(&st).await;
            });
        }
        if (ev.key() == "h" || ev.key() == "H") && !ev.ctrl_key() && !ev.meta_key() && !ev.alt_key() {
            ev.prevent_default();
            state_kb.hfr_enabled.update(|v| *v = !*v);
        }
        if (ev.key() == "b" || ev.key() == "B") && !ev.ctrl_key() && !ev.meta_key() && !ev.alt_key() {
            ev.prevent_default();
            state_kb.bat_book_open.update(|v| *v = !*v);
        }
        // M = drop a marker annotation at the current playhead position.
        if (ev.key() == "m" || ev.key() == "M") && !ev.ctrl_key() && !ev.meta_key() && !ev.alt_key() {
            // If something else wants the key (label editor, etc.), skip.
            if state_kb.annotation_editing.get_untracked() { return; }
            ev.prevent_default();
            let t = state_kb.playhead_time.get_untracked();
            crate::components::overflow_menu::add_marker_at_time(&state_kb, t);
        }
        // Q = toggle frequency bounds on current selection or selected annotations (region ↔ segment)
        if (ev.key() == "q" || ev.key() == "Q") && !ev.ctrl_key() && !ev.meta_key() && !ev.alt_key() {
            if let Some(sel) = state_kb.selection.get_untracked() {
                // Transient selection exists — toggle it
                ev.prevent_default();
                if sel.freq_low.is_some() && sel.freq_high.is_some() {
                    // Strip freq bounds: region → segment
                    state_kb.selection.set(Some(crate::state::Selection {
                        freq_low: None,
                        freq_high: None,
                        ..sel
                    }));
                    state_kb.show_info_toast("Region → Segment (Q)");
                } else {
                    // Restore freq bounds from BandFF range: segment → region
                    let ff = state_kb.focus_stack.get_untracked().effective_range_ignoring_hfr();
                    let (lo, hi) = if ff.is_active() {
                        (ff.lo, ff.hi)
                    } else {
                        let files = state_kb.files.get_untracked();
                        let idx = state_kb.current_file_index.get_untracked().unwrap_or(0);
                        let file_max = files.get(idx).map(|f| f.spectrogram.max_freq).unwrap_or(96_000.0);
                        (state_kb.min_display_freq.get_untracked().unwrap_or(0.0),
                         state_kb.max_display_freq.get_untracked().unwrap_or(file_max))
                    };
                    state_kb.selection.set(Some(crate::state::Selection {
                        freq_low: Some(lo),
                        freq_high: Some(hi),
                        ..sel
                    }));
                    state_kb.show_info_toast("Segment → Region (Q)");
                }
            } else {
                // No transient selection — toggle selected annotations
                let sel_ids = state_kb.selected_annotation_ids.get_untracked();
                if let (false, Some(idx)) = (sel_ids.is_empty(), state_kb.current_file_index.get_untracked()) {
                    ev.prevent_default();
                    // Check if all selected annotations are regions (have freq bounds)
                    let store = state_kb.annotation_store.get_untracked();
                    let all_have_freq = if let Some(Some(ref set)) = store.sets.get(idx) {
                        sel_ids.iter().all(|id| {
                            set.annotations.iter().find(|a| &a.id == id).is_some_and(|a| {
                                matches!(&a.kind, crate::annotations::AnnotationKind::Region(r) if r.freq_low.is_some() && r.freq_high.is_some())
                            })
                        })
                    } else {
                        false
                    };
                    drop(store);
                    state_kb.snapshot_annotations();
                    if all_have_freq {
                        // Region → Segment: strip freq bounds, don't reset BandFF
                        state_kb.annotation_store.update(|store| {
                            if let Some(Some(ref mut set)) = store.sets.get_mut(idx) {
                                for ann in set.annotations.iter_mut() {
                                    if sel_ids.contains(&ann.id) {
                                        if let crate::annotations::AnnotationKind::Region(ref mut r) = ann.kind {
                                            r.freq_low = None;
                                            r.freq_high = None;
                                            ann.modified_at = js_sys::Date::new_0().to_iso_string().as_string().unwrap_or_default();
                                        }
                                    }
                                }
                            }
                        });
                        state_kb.annotations_dirty.set(true);
                        state_kb.show_info_toast("Region → Segment (Q)");
                    } else {
                        // Segment → Region: use BandFF height
                        let ff = state_kb.focus_stack.get_untracked().effective_range_ignoring_hfr();
                        let (lo, hi) = if ff.is_active() {
                            (ff.lo, ff.hi)
                        } else {
                            let files = state_kb.files.get_untracked();
                            let file_max = files.get(idx).map(|f| f.spectrogram.max_freq).unwrap_or(96_000.0);
                            (state_kb.min_display_freq.get_untracked().unwrap_or(0.0),
                             state_kb.max_display_freq.get_untracked().unwrap_or(file_max))
                        };
                        state_kb.annotation_store.update(|store| {
                            if let Some(Some(ref mut set)) = store.sets.get_mut(idx) {
                                for ann in set.annotations.iter_mut() {
                                    if sel_ids.contains(&ann.id) {
                                        if let crate::annotations::AnnotationKind::Region(ref mut r) = ann.kind {
                                            if r.freq_low.is_none() || r.freq_high.is_none() {
                                                r.freq_low = Some(lo);
                                                r.freq_high = Some(hi);
                                                ann.modified_at = js_sys::Date::new_0().to_iso_string().as_string().unwrap_or_default();
                                            }
                                        }
                                    }
                                }
                            }
                        });
                        state_kb.annotations_dirty.set(true);
                        state_kb.show_info_toast("Segment → Region (Q)");
                    }
                }
            }
        }
        // Ctrl+Z / Cmd+Z = Undo, Ctrl+Shift+Z / Cmd+Shift+Z / Ctrl+Y = Redo
        if (ev.key() == "z" || ev.key() == "Z") && (ev.ctrl_key() || ev.meta_key()) && !ev.alt_key() {
            ev.prevent_default();
            if ev.shift_key() {
                state_kb.redo_annotations();
            } else {
                state_kb.undo_annotations();
            }
        }
        if ev.key() == "y" && (ev.ctrl_key() || ev.meta_key()) && !ev.shift_key() && !ev.alt_key() {
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
            let files = state_kb.files.get_untracked();
            let timeline = state_kb.active_timeline.get_untracked();
            let (time_res, duration) = if let Some(ref tl) = timeline {
                let tr = tl.segments.first().and_then(|s| files.get(s.file_index))
                    .map(|f| f.spectrogram.time_resolution).unwrap_or(1.0);
                (tr, tl.total_duration_secs)
            } else {
                let idx = state_kb.current_file_index.get_untracked().unwrap_or(0);
                match files.get(idx) {
                    Some(file) => (file.spectrogram.time_resolution, file.audio.duration_secs),
                    None => (1.0, 0.0),
                }
            };
            {
                let zoom = state_kb.zoom_level.get_untracked();
                let canvas_w = state_kb.spectrogram_canvas_width.get_untracked();
                let visible_time = viewport::visible_time(canvas_w, zoom, time_res);
                let from_here_mode = state_kb.play_start_mode.get_untracked().uses_from_here();
                let (_min_scroll, max_scroll) = viewport::scroll_bounds_for_mode(duration, visible_time, from_here_mode);
                let new_scroll = match key.as_str() {
                    "Home" => viewport::clamp_scroll_for_mode(0.0, duration, visible_time, from_here_mode),
                    "End" => max_scroll,
                    "ArrowLeft" => viewport::clamp_scroll_for_mode(state_kb.scroll_offset.get_untracked() - visible_time * 0.2, duration, visible_time, from_here_mode),
                    "ArrowRight" => viewport::clamp_scroll_for_mode(state_kb.scroll_offset.get_untracked() + visible_time * 0.2, duration, visible_time, from_here_mode),
                    "PageUp" => viewport::clamp_scroll_for_mode(state_kb.scroll_offset.get_untracked() - visible_time * 0.8, duration, visible_time, from_here_mode),
                    "PageDown" => viewport::clamp_scroll_for_mode(state_kb.scroll_offset.get_untracked() + visible_time * 0.8, duration, visible_time, from_here_mode),
                    _ => state_kb.scroll_offset.get_untracked(),
                };
                state_kb.suspend_follow();
                state_kb.scroll_offset.set(new_scroll);
            }
        }
        if ev.key() == "Escape" {
            if state_kb.bat_book_ref_open.get_untracked() {
                state_kb.bat_book_ref_open.set(false);
                return;
            }
            if state_kb.xc_browser_open.get_untracked() {
                state_kb.xc_browser_open.set(false);
                return;
            }
            if state_kb.mic_listening.get_untracked() || state_kb.mic_recording.get_untracked() {
                microphone::stop_all(&state_kb);
            }
        }
        // Backtick: activate clean view (hide overlays)
        if ev.key() == "`" && !ev.ctrl_key() && !ev.meta_key() && !ev.alt_key()
            && !state_kb.clean_view.get_untracked() {
                state_kb.clean_view.set(true);
            }
    });
    let window = web_sys::window().unwrap();
    let _ = window.add_event_listener_with_callback("keydown", handler.as_ref().unchecked_ref());
    handler.forget();

    // Keyup handler: release clean view on backtick release
    let state_ku = state;
    let keyup_handler = Closure::<dyn Fn(web_sys::KeyboardEvent)>::new(move |ev: web_sys::KeyboardEvent| {
        if ev.key() == "`" {
            state_ku.clean_view.set(false);
        }
    });
    let _ = window.add_event_listener_with_callback("keyup", keyup_handler.as_ref().unchecked_ref());
    keyup_handler.forget();

    // Reset clean view if window loses focus (so it doesn't stick)
    let state_blur = state;
    let blur_handler = Closure::<dyn Fn()>::new(move || {
        state_blur.clean_view.set(false);
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
            let was_mobile = state_resize.is_mobile.get_untracked();
            if mobile != was_mobile {
                state_resize.is_mobile.set(mobile);
                if mobile {
                    // Switching to mobile: collapse sidebars (they become overlays)
                    state_resize.sidebar_collapsed.set(true);
                    state_resize.right_sidebar_collapsed.set(true);
                } else {
                    // Switching to desktop: show left sidebar by default
                    state_resize.sidebar_collapsed.set(false);
                }
            }
        });
        let _ = window.add_event_listener_with_callback("resize", on_resize.as_ref().unchecked_ref());
        on_resize.forget();
    }

    let grid_style = move || {
        if state.is_mobile.get() {
            // Sidebars are position:fixed overlays, so single column for main content
            "grid-template-columns: 1fr; grid-template-rows: auto 1fr".to_string()
        } else {
            let left = if state.sidebar_collapsed.get() { 0 } else { state.sidebar_width.get() as i32 };
            let right = if state.right_sidebar_collapsed.get() { 0 } else { state.right_sidebar_width.get() as i32 };
            format!("grid-template-columns: {}px 1fr {}px; grid-template-rows: auto 1fr", left, right)
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
        let _ = doc.add_event_listener_with_callback("dragover", on_dragover.as_ref().unchecked_ref());
        let _ = doc.add_event_listener_with_callback("drop", on_drop.as_ref().unchecked_ref());
        on_dragover.forget();
        on_drop.forget();
    }

    // Tauri: listen for native file drag-drop events (provides real filesystem paths)
    if state.is_tauri {
        let state_drop = state;
        let callback = wasm_bindgen::closure::Closure::<dyn FnMut(wasm_bindgen::JsValue)>::new(move |ev: wasm_bindgen::JsValue| {
            // Payload shape: { event: "tauri://drag-drop", payload: { paths: [...], position: {x, y} } }
            let payload = js_sys::Reflect::get(&ev, &wasm_bindgen::JsValue::from_str("payload")).unwrap_or_default();
            let paths = js_sys::Reflect::get(&payload, &wasm_bindgen::JsValue::from_str("paths")).unwrap_or_default();
            let paths_array = js_sys::Array::from(&paths);
            let file_paths: Vec<String> = paths_array.iter().filter_map(|v| v.as_string()).collect();
            if file_paths.is_empty() { return; }

            log::info!("Tauri drag-drop: {} file(s)", file_paths.len());
            for path in file_paths {
                let name = path.rsplit(['/', '\\']).next().unwrap_or(&path).to_string();
                // Filter to audio-ish extensions
                let ext = name.rsplit('.').next().unwrap_or("").to_lowercase();
                if !matches!(ext.as_str(), "wav" | "w4v" | "flac" | "ogg" | "mp3" | "m4a" | "m4b") {
                    log::info!("Skipping non-audio drop: {name}");
                    continue;
                }
                let state = state_drop;
                let load_id = state.loading_start(&name);
                let name_for_err = name.clone();
                leptos::task::spawn_local(async move {
                    match crate::components::file_sidebar::load_native_file(path, state, load_id).await {
                        Ok(()) => {}
                        Err(e) => {
                            log::error!("Failed to load {}: {}", name_for_err, e);
                            state.show_error_toast(&format!("Couldn't open {name_for_err}: {e}"));
                        }
                    }
                    state.loading_done(load_id);
                });
            }
        });
        crate::tauri_bridge::tauri_listen("tauri://drag-drop", callback);
    }

    // Back button (Android/browser): close sidebar when open.
    // Registered unconditionally — harmless on desktop, needed if layout switches to mobile.
    {
        let state_back = state;
        let on_popstate = wasm_bindgen::closure::Closure::<dyn Fn(web_sys::Event)>::new(move |_: web_sys::Event| {
            if !state_back.is_mobile.get_untracked() { return; }
            if !state_back.right_sidebar_collapsed.get_untracked() {
                state_back.right_sidebar_collapsed.set(true);
                let _ = web_sys::window().unwrap().history().unwrap()
                    .push_state_with_url(&wasm_bindgen::JsValue::NULL, "", None);
            } else if !state_back.sidebar_collapsed.get_untracked() {
                state_back.sidebar_collapsed.set(true);
                let _ = web_sys::window().unwrap().history().unwrap()
                    .push_state_with_url(&wasm_bindgen::JsValue::NULL, "", None);
            }
        });
        let _ = window.add_event_listener_with_callback("popstate", on_popstate.as_ref().unchecked_ref());
        on_popstate.forget();
        // Push initial history entry so back button has something to pop
        let _ = window.history().unwrap()
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
            ).ok().filter(|v| !v.is_undefined() && !v.is_null());
            let scale = vv_obj.as_ref()
                .and_then(|vv| js_sys::Reflect::get(vv, &wasm_bindgen::JsValue::from_str("scale")).ok())
                .and_then(|s| s.as_f64())
                .unwrap_or(1.0);
            let zoomed = scale > 1.05;
            // Track visual viewport position for button placement
            if let Some(ref vv) = vv_obj {
                let offset_top = js_sys::Reflect::get(vv, &wasm_bindgen::JsValue::from_str("offsetTop"))
                    .ok().and_then(|v| v.as_f64()).unwrap_or(0.0);
                let offset_left = js_sys::Reflect::get(vv, &wasm_bindgen::JsValue::from_str("offsetLeft"))
                    .ok().and_then(|v| v.as_f64()).unwrap_or(0.0);
                let vp_width = js_sys::Reflect::get(vv, &wasm_bindgen::JsValue::from_str("width"))
                    .ok().and_then(|v| v.as_f64()).unwrap_or(0.0);
                state_vp.visual_viewport_rect.set((offset_top, offset_left, vp_width, scale));
            }
            let prev = state_vp.viewport_zoomed.get_untracked();
            state_vp.viewport_zoomed.set(zoomed);
            // Toggle body class so CSS can override touch-action on canvas areas
            if let Some(body) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.body()) {
                if zoomed {
                    let _ = body.class_list().add_1("viewport-zoomed");
                } else {
                    let _ = body.class_list().remove_1("viewport-zoomed");
                }
            }
            // When zooming back out, clear any stale drag/interaction state so
            // a single touch doesn't get "stuck" acting like a pinch.
            if prev && !zoomed {
                state_vp.is_dragging.set(false);
                state_vp.spec_drag_handle.set(None);
                state_vp.annotation_drag_handle.set(None);
            }
        });
        let window = web_sys::window().unwrap();
        if let Ok(vv) = js_sys::Reflect::get(
            &window,
            &wasm_bindgen::JsValue::from_str("visualViewport"),
        ) {
            if !vv.is_undefined() && !vv.is_null() {
                // Listen to both "resize" and "scroll" — different platforms fire
                // different events during pinch-zoom gestures.
                let add_fn_val = js_sys::Reflect::get(
                    &vv, &wasm_bindgen::JsValue::from_str("addEventListener"),
                ).ok();
                if let Some(add_fn_val) = add_fn_val {
                    if let Ok(add_fn) = add_fn_val.dyn_into::<js_sys::Function>() {
                        let _ = add_fn.call2(&vv, &wasm_bindgen::JsValue::from_str("resize"), check_zoom.as_ref());
                        let _ = add_fn.call2(&vv, &wasm_bindgen::JsValue::from_str("scroll"), check_zoom.as_ref());
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
                let _ = web_sys::window().unwrap().request_animation_frame(
                    check_ref2.unchecked_ref(),
                );
            });
            let _ = window.add_event_listener_with_callback(
                "touchend",
                touchend_cb.as_ref().unchecked_ref(),
            );
            touchend_cb.forget();
        }
        check_zoom.forget();
    }

    let app_class = move || if state.is_mobile.get() { "app mobile" } else { "app" };

    view! {
        <div class=app_class style=grid_style>
            <Toolbar />
            <FileSidebar />
            {move || state.is_mobile.get().then(|| view! {
                <div
                    class=move || if !state.sidebar_collapsed.get() || !state.right_sidebar_collapsed.get() { "sidebar-backdrop open" } else { "sidebar-backdrop" }
                    on:click=move |_| {
                        state.sidebar_collapsed.set(true);
                        state.right_sidebar_collapsed.set(true);
                    }
                ></div>
            })}
            <MainArea />
            <RightSidebar />
            {move || state.xc_browser_open.get().then(|| view! { <XcBrowser /> })}
        </div>
    }
}

#[component]
fn MainArea() -> impl IntoView {
    let state = expect_context::<AppState>();
    let has_file = move || state.current_file_index.get().is_some() || state.active_timeline.get().is_some();

    // Click/tap anywhere in the main area closes open layer panels (and sidebar on mobile)
    let on_main_click = move |_: web_sys::MouseEvent| {
        state.layer_panel_open.set(None);
        if state.is_mobile.get_untracked() {
            state.sidebar_collapsed.set(true);
            state.right_sidebar_collapsed.set(true);
        }
    };
    // touchstart also closes menus — needed because mobile touch handlers often
    // call preventDefault() which suppresses the synthetic click event
    let on_main_touchstart = move |_: web_sys::TouchEvent| {
        state.layer_panel_open.set(None);
        if state.is_mobile.get_untracked() {
            state.sidebar_collapsed.set(true);
            state.right_sidebar_collapsed.set(true);
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

                        // Main view (takes remaining space)
                        <div class="main-view">
                            // Show the selected main view
                            {move || match state.main_view.get() {
                                MainView::Spectrogram | MainView::XformedSpec | MainView::Flow | MainView::Resonators => view! { <Spectrogram /> }.into_any(),
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
                                let recording = state.mic_recording.get();
                                let listening = state.mic_listening.get();
                                if !recording && !listening { return None; }
                                let level = state.mic_peak_level.get();
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
                                style:display=move || if state.clean_view.get() { "none" } else { "" }
                            >
                                // Unsaved recording banner (web only)
                                {move || {
                                    if state.is_tauri { return None; }
                                    let files = state.files.get();
                                    let idx = state.current_file_index.get()?;
                                    let file = files.get(idx)?;
                                    if !file.is_recording { return None; }
                                    let name = file.name.clone();
                                    Some(view! {
                                        <div
                                            class="unsaved-banner"
                                            on:click=move |_| {
                                                let files = state.files.get_untracked();
                                                let idx = state.current_file_index.get_untracked();
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
                            {move || (state.bat_book_ref_open.get() && !state.clean_view.get()).then(|| view! { <BatBookRefPanel /> })}
                        </div>

                        // Bat book strip (between main view and bottom toolbar)
                        {move || state.bat_book_open.get().then(|| view! { <BatBookStrip /> })}

                        {move || state.show_status_bar.get().then(|| view! { <AnalysisPanel /> })}
                    }.into_any()
                } else {
                    let empty_msg = if state.is_mobile.get() {
                        "Tap \u{2630} to load audio files"
                    } else {
                        "Drop WAV, FLAC or MP3 files into the sidebar"
                    };
                    view! {
                        <div class="empty-state">
                            {empty_msg}
                        </div>
                        {move || state.bat_book_open.get().then(|| view! { <BatBookStrip /> })}
                        {move || state.bat_book_ref_open.get().then(|| view! { <BatBookRefPanel /> })}
                    }.into_any()
                }
            }}
            <BottomToolbar />

            // Zoom-out button — appears when mobile viewport is pinch-zoomed in.
            // Uses absolute positioning relative to the visual viewport (not
            // position:fixed which anchors to the layout viewport and goes off-screen).
            {move || state.viewport_zoomed.get().then(|| {
                let btn_size = 44.0_f64;
                let margin = 10.0_f64;
                view! {
                    <button
                        class="zoom-out-btn"
                        title="Reset zoom"
                        style=move || {
                            let (off_top, off_left, vp_w, _scale) = state.visual_viewport_rect.get();
                            let top = off_top + margin;
                            let left = off_left + vp_w - btn_size - margin;
                            format!("top:{top}px;left:{left}px;")
                        }
                        on:click=move |_| {
                            // Reset viewport zoom via meta viewport trick
                            if let Some(window) = web_sys::window() {
                                if let Some(doc) = window.document() {
                                    if let Some(meta) = doc.query_selector("meta[name=viewport]").ok().flatten() {
                                        let original = meta.get_attribute("content").unwrap_or_default();
                                        meta.set_attribute("content", &format!("{}, maximum-scale=1", original)).ok();
                                        let meta_clone = meta.clone();
                                        let orig_clone = original.clone();
                                        let cb = wasm_bindgen::closure::Closure::once(move || {
                                            meta_clone.set_attribute("content", &orig_clone).ok();
                                        });
                                        window.request_animation_frame(cb.as_ref().unchecked_ref()).ok();
                                        cb.forget();
                                    }
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
            {move || state.show_mic_chooser.get().then(|| view! {
                <crate::components::file_sidebar::mic_chooser::MicChooserModal />
            })}

            // Privacy settings modal
            {move || state.show_privacy_settings.get().then(|| view! {
                <crate::components::file_sidebar::privacy_settings::PrivacySettingsModal />
            })}

            // "Ready to record" modal
            {move || (state.record_ready_state.get() == crate::state::RecordReadyState::AwaitingConfirmation).then(|| {
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
                                    {move || state.mic_device_info.get().map(|info| info.name.clone()).unwrap_or_else(|| "Microphone".to_string())}
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
        </div>
    }
}

fn toggle_panel(state: &AppState, panel: LayerPanel) {
    state.layer_panel_open.update(|p| {
        *p = if *p == Some(panel) { None } else { Some(panel) };
    });
}

/// Current effective sample rate for the Resonators readouts — the live
/// waterfall's rate when mic is active, otherwise the current file's, with a
/// sensible default so the label stays populated with no file loaded.
fn resonator_quick_sample_rate(state: AppState) -> f64 {
    if crate::canvas::live_waterfall::is_active() {
        return crate::canvas::live_waterfall::max_freq() * 2.0;
    }
    let files = state.files.get();
    let idx = state.current_file_index.get();
    idx.and_then(|i| files.get(i))
        .map(|f| f.spectrogram.sample_rate as f64)
        .unwrap_or(192_000.0)
}

fn layer_opt_class(active: bool) -> &'static str {
    if active { "layer-panel-opt sel" } else { "layer-panel-opt" }
}

/// Split-button: click cycles Spec/Wave, down-arrow opens a dropdown with all
/// view modes + DSP settings. Rendered in the bottom toolbar.
#[component]
pub fn MainViewButton() -> impl IntoView {
    use crate::components::combo_button::ComboButton;
    let state = expect_context::<AppState>();
    let is_open = Signal::derive(move || state.layer_panel_open.get() == Some(LayerPanel::MainView));

    let left_class = Signal::derive(move || {
        "layer-btn combo-btn-left"
    });
    let right_class = Signal::derive(move || {
        if is_open.get() { "layer-btn combo-btn-right dim open" } else { "layer-btn combo-btn-right dim" }
    });

    // Helper: handle all side-effects of a view switch synchronously,
    // so the spectrogram render Effect always sees consistent state.
    let switch_view = move |new_view: MainView| {
        let old_view = state.main_view.get_untracked();
        if new_view == old_view { return; }

        let entering_xform = new_view == MainView::XformedSpec && old_view != MainView::XformedSpec;
        let leaving_xform = new_view != MainView::XformedSpec && old_view == MainView::XformedSpec;

        if entering_xform {
            // Enable display processing with all filters defaulting to "Same".
            // Also directly resolve the display_* signals so the render Effect
            // sees correct state immediately (don't wait for the resolve Effect).
            state.display_filter_enabled.set(true);
            state.display_filter_eq.set(DisplayFilterMode::Same);
            state.display_filter_notch.set(DisplayFilterMode::Same);
            state.display_filter_nr.set(DisplayFilterMode::Same);
            state.display_filter_transform.set(DisplayFilterMode::Same);
            state.display_filter_gain.set(DisplayFilterMode::Same);
            state.display_filter_decimate.set(DisplayFilterMode::Same);
            // Eagerly resolve "Same" → mirror current playback state
            state.display_eq.set(state.filter_enabled.get_untracked());
            state.display_noise_filter.set(
                state.noise_reduce_enabled.get_untracked() || state.notch_enabled.get_untracked()
            );
            state.display_transform.set(
                state.playback_mode.get_untracked() != PlaybackMode::Normal
            );
        } else if leaving_xform {
            // Disable display processing and directly reset all display signals.
            // Setting these directly (rather than relying on the resolve Effect)
            // ensures the spectrogram render Effect sees consistent state.
            state.display_filter_enabled.set(false);
            state.display_transform.set(false);
            state.display_eq.set(false);
            state.display_noise_filter.set(false);
            state.display_auto_gain.set(false);
            state.display_gain_boost.set(0.0);
            state.display_decimate_effective.set(0);
        }

        // Set the view last so all display state is consistent when
        // the main_view change triggers the spectrogram render Effect.
        state.main_view.set(new_view);
    };

    let left_click = Callback::new(move |_: web_sys::MouseEvent| {
        let new_view = match state.main_view.get_untracked() {
            MainView::Spectrogram | MainView::XformedSpec => MainView::Waveform,
            _ => MainView::Spectrogram,
        };
        switch_view(new_view);
    });

    let left_value = Signal::derive(move || state.main_view.get().short_label().to_string());
    let right_value = Signal::derive(move || "View".to_string());

    let toggle_menu = Callback::new(move |()| {
        toggle_panel(&state, LayerPanel::MainView);
    });

    let set_view = move |mode: MainView| {
        move |_: web_sys::MouseEvent| {
            switch_view(mode);
            state.layer_panel_open.set(None);
        }
    };

    // Playback active indicators (for DSP rows)
    let eq_active = Signal::derive(move || state.filter_enabled.get());
    let notch_active = Signal::derive(move || state.notch_enabled.get());
    let nr_active = Signal::derive(move || state.noise_reduce_enabled.get());
    let transform_active = Signal::derive(move || state.playback_mode.get() != PlaybackMode::Normal);
    let gain_active = Signal::derive(move || state.gain_mode.get() != GainMode::Off);
    let decim_active = Signal::derive(move || false);

    let browser_is_resampling = Signal::derive(move || {
        let bsr = state.browser_sample_rate.get();
        if bsr == 0 { return false; }
        let files = state.files.get();
        let idx = state.current_file_index.get();
        let file_rate = idx.and_then(|i| files.get(i)).map(|f| f.audio.sample_rate).unwrap_or(0);
        if file_rate == 0 { return false; }
        let decim = state.display_decimate_effective.get();
        let effective = if decim > 0 && decim < file_rate {
            crate::dsp::filters::decimated_rate(file_rate, decim)
        } else {
            file_rate
        };
        effective != bsr
    });

    let resam_tooltip = Signal::derive(move || {
        let bsr = state.browser_sample_rate.get();
        if bsr == 0 { return String::new(); }
        let files = state.files.get();
        let idx = state.current_file_index.get();
        let file_rate = idx.and_then(|i| files.get(i)).map(|f| f.audio.sample_rate).unwrap_or(0);
        if file_rate == 0 { return String::new(); }
        let decim = state.display_decimate_effective.get();
        let effective = if decim > 0 && decim < file_rate {
            crate::dsp::filters::decimated_rate(file_rate, decim)
        } else {
            file_rate
        };
        if effective != bsr {
            format!("Browser resampling {}Hz to {}Hz output", effective, bsr)
        } else {
            format!("Output matches browser rate ({}Hz)", bsr)
        }
    });

    let show_nr_custom = Signal::derive(move || {
        state.main_view.get() == MainView::XformedSpec && state.display_filter_nr.get() == DisplayFilterMode::Custom
    });
    let show_decim_custom = Signal::derive(move || {
        state.main_view.get() == MainView::XformedSpec && state.display_filter_decimate.get() == DisplayFilterMode::Custom
    });

    view! {
        <ComboButton
            left_label=""
            left_value=left_value
            left_click=left_click
            left_class=left_class
            right_value=right_value
            right_class=right_class
            is_open=is_open
            toggle_menu=toggle_menu
            left_title="Toggle view (Spectrogram / Waveform)"
            right_title="View mode menu"
            menu_direction="above"
            panel_style="min-width: 240px;"
        >
            <div class="layer-panel-title">"View Mode"</div>
            {MainView::ALL.iter().map(|&mode| {
                view! {
                    <button
                        class=move || layer_opt_class(state.main_view.get() == mode)
                        on:click=set_view(mode)
                    >
                        {mode.label()}
                    </button>
                }
            }).collect_view()}

            // Waveform sub-view (when Waveform is the active main view)
            {move || (state.main_view.get() == MainView::Waveform).then(|| {
                view! {
                    <hr />
                    <div class="layer-panel-title">"Waveform Mode"</div>
                    {WaveformView::ALL.iter().map(|&wv| {
                        view! {
                            <button
                                class=move || layer_opt_class(state.waveform_view.get() == wv)
                                on:click=move |_: web_sys::MouseEvent| {
                                    state.waveform_view.set(wv);
                                }
                            >
                                {wv.label()}
                            </button>
                        }
                    }).collect_view()}
                }
            })}

            // Reassignment checkbox (spectrogram views)
            {move || matches!(state.main_view.get(), MainView::Spectrogram | MainView::XformedSpec).then(|| {
                view! {
                    <hr />
                    <label style="display:flex;align-items:center;gap:4px;cursor:pointer;padding:4px 8px;font-size:12px;"
                        title="Sharpen time-frequency localization using the reassignment method (3x FFT cost)">
                        <input
                            type="checkbox"
                            prop:checked=move || state.reassign_enabled.get()
                            on:change=move |ev: web_sys::Event| {
                                let target = ev.target().unwrap();
                                let input: web_sys::HtmlInputElement = target.unchecked_into();
                                state.reassign_enabled.set(input.checked());
                            }
                        />
                        "Reassignment"
                    </label>
                }
            })}

            // Resonator quick controls (shown only when Resonators view is active)
            {move || (state.main_view.get() == MainView::Resonators).then(|| {
                view! {
                    <hr />
                    <div class="layer-panel-title">"Resonators"</div>
                    <div class="dsp-custom-section">
                        <div class="dsp-custom-title">{move || {
                            let bw = state.resonator_bandwidth_hz.get().max(0.001);
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
                                    resonator_bw_to_slider(state.resonator_bandwidth_hz.get())
                                        .round()
                                        .to_string()
                                }
                                on:input=move |ev: web_sys::Event| {
                                    let target = ev.target().unwrap();
                                    let input: web_sys::HtmlInputElement = target.unchecked_into();
                                    if let Ok(pos) = input.value().parse::<f32>() {
                                        state.resonator_bandwidth_hz.set(resonator_slider_to_bw(pos));
                                    }
                                }
                                on:dblclick=move |_| state.resonator_bandwidth_hz.set(20.0)
                            />
                        </div>
                    </div>
                    <div class="setting-row" style="padding: 4px 8px;">
                        <span class="setting-label">{move || {
                            let mode = state.resonator_fft_mode.get();
                            let sr = resonator_quick_sample_rate(state);
                            let current_lod = crate::canvas::tile_cache::select_lod(
                                state.zoom_level.get(),
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
                                let new_mode = if v == "adaptive" {
                                    ResonatorFftMode::Adaptive
                                } else if let Ok(sz) = v.parse::<usize>() {
                                    ResonatorFftMode::Single(sz.max(16))
                                } else {
                                    return;
                                };
                                state.resonator_fft_mode.set(new_mode);
                            }
                            prop:value=move || match state.resonator_fft_mode.get() {
                                ResonatorFftMode::Adaptive => "adaptive".to_string(),
                                ResonatorFftMode::Single(sz) => sz.to_string(),
                            }
                        >
                            <option value="adaptive">"Adaptive"</option>
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
                                state.resonator_layout.set(new_layout);
                            }
                            prop:value=move || match state.resonator_layout.get() {
                                ResonatorLayout::Linear => "linear",
                                ResonatorLayout::Log => "log",
                            }
                        >
                            <option value="linear">"Linear"</option>
                            <option value="log">"Log"</option>
                        </select>
                    </div>
                    <label style="display:flex;align-items:center;gap:4px;cursor:pointer;padding:4px 8px;font-size:12px;"
                        title="Concentrate all bins on the visible freq range for finer vertical zoom. Rebuilds ~0.5s after you stop zooming vertically.">
                        <input
                            type="checkbox"
                            prop:checked=move || state.resonator_viewport_bins.get()
                            on:change=move |ev: web_sys::Event| {
                                let target = ev.target().unwrap();
                                let input: web_sys::HtmlInputElement = target.unchecked_into();
                                state.resonator_viewport_bins.set(input.checked());
                            }
                        />
                        "Viewport zoom"
                    </label>
                }
            })}

            // DSP filter rows (only when XformedSpec is active)
            {move || (state.main_view.get() == MainView::XformedSpec).then(|| {
                view! {
                    <hr />
                    <div class="layer-panel-title">"Display Processing"</div>
                    <div class="dsp-filter-row dsp-filter-header">
                        <span class="dsp-filter-label"></span>
                        <div class="dsp-filter-seg">
                            <span>"off"</span>
                            <span>"aut"</span>
                            <span>"sam"</span>
                            <span>"cst"</span>
                        </div>
                        <div class="dsp-filter-indicator-header" title="Playback active">
                            {"\u{1F50A}"}
                        </div>
                    </div>
                    <DspFilterRow label="EQ" signal=state.display_filter_eq playback_active=eq_active custom_available=false />
                    <DspFilterRow label="Notch" signal=state.display_filter_notch playback_active=notch_active custom_available=false auto_available=false />
                    <DspFilterRow label="NR" signal=state.display_filter_nr playback_active=nr_active custom_available=false />
                    <DspFilterRow label="Xform" signal=state.display_filter_transform playback_active=transform_active custom_available=false auto_available=false />
                    <DspFilterRow label="Gain" signal=state.display_filter_gain playback_active=gain_active custom_available=true />
                    <DspFilterRow label="Resam" signal=state.display_filter_decimate playback_active=decim_active custom_available=true browser_resampling=browser_is_resampling sam_tooltip=resam_tooltip />
                }
            })}

            // Custom NR section
            {move || show_nr_custom.get().then(|| {
                let strength = state.display_nr_strength;
                view! {
                    <div class="dsp-custom-section">
                        <div class="dsp-custom-title">"NR Strength"</div>
                        <div class="dsp-custom-slider-row">
                            <input
                                type="range"
                                class="setting-range"
                                min="0" max="2" step="0.05"
                                prop:value=move || strength.get().to_string()
                                on:input=move |ev: web_sys::Event| {
                                    let target = ev.target().unwrap();
                                    let input: web_sys::HtmlInputElement = target.unchecked_into();
                                    if let Ok(v) = input.value().parse::<f64>() {
                                        strength.set(v);
                                    }
                                }
                                on:dblclick=move |_| strength.set(0.8)
                            />
                            <span class="dsp-custom-value">{move || format!("{:.2}", strength.get())}</span>
                        </div>
                    </div>
                }
            })}

            // Custom Decimate rate section
            {move || show_decim_custom.get().then(|| {
                let rate = state.display_decimate_rate;
                let rates: [(u32, &str); 4] = [
                    (44100, "44.1k"),
                    (48000, "48k"),
                    (96000, "96k"),
                    (192000, "192k"),
                ];
                view! {
                    <div class="dsp-custom-section">
                        <div class="dsp-custom-title">"Decimate Rate"</div>
                        <div class="dsp-filter-seg" style="justify-content: center; gap: 2px; padding: 2px 4px;">
                            {rates.into_iter().map(|(r, label)| {
                                view! {
                                    <button
                                        class=move || if rate.get() == r { "sel" } else { "" }
                                        on:click=move |_| rate.set(r)
                                    >{label}</button>
                                }
                            }).collect_view()}
                        </div>
                    </div>
                }
            })}

            // FFT size selector (when any spectrogram/flow view is active)
            {move || matches!(state.main_view.get(), MainView::Spectrogram | MainView::XformedSpec | MainView::Flow).then(|| {
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
                            state.spect_fft_mode.set(mode);
                        }
                    >
                        {move || {
                            let current = state.spect_fft_mode.get();
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
            {move || matches!(state.main_view.get(), MainView::Spectrogram | MainView::XformedSpec | MainView::Flow | MainView::Resonators).then(|| {
                let file_max = move || {
                    let files = state.files.get();
                    let idx = state.current_file_index.get();
                    idx.and_then(|i| files.get(i))
                        .map(|f| f.spectrogram.max_freq)
                        .unwrap_or(96_000.0)
                };
                let is_range = move |lo: Option<f64>, hi: Option<f64>| -> bool {
                    let cur_min = state.min_display_freq.get();
                    let cur_max = state.max_display_freq.get();
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
                                    state.min_display_freq.set(None);
                                    state.max_display_freq.set(None);
                                }
                                "22k" => {
                                    state.min_display_freq.set(Some(0.0));
                                    state.max_display_freq.set(Some(22_000.0));
                                }
                                "50k" => {
                                    state.min_display_freq.set(Some(0.0));
                                    state.max_display_freq.set(Some(50_000.0));
                                }
                                "100k" => {
                                    state.min_display_freq.set(Some(0.0));
                                    state.max_display_freq.set(Some(100_000.0));
                                }
                                "192k" => {
                                    state.min_display_freq.set(Some(0.0));
                                    state.max_display_freq.set(Some(192_000.0));
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

            // Intensity sliders (for Spectrogram, XformedSpec, Flow, or Resonators)
            {move || matches!(state.main_view.get(), MainView::Spectrogram | MainView::XformedSpec | MainView::Flow | MainView::Resonators).then(|| {
                let is_xform = state.main_view.get_untracked() == MainView::XformedSpec;
                let gain_sig = if is_xform { state.xform_spect_gain_db } else { state.spect_gain_db };
                let range_sig = if is_xform { state.xform_spect_range_db } else { state.spect_range_db };
                let floor_sig = if is_xform { state.xform_spect_floor_db } else { state.spect_floor_db };
                let gamma_sig = if is_xform { state.xform_spect_gamma } else { state.spect_gamma };
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
                                prop:value=move || gain_sig.get().to_string()
                                on:input=move |ev: web_sys::Event| {
                                    let target = ev.target().unwrap();
                                    let input: web_sys::HtmlInputElement = target.unchecked_into();
                                    if let Ok(v) = input.value().parse::<f32>() {
                                        gain_sig.set(v);
                                        if is_xform {
                                            state.display_filter_gain.set(DisplayFilterMode::Custom);
                                        }
                                        state.display_auto_gain.set(false);
                                    }
                                }
                                on:dblclick=move |_| gain_sig.set(0.0)
                            />
                            <span class="dsp-custom-value">{move || {
                                if is_xform {
                                    let gain_mode = state.display_filter_gain.get();
                                    let boost = state.display_gain_boost.get();
                                    if gain_mode == DisplayFilterMode::Off {
                                        "off".to_string()
                                    } else if gain_mode == DisplayFilterMode::Auto {
                                        if boost.abs() < 0.5 { "auto".to_string() }
                                        else { format!("a{:+.0}", boost) }
                                    } else if gain_mode == DisplayFilterMode::Same {
                                        if boost.abs() < 0.5 { "same".to_string() }
                                        else { format!("={:+.0}", boost) }
                                    } else {
                                        format!("{:+.0} dB", gain_sig.get())
                                    }
                                } else {
                                    format!("{:+.0} dB", gain_sig.get())
                                }
                            }}</span>
                        </div>
                        <div class="dsp-custom-slider-row">
                            <span class="dsp-slider-label">"Range"</span>
                            <input
                                type="range"
                                class="setting-range"
                                min="20" max="120" step="5"
                                prop:value=move || range_sig.get().to_string()
                                on:input=move |ev: web_sys::Event| {
                                    let target = ev.target().unwrap();
                                    let input: web_sys::HtmlInputElement = target.unchecked_into();
                                    if let Ok(v) = input.value().parse::<f32>() {
                                        range_sig.set(v);
                                        floor_sig.set(-v);
                                    }
                                }
                                on:dblclick=move |_| {
                                    range_sig.set(120.0);
                                    floor_sig.set(-120.0);
                                }
                            />
                            <span class="dsp-custom-value">{move || format!("{:.0} dB", range_sig.get())}</span>
                        </div>
                        <div class="dsp-custom-slider-row">
                            <span class="dsp-slider-label">"Contrast"</span>
                            <input
                                type="range"
                                class="setting-range"
                                min="0.2" max="3.0" step="0.05"
                                prop:value=move || gamma_sig.get().to_string()
                                on:input=move |ev: web_sys::Event| {
                                    let target = ev.target().unwrap();
                                    let input: web_sys::HtmlInputElement = target.unchecked_into();
                                    if let Ok(v) = input.value().parse::<f32>() {
                                        gamma_sig.set(v);
                                    }
                                }
                                on:dblclick=move |_| gamma_sig.set(1.0)
                            />
                            <span class="dsp-custom-value">{move || {
                                let g = gamma_sig.get();
                                if g == 1.0 { "linear".to_string() } else { format!("{:.2}", g) }
                            }}</span>
                        </div>
                        <div style="text-align: right; padding-top: 4px;">
                            <button
                                class="layer-panel-opt"
                                style="display: inline; width: auto; padding: 2px 8px; font-size: 9px;"
                                on:click=move |_| {
                                    gain_sig.set(0.0);
                                    floor_sig.set(-120.0);
                                    range_sig.set(120.0);
                                    gamma_sig.set(1.0);
                                    state.display_auto_gain.set(false);
                                    if is_xform {
                                        state.display_filter_eq.set(DisplayFilterMode::Same);
                                        state.display_filter_notch.set(DisplayFilterMode::Same);
                                        state.display_filter_nr.set(DisplayFilterMode::Same);
                                        state.display_filter_transform.set(DisplayFilterMode::Same);
                                        state.display_filter_gain.set(DisplayFilterMode::Same);
                                        state.display_filter_decimate.set(DisplayFilterMode::Same);
                                        state.display_decimate_rate.set(48000);
                                        state.display_nr_strength.set(0.8);
                                    }
                                }
                            >"Reset"</button>
                        </div>
                    </div>
                }
            })}

            // Waveform view gain (when Waveform is active)
            {move || (state.main_view.get() == MainView::Waveform).then(|| {
                view! {
                    <hr />
                    <div class="dsp-custom-section">
                        <div class="dsp-custom-title">"Waveform Gain"</div>
                        <div class="dsp-custom-slider-row">
                            <button
                                class=move || if state.wave_view_auto_gain.get() {
                                    "layer-panel-opt selected"
                                } else {
                                    "layer-panel-opt"
                                }
                                style="font-size: 9px; padding: 2px 6px; width: auto; display: inline;"
                                on:click=move |_| {
                                    state.wave_view_auto_gain.set(!state.wave_view_auto_gain.get_untracked());
                                }
                            >"Auto"</button>
                        </div>
                        <div class="dsp-custom-slider-row">
                            <span class="dsp-slider-label">"Gain"</span>
                            <input
                                type="range"
                                class="setting-range"
                                min="-12" max="60" step="1"
                                prop:value=move || if state.wave_view_auto_gain.get() {
                                    state.compute_auto_gain().to_string()
                                } else {
                                    state.wave_view_gain_db.get().to_string()
                                }
                                on:input=move |ev: web_sys::Event| {
                                    let target = ev.target().unwrap();
                                    let input: web_sys::HtmlInputElement = target.unchecked_into();
                                    if let Ok(v) = input.value().parse::<f64>() {
                                        state.wave_view_gain_db.set(v);
                                        state.wave_view_auto_gain.set(false);
                                    }
                                }
                                on:dblclick=move |_| {
                                    state.wave_view_gain_db.set(0.0);
                                    state.wave_view_auto_gain.set(false);
                                }
                            />
                            <span class="dsp-custom-value">{move || {
                                if state.wave_view_auto_gain.get() {
                                    let db = state.compute_auto_gain();
                                    if db.abs() < 0.5 { "auto".to_string() }
                                    else { format!("a{:+.0}", db) }
                                } else {
                                    format!("{:+.0} dB", state.wave_view_gain_db.get())
                                }
                            }}</span>
                        </div>
                    </div>
                }
            })}

            // Flow algorithm options (when Flow is active)
            {move || (state.main_view.get() == MainView::Flow).then(|| {
                view! {
                    <hr />
                    <div class="layer-panel-title">"Algorithm"</div>
                    <button
                        class=move || layer_opt_class(state.spectrogram_display.get() == SpectrogramDisplay::FlowOptical)
                        on:click=move |_| state.spectrogram_display.set(SpectrogramDisplay::FlowOptical)
                    >"Optical"</button>
                    <button
                        class=move || layer_opt_class(state.spectrogram_display.get() == SpectrogramDisplay::PhaseCoherence)
                        on:click=move |_| state.spectrogram_display.set(SpectrogramDisplay::PhaseCoherence)
                    >"Phase Coherence"</button>
                    <button
                        class=move || layer_opt_class(state.spectrogram_display.get() == SpectrogramDisplay::FlowCentroid)
                        on:click=move |_| state.spectrogram_display.set(SpectrogramDisplay::FlowCentroid)
                    >"Centroid"</button>
                    <button
                        class=move || layer_opt_class(state.spectrogram_display.get() == SpectrogramDisplay::FlowGradient)
                        on:click=move |_| state.spectrogram_display.set(SpectrogramDisplay::FlowGradient)
                    >"Gradient"</button>
                    <button
                        class=move || layer_opt_class(state.spectrogram_display.get() == SpectrogramDisplay::Phase)
                        on:click=move |_| state.spectrogram_display.set(SpectrogramDisplay::Phase)
                    >"Phase"</button>

                    // Color scheme (only for non-phase flow algorithms)
                    {move || {
                        let display = state.spectrogram_display.get();
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
                                    state.flow_color_scheme.set(scheme);
                                }
                                prop:value=move || match state.flow_color_scheme.get() {
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
                                prop:value=move || (state.flow_intensity_gate.get() * 100.0).round().to_string()
                                on:input=move |ev: web_sys::Event| {
                                    let target = ev.target().unwrap();
                                    let input: web_sys::HtmlInputElement = target.unchecked_into();
                                    if let Ok(v) = input.value().parse::<f32>() {
                                        state.flow_intensity_gate.set(v / 100.0);
                                    }
                                }
                                on:dblclick=move |_| state.flow_intensity_gate.set(0.0)
                            />
                            <span class="dsp-custom-value">{move || format!("{}%", (state.flow_intensity_gate.get() * 100.0).round() as u32)}</span>
                        </div>
                        <div class="dsp-custom-slider-row">
                            <span class="dsp-slider-label">"Flow gate"</span>
                            <input
                                type="range"
                                class="setting-range"
                                min="0" max="100" step="1"
                                prop:value=move || (state.flow_gate.get() * 100.0).round().to_string()
                                on:input=move |ev: web_sys::Event| {
                                    let target = ev.target().unwrap();
                                    let input: web_sys::HtmlInputElement = target.unchecked_into();
                                    if let Ok(v) = input.value().parse::<f32>() {
                                        state.flow_gate.set(v / 100.0);
                                    }
                                }
                                on:dblclick=move |_| state.flow_gate.set(0.0)
                            />
                            <span class="dsp-custom-value">{move || format!("{}%", (state.flow_gate.get() * 100.0).round() as u32)}</span>
                        </div>
                        <div class="dsp-custom-slider-row">
                            <span class="dsp-slider-label">"Color gain"</span>
                            <input
                                type="range"
                                class="setting-range"
                                min="0.5" max="10.0" step="0.5"
                                prop:value=move || state.flow_shift_gain.get().to_string()
                                on:input=move |ev: web_sys::Event| {
                                    let target = ev.target().unwrap();
                                    let input: web_sys::HtmlInputElement = target.unchecked_into();
                                    if let Ok(v) = input.value().parse::<f32>() {
                                        state.flow_shift_gain.set(v);
                                    }
                                }
                                on:dblclick=move |_| state.flow_shift_gain.set(3.0)
                            />
                            <span class="dsp-custom-value">{move || format!("{:.1}x", state.flow_shift_gain.get())}</span>
                        </div>
                        <div class="dsp-custom-slider-row">
                            <span class="dsp-slider-label">"Contrast"</span>
                            <input
                                type="range"
                                class="setting-range"
                                min="0.2" max="3.0" step="0.05"
                                prop:value=move || state.flow_color_gamma.get().to_string()
                                on:input=move |ev: web_sys::Event| {
                                    let target = ev.target().unwrap();
                                    let input: web_sys::HtmlInputElement = target.unchecked_into();
                                    if let Ok(v) = input.value().parse::<f32>() {
                                        state.flow_color_gamma.set(v);
                                    }
                                }
                                on:dblclick=move |_| state.flow_color_gamma.set(1.0)
                            />
                            <span class="dsp-custom-value">{move || {
                                let g = state.flow_color_gamma.get();
                                if g == 1.0 { "linear".to_string() } else { format!("{:.2}", g) }
                            }}</span>
                        </div>
                    </div>
                }
            })}

            // Chromagram options (when Chromagram is active)
            {move || (state.main_view.get() == MainView::Chromagram).then(|| {
                view! {
                    <hr />
                    <div class="layer-panel-title">"Colormap"</div>
                    {ChromaColormap::ALL.iter().map(|&mode| {
                        view! {
                            <button
                                class=move || layer_opt_class(state.chroma_colormap.get() == mode)
                                on:click=move |_| state.chroma_colormap.set(mode)
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
                                class=move || layer_opt_class(state.chroma_range.get() == range)
                                on:click=move |_| state.chroma_range.set(range)
                            >
                                {range.label()}
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
                                prop:value=move || state.chroma_gain.get().round().to_string()
                                on:input=move |ev: web_sys::Event| {
                                    let target = ev.target().unwrap();
                                    let input: web_sys::HtmlInputElement = target.unchecked_into();
                                    if let Ok(v) = input.value().parse::<f32>() {
                                        state.chroma_gain.set(v);
                                    }
                                }
                                on:dblclick=move |_| state.chroma_gain.set(0.0)
                            />
                            <span class="dsp-custom-value">{move || {
                                let db = state.chroma_gain.get();
                                if db == 0.0 { "0 dB".to_string() } else { format!("{:+.0} dB", db) }
                            }}</span>
                        </div>
                        <div class="dsp-custom-slider-row">
                            <span class="dsp-slider-label">"Contrast"</span>
                            <input
                                type="range"
                                class="setting-range"
                                min="0.2" max="3.0" step="0.05"
                                prop:value=move || state.chroma_gamma.get().to_string()
                                on:input=move |ev: web_sys::Event| {
                                    let target = ev.target().unwrap();
                                    let input: web_sys::HtmlInputElement = target.unchecked_into();
                                    if let Ok(v) = input.value().parse::<f32>() {
                                        state.chroma_gamma.set(v);
                                    }
                                }
                                on:dblclick=move |_| state.chroma_gamma.set(1.0)
                            />
                            <span class="dsp-custom-value">{move || {
                                let g = state.chroma_gamma.get();
                                if g == 1.0 { "linear".to_string() } else { format!("{:.2}", g) }
                            }}</span>
                        </div>
                    </div>
                }
            })}
        </ComboButton>
    }
}

