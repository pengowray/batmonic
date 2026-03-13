use leptos::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use wasm_bindgen::closure::Closure;
use js_sys;
use std::cell::Cell;
use std::rc::Rc;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, MouseEvent};
use crate::canvas::spectrogram_renderer::{self, Colormap, ColormapMode, FreqMarkerState, FreqShiftMode, FlowAlgo, PreRendered, SpectDisplaySettings};
use crate::state::{AppState, CanvasTool, ColormapPreference, DisplayFilterMode, SpectrogramHandle, MainView, PlaybackMode, Selection, SpectrogramDisplay};

/// Compute per-row dB adjustments for display EQ and noise filtering.
/// Returns None if no adjustments are needed (both checkboxes off).
/// Row 0 = highest frequency, row (tile_height-1) = 0 Hz.
fn compute_freq_adjustments(state: &AppState, file_max_freq: f64, tile_height: usize) -> Option<Vec<f32>> {
    let show_eq = state.display_eq.get_untracked();
    let show_noise = state.display_noise_filter.get_untracked();
    if !show_eq && !show_noise {
        return None;
    }
    if tile_height == 0 { return None; }

    let mut adj = vec![0.0f32; tile_height];

    // EQ: apply per-band dB offsets
    if show_eq && state.filter_enabled.get_untracked() {
        let freq_low = state.filter_freq_low.get_untracked();
        let freq_high = state.filter_freq_high.get_untracked();
        let db_below = state.filter_db_below.get_untracked() as f32;
        let db_selected = state.filter_db_selected.get_untracked() as f32;
        let db_harmonics = state.filter_db_harmonics.get_untracked() as f32;
        let db_above = state.filter_db_above.get_untracked() as f32;
        let band_mode = state.filter_band_mode.get_untracked();
        let harm_active = band_mode >= 4 && freq_high > 0.0 && (freq_high / freq_low.max(1.0)) < 2.0;
        let harm_upper = freq_high * 2.0;

        for row in 0..tile_height {
            let bin = tile_height - 1 - row; // bin 0 = DC
            let freq = file_max_freq * bin as f64 / (tile_height - 1).max(1) as f64;
            let eq_db = if freq < freq_low {
                db_below
            } else if freq <= freq_high {
                db_selected
            } else if band_mode <= 2 {
                db_selected
            } else if harm_active && freq <= harm_upper {
                db_harmonics
            } else {
                db_above
            };
            adj[row] += eq_db;
        }
    }

    // Noise filtering: notch bands + spectral subtraction
    if show_noise {
        // Notch bands: check DSP filter state to determine if notch should show
        let show_notch = {
            let dsp_on = state.display_filter_enabled.get_untracked();
            if dsp_on {
                // DSP panel controls notch display
                match state.display_filter_notch.get_untracked() {
                    DisplayFilterMode::Off => false,
                    DisplayFilterMode::Auto | DisplayFilterMode::Same => state.notch_enabled.get_untracked(),
                    DisplayFilterMode::Custom => false,
                }
            } else {
                // Legacy: notch shows when playback notch is on
                state.notch_enabled.get_untracked()
            }
        };
        if show_notch {
            let bands = state.notch_bands.get_untracked();
            let harm_supp = state.notch_harmonic_suppression.get_untracked();
            for row in 0..tile_height {
                let bin = tile_height - 1 - row;
                let freq = file_max_freq * bin as f64 / (tile_height - 1).max(1) as f64;
                for band in &bands {
                    if !band.enabled { continue; }
                    let half_bw = band.bandwidth_hz / 2.0;
                    // Primary notch
                    if (freq - band.center_hz).abs() <= half_bw {
                        adj[row] -= band.strength_db as f32;
                    }
                    // Harmonic suppression at 2x and 3x
                    if harm_supp > 0.0 {
                        for harmonic in [2.0, 3.0] {
                            let hfreq = band.center_hz * harmonic;
                            if (freq - hfreq).abs() <= half_bw * harmonic {
                                adj[row] -= (band.strength_db * harm_supp) as f32;
                            }
                        }
                    }
                }
            }
        }

        // Spectral subtraction: use display auto noise floor when DSP NR is Auto,
        // custom strength when Custom, or playback noise floor when Same/fallback.
        {
            let dsp_enabled = state.display_filter_enabled.get_untracked();
            let nr_mode = state.display_filter_nr.get_untracked();

            let (nf_opt, strength) = if dsp_enabled && matches!(nr_mode, DisplayFilterMode::Auto) {
                // Auto: use display-specific auto-learned floor
                (state.display_auto_noise_floor.get_untracked(), 0.8)
            } else if dsp_enabled && matches!(nr_mode, DisplayFilterMode::Custom) {
                // Custom: prefer display auto floor with custom strength
                let floor = state.display_auto_noise_floor.get_untracked()
                    .or_else(|| state.noise_reduce_floor.get_untracked());
                (floor, state.display_nr_strength.get_untracked())
            } else if state.noise_reduce_enabled.get_untracked() {
                // Same/fallback: use playback noise floor
                (state.noise_reduce_floor.get_untracked(), state.noise_reduce_strength.get_untracked())
            } else {
                (None, 0.0)
            };

            if let Some(nf) = nf_opt {
                let nf_bins = nf.bin_magnitudes.len();
                let nf_max_freq = nf.sample_rate as f64 / 2.0;
                for row in 0..tile_height {
                    let bin = tile_height - 1 - row;
                    let freq = file_max_freq * bin as f64 / (tile_height - 1).max(1) as f64;
                    let nf_bin = ((freq / nf_max_freq) * (nf_bins - 1) as f64).round() as usize;
                    if nf_bin < nf_bins {
                        let noise_mag = nf.bin_magnitudes[nf_bin];
                        if noise_mag > 1e-15 {
                            let noise_db = 20.0 * (noise_mag as f32).log10();
                            adj[row] -= noise_db * strength as f32;
                        }
                    }
                }
            }
        }
    }

    Some(adj)
}

const LABEL_AREA_WIDTH: f64 = 60.0;

/// Hit-test all spectrogram overlay handles (FF + HET).
/// Returns the closest handle within `threshold` pixels, or None.
/// HET handles take priority over FF when they overlap and HET is manual.
fn hit_test_spec_handles(
    state: &AppState,
    mouse_y: f64,
    min_freq: f64,
    max_freq: f64,
    canvas_height: f64,
    threshold: f64,
) -> Option<SpectrogramHandle> {
    let mut candidates: Vec<(SpectrogramHandle, f64)> = Vec::new();

    // FF handles (always active when FF range is set)
    let ff_lo = state.ff_freq_lo.get_untracked();
    let ff_hi = state.ff_freq_hi.get_untracked();
    if ff_hi > ff_lo {
        let y_upper = spectrogram_renderer::freq_to_y(ff_hi.min(max_freq), min_freq, max_freq, canvas_height);
        let y_lower = spectrogram_renderer::freq_to_y(ff_lo.max(min_freq), min_freq, max_freq, canvas_height);
        let d_upper = (mouse_y - y_upper).abs();
        let d_lower = (mouse_y - y_lower).abs();
        if d_upper <= threshold { candidates.push((SpectrogramHandle::FfUpper, d_upper)); }
        if d_lower <= threshold { candidates.push((SpectrogramHandle::FfLower, d_lower)); }
        // Middle handle (midpoint between boundaries)
        let mid_freq = (ff_lo + ff_hi) / 2.0;
        let y_mid = spectrogram_renderer::freq_to_y(mid_freq.clamp(min_freq, max_freq), min_freq, max_freq, canvas_height);
        let d_mid = (mouse_y - y_mid).abs();
        if d_mid <= threshold { candidates.push((SpectrogramHandle::FfMiddle, d_mid)); }
    }

    // HET handles (only when in HET mode and parameter is manual)
    if state.playback_mode.get_untracked() == PlaybackMode::Heterodyne {
        let het_freq = state.het_frequency.get_untracked();
        let het_cutoff = state.het_cutoff.get_untracked();

        if !state.het_freq_auto.get_untracked() {
            let y_center = spectrogram_renderer::freq_to_y(het_freq, min_freq, max_freq, canvas_height);
            let d = (mouse_y - y_center).abs();
            if d <= threshold { candidates.push((SpectrogramHandle::HetCenter, d)); }
        }
        if !state.het_cutoff_auto.get_untracked() {
            let y_upper = spectrogram_renderer::freq_to_y(
                (het_freq + het_cutoff).min(max_freq), min_freq, max_freq, canvas_height,
            );
            let y_lower = spectrogram_renderer::freq_to_y(
                (het_freq - het_cutoff).max(min_freq), min_freq, max_freq, canvas_height,
            );
            let d_upper = (mouse_y - y_upper).abs();
            let d_lower = (mouse_y - y_lower).abs();
            if d_upper <= threshold { candidates.push((SpectrogramHandle::HetBandUpper, d_upper)); }
            if d_lower <= threshold { candidates.push((SpectrogramHandle::HetBandLower, d_lower)); }
        }
    }

    if candidates.is_empty() { return None; }

    // Sort by distance, then prefer HET over FF when tied
    candidates.sort_by(|a, b| {
        a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                let a_het = matches!(a.0, SpectrogramHandle::HetCenter | SpectrogramHandle::HetBandUpper | SpectrogramHandle::HetBandLower);
                let b_het = matches!(b.0, SpectrogramHandle::HetCenter | SpectrogramHandle::HetBandUpper | SpectrogramHandle::HetBandLower);
                b_het.cmp(&a_het) // HET first
            })
    });

    Some(candidates[0].0)
}

