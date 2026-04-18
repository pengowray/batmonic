use leptos::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use wasm_bindgen::closure::Closure;
use js_sys;
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement};
use crate::canvas::freq_adjustments::compute_freq_adjustments;
use crate::canvas::spectrogram_renderer::{self, Colormap, ColormapMode, FreqMarkerState, FreqShiftMode, PreRendered, SpectDisplaySettings};
use crate::components::spectrogram_events::{self, SpectInteraction, LABEL_AREA_WIDTH};
use crate::state::{AppState, CanvasTool, SpectrogramHandle, MainView, PlaybackMode};
use crate::viewport;

#[component]
pub fn Spectrogram() -> impl IntoView {
    let state = expect_context::<AppState>();
    let canvas_ref = NodeRef::<leptos::html::Canvas>::new();

    let pre_rendered: RwSignal<Option<PreRendered>> = RwSignal::new(None);
    let _flow_cache_removed = (); // flow tiles are now in tile_cache::MV_CACHE

    // Interaction state for event handlers (drag, pinch, axis drag, etc.)
    let ix = SpectInteraction::new();
    let label_hover_target = ix.label_hover_target;
    let time_axis_tooltip = ix.time_axis_tooltip;
    let anim_gen: Rc<Cell<u32>> = Rc::new(Cell::new(0));

    // Disposal guard: async callbacks (rAF, setTimeout) check this before
    // accessing any reactive state, preventing panics after component unmount.
    let disposed = Arc::new(AtomicBool::new(false));
    {
        let d = disposed.clone();
        on_cleanup(move || d.store(true, Ordering::Relaxed));
    }

    // Label hover animation: lerp label_hover_opacity toward target via rAF.
    // IMPORTANT: The Effect must NOT call .set() on label_hover_opacity directly,
    // since it subscribes to it via .get() — that would cause "closure invoked
    // recursively". Instead, ALL writes go through rAF callbacks (which run
    // outside the Effect scope) and convergence snapping also uses rAF.
    Effect::new({
        let disposed = disposed.clone();
        move || {
        let target = label_hover_target.get();
        let current = state.label_hover_opacity.get();
        if (current - target).abs() < 0.01 {
            // Close enough — schedule a final snap via rAF to avoid
            // setting the signal inside this Effect (which would recurse).
            if current != target {
                let generation = anim_gen.get().wrapping_add(1);
                anim_gen.set(generation);
                let ag = anim_gen.clone();
                let disposed_rc = disposed.clone();
                let cb = Closure::once(move || {
                    if disposed_rc.load(Ordering::Relaxed) || ag.get() != generation { return; }
                    let Some(tgt) = label_hover_target.try_get_untracked() else { return; };
                    state.label_hover_opacity.set(tgt);
                });
                let _ = web_sys::window().unwrap().request_animation_frame(
                    cb.as_ref().unchecked_ref(),
                );
                cb.forget();
            }
            return;
        }
        let generation = anim_gen.get().wrapping_add(1);
        anim_gen.set(generation);
        let ag = anim_gen.clone();
        let disposed_rc = disposed.clone();
        let cb = Closure::once(move || {
            if disposed_rc.load(Ordering::Relaxed) || ag.get() != generation { return; }
            let Some(cur) = state.label_hover_opacity.try_get_untracked() else { return; };
            let Some(tgt) = label_hover_target.try_get_untracked() else { return; };
            let speed = if tgt > cur { 0.35 } else { 0.20 };
            let next = cur + (tgt - cur) * speed;
            let next = if (next - tgt).abs() < 0.01 { tgt } else { next };
            state.label_hover_opacity.set(next);
        });
        let _ = web_sys::window().unwrap().request_animation_frame(
            cb.as_ref().unchecked_ref(),
        );
        cb.forget();
    }});

    // Effect 1: keep legacy pre-render state cleared.
    // The active draw path is tile-based for normal spectrogram rendering, so
    // building a full-image pre-render here only duplicates load-time work.
    Effect::new(move || {
        let _files = state.files.get();
        let _idx = state.current_file_index.get();
        let _enabled = state.flow_enabled.get();
        pre_rendered.set(None);
    });

    // Cache-clearing effects: invalidate tile caches when FFT mode, flow, transform, or reassignment changes
    crate::canvas::tile_scheduler::setup_cache_clearing_effects(state);

    // Effect 3: redraw when pre-rendered data, scroll, zoom, selection, playhead, overlays, hover, or new tile change
    Effect::new({
        let disposed = disposed.clone();
        move || {
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
        let pointer_down = state.pointer_is_down.get();
        let band_ff_lo = state.band_ff_freq_lo.get();
        let band_ff_hi = state.band_ff_freq_hi.get();
        let het_freq_auto = state.het_freq_auto.get();
        let het_cutoff_auto = state.het_cutoff_auto.get();
        let hfr_enabled = state.hfr_enabled.get();
        let output_freq_hl = state.output_freq_highlight.get();
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
        let annotation_hover_handle = state.annotation_hover_handle.get();
        let annotations_visible = state.annotations_visible.get();
        let active_focus = state.active_focus.get();
        let _timeline = state.active_timeline.get(); // trigger redraw on timeline change
        pre_rendered.track();
        // Re-read canvas dimensions when sidebar layout changes
        let _sidebar = state.sidebar_collapsed.get();
        let _sidebar_width = state.sidebar_width.get();
        let _rsidebar = state.right_sidebar_collapsed.get();
        let _rsidebar_width = state.right_sidebar_width.get();
        let clean_view = state.clean_view.get();

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
        let timeline = state.active_timeline.get_untracked();
        let idx = if timeline.is_some() { None } else { state.current_file_index.get_untracked() };

        // In timeline mode, use the first segment's file for freq/resolution defaults
        let primary_file_idx = if let Some(ref tl) = timeline {
            tl.segments.first().map(|s| s.file_index)
        } else {
            idx
        };
        // Check for active waterfall early — when active, use waterfall
        // parameters so listening/recording works identically with or without
        // a file open.
        let is_recording = state.mic_recording.get_untracked();
        let is_listening = state.mic_listening.get_untracked();
        let waterfall_active = (is_recording || is_listening) && crate::canvas::live_waterfall::is_active();

        let (time_res, original_max_freq) = if waterfall_active {
            (crate::canvas::live_waterfall::time_resolution(),
             crate::canvas::live_waterfall::max_freq())
        } else {
            let tr = primary_file_idx
                .and_then(|i| files.get(i))
                .map(|f| f.spectrogram.time_resolution)
                .unwrap_or(1.0);
            let mf = primary_file_idx
                .and_then(|i| files.get(i))
                .map(|f| f.spectrogram.max_freq)
                .unwrap_or(96_000.0);
            (tr, mf)
        };
        let scroll_col = scroll / time_res;
        let decim_effective = state.display_decimate_effective.get_untracked();
        let original_sample_rate = primary_file_idx
            .and_then(|i| files.get(i))
            .map(|f| f.spectrogram.sample_rate)
            .unwrap_or(192_000);
        let file_max_freq = if waterfall_active {
            original_max_freq
        } else if decim_effective > 0 && decim_effective < original_sample_rate {
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
        let xform_or_decim = state.display_transform.get_untracked()
            || main_view == MainView::XformedSpec
            || decim_effective > 0;
        let colormap = if flow_on {
            ColormapMode::Uniform(Colormap::Greyscale)
        } else if hfr_enabled && band_ff_hi > band_ff_lo && !xform_or_decim {
            ColormapMode::HfrFocus {
                colormap: hfr_colormap_pref,
                band_ff_lo_frac: band_ff_lo / file_max_freq,
                band_ff_hi_frac: band_ff_hi / file_max_freq,
            }
        } else if hfr_enabled {
            ColormapMode::Uniform(hfr_colormap_pref)
        } else {
            ColormapMode::Uniform(colormap_pref)
        };

        // Timeline or single-file duration and rendering setup
        let duration = if waterfall_active {
            crate::canvas::live_waterfall::total_columns() as f64 * time_res
        } else if let Some(ref tl) = timeline {
            tl.total_duration_secs
        } else {
            idx.and_then(|i| files.get(i)).map(|f| f.audio.duration_secs).unwrap_or(0.0)
        };
        let visible_time = (display_w as f64 / zoom) * time_res;

        // In timeline mode, use primary segment for ref_db/auto-gain/debug;
        // in single-file mode, use the current file index.
        let effective_idx = if timeline.is_some() { primary_file_idx } else { idx };
        let file = effective_idx.and_then(|i| files.get(i));
        let total_cols = file.map(|f| {
            let tc = f.spectrogram.total_columns;
            if tc > 0 { tc } else { f.spectrogram.columns.len() }
        }).unwrap_or(0);
        let file_idx_val = effective_idx.unwrap_or(0);

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

        // ── Live waterfall rendering (recording / listening) ──
        // When the waterfall is active, render directly from it and skip the
        // tile-based pipeline entirely.
        let live_data_cols = state.mic_live_data_cols.get_untracked();

        let base_drawn = if waterfall_active {
            // Use waterfall's own time_res and max_freq for correct viewport mapping
            let wf_time_res = crate::canvas::live_waterfall::time_resolution();
            let wf_max_freq = crate::canvas::live_waterfall::max_freq();
            let wf_scroll_col = scroll / wf_time_res;
            let wf_freq_crop_lo = min_freq / wf_max_freq;
            let wf_freq_crop_hi = (max_display_freq.unwrap_or(wf_max_freq).min(wf_max_freq) / wf_max_freq).min(1.0);

            // Auto-gain from waterfall's running max magnitude
            let wf_ref_db = if display_auto_gain {
                let max_mag = crate::canvas::live_waterfall::get_max_magnitude();
                if max_mag > 0.0 { 20.0 * max_mag.log10() } else { fixed_ref_db }
            } else {
                fixed_ref_db
            };
            let wf_display_settings = SpectDisplaySettings {
                floor_db: spect_floor,
                range_db: spect_range,
                gamma: spect_gamma,
                gain_db: spect_gain - wf_ref_db + display_boost,
            };

            {
                let rendered = crate::canvas::live_waterfall::render_viewport(
                    &ctx,
                    display_w as f64,
                    display_h as f64,
                    wf_scroll_col,
                    zoom,
                    wf_freq_crop_lo,
                    wf_freq_crop_hi,
                    &wf_display_settings,
                    colormap,
                    live_data_cols,
                );
                if !rendered {
                    // No waterfall data yet — clear to black so the old file's
                    // spectrogram doesn't remain visible.
                    ctx.set_fill_style_str("#000");
                    ctx.fill_rect(0.0, 0.0, display_w as f64, display_h as f64);
                }
                rendered
            }
        } else {

        // Pre-compute per-frequency dB adjustments for display EQ / noise filter
        let tile_height = state.spect_fft_mode.get_untracked().max_fft_size() / 2 + 1;
        let freq_adjustments = compute_freq_adjustments(&state, file_max_freq, tile_height);

        // Render base spectrogram.
        // Priority: timeline | flow tiles | normal tiles > pre_rendered > preview > black
        if let Some(tl) = timeline.as_ref() {
            // ── Timeline mode: render each visible segment ──
            let px_per_sec = zoom / time_res;
            let visible_start = scroll;
            let visible_end = scroll + visible_time;

            // Fill entire canvas with black first (covers gaps)
            ctx.set_fill_style_str("#000");
            ctx.fill_rect(0.0, 0.0, display_w as f64, display_h as f64);

            let mut any_drawn = false;
            for seg in tl.segments_in_range(visible_start, visible_end) {
                let seg_file = match files.get(seg.file_index) {
                    Some(f) => f,
                    None => continue,
                };
                let seg_time_res = seg_file.spectrogram.time_resolution;
                let seg_total_cols = {
                    let tc = seg_file.spectrogram.total_columns;
                    if tc > 0 { tc } else { seg_file.spectrogram.columns.len() }
                };
                if seg_total_cols == 0 { continue; }

                // Canvas pixel range for this segment
                let seg_canvas_start = (seg.timeline_offset_secs - scroll) * px_per_sec;
                let seg_canvas_end = ((seg.timeline_offset_secs + seg.duration_secs) - scroll) * px_per_sec;
                let clip_left = seg_canvas_start.max(0.0);
                let clip_right = seg_canvas_end.min(display_w as f64);
                if clip_left >= clip_right { continue; }

                // Scroll offset within this file
                let file_scroll = (scroll - seg.timeline_offset_secs).max(0.0);
                let file_scroll_col = file_scroll / seg_time_res;

                ctx.save();
                ctx.begin_path();
                ctx.rect(clip_left, 0.0, clip_right - clip_left, display_h as f64);
                ctx.clip();

                // Translate to the visible start of this segment so the helper
                // can render against a segment-local viewport.
                let translate_x = clip_left;
                ctx.translate(translate_x, 0.0).unwrap_or(());

                let seg_visible_time = (clip_right - clip_left) / px_per_sec;
                let seg_px_per_sec = px_per_sec;
                let seg_zoom = seg_px_per_sec * seg_time_res;

                let ideal_lod_for_source = crate::canvas::tile_cache::select_lod(seg_zoom);
                let tile_source = if reassign_on && ideal_lod_for_source > 1 {
                    spectrogram_renderer::TileSource::Reassigned
                } else {
                    spectrogram_renderer::TileSource::Normal
                };
                let xform_on = state.display_transform.get_untracked();
                let preview_ref = if xform_on || decim_effective > 0 {
                    None
                } else {
                    seg_file.preview.as_ref()
                };

                let drawn = spectrogram_renderer::blit_tiles_viewport(
                    &ctx, clip_right - clip_left, display_h as f64, seg.file_index, seg_total_cols,
                    file_scroll_col, seg_zoom, freq_crop_lo, freq_crop_hi,
                    spectrogram_renderer::TileRenderMode::Spectrogram(colormap),
                    &display_settings,
                    freq_adjustments.as_deref(),
                    preview_ref,
                    file_scroll, seg_visible_time, seg.duration_secs,
                    tile_source,
                );

                // Schedule missing tiles for this segment
                crate::canvas::tile_scheduler::schedule_normal_tiles(
                    state, seg.file_index, seg_total_cols, file_scroll_col, seg_zoom,
                    clip_right - clip_left, seg_time_res, is_playing, reassign_on, &disposed,
                );

                ctx.restore();
                if drawn { any_drawn = true; }
            }

            // Draw gap indicators
            let segments = &tl.segments;
            for i in 0..segments.len() {
                let seg_end = segments[i].timeline_offset_secs + segments[i].duration_secs;
                let next_start = if i + 1 < segments.len() {
                    segments[i + 1].timeline_offset_secs
                } else {
                    continue;
                };
                if next_start > seg_end + 0.001 {
                    // There's a gap
                    let gap_x1 = (seg_end - scroll) * px_per_sec;
                    let gap_x2 = (next_start - scroll) * px_per_sec;
                    if gap_x2 > 0.0 && gap_x1 < display_w as f64 {
                        let x1 = gap_x1.max(0.0);
                        let x2 = gap_x2.min(display_w as f64);
                        // Draw subtle dashed line at gap center
                        let mid = (x1 + x2) / 2.0;
                        ctx.set_stroke_style_str("#333");
                        ctx.set_line_width(1.0);
                        let _ = ctx.set_line_dash(&js_sys::Array::of2(
                            &JsValue::from_f64(3.0),
                            &JsValue::from_f64(3.0),
                        ));
                        ctx.begin_path();
                        ctx.move_to(mid, 0.0);
                        ctx.line_to(mid, display_h as f64);
                        ctx.stroke();
                        let _ = ctx.set_line_dash(&js_sys::Array::new());
                    }
                }
            }

            any_drawn
        } else if flow_on && total_cols > 0 {
            // Flow mode (includes phase coherence): composite dB+shift tiles at render time
            let ig = state.flow_intensity_gate.get_untracked();
            let mg = state.flow_gate.get_untracked();
            let op = 1.0_f32; // opacity consolidated into color gain
            let sg = state.flow_shift_gain.get_untracked();
            let cg = state.flow_color_gamma.get_untracked();
            let algo = state.spectrogram_display.get_untracked().flow_algo();
            let flow_scheme = state.flow_color_scheme.get_untracked();
            let flow_render_mode = spectrogram_renderer::TileRenderMode::Flow {
                intensity_gate: ig,
                flow_gate: mg,
                opacity: op,
                shift_gain: sg,
                color_gamma: cg,
                algo,
                scheme: flow_scheme,
            };
            let drawn = spectrogram_renderer::blit_tiles_viewport(
                &ctx, display_w as f64, display_h as f64, file_idx_val, total_cols,
                scroll_col, zoom, freq_crop_lo, freq_crop_hi,
                flow_render_mode, &display_settings, freq_adjustments.as_deref(),
                file.and_then(|f| f.preview.as_ref()),
                scroll, visible_time, duration,
                spectrogram_renderer::TileSource::Flow,
            );

            // Schedule missing flow tiles
            crate::canvas::tile_scheduler::schedule_flow_tiles(
                state, file_idx_val, total_cols, scroll_col, zoom, display_w as f64, algo,
            );

            drawn
        } else if !flow_on && total_cols > 0 {
            // Normal or reassignment tile-based rendering
            let ideal_lod_for_source = crate::canvas::tile_cache::select_lod(zoom);
            let tile_source = if reassign_on && ideal_lod_for_source > 1 {
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
                &ctx, display_w as f64, display_h as f64, file_idx_val, total_cols,
                scroll_col, zoom, freq_crop_lo, freq_crop_hi,
                spectrogram_renderer::TileRenderMode::Spectrogram(colormap),
                &display_settings,
                freq_adjustments.as_deref(),
                preview_ref,
                scroll, visible_time, duration,
                tile_source,
            );

            // Schedule missing tiles
            crate::canvas::tile_scheduler::schedule_normal_tiles(
                state, file_idx_val, total_cols, scroll_col, zoom,
                display_w as f64, time_res, is_playing, reassign_on, &disposed,
            );

            drawn
        } else if pre_rendered.with_untracked(|pr| pr.is_some()) {
            // Small file with columns in memory — use monolithic pre_rendered
            pre_rendered.with_untracked(|pr| {
                if let Some(rendered) = pr {
                    spectrogram_renderer::blit_viewport(
                        &ctx, rendered, display_w as f64, display_h as f64, scroll_col, zoom,
                        freq_crop_lo, freq_crop_hi, colormap,
                    );
                }
            });
            true
        } else if let Some(pv) = file.and_then(|f| f.preview.as_ref()) {
            spectrogram_renderer::blit_preview_as_background(
                &ctx, pv, display_w as f64, display_h as f64,
                scroll, visible_time, duration,
                freq_crop_lo, freq_crop_hi,
                colormap,
            );
            true
        } else {
            ctx.set_fill_style_str("#000");
            ctx.fill_rect(0.0, 0.0, display_w as f64, display_h as f64);
            false
        }
        }; // end of waterfall-or-tile if/else

        // Tile debug overlay (drawn on top of tiles, under other overlays)
        if debug_tiles {
            if let Some(ref tl) = timeline {
                // Timeline mode: draw debug overlay per segment
                let px_per_sec = zoom / time_res;
                let visible_start = scroll;
                let visible_end = scroll + visible_time;
                for seg in tl.segments_in_range(visible_start, visible_end) {
                    let seg_file = match files.get(seg.file_index) {
                        Some(f) => f,
                        None => continue,
                    };
                    let seg_time_res = seg_file.spectrogram.time_resolution;
                    let seg_tc = {
                        let tc = seg_file.spectrogram.total_columns;
                        if tc > 0 { tc } else { seg_file.spectrogram.columns.len() }
                    };
                    if seg_tc == 0 { continue; }
                    let seg_canvas_start = (seg.timeline_offset_secs - scroll) * px_per_sec;
                    let seg_canvas_end = ((seg.timeline_offset_secs + seg.duration_secs) - scroll) * px_per_sec;
                    let clip_left = seg_canvas_start.max(0.0);
                    let clip_right = seg_canvas_end.min(display_w as f64);
                    if clip_left >= clip_right { continue; }
                    let file_scroll = (scroll - seg.timeline_offset_secs).max(0.0);
                    let file_scroll_col = file_scroll / seg_time_res;
                    let seg_zoom = px_per_sec * seg_time_res;
                    ctx.save();
                    ctx.begin_path();
                    ctx.rect(clip_left, 0.0, clip_right - clip_left, display_h as f64);
                    ctx.clip();
                    ctx.translate(clip_left, 0.0).unwrap_or(());
                    spectrogram_renderer::draw_tile_debug_overlay(
                        &ctx, clip_right - clip_left, display_h as f64, seg.file_index, seg_tc, file_scroll_col, seg_zoom,
                        state.spect_fft_mode.get_untracked(), flow_on,
                    );
                    ctx.restore();
                }
            } else if total_cols > 0 {
                spectrogram_renderer::draw_tile_debug_overlay(
                    &ctx, display_w as f64, display_h as f64, file_idx_val, total_cols, scroll_col, zoom,
                    state.spect_fft_mode.get_untracked(), flow_on,
                );
            }
        }

        // Step 2: Draw overlays on top of the base spectrogram
        if base_drawn && !clean_view {
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
            let band_ff_drag_active2 = matches!(spec_drag, Some(SpectrogramHandle::BandFfUpper) | Some(SpectrogramHandle::BandFfLower) | Some(SpectrogramHandle::BandFfMiddle));
            // Only pass BandFF range to markers when HFR is on (or during axis drag which is
            // handled separately by axis_drag_lo/hi). This prevents color bands from showing
            // for a hidden/inactive BandFF range.
            let (marker_band_ff_lo, marker_band_ff_hi) = if hfr_enabled || band_ff_drag_active2 { (band_ff_lo, band_ff_hi) } else { (0.0, 0.0) };
            let marker_state = FreqMarkerState {
                mouse_freq,
                mouse_in_label_area: mouse_freq.is_some() && mouse_cx < LABEL_AREA_WIDTH,
                label_hover_opacity: label_opacity,
                file_max_freq,
                axis_drag_lo: adl2,
                axis_drag_hi: adh2,
                band_ff_drag_active: band_ff_drag_active2,
                band_ff_lo: marker_band_ff_lo,
                band_ff_hi: marker_band_ff_hi,
                band_ff_handles_active: spec_hover.is_some() || spec_drag.is_some(),
                shield_style: state.shield_style.get_untracked(),
            };

            let xform_on = state.display_transform.get_untracked()
                || main_view == MainView::XformedSpec;
            // When xform/decim is on, adjust marker state: right-side labels, hide focus handles
            let marker_state = if xform_on || decim_effective > 0 {
                FreqMarkerState {
                    mouse_in_label_area: mouse_freq.is_some() && mouse_cx > (display_w as f64 - LABEL_AREA_WIDTH),
                    band_ff_lo: 0.0,
                    band_ff_hi: 0.0,
                    band_ff_drag_active: false,
                    band_ff_handles_active: false,
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
                let clock_cfg = if let Some(ref tl) = timeline {
                    // In timeline mode, use the timeline origin as the clock reference
                    if tl.origin_epoch_ms > 0.0 {
                        Some(crate::canvas::time_markers::ClockTimeConfig {
                            recording_start_epoch_ms: tl.origin_epoch_ms,
                        })
                    } else {
                        None
                    }
                } else {
                    state.current_file()
                        .and_then(|f| f.recording_start_epoch_ms())
                        .map(|ms| crate::canvas::time_markers::ClockTimeConfig {
                            recording_start_epoch_ms: ms,
                        })
                };
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

            // BandFF overlay (dim outside focus range) — skip in xform view
            // During axis drag with HFR off, show the overlay using the drag range
            // so the user gets visual feedback of their selection
            let (overlay_lo, overlay_hi) = if band_ff_hi > band_ff_lo {
                (band_ff_lo, band_ff_hi)
            } else if let (Some(a), Some(b)) = (axis_drag_start, axis_drag_current) {
                let lo = a.min(b);
                let hi = a.max(b);
                if hi - lo > 500.0 { (lo, hi) } else { (0.0, 0.0) }
            } else {
                (0.0, 0.0)
            };
            if overlay_hi > overlay_lo && !xform_on {
                spectrogram_renderer::draw_band_ff_overlay(
                    &ctx,
                    overlay_lo, overlay_hi,
                    min_freq, max_freq,
                    display_h as f64, display_w as f64,
                    spec_hover, spec_drag,
                    state.is_mobile.get_untracked(),
                    active_focus == Some(crate::state::ActiveFocus::FrequencyFocus),
                    pointer_down,
                    mouse_freq,
                );
            }

            // Output frequency highlight (from HFR panel hover)
            if let Some((hl_lo, hl_hi)) = output_freq_hl {
                if hl_hi > hl_lo && !xform_on {
                    let ch = display_h as f64;
                    let freq_range = max_freq - min_freq;
                    if freq_range > 0.0 {
                        let y_top = ch * (1.0 - (hl_hi - min_freq) / freq_range);
                        let y_bot = ch * (1.0 - (hl_lo - min_freq) / freq_range);
                        // Semi-transparent blue band
                        ctx.set_fill_style_str("rgba(100, 180, 255, 0.15)");
                        ctx.fill_rect(0.0, y_top, display_w as f64, y_bot - y_top);
                        // Edge lines
                        ctx.set_stroke_style_str("rgba(100, 180, 255, 0.5)");
                        ctx.set_line_width(1.0);
                        ctx.begin_path();
                        ctx.move_to(0.0, y_top);
                        ctx.line_to(display_w as f64, y_top);
                        ctx.move_to(0.0, y_bot);
                        ctx.line_to(display_w as f64, y_bot);
                        ctx.stroke();
                    }
                }
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

            // Draw saved annotation selections (skip in xform view or when hidden via toolbar toggle)
            if !xform_on && annotations_visible {
                if let Some(file_idx_val) = idx {
                    if let Some(Some(set)) = annotation_store.sets.get(file_idx_val) {
                        let hover_ref = annotation_hover_handle.as_ref()
                            .map(|(id, pos)| (id.as_str(), *pos));
                        spectrogram_renderer::draw_annotations(
                            &ctx,
                            set,
                            &selected_annotation_ids,
                            hover_ref,
                            min_freq,
                            max_freq,
                            scroll,
                            time_res,
                            zoom,
                            display_w as f64,
                            display_h as f64,
                            state.is_mobile.get_untracked(),
                            active_focus == Some(crate::state::ActiveFocus::Annotations),
                        );
                    }
                }
            }

            // File-embedded time markers (WAV cue points, M4A chapters).
            // Rendered on top of annotations so a label on the line is visible.
            if !xform_on && annotations_visible {
                if let Some(file_idx_val) = idx {
                    if let Some(file) = files.get(file_idx_val) {
                        if !file.wav_markers.is_empty() {
                            let sr = file.audio.sample_rate as f64;
                            let markers: Vec<(f64, Option<String>)> = file.wav_markers.iter()
                                .map(|m| (m.position as f64 / sr, m.label.clone()))
                                .collect();
                            spectrogram_renderer::draw_time_marker_lines(
                                &ctx,
                                &markers,
                                spectrogram_renderer::TimeMarkerStyle::FileEmbedded,
                                scroll,
                                time_res,
                                zoom,
                                display_w as f64,
                                display_h as f64,
                            );
                        }
                    }
                    // User annotation markers (AnnotationKind::Marker).
                    if let Some(Some(set)) = annotation_store.sets.get(file_idx_val) {
                        let ann_markers: Vec<(f64, Option<String>)> = set.annotations.iter()
                            .filter_map(|a| match &a.kind {
                                crate::annotations::AnnotationKind::Marker(m) => {
                                    Some((m.time, m.label.clone()))
                                }
                                _ => None,
                            })
                            .collect();
                        if !ann_markers.is_empty() {
                            spectrogram_renderer::draw_time_marker_lines(
                                &ctx,
                                &ann_markers,
                                spectrogram_renderer::TimeMarkerStyle::Annotation,
                                scroll,
                                time_res,
                                zoom,
                                display_w as f64,
                                display_h as f64,
                            );
                        }
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

            // Draw static position marker when not playing in FromHere mode
            if state.play_start_mode.get() .uses_from_here() && !is_playing && canvas_tool == CanvasTool::Hand {
                let here_x = display_w as f64 * viewport::PLAY_FROM_HERE_FRACTION;
                let here_time = viewport::play_from_here_time(scroll, visible_time);
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
                    ctx.fill();
                }
            }
        }
    }});

    // Effect 4: auto-scroll to follow playhead during playback
    // Supports temporary suspension: when the user manually scrolls, following
    // pauses until the playhead is on-screen and 200ms have passed since the
    // last scroll action — so it resumes even when very zoomed in.
    Effect::new(move || {
        let playhead = state.playhead_time.get();
        let is_playing = state.is_playing.get();
        let follow = state.follow_cursor.get();
        // Use get_untracked to avoid recursive Effect invocation — this Effect
        // already re-runs via playhead_time / is_playing / follow_cursor changes.
        let suspended = state.follow_suspended.get_untracked();

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
        let from_here_mode = state.play_start_mode.get_untracked() .uses_from_here();

        let visible_time = viewport::visible_time(display_w, zoom, time_res);
        let playhead_rel = playhead - scroll;

        if suspended {
            // Resume once the playhead is on-screen and user stopped scrolling
            let playhead_visible = playhead_rel >= 0.0 && playhead_rel <= visible_time;
            if playhead_visible {
                let resume = match state.follow_visible_since.get_untracked() {
                    Some(since) => js_sys::Date::now() - since >= 200.0,
                    None => true, // no recorded scroll time — safe to resume
                };
                if resume {
                    state.follow_suspended.set(false);
                    state.follow_visible_since.set(None);
                }
            }
            return;
        }

        // When very zoomed in (viewport < 0.5s), follow every frame to keep
        // the playhead pinned at the FOLLOW_CURSOR_FRACTION position — the edge
        // trigger would fire too frequently and look jarring.
        if visible_time < viewport::FOLLOW_EXACT_THRESHOLD_SECS {
            let target_scroll = playhead - visible_time * viewport::FOLLOW_CURSOR_FRACTION;
            state.scroll_offset.set(viewport::clamp_scroll_for_mode(target_scroll, duration, visible_time, from_here_mode));
        }
        // Normal follow: scroll when playhead nears the edge
        else if playhead_rel > visible_time * viewport::FOLLOW_CURSOR_EDGE_FRACTION || playhead_rel < 0.0 {
            let target_scroll = playhead - visible_time * viewport::FOLLOW_CURSOR_FRACTION;
            state.scroll_offset.set(viewport::clamp_scroll_for_mode(target_scroll, duration, visible_time, from_here_mode));
        }
    });

    // Effect 5: pre-fetch tiles ahead of the viewport and at the start of the file.
    // Debounced at 200ms so it doesn't fire at 60fps during playback.
    {
        let prefetch_handle: Rc<Cell<Option<i32>>> = Rc::new(Cell::new(None));

        Effect::new({
            let disposed = disposed.clone();
            move || {
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
                web_sys::window().unwrap().clear_timeout_with_handle(h);
            }

            let handle_rc = prefetch_handle.clone();
            let disposed_rc = disposed.clone();
            let cb = Closure::once(move || {
                if disposed_rc.load(Ordering::Relaxed) { return; }
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
                    Some(state.spectrogram_display.get_untracked().flow_algo())
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
        }});

        // Note: pending prefetch timeout (200ms) is not explicitly cancelled on
        // disposal — the callback checks the `disposed` flag and exits early.
    }

    // Effect 6: background preload — progressively pre-compute tiles for the whole file
    // at the current LOD, expanding outward from the viewport center.
    Effect::new(move || {
        let _file_idx = state.current_file_index.get();
        let _scroll = state.scroll_offset.get();
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

    // Stop background preload on component disposal
    on_cleanup(move || {
        state.bg_preload_gen.update(|g| *g = g.wrapping_add(1));
        crate::canvas::tile_cache::stop_background_preload();
    });

    // ── Event handler delegates ────────────────────────────────────────────────
    let on_pointerdown = move |ev: web_sys::PointerEvent| {
        spectrogram_events::on_pointerdown(ev, ix, &canvas_ref, state);
    };
    let on_pointermove = move |ev: web_sys::PointerEvent| {
        spectrogram_events::on_pointermove(ev, ix, &canvas_ref, state);
    };
    let on_pointerleave = move |ev: web_sys::PointerEvent| {
        spectrogram_events::on_pointerleave(ev, ix, state);
    };
    let on_pointerup = move |ev: web_sys::PointerEvent| {
        spectrogram_events::on_pointerup(ev, ix, &canvas_ref, state);
    };
    let on_dblclick = move |ev: web_sys::MouseEvent| {
        spectrogram_events::on_dblclick(ev, &canvas_ref, state);
    };
    let on_touchstart = move |ev: web_sys::TouchEvent| {
        spectrogram_events::on_touchstart(ev, ix, &canvas_ref, state);
    };
    let on_touchmove = move |ev: web_sys::TouchEvent| {
        spectrogram_events::on_touchmove(ev, ix, &canvas_ref, state);
    };
    let on_touchend = move |ev: web_sys::TouchEvent| {
        spectrogram_events::on_touchend(ev, ix, &canvas_ref, state);
    };
    let on_wheel = move |ev: web_sys::WheelEvent| {
        spectrogram_events::on_wheel(ev, state);
    };

    view! {
        <div class="spectrogram-container"
            style=move || {
                // When viewport is pinch-zoomed, allow native pinch so user can zoom back out
                let ta = if state.viewport_zoomed.get() { "pinch-zoom" } else { "none" };
                if state.axis_drag_start_freq.get().is_some() || state.mouse_in_label_area.get()
                    || state.mouse_in_time_axis.get() {
                    return format!("cursor: cell; touch-action: {ta};");
                }
                if state.spec_drag_handle.get().is_some() {
                    return format!("cursor: ns-resize; touch-action: {ta};");
                }
                if let Some(handle) = state.spec_hover_handle.get() {
                    let is_ff = matches!(handle, SpectrogramHandle::BandFfUpper | SpectrogramHandle::BandFfLower | SpectrogramHandle::BandFfMiddle);
                    if !is_ff || crate::canvas::hit_test::is_in_band_ff_drag_zone(
                        state.mouse_canvas_x.get(),
                        state.spectrogram_canvas_width.get(),
                    ) {
                        return format!("cursor: ns-resize; touch-action: {ta};");
                    }
                }
                // Annotation resize handle cursor
                if let Some((_, pos)) = state.annotation_hover_handle.get() {
                    use crate::state::ResizeHandlePosition::*;
                    let cursor = match pos {
                        TopLeft | BottomRight => "nwse-resize",
                        TopRight | BottomLeft => "nesw-resize",
                        Top | Bottom => "ns-resize",
                        Left | Right => "ew-resize",
                    };
                    return format!("cursor: {cursor}; touch-action: {ta};");
                }
                // Annotation drag in progress
                if state.annotation_drag_handle.get().is_some() {
                    return format!("cursor: move; touch-action: {ta};");
                }
                match state.canvas_tool.get() {
                    CanvasTool::Hand => if state.is_dragging.get() {
                        format!("cursor: grabbing; touch-action: {ta};")
                    } else {
                        format!("cursor: grab; touch-action: {ta};")
                    },
                    CanvasTool::Selection => format!("cursor: crosshair; touch-action: {ta};"),
                }
            }
        >
            <canvas
                node_ref=canvas_ref
                style:pointer-events=move || if state.viewport_zoomed.get() { "none" } else { "auto" }
                on:wheel=on_wheel
                on:pointerdown=on_pointerdown
                on:pointermove=on_pointermove
                on:pointerup=on_pointerup
                on:pointerleave=on_pointerleave
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
                    let time_res = if let Some(ref tl) = state.active_timeline.get_untracked() {
                        tl.segments.first().and_then(|s| files.get(s.file_index))
                            .map(|f| f.spectrogram.time_resolution).unwrap_or(1.0)
                    } else {
                        let idx = state.current_file_index.get_untracked();
                        idx.and_then(|i| files.get(i))
                            .map(|f| f.spectrogram.time_resolution)
                            .unwrap_or(1.0)
                    };
                    let visible_time = (cw / zoom) * time_res;
                    let px_per_sec = if visible_time > 0.0 { cw / visible_time } else { 0.0 };
                    let x = (playhead - scroll) * px_per_sec;
                    format!("translateX({:.1}px)", x)
                }
                style:display=move || if state.is_playing.get() && !state.clean_view.get() { "block" } else { "none" }
            />
            // Time-axis hover tooltip (shows full date/time/timezone + source)
            {move || {
                if state.clean_view.get() { return None; }
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