#[component]
pub fn Spectrogram() -> impl IntoView {
    let state = expect_context::<AppState>();
    let canvas_ref = NodeRef::<leptos::html::Canvas>::new();

    let pre_rendered: RwSignal<Option<PreRendered>> = RwSignal::new(None);
    let _flow_cache_removed = (); // flow tiles are now in tile_cache::MV_CACHE

    // Drag state for selection (time, freq)
    let drag_start = RwSignal::new((0.0f64, 0.0f64));
    // Hand-tool drag state: (initial_client_x, initial_scroll_offset)
    let hand_drag_start = RwSignal::new((0.0f64, 0.0f64));
    let pinch_state: RwSignal<Option<crate::components::pinch::PinchState>> = RwSignal::new(None);
    let axis_drag_raw_start = RwSignal::new(0.0f64);
    let last_tap_time = RwSignal::new(0.0f64);
    // Time-axis tooltip: (x_px, tooltip_text) — None when not hovering the axis
    let time_axis_tooltip: RwSignal<Option<(f64, String)>> = RwSignal::new(None);
    let last_tap_x = RwSignal::new(0.0f64);

    // Label hover animation: lerp label_hover_opacity toward target.
    // The Effect subscribes to BOTH label_hover_target and label_hover_opacity.
    // When the rAF callback sets opacity, the Effect re-runs automatically —
    // no need to re-trigger via setting the target signal.
    // A generation counter ensures stale rAF callbacks are discarded when a new
    // animation cycle starts (e.g. target changes mid-flight).
    let label_hover_target = RwSignal::new(0.0f64);
    let anim_gen: Rc<Cell<u32>> = Rc::new(Cell::new(0));
    Effect::new(move || {
        let target = label_hover_target.get();
        let current = state.label_hover_opacity.get();
        if (current - target).abs() < 0.01 {
            if current != target {
                state.label_hover_opacity.set(target);
            }
            return;
        }
        let generation = anim_gen.get().wrapping_add(1);
        anim_gen.set(generation);
        let ag = anim_gen.clone();
        let cb = Closure::once(move || {
            if ag.get() != generation { return; }
            let cur = state.label_hover_opacity.get_untracked();
            let tgt = label_hover_target.get_untracked();
            let speed = if tgt > cur { 0.35 } else { 0.20 };
            let next = cur + (tgt - cur) * speed;
            let next = if (next - tgt).abs() < 0.01 { tgt } else { next };
            state.label_hover_opacity.set(next);
        });
        let _ = web_sys::window().unwrap().request_animation_frame(
            cb.as_ref().unchecked_ref(),
        );
        cb.forget();
    });

    // Effect 1: pre-render small files (when columns are in memory and not in flow mode)
    Effect::new(move || {
        let files = state.files.get();
        let idx = state.current_file_index.get();
        let enabled = state.flow_enabled.get();
        if let Some(i) = idx {
            if let Some(file) = files.get(i) {
                if file.spectrogram.columns.is_empty() || enabled {
                    // Tile-based rendering (normal or flow) — no monolithic pre-render
                    pre_rendered.set(None);
                } else {
                    pre_rendered.set(Some(spectrogram_renderer::pre_render(&file.spectrogram)));
                }
            }
        } else {
            pre_rendered.set(None);
        }
    });

    // Effect 2: clear flow tile cache when algorithm or enabled state changes
    // Gate/opacity/gain are now applied at render time (not baked into tiles)
    Effect::new(move || {
        let _display = state.spectrogram_display.get();
        let _enabled = state.flow_enabled.get();
        // Clear flow tiles so they recompute with new algorithm
        crate::canvas::tile_cache::clear_flow_cache();
    });

    // (coherence tiles now use flow cache — cleared in Effect 2 above)

    // Effect 2b: clear all magnitude tiles AND flow tiles AND reassignment tiles when FFT mode changes
    Effect::new(move || {
        let _fft = state.spect_fft_mode.get();
        crate::canvas::tile_cache::clear_all_tiles();
        crate::canvas::tile_cache::clear_flow_cache();
        crate::canvas::tile_cache::clear_reassign_cache();
        state.tile_ready_signal.update(|n| *n = n.wrapping_add(1));
    });

    // Effect 2c: clear magnitude tiles when display transform/decimation toggles or params change
    {
        let prev_xform = RwSignal::new(false);
        let prev_decim = RwSignal::new(0u32);
        Effect::new(move || {
            let xform_on = state.display_transform.get();
            let _mode = state.playback_mode.get();
            let _het = state.het_frequency.get();
            let _het_cut = state.het_cutoff.get();
            let _te = state.te_factor.get();
            let _ps = state.ps_factor.get();
            let _pv = state.pv_factor.get();
            let _zc = state.zc_factor.get();
            let decim = state.display_decimate_effective.get();
            let decim_changed = decim != prev_decim.get_untracked();
            // Clear tiles when transform is active (param changed), toggled off, or decimation changed
            if xform_on || prev_xform.get_untracked() || decim_changed {
                crate::canvas::tile_cache::clear_all_tiles();
                state.tile_ready_signal.update(|n| *n = n.wrapping_add(1));
            }
            prev_xform.set(xform_on);
            prev_decim.set(decim);
        });
    }

    // Effect 2d: clear reassignment tile cache when toggle changes
    Effect::new(move || {
        let _reassign = state.reassign_enabled.get();
        crate::canvas::tile_cache::clear_reassign_cache();
    });

    // Effect 3: redraw when pre-rendered data, scroll, zoom, selection, playhead, overlays, hover, or new tile change
    Effect::new(move || {
        let _tile_ready = state.tile_ready_signal.get(); // trigger redraw when tiles arrive
        let scroll = state.scroll_offset.get();
        let zoom = state.zoom_level.get();
        let bookmarks = state.bookmarks.get();
        let canvas_tool = state.canvas_tool.get();
        let selection = state.selection.get();
        let is_playing = state.is_playing.get();
        let het_interacting = state.het_interacting.get();
        let dragging = state.is_dragging.get();
        let het_freq = state.het_frequency.get();
        let het_cutoff = state.het_cutoff.get();
        let te_factor = state.te_factor.get();
        let ps_factor = state.ps_factor.get();
        let pv_factor = state.pv_factor.get();
        let playback_mode = state.playback_mode.get();
        let min_display_freq = state.min_display_freq.get();
        let max_display_freq = state.max_display_freq.get();
        let mouse_freq = state.mouse_freq.get();
        let mouse_cx = state.mouse_canvas_x.get();
        let label_opacity = state.label_hover_opacity.get();
        let filter_hovering = state.filter_hovering_band.get();
        let filter_enabled = state.filter_enabled.get();
        let spec_hover = state.spec_hover_handle.get();
        let spec_drag = state.spec_drag_handle.get();
        let ff_lo = state.ff_freq_lo.get();
        let ff_hi = state.ff_freq_hi.get();
        let het_freq_auto = state.het_freq_auto.get();
        let het_cutoff_auto = state.het_cutoff_auto.get();
        let hfr_enabled = state.hfr_enabled.get();
        let flow_on = state.flow_enabled.get_untracked();
        let _flow_ig = state.flow_intensity_gate.get(); // trigger redraw on flow setting change
        let _flow_mg = state.flow_gate.get();
        let _flow_sg = state.flow_shift_gain.get();
        let _flow_cg = state.flow_color_gamma.get();
        let _flow_scheme = state.flow_color_scheme.get(); // trigger redraw on color scheme change
        let colormap_pref = state.colormap_preference.get();
        let hfr_colormap_pref = state.hfr_colormap_preference.get();
        let axis_drag_start = state.axis_drag_start_freq.get();
        let axis_drag_current = state.axis_drag_current_freq.get();
        let notch_bands = state.notch_bands.get();
        let notch_enabled = state.notch_enabled.get();
        let notch_hovering = state.notch_hovering_band.get();
        let harmonic_suppression = state.notch_harmonic_suppression.get();
        let detected_pulses = state.detected_pulses.get();
        let pulse_overlay = state.pulse_overlay_enabled.get();
        let selected_pulse = state.selected_pulse_index.get();
        let main_view = state.main_view.get();
        let (spect_floor, spect_range, spect_gamma, spect_gain) = if main_view == MainView::XformedSpec {
            (state.xform_spect_floor_db.get(), state.xform_spect_range_db.get(), state.xform_spect_gamma.get(), state.xform_spect_gain_db.get())
        } else {
            (state.spect_floor_db.get(), state.spect_range_db.get(), state.spect_gamma.get(), state.spect_gain_db.get())
        };
        let debug_tiles = state.debug_tiles.get();
        let reassign_on = state.reassign_enabled.get();
        // Display-affecting checkbox subscriptions
        let display_auto_gain = state.display_auto_gain.get();
        let _display_eq = state.display_eq.get();
        let _display_noise_filter = state.display_noise_filter.get();
        let _f_freq_lo = state.filter_freq_low.get();
        let _f_freq_hi = state.filter_freq_high.get();
        let _f_db_below = state.filter_db_below.get();
        let _f_db_selected = state.filter_db_selected.get();
        let _f_db_harmonics = state.filter_db_harmonics.get();
        let _f_db_above = state.filter_db_above.get();
        let _f_band_mode = state.filter_band_mode.get();
        let _nr_enabled = state.noise_reduce_enabled.get();
        let _nr_strength = state.noise_reduce_strength.get();
        let _nr_floor_v = state.noise_reduce_floor.get();
        // Display DSP filter subscriptions
        let _dsp_enabled = state.display_filter_enabled.get();
        let _dsp_nr = state.display_filter_nr.get();
        let _dsp_eq = state.display_filter_eq.get();
        let _dsp_notch = state.display_filter_notch.get();
        let _dsp_gain = state.display_filter_gain.get();
        let _dsp_nr_strength = state.display_nr_strength.get();
        let _dsp_auto_floor = state.display_auto_noise_floor.get();
        let _dsp_transform = state.display_transform.get();
        let _dsp_decimate = state.display_decimate_effective.get();
        let annotation_store = state.annotation_store.get();
        let selected_annotation_ids = state.selected_annotation_ids.get();
        let _pre = pre_rendered.track();

        let Some(canvas_el) = canvas_ref.get() else { return };
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
        // Keep overview in sync with actual canvas width
        state.spectrogram_canvas_width.set(display_w as f64);

        let ctx = canvas
            .get_context("2d")
            .unwrap()
            .unwrap()
            .dyn_into::<CanvasRenderingContext2d>()
            .unwrap();

        let files = state.files.get_untracked();
        let idx = state.current_file_index.get_untracked();
        let time_res = idx
            .and_then(|i| files.get(i))
            .map(|f| f.spectrogram.time_resolution)
            .unwrap_or(1.0);
        let scroll_col = scroll / time_res;
        let original_max_freq = idx
            .and_then(|i| files.get(i))
            .map(|f| f.spectrogram.max_freq)
            .unwrap_or(96_000.0);
        let decim_effective = state.display_decimate_effective.get_untracked();
        let original_sample_rate = idx
            .and_then(|i| files.get(i))
            .map(|f| f.spectrogram.sample_rate)
            .unwrap_or(192_000);
        let file_max_freq = if decim_effective > 0 && decim_effective < original_sample_rate {
            let effective_rate = crate::dsp::filters::decimated_rate(original_sample_rate, decim_effective);
            effective_rate as f64 / 2.0
        } else {
            original_max_freq
        };
        let max_freq = max_display_freq.unwrap_or(file_max_freq).min(file_max_freq);
        let min_freq = min_display_freq.unwrap_or(0.0);
        let freq_crop_lo = min_freq / file_max_freq;
        let freq_crop_hi = (max_freq / file_max_freq).min(1.0);

        // --- Normal spectrogram mode ---

        // Build colormap
        let pref_to_colormap = |p: ColormapPreference| -> Colormap {
            match p {
                ColormapPreference::Viridis => Colormap::Viridis,
                ColormapPreference::Inferno => Colormap::Inferno,
                ColormapPreference::Magma => Colormap::Magma,
                ColormapPreference::Plasma => Colormap::Plasma,
                ColormapPreference::Cividis => Colormap::Cividis,
                ColormapPreference::Turbo => Colormap::Turbo,
                ColormapPreference::Greyscale => Colormap::Greyscale,
            }
        };
        let xform_or_decim = state.display_transform.get_untracked()
            || main_view == MainView::XformedSpec
            || decim_effective > 0;
        let colormap = if flow_on {
            ColormapMode::Uniform(Colormap::Greyscale)
        } else if hfr_enabled && ff_hi > ff_lo && !xform_or_decim {
            ColormapMode::HfrFocus {
                colormap: pref_to_colormap(hfr_colormap_pref),
                ff_lo_frac: ff_lo / file_max_freq,
                ff_hi_frac: ff_hi / file_max_freq,
            }
        } else if hfr_enabled {
            ColormapMode::Uniform(pref_to_colormap(hfr_colormap_pref))
        } else {
            ColormapMode::Uniform(pref_to_colormap(colormap_pref))
        };

        let file = idx.and_then(|i| files.get(i));
        let total_cols = file.map(|f| {
            let tc = f.spectrogram.total_columns;
            if tc > 0 { tc } else { f.spectrogram.columns.len() }
        }).unwrap_or(0);
        let file_idx_val = idx.unwrap_or(0);
        let visible_time = (display_w as f64 / zoom) * time_res;
        let duration = file.map(|f| f.audio.duration_secs).unwrap_or(0.0);

        // Compute reference dB level for mapping absolute-dB tile data to display.
        // When display_auto_gain is ON: peak-normalize using the file's running
        // max magnitude (ref_db shifts 0 dB to the file's loudest point).
        // When OFF: use a fixed reference based on FFT size so brightness is
        // independent of file content and stable during progressive loading.
        // Fixed ref ≈ 20*log10(fft_size/4) accounts for the Hann window's
        // coherent gain (~0.5) on the one-sided spectrum, giving ~dBFS values.
        let fft_size = state.spect_fft_mode.get_untracked().max_fft_size() as f32;
        let fixed_ref_db = 20.0 * (fft_size / 4.0).log10();

        let ref_db = if display_auto_gain && total_cols > 0 {
            use crate::canvas::spectral_store;
            let max_mag = spectral_store::get_max_magnitude(file_idx_val);
            if max_mag > 0.0 { 20.0 * max_mag.log10() } else { fixed_ref_db }
        } else {
            fixed_ref_db
        };

        // Extra dB boost from Auto/Same gain modes (computed in app.rs Effect)
        let display_boost = state.display_gain_boost.get();

        let display_settings = SpectDisplaySettings {
            floor_db: spect_floor,
            range_db: spect_range,
            gamma: spect_gamma,
            gain_db: spect_gain - ref_db + display_boost,
        };

        // During recording, clip the canvas so partial tiles don't show black padding.
        // The clipping region ends at the rightmost column with actual data.
        let is_recording = state.mic_recording.get_untracked();
        let live_data_cols = state.mic_live_data_cols.get_untracked();
        if is_recording && live_data_cols > 0 {
            ctx.save();
            let data_end_col = live_data_cols as f64;
            let data_end_px = (data_end_col - scroll_col) * zoom;
            // Clip to [0, 0, data_end_px, canvas_height]
            ctx.begin_path();
            ctx.rect(0.0, 0.0, data_end_px.max(0.0), display_h as f64);
            ctx.clip();
        }
        // Pre-compute per-frequency dB adjustments for display EQ / noise filter
        let tile_height = state.spect_fft_mode.get_untracked().max_fft_size() / 2 + 1;
        let freq_adjustments = compute_freq_adjustments(&state, file_max_freq, tile_height);

        // Step 1: Render base spectrogram.
        // Priority: flow tiles | normal tiles > pre_rendered > preview > black
        let base_drawn = if flow_on && total_cols > 0 {
            // Flow mode (includes phase coherence): composite dB+shift tiles at render time
            let ig = state.flow_intensity_gate.get_untracked();
            let mg = state.flow_gate.get_untracked();
            let op = 1.0_f32; // opacity consolidated into color gain
            let sg = state.flow_shift_gain.get_untracked();
            let cg = state.flow_color_gamma.get_untracked();
            let display = state.spectrogram_display.get_untracked();
            let algo = match display {
                SpectrogramDisplay::FlowOptical => FlowAlgo::Optical,
                SpectrogramDisplay::PhaseCoherence => FlowAlgo::PhaseCoherence,
                SpectrogramDisplay::FlowCentroid => FlowAlgo::Centroid,
                SpectrogramDisplay::FlowGradient => FlowAlgo::Gradient,
                SpectrogramDisplay::Phase => FlowAlgo::Phase,
            };
            let flow_scheme = state.flow_color_scheme.get_untracked();
            let drawn = spectrogram_renderer::blit_flow_tiles_viewport(
                &ctx, canvas, file_idx_val, total_cols,
                scroll_col, zoom, freq_crop_lo, freq_crop_hi,
                &display_settings, freq_adjustments.as_deref(),
                ig, mg, op, sg, cg, algo, flow_scheme,
                file.and_then(|f| f.preview.as_ref()),
                scroll, visible_time, duration,
            );

            // Schedule missing flow tiles at ideal LOD
            {
                use crate::canvas::tile_cache::{self, TILE_COLS};

                let ideal_lod = tile_cache::select_lod(zoom);
                let ratio = tile_cache::lod_ratio(ideal_lod);

                let vis_start = scroll_col.max(0.0).min((total_cols as f64 - 1.0).max(0.0));
                let vis_end = (vis_start + display_w as f64 / zoom).min(total_cols as f64);
                if vis_end <= vis_start { /* nothing visible */ }
                else {

                // Convert to ideal-LOD tile space
                let vis_start_lod = vis_start * ratio;
                let vis_end_lod = vis_end * ratio;
                let first_tile = (vis_start_lod / TILE_COLS as f64).floor() as usize;
                let last_tile = ((vis_end_lod - 0.001).max(0.0) / TILE_COLS as f64).floor() as usize;

                for t in first_tile..=last_tile {
                    // Schedule ideal LOD tile
                    if tile_cache::get_flow_tile(file_idx_val, ideal_lod, t).is_none() {
                        tile_cache::schedule_flow_tile(state.clone(), file_idx_val, ideal_lod, t, algo);
                    }

                    // Also ensure a LOD1 fallback exists for smooth transitions
                    if ideal_lod != 1 {
                        let (fb_tile, _, _) = tile_cache::fallback_tile_info(ideal_lod, t, 1);
                        if tile_cache::get_flow_tile(file_idx_val, 1, fb_tile).is_none() {
                            tile_cache::schedule_flow_tile(state.clone(), file_idx_val, 1, fb_tile, algo);
                        }
                    }
                }
                }
            }

            drawn
        } else if !flow_on && total_cols > 0 {
            // Normal or reassignment tile-based rendering
            let ideal_lod_for_source = crate::canvas::tile_cache::select_lod(zoom);
            let tile_source = if reassign_on && ideal_lod_for_source > 0 {
                spectrogram_renderer::TileSource::Reassigned
            } else {
                spectrogram_renderer::TileSource::Normal
            };
            // Skip preview fallback when xform/decimation is active (preview shows original untransformed data)
            let xform_on = state.display_transform.get_untracked();
            let preview_ref = if xform_on || decim_effective > 0 {
                None
            } else {
                file.and_then(|f| f.preview.as_ref())
            };
            let drawn = spectrogram_renderer::blit_tiles_viewport(
                &ctx, canvas, file_idx_val, total_cols,
                scroll_col, zoom, freq_crop_lo, freq_crop_hi, colormap,
                &display_settings,
                freq_adjustments.as_deref(),
                preview_ref,
                scroll, visible_time, duration,
                tile_source,
            );

            // Schedule missing tiles at the ideal LOD for the current zoom
            {
                use crate::canvas::tile_cache::{self, TILE_COLS};
                use crate::canvas::spectral_store;

                let ideal_lod = tile_cache::select_lod(zoom);
                let ratio = tile_cache::lod_ratio(ideal_lod);

                // Clamp vis_start to valid range (must match renderer's clamping)
                let vis_start = scroll_col.max(0.0).min((total_cols as f64 - 1.0).max(0.0));
                let vis_end = (vis_start + display_w as f64 / zoom).min(total_cols as f64);

                if vis_end > vis_start {
                // Tile range at ideal LOD
                let vis_start_lod = vis_start * ratio;
                let vis_end_lod = vis_end * ratio;
                let first_tile = (vis_start_lod / TILE_COLS as f64).floor() as usize;
                let last_tile = ((vis_end_lod - 0.001).max(0.0) / TILE_COLS as f64).floor() as usize;

                // Cancel stale in-flight entries far from viewport (prevents stuck tiles during fast scroll)
                let viewport_center_tile = ((vis_start_lod + vis_end_lod) / 2.0 / TILE_COLS as f64) as usize;
                let visible_tile_count = last_tile.saturating_sub(first_tile) + 1;
                let keep_cancel = visible_tile_count.max(10) * 3;
                let keep_evict = visible_tile_count.max(10) * 5;

                // During playback, also protect tiles near the pre-play scroll position
                // so they don't need to be regenerated when playback stops.
                if is_playing {
                    let pre_scroll = state.pre_play_scroll.get_untracked();
                    let pre_col = (pre_scroll / time_res).max(0.0).min((total_cols as f64 - 1.0).max(0.0));
                    let pre_end_col = (pre_col + display_w as f64 / zoom).min(total_cols as f64);
                    let pre_center = (((pre_col * ratio) + (pre_end_col * ratio)) / 2.0 / TILE_COLS as f64) as usize;

                    tile_cache::cancel_far_in_flight_multi(file_idx_val, ideal_lod, &[
                        (viewport_center_tile, keep_cancel), (pre_center, keep_cancel)
                    ]);
                    tile_cache::evict_far_multi(file_idx_val, ideal_lod, &[
                        (viewport_center_tile, keep_evict), (pre_center, keep_evict)
                    ]);
                } else {
                    tile_cache::cancel_far_in_flight(file_idx_val, ideal_lod, viewport_center_tile, keep_cancel);
                    tile_cache::evict_far(file_idx_val, ideal_lod, viewport_center_tile, keep_evict);
                }

                let is_loading = state.loading_files.with_untracked(|v| !v.is_empty());

                let use_reassign = reassign_on && ideal_lod > 0;

                for t in first_tile..=last_tile {
                    // Schedule reassignment tiles when enabled (skip LOD0)
                    if use_reassign {
                        if tile_cache::get_reassign_tile(file_idx_val, ideal_lod, t).is_none() {
                            tile_cache::schedule_reassign_tile(state.clone(), file_idx_val, ideal_lod, t);
                        }
                    }

                    // Always schedule normal tiles (for fallback and non-reassign mode)
                    if tile_cache::get_tile(file_idx_val, ideal_lod, t).is_none() {
                        tile_cache::schedule_tile_lod(state.clone(), file_idx_val, ideal_lod, t);
                    }

                    // Also ensure a LOD1 fallback tile exists (for smooth transitions)
                    if ideal_lod != 1 {
                        // Map this ideal-LOD tile back to LOD1 tile space
                        let (fb_tile, _, _) = tile_cache::fallback_tile_info(ideal_lod, t, 1);
                        if tile_cache::get_tile(file_idx_val, 1, fb_tile).is_none() {
                            if !is_loading {
                                let tile_start = fb_tile * TILE_COLS;
                                let tile_end = (tile_start + TILE_COLS).min(total_cols);
                                if spectral_store::has_store(file_idx_val)
                                    && spectral_store::tile_complete(file_idx_val, tile_start, tile_end)
                                {
                                    tile_cache::schedule_tile_from_store(state.clone(), file_idx_val, fb_tile);
                                } else {
                                    tile_cache::schedule_tile_on_demand(state.clone(), file_idx_val, fb_tile);
                                }
                            }
                        }
                    }
                }

                // When ideal LOD is 1, also schedule LOD1 from store/on-demand
                // for any missing tiles (same as before)
                if ideal_lod == 1 && !is_loading {
                    let lod1_first = (vis_start / TILE_COLS as f64).floor() as usize;
                    let lod1_last = ((vis_end - 0.001).max(0.0) / TILE_COLS as f64).floor() as usize;
                    for t in lod1_first..=lod1_last {
                        if tile_cache::get_tile(file_idx_val, 1, t).is_none() {
                            let tile_start = t * TILE_COLS;
                            let tile_end = (tile_start + TILE_COLS).min(total_cols);
                            if spectral_store::has_store(file_idx_val)
                                && spectral_store::tile_complete(file_idx_val, tile_start, tile_end)
                            {
                                tile_cache::schedule_tile_from_store(state.clone(), file_idx_val, t);
                            } else {
                                tile_cache::schedule_tile_on_demand(state.clone(), file_idx_val, t);
                            }
                        }
                    }
                }

                // Recovery: if visible tiles are missing and not being computed,
                // force a retry after a short delay to break stuck states.
                let missing = tile_cache::count_missing_visible(file_idx_val, ideal_lod, first_tile, last_tile);
                if missing > 0 {
                    let state_recovery = state;
                    let recovery_cb = Closure::once(move || {
                        state_recovery.tile_ready_signal.update(|n| *n = n.wrapping_add(1));
                    });
                    let _ = web_sys::window().unwrap()
                        .set_timeout_with_callback_and_timeout_and_arguments_0(
                            recovery_cb.as_ref().unchecked_ref(), 500,
                        );
                    recovery_cb.forget();
                }
                }
            }

            drawn
        } else if pre_rendered.with_untracked(|pr| pr.is_some()) {
            // Small file with columns in memory — use monolithic pre_rendered
            pre_rendered.with_untracked(|pr| {
                if let Some(rendered) = pr {
                    spectrogram_renderer::blit_viewport(
                        &ctx, rendered, canvas, scroll_col, zoom,
                        freq_crop_lo, freq_crop_hi, colormap,
                    );
                }
            });
            true
        } else if let Some(pv) = file.and_then(|f| f.preview.as_ref()) {
            spectrogram_renderer::blit_preview_as_background(
                &ctx, pv, canvas,
                scroll, visible_time, duration,
                freq_crop_lo, freq_crop_hi,
                colormap,
            );
            true
        } else {
            ctx.set_fill_style_str("#000");
            ctx.fill_rect(0.0, 0.0, display_w as f64, display_h as f64);
            false
        };

        // Restore canvas state if we clipped for recording
        if is_recording && live_data_cols > 0 {
            ctx.restore();
        }

        // Tile debug overlay (drawn on top of tiles, under other overlays)
        if debug_tiles && total_cols > 0 {
            spectrogram_renderer::draw_tile_debug_overlay(
                &ctx, canvas, file_idx_val, total_cols, scroll_col, zoom,
                state.spect_fft_mode.get_untracked().max_fft_size(), flow_on,
            );
        }

        // Step 2: Draw overlays on top of the base spectrogram
        if base_drawn {
            let show_het = het_interacting
                || playback_mode == PlaybackMode::Heterodyne;
            let shift_mode = if show_het {
                FreqShiftMode::Heterodyne(het_freq)
            } else {
                match playback_mode {
                    PlaybackMode::TimeExpansion if te_factor > 1.0 => FreqShiftMode::Divide(te_factor),
                    PlaybackMode::TimeExpansion if te_factor < -1.0 => FreqShiftMode::Multiply(te_factor.abs()),
                    PlaybackMode::PitchShift if ps_factor > 1.0 => FreqShiftMode::Divide(ps_factor),
                    PlaybackMode::PitchShift if ps_factor < -1.0 => FreqShiftMode::Multiply(ps_factor.abs()),
                    PlaybackMode::PhaseVocoder if pv_factor > 1.0 => FreqShiftMode::Divide(pv_factor),
                    PlaybackMode::PhaseVocoder if pv_factor < -1.0 => FreqShiftMode::Multiply(pv_factor.abs()),
                    PlaybackMode::ZeroCrossing => FreqShiftMode::Divide(state.zc_factor.get()),
                    _ => FreqShiftMode::None,
                }
            };

            let (adl2, adh2) = match (axis_drag_start, axis_drag_current) {
                (Some(a), Some(b)) => (Some(a.min(b)), Some(a.max(b))),
                _ => (None, None),
            };
            let ff_drag_active2 = matches!(spec_drag, Some(SpectrogramHandle::FfUpper) | Some(SpectrogramHandle::FfLower) | Some(SpectrogramHandle::FfMiddle));
            let marker_state = FreqMarkerState {
                mouse_freq,
                mouse_in_label_area: mouse_freq.is_some() && mouse_cx < LABEL_AREA_WIDTH,
                label_hover_opacity: label_opacity,
                has_selection: selection.is_some() || (dragging && axis_drag_start.is_none()),
                file_max_freq,
                axis_drag_lo: adl2,
                axis_drag_hi: adh2,
                ff_drag_active: ff_drag_active2,
                ff_lo,
                ff_hi,
                ff_handles_active: spec_hover.is_some() || spec_drag.is_some(),
            };

            let xform_on = state.display_transform.get_untracked()
                || main_view == MainView::XformedSpec;
            // When xform/decim is on, adjust marker state: right-side labels, hide focus handles
            let marker_state = if xform_on || decim_effective > 0 {
                FreqMarkerState {
                    mouse_in_label_area: mouse_freq.is_some() && mouse_cx > (display_w as f64 - LABEL_AREA_WIDTH),
                    ff_lo: 0.0,
                    ff_hi: 0.0,
                    ff_drag_active: false,
                    ff_handles_active: false,
                    ..marker_state
                }
            } else {
                marker_state
            };
            spectrogram_renderer::draw_freq_markers(
                &ctx,
                min_freq,
                max_freq,
                display_h as f64,
                display_w as f64,
                if xform_on { FreqShiftMode::None } else { shift_mode },
                &marker_state,
                het_cutoff,
                xform_on,
            );

            // Time scale along the bottom edge
            {
                let clock_cfg = state.current_file()
                    .and_then(|f| f.recording_start_epoch_ms())
                    .map(|ms| crate::canvas::time_markers::ClockTimeConfig {
                        recording_start_epoch_ms: ms,
                    });
                let time_scale = if xform_on && playback_mode == PlaybackMode::TimeExpansion && te_factor.abs() > 1.0 {
                    te_factor.abs()
                } else {
                    1.0
                };
                spectrogram_renderer::draw_time_markers(
                    &ctx,
                    scroll,
                    visible_time,
                    display_w as f64,
                    display_h as f64,
                    duration,
                    clock_cfg,
                    state.show_clock_time.get(),
                    time_scale,
                );
            }

            // Pulse detection overlay
            if pulse_overlay && !detected_pulses.is_empty() {
                spectrogram_renderer::draw_pulses(
                    &ctx,
                    &detected_pulses,
                    selected_pulse,
                    scroll,
                    time_res,
                    zoom,
                    display_w as f64,
                    display_h as f64,
                );
            }

            // Notch filter band markers
            if !notch_bands.is_empty() {
                spectrogram_renderer::draw_notch_bands(
                    &ctx,
                    min_freq, max_freq,
                    display_h as f64, display_w as f64,
                    &notch_bands, notch_enabled,
                    notch_hovering,
                    harmonic_suppression,
                );
            }

            // FF overlay (dim outside focus range) — skip in xform view
            if ff_hi > ff_lo && !xform_on {
                spectrogram_renderer::draw_ff_overlay(
                    &ctx,
                    ff_lo, ff_hi,
                    min_freq, max_freq,
                    display_h as f64, display_w as f64,
                    spec_hover, spec_drag,
                );
            }

            // HET overlay (cyan lines on top, no dimming) — skip in xform view
            if show_het && !xform_on {
                let het_interactive = !het_freq_auto || !het_cutoff_auto;
                spectrogram_renderer::draw_het_overlay(
                    &ctx,
                    het_freq,
                    het_cutoff,
                    min_freq,
                    max_freq,
                    display_h as f64,
                    display_w as f64,
                    spec_hover,
                    spec_drag,
                    het_interactive,
                );
            }

            // Draw selection overlay
            if let Some(sel) = selection {
                spectrogram_renderer::draw_selection(
                    &ctx,
                    &sel,
                    min_freq,
                    max_freq,
                    scroll,
                    time_res,
                    zoom,
                    display_w as f64,
                    display_h as f64,
                );
                if dragging {
                    spectrogram_renderer::draw_harmonic_shadows(
                        &ctx,
                        &sel,
                        min_freq,
                        max_freq,
                        scroll,
                        time_res,
                        zoom,
                        display_w as f64,
                        display_h as f64,
                    );
                }
            }

            // Draw saved annotation selections (skip in xform view)
            if !xform_on {
                if let Some(file_idx_val) = idx {
                    if let Some(Some(set)) = annotation_store.sets.get(file_idx_val) {
                        spectrogram_renderer::draw_annotations(
                            &ctx,
                            set,
                            &selected_annotation_ids,
                            min_freq,
                            max_freq,
                            scroll,
                            time_res,
                            zoom,
                            display_w as f64,
                            display_h as f64,
                        );
                    }
                }
            }

            // Draw filter band overlay when hovering a slider
            if filter_enabled {
                if let Some(band) = filter_hovering {
                    spectrogram_renderer::draw_filter_overlay(
                        &ctx,
                        band,
                        state.filter_freq_low.get_untracked(),
                        state.filter_freq_high.get_untracked(),
                        state.filter_band_mode.get_untracked(),
                        min_freq,
                        max_freq,
                        display_w as f64,
                        display_h as f64,
                    );
                }
            }

            // Draw PSD hover frequency overlays
            {
                let psd_hovers = state.psd_hover_freqs.get();
                if !psd_hovers.is_empty() && max_freq > min_freq {
                    let dh = display_h as f64;
                    let dw = display_w as f64;
                    for (freq, label, color) in &psd_hovers {
                        if *freq < min_freq || *freq > max_freq { continue; }
                        let y = dh * (1.0 - (freq - min_freq) / (max_freq - min_freq));
                        // Horizontal line
                        ctx.set_stroke_style_str(color);
                        ctx.set_line_width(1.5);
                        let _ = ctx.set_line_dash(&js_sys::Array::of2(
                            &JsValue::from(4.0),
                            &JsValue::from(3.0),
                        ));
                        ctx.begin_path();
                        ctx.move_to(0.0, y);
                        ctx.line_to(dw, y);
                        ctx.stroke();
                        let _ = ctx.set_line_dash(&js_sys::Array::new());
                        // Label
                        ctx.set_fill_style_str(color);
                        ctx.set_font("10px monospace");
                        let _ = ctx.fill_text(label, 4.0, y - 3.0);
                    }
                }
            }

            if visible_time <= 0.0 { return; }
            let px_per_sec = display_w as f64 / visible_time;

            // Draw static position marker when not playing
            if !is_playing && canvas_tool == CanvasTool::Hand {
                let here_x = display_w as f64 * 0.10;
                let here_time = scroll + visible_time * 0.10;
                state.play_from_here_time.set(here_time);
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

            // Draw bookmark dots (yellow circles at top edge)
            ctx.set_fill_style_str("rgba(255, 200, 50, 0.9)");
            for bm in &bookmarks {
                let x = (bm.time - scroll) * px_per_sec;
                if x >= 0.0 && x <= display_w as f64 {
                    ctx.begin_path();
                    let _ = ctx.arc(x, 6.0, 4.0, 0.0, std::f64::consts::TAU);
                    let _ = ctx.fill();
                }
            }
        }
    });

    // Effect 4: auto-scroll to follow playhead during playback
    // Supports temporary suspension: when the user manually scrolls, following
    // pauses until the playhead is back on-screen for 500 ms continuously.
    Effect::new(move || {
        let playhead = state.playhead_time.get();
        let is_playing = state.is_playing.get();
        let follow = state.follow_cursor.get();
        let suspended = state.follow_suspended.get();

        if !follow {
            return;
        }
        if !is_playing {
            // Reset suspension when playback stops so next play starts fresh
            if suspended {
                state.follow_suspended.set(false);
                state.follow_visible_since.set(None);
            }
            return;
        }

        let Some(canvas_el) = canvas_ref.get() else { return };
        let canvas: &HtmlCanvasElement = canvas_el.as_ref();
        let display_w = canvas.width() as f64;
        if display_w == 0.0 { return; }

        let files = state.files.get_untracked();
        let idx = state.current_file_index.get_untracked();
        let (time_res, duration) = idx
            .and_then(|i| files.get(i))
            .map(|f| (f.spectrogram.time_resolution, f.audio.duration_secs))
            .unwrap_or((1.0, 0.0));
        let zoom = state.zoom_level.get_untracked();
        let scroll = state.scroll_offset.get_untracked();

        let visible_time = (display_w / zoom) * time_res;
        let playhead_rel = playhead - scroll;

        if suspended {
            let playhead_visible = playhead_rel >= 0.0 && playhead_rel <= visible_time;
            if playhead_visible {
                let now = js_sys::Date::now(); // milliseconds
                match state.follow_visible_since.get_untracked() {
                    None => {
                        state.follow_visible_since.set(Some(now));
                    }
                    Some(since) if now - since >= 500.0 => {
                        // Playhead has been on-screen for 500 ms — resume following
                        state.follow_suspended.set(false);
                        state.follow_visible_since.set(None);
                    }
                    _ => {}
                }
            } else {
                // Playhead wandered off-screen; reset the visibility timer
                state.follow_visible_since.set(None);
            }
            return;
        }

        // Normal follow: scroll when playhead nears the edge
        if playhead_rel > visible_time * 0.8 || playhead_rel < 0.0 {
            let max_scroll = (duration - visible_time).max(0.0);
            state.scroll_offset.set((playhead - visible_time * 0.2).max(0.0).min(max_scroll));
        }
    });

    // Effect 5: pre-fetch tiles ahead of the viewport and at the start of the file.
    // Debounced at 200ms so it doesn't fire at 60fps during playback.
    {
        let prefetch_handle: Rc<Cell<Option<i32>>> = Rc::new(Cell::new(None));

        Effect::new(move || {
            // Subscribe to coarse-grained signals (NOT playhead_time)
            let _scroll = state.scroll_offset.get();
            let _zoom = state.zoom_level.get();
            let _playing = state.is_playing.get();
            let _file_idx = state.current_file_index.get();
            let _main_view = state.main_view.get();
            let _reassign = state.reassign_enabled.get();
            let _flow = state.flow_enabled.get();
            // NOTE: intentionally NOT subscribing to tile_ready_signal here —
            // that would create a feedback loop (tile completes -> prefetch fires -> schedules more).
            // Scroll/zoom/playing changes are sufficient triggers for prefetch.

            // Cancel previous debounce timer
            if let Some(h) = prefetch_handle.get() {
                let _ = web_sys::window().unwrap().clear_timeout_with_handle(h);
            }

            let handle_rc = prefetch_handle.clone();
            let cb = Closure::once(move || {
                use crate::canvas::tile_cache;

                handle_rc.set(None);

                let main_view = state.main_view.get_untracked();
                if matches!(main_view, MainView::Waveform | MainView::ZcChart) {
                    return;
                }

                let Some(file_idx) = state.current_file_index.get_untracked() else { return };
                let (total_samples, sample_rate, time_res) = state.files.with_untracked(|files| {
                    files.get(file_idx).map(|f| {
                        (f.audio.source.total_samples() as usize, f.audio.sample_rate, f.spectrogram.time_resolution)
                    })
                }).unwrap_or((0, 44100, 0.01));
                if total_samples == 0 { return; }

                let zoom = state.zoom_level.get_untracked();
                let scroll = state.scroll_offset.get_untracked();
                let canvas_w = state.spectrogram_canvas_width.get_untracked();
                let visible_time = if zoom > 0.0 { (canvas_w / zoom) * time_res } else { 1.0 };
                let viewport_right = scroll + visible_time;

                let is_playing = state.is_playing.get_untracked();
                let center = if is_playing {
                    let playhead = state.playhead_time.get_untracked();
                    playhead.max(viewport_right)
                } else {
                    viewport_right
                };

                let flow_on = state.flow_enabled.get_untracked();
                let flow_algo = if flow_on {
                    let display = state.spectrogram_display.get_untracked();
                    Some(match display {
                        SpectrogramDisplay::FlowOptical => FlowAlgo::Optical,
                        SpectrogramDisplay::PhaseCoherence => FlowAlgo::PhaseCoherence,
                        SpectrogramDisplay::FlowCentroid => FlowAlgo::Centroid,
                        SpectrogramDisplay::FlowGradient => FlowAlgo::Gradient,
                        SpectrogramDisplay::Phase => FlowAlgo::Phase,
                    })
                } else {
                    None
                };

                let reassign = state.reassign_enabled.get_untracked();

                let (ahead_secs, max_prefetch) = if is_playing {
                    (15.0, 60)  // aggressive prefetch during playback
                } else {
                    (5.0, 30)
                };

                tile_cache::schedule_prefetch_tiles(
                    state,
                    file_idx,
                    total_samples,
                    sample_rate,
                    center,
                    ahead_secs,
                    3.0,  // keep first 3 seconds ready
                    zoom,
                    flow_algo,
                    reassign,
                    max_prefetch,
                );
            });

            let h = web_sys::window()
                .unwrap()
                .set_timeout_with_callback_and_timeout_and_arguments_0(
                    cb.as_ref().unchecked_ref(),
                    200,
                )
                .unwrap_or(0);
            cb.forget();
            prefetch_handle.set(Some(h));
        });
    }

    // Effect 6: background preload — progressively pre-compute tiles for the whole file
    // at the current LOD, expanding outward from the viewport center.
    Effect::new(move || {
        let _file_idx = state.current_file_index.get();
        let _zoom = state.zoom_level.get();
        let _loading = state.loading_files.get();
        let _fft = state.spect_fft_mode.get();

        use crate::canvas::tile_cache;

        // Don't preload while still loading a file
        if state.loading_files.with_untracked(|v| !v.is_empty()) {
            return;
        }

        let Some(file_idx) = state.current_file_index.get_untracked() else {
            tile_cache::stop_background_preload();
            return;
        };

        let total_samples = state.files.with_untracked(|files| {
            files.get(file_idx).map(|f| f.audio.source.total_samples() as usize).unwrap_or(0)
        });
        if total_samples == 0 {
            tile_cache::stop_background_preload();
            return;
        }

        let zoom = state.zoom_level.get_untracked();
        let lod = tile_cache::select_lod(zoom);
        let max_tiles = tile_cache::tile_count_for_samples(total_samples, lod);
        if max_tiles == 0 { return; }

        // Compute center tile from current viewport
        let scroll = state.scroll_offset.get_untracked();
        let time_res = state.files.with_untracked(|files| {
            files.get(file_idx).map(|f| f.spectrogram.time_resolution).unwrap_or(0.01)
        });
        let canvas_w = state.spectrogram_canvas_width.get_untracked();
        let visible_time = if zoom > 0.0 { (canvas_w / zoom) * time_res } else { 1.0 };
        let center_time = scroll + visible_time / 2.0;
        let ratio = tile_cache::lod_ratio(lod);
        let center_col = (center_time / time_res) as f64 * ratio;
        let center_tile = (center_col / tile_cache::TILE_COLS as f64) as usize;

        // Bump generation to cancel any stale preload
        state.bg_preload_gen.update(|g| *g = g.wrapping_add(1));
        let generation = state.bg_preload_gen.get_untracked();

        tile_cache::start_background_preload(state, file_idx, lod, center_tile, max_tiles, generation);
    });

    // Helper to get (px_x, px_y, time, freq) from mouse event
    let mouse_to_xtf = move |ev: &MouseEvent| -> Option<(f64, f64, f64, f64)> {
        let canvas_el = canvas_ref.get()?;
        let canvas: &HtmlCanvasElement = canvas_el.as_ref();
        let rect = canvas.get_bounding_client_rect();
        let px_x = ev.client_x() as f64 - rect.left();
        let px_y = ev.client_y() as f64 - rect.top();
        let cw = canvas.width() as f64;
        let ch = canvas.height() as f64;

        let files = state.files.get_untracked();
        let idx = state.current_file_index.get_untracked()?;
        let file = files.get(idx)?;
        let time_res = file.spectrogram.time_resolution;
        let file_max_freq = file.spectrogram.max_freq;
        let max_freq = state.max_display_freq.get_untracked()
            .unwrap_or(file_max_freq);
        let min_freq = state.min_display_freq.get_untracked()
            .unwrap_or(0.0);
        let scroll = state.scroll_offset.get_untracked();
        let zoom = state.zoom_level.get_untracked();

        let (t, f) = spectrogram_renderer::pixel_to_time_freq(
            px_x, px_y, min_freq, max_freq, scroll, time_res, zoom, cw, ch,
        );
        Some((px_x, px_y, t, f))
    };

    let on_mousedown = move |ev: MouseEvent| {
        if ev.button() != 0 { return; }

        // Check for spec handle drag first (FF or HET — takes priority over tool)
        if let Some(handle) = state.spec_hover_handle.get_untracked() {
            state.spec_drag_handle.set(Some(handle));
            state.is_dragging.set(true);
            ev.prevent_default();
            return;
        }

        // Check for axis drag (left axis frequency range selection) — disabled in xform view
        if let Some((px_x, _, _, freq)) = mouse_to_xtf(&ev) {
            if px_x < LABEL_AREA_WIDTH && !state.display_transform.get_untracked() {
                let snap = if ev.shift_key() { 10_000.0 } else { 5_000.0 };
                let snapped = (freq / snap).round() * snap;
                axis_drag_raw_start.set(freq);
                state.axis_drag_start_freq.set(Some(snapped));
                state.axis_drag_current_freq.set(Some(snapped));
                state.is_dragging.set(true);
                ev.prevent_default();
                return;
            }
        }

        match state.canvas_tool.get_untracked() {
            CanvasTool::Hand => {
                // Start hand panning (works whether playing or not)
                state.is_dragging.set(true);
                hand_drag_start.set((ev.client_x() as f64, state.scroll_offset.get_untracked()));
            }
            CanvasTool::Selection => {
                if let Some((_, _, t, f)) = mouse_to_xtf(&ev) {
                    state.is_dragging.set(true);
                    drag_start.set((t, f));
                    state.selection.set(None);
                }
            }
        }
    };

    let on_mousemove = move |ev: MouseEvent| {
        if let Some((px_x, px_y, t, f)) = mouse_to_xtf(&ev) {
            // Always track hover position
            state.mouse_freq.set(Some(f));
            state.mouse_canvas_x.set(px_x);
            state.cursor_time.set(Some(t));

            // Time-axis tooltip: show full datetime when hovering bottom 16px
            if let Some(canvas_el) = canvas_ref.get() {
                let canvas: &HtmlCanvasElement = canvas_el.as_ref();
                let ch = canvas.get_bounding_client_rect().height();
                if px_y > ch - 16.0 && px_x > LABEL_AREA_WIDTH {
                    let tooltip = state.current_file()
                        .and_then(|f| f.recording_start_info())
                        .map(|(epoch, source)| {
                            crate::canvas::time_markers::format_clock_time_full(epoch, t, source)
                        });
                    if let Some(text) = tooltip {
                        time_axis_tooltip.set(Some((px_x, text)));
                    } else {
                        time_axis_tooltip.set(None);
                    }
                } else {
                    time_axis_tooltip.set(None);
                }
            }

            // Update label hover target and in-label-area state
            let in_label_area = px_x < LABEL_AREA_WIDTH;
            state.mouse_in_label_area.set(in_label_area);
            let current_target = label_hover_target.get_untracked();
            let new_target = if in_label_area { 1.0 } else { 0.0 };
            if current_target != new_target {
                label_hover_target.set(new_target);
            }

            if state.is_dragging.get_untracked() {
                // Spec handle drag takes priority
                if let Some(handle) = state.spec_drag_handle.get_untracked() {
                    let Some(canvas_el) = canvas_ref.get() else { return };
                    let canvas: &HtmlCanvasElement = canvas_el.as_ref();
                    let ch = canvas.height() as f64;
                    let files = state.files.get_untracked();
                    let idx = state.current_file_index.get_untracked();
                    let file = idx.and_then(|i| files.get(i));
                    let file_max_freq = file.map(|f| f.spectrogram.max_freq).unwrap_or(96_000.0);
                    let min_freq_val = state.min_display_freq.get_untracked().unwrap_or(0.0);
                    let max_freq_val = state.max_display_freq.get_untracked().unwrap_or(file_max_freq);
                    let freq_at_mouse = spectrogram_renderer::y_to_freq(px_y, min_freq_val, max_freq_val, ch);

                    match handle {
                        SpectrogramHandle::FfUpper => {
                            let lo = state.ff_freq_lo.get_untracked();
                            let clamped = freq_at_mouse.clamp(lo + 500.0, file_max_freq);
                            state.set_ff_hi(clamped);
                        }
                        SpectrogramHandle::FfLower => {
                            let hi = state.ff_freq_hi.get_untracked();
                            let clamped = freq_at_mouse.clamp(0.0, hi - 500.0);
                            state.set_ff_lo(clamped);
                        }
                        SpectrogramHandle::FfMiddle => {
                            let lo = state.ff_freq_lo.get_untracked();
                            let hi = state.ff_freq_hi.get_untracked();
                            let bw = hi - lo;
                            let mid = (lo + hi) / 2.0;
                            let delta = freq_at_mouse - mid;
                            let new_lo = (lo + delta).clamp(0.0, file_max_freq - bw);
                            let new_hi = new_lo + bw;
                            state.set_ff_range(new_lo, new_hi);
                        }
                        SpectrogramHandle::HetCenter => {
                            state.het_freq_auto.set(false);
                            let clamped = freq_at_mouse.clamp(1000.0, file_max_freq);
                            state.het_frequency.set(clamped);
                        }
                        SpectrogramHandle::HetBandUpper => {
                            state.het_cutoff_auto.set(false);
                            let het_freq = state.het_frequency.get_untracked();
                            let new_cutoff = (freq_at_mouse - het_freq).clamp(1000.0, 30000.0);
                            state.het_cutoff.set(new_cutoff);
                        }
                        SpectrogramHandle::HetBandLower => {
                            state.het_cutoff_auto.set(false);
                            let het_freq = state.het_frequency.get_untracked();
                            let new_cutoff = (het_freq - freq_at_mouse).clamp(1000.0, 30000.0);
                            state.het_cutoff.set(new_cutoff);
                        }
                    }
                    return;
                }

                // Axis drag takes second priority (after spec handle drag)
                if state.axis_drag_start_freq.get_untracked().is_some() {
                    let raw_start = axis_drag_raw_start.get_untracked();
                    let snap = if ev.shift_key() { 10_000.0 } else { 5_000.0 };
                    // Snap both start and end away from each other to include
                    // the full segment under each endpoint
                    let (snapped_start, snapped_end) = if f > raw_start {
                        // Dragging up: start floors down, end ceils up
                        ((raw_start / snap).floor() * snap, (f / snap).ceil() * snap)
                    } else if f < raw_start {
                        // Dragging down: start ceils up, end floors down
                        ((raw_start / snap).ceil() * snap, (f / snap).floor() * snap)
                    } else {
                        let s = (raw_start / snap).round() * snap;
                        (s, s)
                    };
                    state.axis_drag_start_freq.set(Some(snapped_start));
                    state.axis_drag_current_freq.set(Some(snapped_end));
                    // Live update FF range
                    let lo = snapped_start.min(snapped_end);
                    let hi = snapped_start.max(snapped_end);
                    if hi - lo > 500.0 {
                        state.set_ff_range(lo, hi);
                    }
                    return;
                }

                match state.canvas_tool.get_untracked() {
                    CanvasTool::Hand => {
                        // Pan view
                        let (start_client_x, start_scroll) = hand_drag_start.get_untracked();
                        let dx = ev.client_x() as f64 - start_client_x;
                        let Some(canvas_el) = canvas_ref.get() else { return };
                        let canvas: &HtmlCanvasElement = canvas_el.as_ref();
                        let cw = canvas.width() as f64;
                        if cw == 0.0 { return; }
                        let files = state.files.get_untracked();
                        let idx = state.current_file_index.get_untracked();
                        let file = idx.and_then(|i| files.get(i));
                        let time_res = file.as_ref().map(|f| f.spectrogram.time_resolution).unwrap_or(1.0);
                        let zoom = state.zoom_level.get_untracked();
                        let visible_time = (cw / zoom) * time_res;
                        let duration = file.as_ref().map(|f| f.audio.duration_secs).unwrap_or(f64::MAX);
                        let max_scroll = (duration - visible_time).max(0.0);
                        let dt = -(dx / cw) * visible_time;
                        state.suspend_follow();
                        state.scroll_offset.set((start_scroll + dt).clamp(0.0, max_scroll));
                    }
                    CanvasTool::Selection => {
                        let (t0, f0) = drag_start.get_untracked();
                        state.selection.set(Some(Selection {
                            time_start: t0.min(t),
                            time_end: t0.max(t),
                            freq_low: Some(f0.min(f)),
                            freq_high: Some(f0.max(f)),
                        }));
                    }
                }
            } else {
                // Not dragging — do spec handle hover detection (FF + HET)
                // Skip handle hover when in label area (to allow axis drag)
                if !in_label_area {
                    let Some(canvas_el) = canvas_ref.get() else { return };
                    let canvas: &HtmlCanvasElement = canvas_el.as_ref();
                    let ch = canvas.height() as f64;
                    let files = state.files.get_untracked();
                    let idx = state.current_file_index.get_untracked();
                    let file = idx.and_then(|i| files.get(i));
                    let file_max_freq = file.map(|f| f.spectrogram.max_freq).unwrap_or(96_000.0);
                    let min_freq_val = state.min_display_freq.get_untracked().unwrap_or(0.0);
                    let max_freq_val = state.max_display_freq.get_untracked().unwrap_or(file_max_freq);

                    let handle = hit_test_spec_handles(
                        &state, px_y, min_freq_val, max_freq_val, ch, 8.0,
                    );
                    state.spec_hover_handle.set(handle);
                } else {
                    state.spec_hover_handle.set(None);
                }
            }
        }
    };

    let on_mouseleave = move |_ev: MouseEvent| {
        state.mouse_freq.set(None);
        state.mouse_in_label_area.set(false);
        state.cursor_time.set(None);
        label_hover_target.set(0.0);
        state.is_dragging.set(false);
        state.spec_drag_handle.set(None);
        state.spec_hover_handle.set(None);
        state.axis_drag_start_freq.set(None);
        state.axis_drag_current_freq.set(None);
        time_axis_tooltip.set(None);
    };

    let on_mouseup = move |ev: MouseEvent| {
        if !state.is_dragging.get_untracked() { return; }

        // End HET/FF handle drag
        if state.spec_drag_handle.get_untracked().is_some() {
            state.spec_drag_handle.set(None);
            state.is_dragging.set(false);
            return;
        }

        // End axis drag (FF range already updated live during drag)
        if state.axis_drag_start_freq.get_untracked().is_some() {
            let lo = state.ff_freq_lo.get_untracked();
            let hi = state.ff_freq_hi.get_untracked();
            if hi - lo > 500.0 && !state.focus_stack.get_untracked().hfr_enabled() {
                // Enable HFR — the focus stack already has the user's range
                state.toggle_hfr();
            }
            state.axis_drag_start_freq.set(None);
            state.axis_drag_current_freq.set(None);
            state.is_dragging.set(false);
            return;
        }

        state.is_dragging.set(false);

        if state.canvas_tool.get_untracked() == CanvasTool::Hand {
            // If the mouse barely moved, treat as a click → bookmark while playing
            let (start_x, _) = hand_drag_start.get_untracked();
            let dx = (ev.client_x() as f64 - start_x).abs();
            if dx < 3.0 && state.is_playing.get_untracked() {
                let t = state.playhead_time.get_untracked();
                state.bookmarks.update(|bm| bm.push(crate::state::Bookmark { time: t }));
            }
            return;
        }
        if state.canvas_tool.get_untracked() != CanvasTool::Selection { return; }
        if let Some((_, _, t, f)) = mouse_to_xtf(&ev) {
            let (t0, f0) = drag_start.get_untracked();
            let sel = Selection {
                time_start: t0.min(t),
                time_end: t0.max(t),
                freq_low: Some(f0.min(f)),
                freq_high: Some(f0.max(f)),
            };
            if sel.time_end - sel.time_start > 0.0001 {
                state.selection.set(Some(sel));
                // Update frequency focus to match selection's frequency range
                if let (Some(lo), Some(hi)) = (sel.freq_low, sel.freq_high) {
                    if hi - lo > 100.0 {
                        state.set_ff_range(lo, hi);
                    }
                }
            } else {
                state.selection.set(None);
            }
        }
    };

    // Helper to get (px_x, px_y, time, freq) from touch event
    let touch_to_xtf = move |touch: &web_sys::Touch| -> Option<(f64, f64, f64, f64)> {
        let canvas_el = canvas_ref.get()?;
        let canvas: &HtmlCanvasElement = canvas_el.as_ref();
        let rect = canvas.get_bounding_client_rect();
        let px_x = touch.client_x() as f64 - rect.left();
        let px_y = touch.client_y() as f64 - rect.top();
        let cw = canvas.width() as f64;
        let ch = canvas.height() as f64;

        let files = state.files.get_untracked();
        let idx = state.current_file_index.get_untracked()?;
        let file = files.get(idx)?;
        let time_res = file.spectrogram.time_resolution;
        let file_max_freq = file.spectrogram.max_freq;
        let max_freq = state.max_display_freq.get_untracked().unwrap_or(file_max_freq);
        let min_freq = state.min_display_freq.get_untracked().unwrap_or(0.0);
        let scroll = state.scroll_offset.get_untracked();
        let zoom = state.zoom_level.get_untracked();

        let (t, f) = spectrogram_renderer::pixel_to_time_freq(
            px_x, px_y, min_freq, max_freq, scroll, time_res, zoom, cw, ch,
        );
        Some((px_x, px_y, t, f))
    };

    // ── Touch event handlers (mobile) ──────────────────────────────────────────
    let on_touchstart = move |ev: web_sys::TouchEvent| {
        let touches = ev.touches();
        let n = touches.length();

        // Two-finger: initialize pinch-to-zoom (works with any tool, like ctrl+scroll)
        if n == 2 {
            ev.prevent_default();
            use crate::components::pinch::{two_finger_geometry, PinchState};
            if let Some((mid_x, dist)) = two_finger_geometry(&touches) {
                let files = state.files.get_untracked();
                let idx = state.current_file_index.get_untracked();
                let file = idx.and_then(|i| files.get(i));
                let time_res = file.as_ref().map(|f| f.spectrogram.time_resolution).unwrap_or(1.0);
                let duration = file.as_ref().map(|f| f.audio.duration_secs).unwrap_or(f64::MAX);
                pinch_state.set(Some(PinchState {
                    initial_dist: dist,
                    initial_zoom: state.zoom_level.get_untracked(),
                    initial_scroll: state.scroll_offset.get_untracked(),
                    initial_mid_client_x: mid_x,
                    time_res,
                    duration,
                }));
            }
            // End any in-progress single-touch gesture
            state.is_dragging.set(false);
            state.spec_drag_handle.set(None);
            state.axis_drag_start_freq.set(None);
            state.axis_drag_current_freq.set(None);
            return;
        }

        if n != 1 { return; }
        // Transitioning from 2 to 1 finger — re-anchor pan position
        if pinch_state.get_untracked().is_some() {
            pinch_state.set(None);
            if let Some(touch) = touches.get(0) {
                hand_drag_start.set((touch.client_x() as f64, state.scroll_offset.get_untracked()));
                if state.canvas_tool.get_untracked() == CanvasTool::Hand {
                    state.is_dragging.set(true);
                }
            }
            return;
        }

        let touch = touches.get(0).unwrap();

        // Check for spec handle drag first — hit-test at touch position
        if let Some((_, px_y, _, _)) = touch_to_xtf(&touch) {
            let canvas_el = canvas_ref.get();
            if let Some(canvas_el) = canvas_el {
                let canvas: &HtmlCanvasElement = canvas_el.as_ref();
                let ch = canvas.height() as f64;
                let files = state.files.get_untracked();
                let idx = state.current_file_index.get_untracked();
                let file = idx.and_then(|i| files.get(i));
                let file_max_freq = file.map(|f| f.spectrogram.max_freq).unwrap_or(96_000.0);
                let min_freq_val = state.min_display_freq.get_untracked().unwrap_or(0.0);
                let max_freq_val = state.max_display_freq.get_untracked().unwrap_or(file_max_freq);
                let handle = hit_test_spec_handles(
                    &state, px_y, min_freq_val, max_freq_val, ch, 16.0, // wider touch target
                );
                if let Some(handle) = handle {
                    state.spec_drag_handle.set(Some(handle));
                    state.is_dragging.set(true);
                    ev.prevent_default();
                    return;
                }
            }
        }

        // Check for axis drag (left axis frequency range selection)
        if let Some((px_x, _, _, freq)) = touch_to_xtf(&touch) {
            if px_x < LABEL_AREA_WIDTH {
                let snap = 5_000.0;
                let snapped = (freq / snap).round() * snap;
                axis_drag_raw_start.set(freq);
                state.axis_drag_start_freq.set(Some(snapped));
                state.axis_drag_current_freq.set(Some(snapped));
                state.is_dragging.set(true);
                ev.prevent_default();
                return;
            }
        }

        match state.canvas_tool.get_untracked() {
            CanvasTool::Hand => {
                // Always start pan drag (bookmark on tap handled in touchend)
                ev.prevent_default();
                state.is_dragging.set(true);
                hand_drag_start.set((touch.client_x() as f64, state.scroll_offset.get_untracked()));
            }
            CanvasTool::Selection => {
                ev.prevent_default();
            }
        }
    };

    let on_touchmove = move |ev: web_sys::TouchEvent| {
        let touches = ev.touches();
        let n = touches.length();

        // Two-finger pinch/pan
        if n == 2 {
            if let Some(ps) = pinch_state.get_untracked() {
                ev.prevent_default();
                use crate::components::pinch::{two_finger_geometry, apply_pinch};
                if let Some((mid_x, dist)) = two_finger_geometry(&touches) {
                    let Some(canvas_el) = canvas_ref.get() else { return };
                    let canvas: &HtmlCanvasElement = canvas_el.as_ref();
                    let rect = canvas.get_bounding_client_rect();
                    let cw = canvas.width() as f64;
                    let (new_zoom, new_scroll) = apply_pinch(&ps, dist, mid_x, rect.left(), cw);
                    state.suspend_follow();
                    state.zoom_level.set(new_zoom);
                    state.scroll_offset.set(new_scroll);
                }
            }
            return;
        }

        if n != 1 { return; }
        let touch = touches.get(0).unwrap();

        if !state.is_dragging.get_untracked() { return; }
        ev.prevent_default();

        // Spec handle drag takes priority
        if let Some(handle) = state.spec_drag_handle.get_untracked() {
            if let Some((_, px_y, _, _)) = touch_to_xtf(&touch) {
                let Some(canvas_el) = canvas_ref.get() else { return };
                let canvas: &HtmlCanvasElement = canvas_el.as_ref();
                let ch = canvas.height() as f64;
                let files = state.files.get_untracked();
                let idx = state.current_file_index.get_untracked();
                let file = idx.and_then(|i| files.get(i));
                let file_max_freq = file.map(|f| f.spectrogram.max_freq).unwrap_or(96_000.0);
                let min_freq_val = state.min_display_freq.get_untracked().unwrap_or(0.0);
                let max_freq_val = state.max_display_freq.get_untracked().unwrap_or(file_max_freq);
                let freq_at_touch = spectrogram_renderer::y_to_freq(px_y, min_freq_val, max_freq_val, ch);

                match handle {
                    SpectrogramHandle::FfUpper => {
                        let lo = state.ff_freq_lo.get_untracked();
                        state.set_ff_hi(freq_at_touch.clamp(lo + 500.0, file_max_freq));
                    }
                    SpectrogramHandle::FfLower => {
                        let hi = state.ff_freq_hi.get_untracked();
                        state.set_ff_lo(freq_at_touch.clamp(0.0, hi - 500.0));
                    }
                    SpectrogramHandle::FfMiddle => {
                        let lo = state.ff_freq_lo.get_untracked();
                        let hi = state.ff_freq_hi.get_untracked();
                        let bw = hi - lo;
                        let mid = (lo + hi) / 2.0;
                        let delta = freq_at_touch - mid;
                        let new_lo = (lo + delta).clamp(0.0, file_max_freq - bw);
                        let new_hi = new_lo + bw;
                        state.set_ff_range(new_lo, new_hi);
                    }
                    SpectrogramHandle::HetCenter => {
                        state.het_freq_auto.set(false);
                        state.het_frequency.set(freq_at_touch.clamp(1000.0, file_max_freq));
                    }
                    SpectrogramHandle::HetBandUpper => {
                        state.het_cutoff_auto.set(false);
                        let het_freq = state.het_frequency.get_untracked();
                        state.het_cutoff.set((freq_at_touch - het_freq).clamp(1000.0, 30000.0));
                    }
                    SpectrogramHandle::HetBandLower => {
                        state.het_cutoff_auto.set(false);
                        let het_freq = state.het_frequency.get_untracked();
                        state.het_cutoff.set((het_freq - freq_at_touch).clamp(1000.0, 30000.0));
                    }
                }
            }
            return;
        }

        // Axis drag takes second priority
        if state.axis_drag_start_freq.get_untracked().is_some() {
            if let Some((_, _, _, f)) = touch_to_xtf(&touch) {
                let raw_start = axis_drag_raw_start.get_untracked();
                let snap = 5_000.0;
                let (snapped_start, snapped_end) = if f > raw_start {
                    ((raw_start / snap).floor() * snap, (f / snap).ceil() * snap)
                } else if f < raw_start {
                    ((raw_start / snap).ceil() * snap, (f / snap).floor() * snap)
                } else {
                    let s = (raw_start / snap).round() * snap;
                    (s, s)
                };
                state.axis_drag_start_freq.set(Some(snapped_start));
                state.axis_drag_current_freq.set(Some(snapped_end));
                let lo = snapped_start.min(snapped_end);
                let hi = snapped_start.max(snapped_end);
                if hi - lo > 500.0 {
                    state.set_ff_range(lo, hi);
                }
            }
            return;
        }

        match state.canvas_tool.get_untracked() {
            CanvasTool::Hand => {
                let (start_client_x, start_scroll) = hand_drag_start.get_untracked();
                let dx = touch.client_x() as f64 - start_client_x;
                let Some(canvas_el) = canvas_ref.get() else { return };
                let canvas: &HtmlCanvasElement = canvas_el.as_ref();
                let cw = canvas.width() as f64;
                if cw == 0.0 { return; }
                let files = state.files.get_untracked();
                let idx = state.current_file_index.get_untracked();
                let file = idx.and_then(|i| files.get(i));
                let time_res = file.as_ref().map(|f| f.spectrogram.time_resolution).unwrap_or(1.0);
                let zoom = state.zoom_level.get_untracked();
                let visible_time = (cw / zoom) * time_res;
                let duration = file.as_ref().map(|f| f.audio.duration_secs).unwrap_or(f64::MAX);
                let max_scroll = (duration - visible_time).max(0.0);
                let dt = -(dx / cw) * visible_time;
                state.suspend_follow();
                state.scroll_offset.set((start_scroll + dt).clamp(0.0, max_scroll));
            }
            CanvasTool::Selection => {}
        }
    };

    let on_touchend = move |_ev: web_sys::TouchEvent| {
        let remaining = _ev.touches().length();

        if remaining < 2 {
            pinch_state.set(None);
        }

        // One finger remains after pinch — re-anchor pan to avoid jump
        if remaining == 1 {
            if let Some(touch) = _ev.touches().get(0) {
                hand_drag_start.set((touch.client_x() as f64, state.scroll_offset.get_untracked()));
                if state.canvas_tool.get_untracked() == CanvasTool::Hand {
                    state.is_dragging.set(true);
                }
            }
            return;
        }

        if remaining == 0 {
            if state.spec_drag_handle.get_untracked().is_some() {
                state.spec_drag_handle.set(None);
                state.is_dragging.set(false);
                return;
            }
            // Finalize axis drag — auto-enable HFR if a meaningful range was selected
            if state.axis_drag_start_freq.get_untracked().is_some() {
                let lo = state.ff_freq_lo.get_untracked();
                let hi = state.ff_freq_hi.get_untracked();
                if hi - lo > 500.0 && !state.focus_stack.get_untracked().hfr_enabled() {
                    state.toggle_hfr();
                }
                state.axis_drag_start_freq.set(None);
                state.axis_drag_current_freq.set(None);
                state.is_dragging.set(false);
                return;
            }
            state.is_dragging.set(false);

            // Hand tool: bookmark on tap (no significant drag) while playing
            if state.canvas_tool.get_untracked() == CanvasTool::Hand {
                if let Some(touch) = _ev.changed_touches().get(0) {
                    let (start_x, _) = hand_drag_start.get_untracked();
                    let dx = (touch.client_x() as f64 - start_x).abs();
                    if dx < 5.0 && state.is_playing.get_untracked() {
                        let t = state.playhead_time.get_untracked();
                        state.bookmarks.update(|bm| bm.push(crate::state::Bookmark { time: t }));
                    }
                }
            }

            // Update frequency focus from selection (touch equivalent of mouseup logic)
            if state.canvas_tool.get_untracked() == CanvasTool::Selection {
                if let Some(sel) = state.selection.get_untracked() {
                    if let (Some(lo), Some(hi)) = (sel.freq_low, sel.freq_high) {
                        if hi - lo > 100.0 {
                            state.set_ff_range(lo, hi);
                        }
                    }
                }
            }

            // Double-tap detection: if two taps within 400ms in label area → remove range
            if let Some(touch) = _ev.changed_touches().get(0) {
                if let Some((px_x, _, _, _)) = touch_to_xtf(&touch) {
                    let now = js_sys::Date::now();
                    let prev_time = last_tap_time.get_untracked();
                    let prev_x = last_tap_x.get_untracked();
                    last_tap_time.set(now);
                    last_tap_x.set(px_x);
                    let in_label = px_x < LABEL_AREA_WIDTH;
                    let prev_in_label = prev_x < LABEL_AREA_WIDTH;
                    if now - prev_time < 400.0 && in_label && prev_in_label {
                        let has_range = state.ff_freq_hi.get_untracked() > state.ff_freq_lo.get_untracked();
                        if has_range {
                            state.toggle_hfr();
                        }
                    }
                }
            }
        }
    };

    // Double-click in label area or on FF handles → remove range selection / turn off HFR
    let on_dblclick = move |ev: MouseEvent| {
        let has_range = state.ff_freq_hi.get_untracked() > state.ff_freq_lo.get_untracked();
        if !has_range { return; }

        if let Some((px_x, _, _, _)) = mouse_to_xtf(&ev) {
            let in_label = px_x < LABEL_AREA_WIDTH;
            let on_handle = matches!(
                state.spec_hover_handle.get_untracked(),
                Some(SpectrogramHandle::FfUpper | SpectrogramHandle::FfLower | SpectrogramHandle::FfMiddle)
            );
            if in_label || on_handle {
                state.toggle_hfr();
                ev.prevent_default();
            }
        }
    };

    let on_wheel = move |ev: web_sys::WheelEvent| {
        ev.prevent_default();
        if ev.shift_key() {
            // Shift+scroll: vertical freq zoom around mouse position
            let files = state.files.get_untracked();
            let idx = state.current_file_index.get_untracked();
            let file_max_freq = idx
                .and_then(|i| files.get(i))
                .map(|f| f.spectrogram.max_freq)
                .unwrap_or(96_000.0);
            let cur_max = state.max_display_freq.get_untracked().unwrap_or(file_max_freq);
            let cur_min = state.min_display_freq.get_untracked().unwrap_or(0.0);
            let range = cur_max - cur_min;
            if range < 1.0 { return; }

            // Determine anchor freq from mouse Y
            let anchor_frac = if let Some(mf) = state.mouse_freq.get_untracked() {
                ((mf - cur_min) / range).clamp(0.0, 1.0)
            } else {
                0.5
            };

            let factor = if ev.delta_y() > 0.0 { 1.15 } else { 1.0 / 1.15 };
            let new_range = (range * factor).clamp(500.0, file_max_freq);
            let anchor_freq = cur_min + anchor_frac * range;
            let new_min = (anchor_freq - anchor_frac * new_range).max(0.0);
            let new_max = (new_min + new_range).min(file_max_freq);
            let new_min = (new_max - new_range).max(0.0);

            state.min_display_freq.set(Some(new_min));
            state.max_display_freq.set(Some(new_max));
        } else if ev.ctrl_key() {
            let delta = if ev.delta_y() > 0.0 { 0.9 } else { 1.1 };
            state.zoom_level.update(|z| {
                *z = (*z * delta).max(0.1).min(400.0);
            });
        } else {
            let delta = (ev.delta_y() + ev.delta_x()) * 0.001;
            let max_scroll = {
                let files = state.files.get_untracked();
                let idx = state.current_file_index.get_untracked().unwrap_or(0);
                if let Some(file) = files.get(idx) {
                    let zoom = state.zoom_level.get_untracked();
                    let canvas_w = state.spectrogram_canvas_width.get_untracked();
                    let visible_time = (canvas_w / zoom) * file.spectrogram.time_resolution;
                    (file.audio.duration_secs - visible_time).max(0.0)
                } else {
                    f64::MAX
                }
            };
            state.suspend_follow();
            state.scroll_offset.update(|s| {
                *s = (*s + delta).clamp(0.0, max_scroll);
            });
        }
    };

    view! {
        <div class="spectrogram-container"
            style=move || {
                if state.axis_drag_start_freq.get().is_some() || state.mouse_in_label_area.get() {
                    return "cursor: crosshair; touch-action: none;".to_string();
                }
                if state.spec_drag_handle.get().is_some() || state.spec_hover_handle.get().is_some() {
                    return "cursor: ns-resize; touch-action: none;".to_string();
                }
                match state.canvas_tool.get() {
                    CanvasTool::Hand => if state.is_dragging.get() {
                        "cursor: grabbing; touch-action: none;".to_string()
                    } else {
                        "cursor: grab; touch-action: none;".to_string()
                    },
                    CanvasTool::Selection => "cursor: crosshair; touch-action: none;".to_string(),
                }
            }
        >
            <canvas
                node_ref=canvas_ref
                on:wheel=on_wheel
                on:mousedown=on_mousedown
                on:mousemove=on_mousemove
                on:mouseup=on_mouseup
                on:mouseleave=on_mouseleave
                on:dblclick=on_dblclick
                on:touchstart=on_touchstart
                on:touchmove=on_touchmove
                on:touchend=on_touchend
            />
            // DOM playhead overlay — decoupled from heavy canvas redraws
            <div
                class="playhead-line"
                style:transform=move || {
                    let playhead = state.playhead_time.get();
                    let scroll = state.scroll_offset.get();
                    let zoom = state.zoom_level.get();
                    let cw = state.spectrogram_canvas_width.get();
                    let files = state.files.get_untracked();
                    let idx = state.current_file_index.get_untracked();
                    let time_res = idx.and_then(|i| files.get(i))
                        .map(|f| f.spectrogram.time_resolution)
                        .unwrap_or(1.0);
                    let visible_time = (cw / zoom) * time_res;
                    let px_per_sec = if visible_time > 0.0 { cw / visible_time } else { 0.0 };
                    let x = (playhead - scroll) * px_per_sec;
                    format!("translateX({:.1}px)", x)
                }
                style:display=move || if state.is_playing.get() { "block" } else { "none" }
            />
            // Time-axis hover tooltip (shows full date/time/timezone + source)
            {move || {
                time_axis_tooltip.get().map(|(x, text)| {
                    view! {
                        <div
                            class="time-axis-tooltip"
                            style:left=format!("{}px", x)
                        >
                            {text}
                        </div>
                    }
                })
            }}
        </div>
    }
}

