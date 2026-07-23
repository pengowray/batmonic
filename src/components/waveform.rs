use crate::audio::source::ChannelView;
use crate::canvas::waveform_renderer;
use crate::components::gutter::{BandGutter, TimeGutter};
use crate::components::playhead::Playhead;
use crate::dsp::filters::{apply_eq_filter, apply_eq_filter_fast, split_three_bands_fft};
use crate::dsp::zc_divide::zc_rate_per_bin;
use crate::state::store_fields::*;
use crate::state::{AppState, CanvasTool, FilterQuality, PlaybackMode, WaveformView};
use crate::viewport;
use leptos::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement};

const ZC_BIN_DURATION: f64 = 0.001; // 1ms bins

#[component]
pub fn Waveform() -> impl IntoView {
    let state = expect_context::<AppState>();
    let canvas_ref = NodeRef::<leptos::html::Canvas>::new();
    let hand_drag_start = RwSignal::new((0.0f64, 0.0f64));
    let pinch_state: RwSignal<Option<crate::components::pinch::PinchState>> = RwSignal::new(None);
    let velocity_tracker = StoredValue::new(crate::components::inertia::VelocityTracker::new());
    let inertia_generation = StoredValue::new(0u32);

    // Cache ZC bins — recompute when the file, channel, or EQ settings change.
    let zc_bins = Memo::new(move |_| {
        let files = state.library.files().get();
        let idx = state.library.current_index().get();
        let filter_enabled = state.filter.enabled().get();
        let cv = state.viewmode.channel_view().get();
        // Subscribe to EQ params so memo recomputes when they change
        let freq_low = state.filter.freq_low().get();
        let freq_high = state.filter.freq_high().get();
        let db_below = state.filter.db_below().get();
        let db_selected = state.filter.db_selected().get();
        let db_harmonics = state.filter.db_harmonics().get();
        let db_above = state.filter.db_above().get();
        let band_mode = state.filter.band_mode().get();
        let quality = state.filter.quality().get();

        idx.and_then(|i| files.get(i).cloned()).map(|file| {
            let sr = file.audio.sample_rate;
            // For streaming sources, file.audio.samples is only the head (~30s);
            // match that length for non-MonoMix reads instead of pulling the
            // whole file (which would OOM on multi-hour m4b/m4a files).
            let read_len = file.audio.samples.len();
            let ch_samples = match cv {
                ChannelView::MonoMix => std::borrow::Cow::Borrowed(file.audio.samples.as_slice()),
                _ => std::borrow::Cow::Owned(file.audio.source.read_region(cv, 0, read_len)),
            };
            let samples = if filter_enabled {
                match quality {
                    FilterQuality::Fast => apply_eq_filter_fast(
                        &ch_samples,
                        sr,
                        freq_low,
                        freq_high,
                        db_below,
                        db_selected,
                        db_harmonics,
                        db_above,
                        band_mode,
                    ),
                    FilterQuality::Spectral => apply_eq_filter(
                        &ch_samples,
                        sr,
                        freq_low,
                        freq_high,
                        db_below,
                        db_selected,
                        db_harmonics,
                        db_above,
                        band_mode,
                    ),
                }
            } else {
                ch_samples.into_owned()
            };
            zc_rate_per_bin(&samples, sr, ZC_BIN_DURATION, filter_enabled)
        })
    });

    // Band-split samples for Frequency and Triple waveform views.
    // Returns (below, selected, above) bands using brick-wall FFT separation.
    //
    // The FFT runs over full-file samples — for multi-minute files this
    // takes tens of milliseconds, so running it on every `pointermove`
    // during a band-gutter drag makes the drag visibly laggy. While the
    // user is actively dragging, we return the last computed split
    // unchanged; a fresh split fires once when the drag ends.
    let band_split_cache: StoredValue<Option<(Vec<f32>, Vec<f32>, Vec<f32>)>> =
        StoredValue::new(None);
    // Identity of the last computed split's inputs. During live capture the
    // processing loop bumps file metadata via `files().update()` every ~50 ms,
    // which invalidates this Memo every tick — but the actual samples only change
    // on the adaptive snapshot (~1 Hz). Without this guard the O(N·log N) FFT
    // re-ran ~20×/s over identical data (and unbounded for a growing recording).
    let band_split_key: StoredValue<Option<(usize, usize, u64, u64, ChannelView)>> =
        StoredValue::new(None);
    let band_split = Memo::new(move |_| {
        let wv = state.viewmode.waveform_view().get();
        if wv == WaveformView::Simple {
            band_split_cache.set_value(None);
            band_split_key.set_value(None);
            return None;
        }
        // Early-return while dragging. This must happen BEFORE reading
        // focus_stack so the memo un-subscribes from it during the drag
        // and doesn't re-fire on every band-range tweak.
        if state.filter.band_ff_dragging().get() {
            return band_split_cache.get_value();
        }
        let files = state.library.files().get();
        let idx = state.library.current_index().get();
        let cv = state.viewmode.channel_view().get();
        // Use the user's Band regardless of HFR state — the band_ff_freq_lo/hi
        // signals get zeroed when HFR is off, which would make the "above"
        // lane show the entire signal. effective_range_ignoring_hfr() always
        // reflects what the drag handles on the spectrogram show.
        let ff = state
            .viewmode
            .focus_stack()
            .get()
            .effective_range_ignoring_hfr();
        let freq_low = ff.lo;
        let freq_high = ff.hi;

        // Skip the FFT when nothing that affects it changed (samples buffer
        // identity + length + band range + channel). `samples` is swapped to a
        // fresh Arc on each snapshot, so its data pointer tracks real changes.
        let key = idx.and_then(|i| files.get(i)).map(|f| {
            (
                f.audio.samples.as_ptr() as usize,
                f.audio.samples.len(),
                freq_low.to_bits(),
                freq_high.to_bits(),
                cv,
            )
        });
        if key.is_some() && band_split_key.get_value() == key {
            if let Some(cached) = band_split_cache.get_value() {
                return Some(cached);
            }
        }

        let result = idx.and_then(|i| files.get(i).cloned()).map(|file| {
            let sr = file.audio.sample_rate;
            // See zc_bins memo above — cap the read to the in-memory head
            // length so streaming sources don't try to allocate gigabytes.
            let read_len = file.audio.samples.len();
            let ch_samples = match cv {
                // Stereo (default) + MonoMix both resolve to the mono buffer.
                ChannelView::MonoMix | ChannelView::Stereo => {
                    std::borrow::Cow::Borrowed(file.audio.samples.as_slice())
                }
                _ => std::borrow::Cow::Owned(file.audio.source.read_region(cv, 0, read_len)),
            };

            // Brick-wall band separation via overlap-add FFT. Cascaded IIR
            // lowpasses had poor passband flatness so content from the middle
            // band leaked heavily into the 'above' lane.
            split_three_bands_fft(&ch_samples, sr, freq_low, freq_high)
        });
        band_split_key.set_value(key);
        band_split_cache.set_value(result.clone());
        result
    });

    Effect::new(move || {
        // Diagnostic: time the WHOLE Effect body, incl. the top signal reads and
        // band_split (which is above the canvas draw), recorded at the end.
        let _eperf = web_sys::window().and_then(|w| w.performance());
        let _et0 = _eperf.as_ref().map(|p| p.now());
        let scroll = state.view.scroll_offset().get();
        let zoom = state.view.zoom_level().get();
        let selection = state.interaction.selection().get();
        // Subscribe to file changes for reactivity, then get data without
        // re-subscribing (avoids redundant signal subscription in compute_auto_gain).
        state.library.files().track();
        let files = state.library.files().get_untracked();
        let _timeline_trigger = state.timeline.active().get(); // trigger redraw on timeline change
        let idx = state.library.current_index().get();
        let mode = state.playback.mode().get();
        let waveform_view = state.viewmode.waveform_view().get();
        let is_playing = state.playback.is_playing().get();
        let canvas_tool = state.interaction.canvas_tool().get();
        let cv = state.viewmode.channel_view().get();
        let _tile_ready = state.viewmode.tile_ready_signal().get();
        // During live capture, refresh every capture tick (not just on scroll)
        // so the waveform tracks the live ring smoothly instead of the ~1 Hz
        // source-snapshot cadence.
        let _live_cols = state.mic.live_data_cols().get();
        let wave_auto = state.gain.wave_view_auto().get();
        let gain_db = if wave_auto {
            state.compute_auto_gain_untracked()
        } else {
            state.gain.wave_view_db().get()
        };
        // Re-read canvas dimensions when sidebar layout changes
        let _sidebar = state.panels.left_collapsed().get();
        let _sidebar_width = state.panels.left_width().get();
        let _rsidebar = state.panels.right_collapsed().get();
        let _rsidebar_width = state.panels.right_width().get();
        let clean_view = state.viewmode.clean_view().get();
        let focus = state.viewmode.focus_stack().get();
        let hfr_on = focus.hfr_enabled();
        // Only the band-split views actually consume the FFT split: Triple always,
        // and Frequency only when HFR routes audio through the band. Simple and
        // Frequency-without-HFR draw a single full-band wave, so reading band_split
        // there just burns a full-file split_three_bands_fft (O(N·log N)) every
        // recompute — a waveform-only cost the spectrogram never pays. Gate it.
        // Switching INTO a band view re-runs this Effect (it reads waveform_view +
        // hfr), which then subscribes to band_split, so reactivity is preserved.
        let needs_band = matches!(waveform_view, WaveformView::Triple)
            || (waveform_view == WaveformView::Frequency && hfr_on);
        let band_data = if needs_band { band_split.get() } else { None };
        // Band boundaries for lane labels (Band wave + Triple).
        let ff = focus.effective_range_ignoring_hfr();
        let band_freq_low = ff.lo;
        let band_freq_high = ff.hi;

        let Some(canvas_el) = canvas_ref.get() else {
            return;
        };
        let canvas: &HtmlCanvasElement = canvas_el.as_ref();

        let rect = canvas.get_bounding_client_rect();
        let display_w = rect.width() as u32;
        let display_h = rect.height() as u32;
        if display_w == 0 || display_h == 0 {
            // Canvas not yet laid out (e.g. just remounted) — schedule
            // a retry on the next animation frame so the waveform draws
            // once the browser has computed layout.
            let state_retry = state;
            let cb = wasm_bindgen::closure::Closure::once(move || {
                state_retry
                    .viewmode
                    .tile_ready_signal()
                    .update(|n| *n = n.wrapping_add(1));
            });
            let _ = web_sys::window()
                .unwrap()
                .request_animation_frame(cb.as_ref().unchecked_ref());
            cb.forget();
            return;
        }
        if canvas.width() != display_w || canvas.height() != display_h {
            canvas.set_width(display_w);
            canvas.set_height(display_h);
        }
        // Guarded so a redraw doesn't churn canvas_width subscribers every frame.
        if state.viewmode.spectrogram_canvas_width().get_untracked() != display_w as f64 {
            state
                .viewmode
                .spectrogram_canvas_width()
                .set(display_w as f64);
        }

        // The time axis / selection lives in a sibling <TimeGutter/> strip
        // now, so the waveform paints into the full canvas height.
        let wave_h = display_h as f64;

        let ctx = canvas
            .get_context("2d")
            .unwrap()
            .unwrap()
            .dyn_into::<CanvasRenderingContext2d>()
            .unwrap();

        let timeline = state.timeline.active().get_untracked();

        if let Some(ref tl) = timeline {
            // ── Timeline mode: render waveform for each visible segment ──
            let primary_file = tl.segments.first().and_then(|s| files.get(s.file_index));
            let time_res = primary_file
                .map(|f| f.spectrogram.time_resolution)
                .unwrap_or(1.0);
            let _total_duration = tl.total_duration_secs;
            let px_per_sec = zoom / time_res;
            let visible_time = (display_w as f64 / zoom) * time_res;
            let visible_start = scroll;
            let visible_end = scroll + visible_time;
            let sel_time = selection.map(|s| (s.time_start, s.time_end));

            // Clear canvas
            ctx.set_fill_style_str("#111");
            ctx.fill_rect(0.0, 0.0, display_w as f64, display_h as f64);

            for seg in tl.segments_in_range(visible_start, visible_end) {
                let seg_file = match files.get(seg.file_index) {
                    Some(f) => f,
                    None => continue,
                };
                let sr = seg_file.audio.sample_rate;
                let seg_time_res = seg_file.spectrogram.time_resolution;

                // Canvas pixel range for this segment
                let seg_canvas_start = (seg.timeline_offset_secs - scroll) * px_per_sec;
                let seg_canvas_end =
                    ((seg.timeline_offset_secs + seg.duration_secs) - scroll) * px_per_sec;
                let clip_left = seg_canvas_start.max(0.0);
                let clip_right = seg_canvas_end.min(display_w as f64);
                if clip_left >= clip_right {
                    continue;
                }

                // File-local scroll offset
                let file_scroll = (scroll - seg.timeline_offset_secs).max(0.0);
                let vis_start_time = file_scroll;
                let vis_end_time = (file_scroll + visible_time).min(seg.duration_secs);

                let margin_samples = 64usize;
                let region_start =
                    ((vis_start_time * sr as f64) as usize).saturating_sub(margin_samples);
                let region_end = ((vis_end_time * sr as f64) as usize) + margin_samples;
                let region_len = region_end.saturating_sub(region_start);
                // Borrow the visible window zero-copy for Stereo/MonoMix (mirrors the
                // single-file path) instead of read_region's per-frame Vec alloc.
                let waveform_buf: std::borrow::Cow<[f32]> =
                    match (cv, seg_file.audio.source.as_contiguous()) {
                        (ChannelView::MonoMix | ChannelView::Stereo, Some(all)) => {
                            let end = (region_start + region_len).min(all.len());
                            std::borrow::Cow::Borrowed(if region_start < end {
                                &all[region_start..end]
                            } else {
                                &[]
                            })
                        }
                        _ => std::borrow::Cow::Owned(seg_file.audio.source.read_region(
                            cv,
                            region_start as u64,
                            region_len,
                        )),
                    };

                ctx.save();
                ctx.begin_path();
                ctx.rect(clip_left, 0.0, clip_right - clip_left, wave_h);
                ctx.clip();
                ctx.translate(clip_left, 0.0).unwrap_or(());

                waveform_renderer::draw_waveform(
                    &ctx,
                    &waveform_buf[..],
                    sr,
                    file_scroll,
                    zoom,
                    seg_time_res,
                    clip_right - clip_left,
                    wave_h,
                    sel_time,
                    gain_db,
                    seg.duration_secs,
                    region_start,
                    waveform_renderer::WAVEFORM_GREEN,
                );

                ctx.restore();
            }

            // Time labels moved to the sibling <TimeGutter/> strip.
        } else if let Some(file) = idx.and_then(|i| files.get(i)) {
            let sel_time = selection.map(|s| (s.time_start, s.time_end));
            let max_freq_khz = file.spectrogram.max_freq / 1000.0;
            let buf_duration = file.audio.duration_secs;
            let sr = file.audio.sample_rate;

            // During live listening, scroll_offset is in waterfall time (grows
            // forever) but the sample buffer only holds the last ~10s.  Map scroll
            // into the buffer's coordinate space for sample access / waveform draw.
            let is_live = (file.is_live_listen || file.is_recording)
                && crate::canvas::live_waterfall::is_active();
            let (buf_scroll, _wf_total_duration) = if is_live {
                let wf_total = crate::canvas::live_waterfall::total_time();
                let offset = (wf_total - buf_duration).max(0.0);
                ((scroll - offset).clamp(0.0, buf_duration), wf_total)
            } else {
                (scroll, buf_duration)
            };

            // Calculate visible sample range and read from source
            let visible_time =
                viewport::visible_time(display_w as f64, zoom, file.spectrogram.time_resolution);
            let (vis_start_time, vis_end_time) =
                viewport::data_window(buf_scroll, visible_time, buf_duration).unwrap_or((0.0, 0.0));
            // Add a small margin for edge rendering
            let margin_samples = 64usize;
            let region_start =
                ((vis_start_time * sr as f64) as usize).saturating_sub(margin_samples);
            let region_end = ((vis_end_time * sr as f64) as usize) + margin_samples;
            let region_len = region_end.saturating_sub(region_start);
            // Borrow the visible window straight from the contiguous sample
            // buffer instead of allocating + copying it every scroll frame —
            // `read_region` always allocates a fresh Vec. `as_contiguous()` is
            // the mono-mix buffer, which is exactly what `read_samples` returns
            // for both Stereo and MonoMix (see InMemorySource::read_samples →
            // read_mono). The DEFAULT view is Stereo, so matching only MonoMix
            // here missed the common case entirely. Per-channel / Difference
            // views and streaming sources still go through read_region.
            let _rp = web_sys::window().and_then(|w| w.performance());
            let _rt0 = _rp.as_ref().map(|p| p.now());
            let waveform_buf: std::borrow::Cow<[f32]> =
                match (cv, file.audio.source.as_contiguous()) {
                    (ChannelView::MonoMix | ChannelView::Stereo, Some(all)) => {
                        let end = (region_start + region_len).min(all.len());
                        std::borrow::Cow::Borrowed(if region_start < end {
                            &all[region_start..end]
                        } else {
                            &[]
                        })
                    }
                    _ => std::borrow::Cow::Owned(file.audio.source.read_region(
                        cv,
                        region_start as u64,
                        region_len,
                    )),
                };
            if let (Some(p), Some(t0)) = (_rp.as_ref(), _rt0) {
                waveform_renderer::wf_diag_record_read(p.now() - t0);
            }

            if mode == PlaybackMode::ZeroCrossing {
                if let Some(bins) = zc_bins.get().as_ref() {
                    waveform_renderer::draw_zc_rate(
                        &ctx,
                        bins,
                        ZC_BIN_DURATION,
                        buf_duration,
                        buf_scroll,
                        zoom,
                        file.spectrogram.time_resolution,
                        display_w as f64,
                        wave_h,
                        sel_time,
                        max_freq_khz,
                    );
                }
            } else {
                // Helper to window a full-file band buffer to the visible region
                let window_band = |full: &[f32]| -> Vec<f32> {
                    if region_start < full.len() {
                        let end = (region_start + region_len).min(full.len());
                        full[region_start..end].to_vec()
                    } else {
                        Vec::new()
                    }
                };

                // Band wave shows the selected band in blue over a dim green
                // backdrop — but only makes sense when HFR is actually routing
                // audio through that band. When HFR is off, collapse to a
                // single blue wave (same shape as Simple, just tinted blue)
                // so the view still reads as "band mode" without implying an
                // active filter split. (`hfr_on` computed at the top of the Effect.)

                // Full-band single-wave draw. When zoomed out enough (spp >= MIP_D)
                // on an in-memory MonoMix buffer, render from the decimated min/max
                // mip — folded incrementally, indexes ~spp/MIP_D cells per pixel —
                // instead of re-scanning the whole visible window every frame. Else
                // the raw windowed path (zoomed-in needs sub-cell resolution, and
                // streaming / non-mono sources have no contiguous buffer to mip).
                let spp = if display_w > 0 {
                    visible_time * sr as f64 / display_w as f64
                } else {
                    0.0
                };
                let mip_buf = if matches!(cv, ChannelView::MonoMix | ChannelView::Stereo) {
                    file.audio.source.as_contiguous()
                } else {
                    None
                };
                let mip_d = waveform_renderer::MIP_D as f64;
                let draw_full_wave = |color: &str| {
                    let _perf = web_sys::window().and_then(|w| w.performance());
                    let _t0 = _perf.as_ref().map(|p| p.now());
                    let mut used_mip = false;
                    // ── Live: read the raw capture ring DIRECTLY (the file source is
                    // only a ~1 Hz snapshot, so reading it makes the wave look frozen
                    // between updates). The ring slides every tick; the mip folds the
                    // new samples incrementally (abs-anchored) so it stays cheap and
                    // refreshes at frame rate.
                    let live_drew = if is_live && spp >= mip_d {
                        let cap_cells = ((crate::canvas::live_waterfall::capacity_time()
                            * sr as f64) as usize
                            / waveform_renderer::MIP_D)
                            + 8;
                        // True absolute base of the ring (total samples trimmed off
                        // the front), published by the live loop. Deriving this from
                        // the waterfall's processed clock lagged the capture ring by a
                        // fluctuating amount and made the wave jump — this is exact.
                        let abs_offset = crate::audio::mic_backend::live_ring_base();
                        crate::audio::mic_backend::with_live_samples(state.is_tauri, |ring| {
                            if ring.is_empty() {
                                return false;
                            }
                            let ring_dur = ring.len() as f64 / sr as f64;
                            let ring_offset = abs_offset as f64 / sr as f64; // abs time of ring[0]
                            let ring_scroll = (scroll - ring_offset).clamp(0.0, ring_dur);
                            waveform_renderer::draw_waveform_mipped_live(
                                &ctx,
                                ring,
                                abs_offset,
                                cap_cells,
                                sr,
                                ring_scroll,
                                zoom,
                                file.spectrogram.time_resolution,
                                display_w as f64,
                                wave_h,
                                sel_time,
                                gain_db,
                                ring_dur,
                                color,
                            );
                            true
                        })
                    } else {
                        false
                    };
                    if live_drew {
                        used_mip = true;
                    } else {
                        match mip_buf {
                            // Static file (or live with a too-small window): the whole
                            // contiguous buffer is a plain [0, len) buffer (abs_offset 0).
                            Some(all) if spp >= mip_d => {
                                waveform_renderer::draw_waveform_mipped(
                                    &ctx,
                                    all,
                                    0,
                                    0,
                                    sr,
                                    buf_scroll,
                                    zoom,
                                    file.spectrogram.time_resolution,
                                    display_w as f64,
                                    wave_h,
                                    sel_time,
                                    gain_db,
                                    buf_duration,
                                    color,
                                );
                                used_mip = true;
                            }
                            // Zoomed-in (sub-cell detail) or non-contiguous → raw window.
                            _ => {
                                waveform_renderer::draw_waveform(
                                    &ctx,
                                    &waveform_buf[..],
                                    sr,
                                    buf_scroll,
                                    zoom,
                                    file.spectrogram.time_resolution,
                                    display_w as f64,
                                    wave_h,
                                    sel_time,
                                    gain_db,
                                    buf_duration,
                                    region_start,
                                    color,
                                );
                            }
                        }
                    }
                    if let (Some(p), Some(t0)) = (_perf.as_ref(), _t0) {
                        waveform_renderer::wf_diag_record(p.now() - t0, used_mip, spp);
                    }
                };

                match waveform_view {
                    WaveformView::Simple => {
                        draw_full_wave(waveform_renderer::WAVEFORM_GREEN);
                    }
                    WaveformView::Frequency if !hfr_on => {
                        draw_full_wave(waveform_renderer::WAVEFORM_BLUE);
                    }
                    WaveformView::Frequency => {
                        if let Some((ref _below, ref selected, ref _above)) = band_data.as_ref() {
                            let selected_region = window_band(selected);
                            waveform_renderer::draw_waveform_freq(
                                &ctx,
                                &waveform_buf[..],
                                &selected_region,
                                sr,
                                buf_scroll,
                                zoom,
                                file.spectrogram.time_resolution,
                                display_w as f64,
                                wave_h,
                                sel_time,
                                gain_db,
                                buf_duration,
                                region_start,
                                band_freq_low,
                                band_freq_high,
                            );
                        } else {
                            draw_full_wave(waveform_renderer::WAVEFORM_GREEN);
                        }
                    }
                    WaveformView::Triple => {
                        if let Some((ref below, ref selected, ref above)) = band_data.as_ref() {
                            let below_region = window_band(below);
                            let selected_region = window_band(selected);
                            let above_region = window_band(above);
                            waveform_renderer::draw_waveform_triple(
                                &ctx,
                                &below_region,
                                &selected_region,
                                &above_region,
                                sr,
                                buf_scroll,
                                zoom,
                                file.spectrogram.time_resolution,
                                display_w as f64,
                                wave_h,
                                sel_time,
                                gain_db,
                                buf_duration,
                                region_start,
                                band_freq_low,
                                band_freq_high,
                            );
                        } else {
                            draw_full_wave(waveform_renderer::WAVEFORM_GREEN);
                        }
                    }
                }
            }

            // Time labels moved to the sibling <TimeGutter/> strip.

            // File-embedded time markers (WAV cue points, M4A chapters)
            // and user annotation markers.
            if !clean_view {
                if !file.wav_markers.is_empty() {
                    let sr = file.audio.sample_rate as f64;
                    let markers: Vec<(f64, Option<String>)> = file
                        .wav_markers
                        .iter()
                        .map(|m| (m.position as f64 / sr, m.label.clone()))
                        .collect();
                    crate::canvas::overlays::draw_time_marker_lines(
                        &ctx,
                        &markers,
                        crate::canvas::overlays::TimeMarkerStyle::FileEmbedded,
                        buf_scroll,
                        file.spectrogram.time_resolution,
                        zoom,
                        display_w as f64,
                        display_h as f64,
                    );
                }
                if let Some(file_id_val) = state.current_file_id_tracked() {
                    let store = state.annotations.store().get();
                    if let Some(set) = store.get(file_id_val) {
                        let ann_markers: Vec<(f64, Option<String>)> = set
                            .annotations
                            .iter()
                            .filter_map(|a| match &a.kind {
                                crate::annotations::AnnotationKind::Marker(m) => {
                                    Some((m.time, m.label.clone()))
                                }
                                _ => None,
                            })
                            .collect();
                        if !ann_markers.is_empty() {
                            crate::canvas::overlays::draw_time_marker_lines(
                                &ctx,
                                &ann_markers,
                                crate::canvas::overlays::TimeMarkerStyle::Annotation,
                                buf_scroll,
                                file.spectrogram.time_resolution,
                                zoom,
                                display_w as f64,
                                display_h as f64,
                            );
                        }
                    }
                }
            }

            // Draw "play here" marker when not playing
            if !clean_view
                && state.playback.start_mode().get().uses_from_here()
                && !is_playing
                && canvas_tool == CanvasTool::Hand
            {
                let visible_time = viewport::visible_time(
                    display_w as f64,
                    zoom,
                    file.spectrogram.time_resolution,
                );
                let here_x = display_w as f64 * viewport::PLAY_FROM_HERE_FRACTION;
                let here_time = viewport::play_from_here_time(scroll, visible_time);
                state.playback.from_here_time().set(here_time);
                ctx.set_stroke_style_str("rgba(100, 160, 255, 0.35)");
                ctx.set_line_width(1.5);
                let _ = ctx.set_line_dash(&js_sys::Array::of2(
                    &wasm_bindgen::JsValue::from_f64(4.0),
                    &wasm_bindgen::JsValue::from_f64(3.0),
                ));
                ctx.begin_path();
                ctx.move_to(here_x, 0.0);
                ctx.line_to(here_x, wave_h);
                ctx.stroke();
                let _ = ctx.set_line_dash(&js_sys::Array::new());
            }
        } else {
            ctx.set_fill_style_str("#0a0a0a");
            ctx.fill_rect(0.0, 0.0, display_w as f64, display_h as f64);
        }

        if let (Some(p), Some(t0)) = (_eperf.as_ref(), _et0) {
            waveform_renderer::wf_diag_record_effect(p.now() - t0);
        }

        // Time gutter + selection highlight now live in a sibling
        // <TimeGutter/> strip (mounted below the chart-row).
    });

    // Auto-scroll to follow playhead during playback (with suspension support)
    Effect::new(move || {
        crate::components::spectrogram_events::follow_playhead(&state, canvas_ref);
    });

    // Time-selection drags / taps now live on the sibling <TimeGutter/>
    // strip — this component's canvas only handles hand-pan + inertia.

    let on_wheel = move |ev: web_sys::WheelEvent| {
        ev.prevent_default();
        if ev.ctrl_key() {
            let delta = if ev.delta_y() > 0.0 { 0.9 } else { 1.1 };
            state.view.zoom_level().update(|z| {
                *z = (*z * delta).clamp(viewport::MIN_ZOOM, viewport::MAX_ZOOM);
            });
        } else {
            let raw_delta = ev.delta_y() + ev.delta_x();
            let files = state.library.files().get_untracked();
            let idx = state.library.current_index().get_untracked().unwrap_or(0);
            let (visible_time, duration) = if let Some(file) = files.get(idx) {
                let zoom = state.view.zoom_level().get_untracked();
                let canvas_w = state.viewmode.spectrogram_canvas_width().get_untracked();
                (
                    viewport::visible_time(canvas_w, zoom, file.spectrogram.time_resolution),
                    file.audio.duration_secs,
                )
            } else {
                return;
            };
            let delta =
                raw_delta.signum() * visible_time * 0.1 * (raw_delta.abs() / 100.0).min(3.0);
            let from_here_mode = state.playback.start_mode().get_untracked().uses_from_here();
            state.suspend_follow();
            state.view.scroll_offset().update(|s| {
                *s = viewport::clamp_scroll_for_mode(
                    *s + delta,
                    duration,
                    visible_time,
                    from_here_mode,
                );
            });
        }
    };

    let on_pointerdown = move |ev: web_sys::PointerEvent| {
        if ev.button() != 0 {
            return;
        }
        if state.status.viewport_zoomed().get_untracked() {
            return;
        }

        if state.interaction.canvas_tool().get_untracked() != CanvasTool::Hand {
            return;
        }
        // Always start pan drag (bookmark on click is handled in pointerup)
        state.interaction.is_dragging().set(true);
        hand_drag_start.set((
            ev.client_x() as f64,
            state.view.scroll_offset().get_untracked(),
        ));
        // Capture pointer so drag continues when cursor leaves the canvas
        if let Some(target) = ev.target() {
            if let Ok(el) = target.dyn_into::<web_sys::Element>() {
                let _ = el.set_pointer_capture(ev.pointer_id());
            }
        }
    };

    let on_pointermove = move |ev: web_sys::PointerEvent| {
        if !state.interaction.is_dragging().get_untracked() {
            return;
        }
        if state.interaction.canvas_tool().get_untracked() != CanvasTool::Hand {
            return;
        }
        let (start_client_x, start_scroll) = hand_drag_start.get_untracked();
        let dx = ev.client_x() as f64 - start_client_x;
        let cw = state.viewmode.spectrogram_canvas_width().get_untracked();
        if cw == 0.0 {
            return;
        }
        let files = state.library.files().get_untracked();
        let idx = state.library.current_index().get_untracked();
        let file = idx.and_then(|i| files.get(i));
        let waterfall_active = (state.mic.recording().get_untracked()
            || state.mic.listening().get_untracked())
            && crate::canvas::live_waterfall::is_active();
        let time_res = if waterfall_active {
            crate::canvas::live_waterfall::time_resolution()
        } else {
            file.as_ref()
                .map(|f| f.spectrogram.time_resolution)
                .unwrap_or(1.0)
        };
        let zoom = state.view.zoom_level().get_untracked();
        let visible_time = viewport::visible_time(cw, zoom, time_res);
        let dt = -(dx / cw) * visible_time;
        state.suspend_follow();
        state.suspend_waterfall_follow(2000.0);
        let new_scroll = if waterfall_active {
            let total_time = crate::canvas::live_waterfall::total_time();
            let oldest = crate::canvas::live_waterfall::oldest_time();
            let max_scroll = (total_time - visible_time).max(oldest);
            (start_scroll + dt).clamp(oldest, max_scroll)
        } else {
            let duration = file.as_ref().map(|f| f.audio.duration_secs).unwrap_or(0.0);
            let from_here_mode = state.playback.start_mode().get_untracked().uses_from_here();
            viewport::clamp_scroll_for_mode(
                start_scroll + dt,
                duration,
                visible_time,
                from_here_mode,
            )
        };
        state.view.scroll_offset().set(new_scroll);
    };

    let on_pointerup = move |ev: web_sys::PointerEvent| {
        if state.interaction.is_dragging().get_untracked()
            && state.interaction.canvas_tool().get_untracked() == CanvasTool::Hand
        {
            let (start_x, _) = hand_drag_start.get_untracked();
            let dx = (ev.client_x() as f64 - start_x).abs();
            if dx < 3.0 && state.playback.is_playing().get_untracked() {
                let t = state.playback.playhead_time().get_untracked();
                state
                    .viewmode
                    .bookmarks()
                    .update(|bm| bm.push(crate::state::Bookmark { time: t }));
            }
        }
        state.interaction.is_dragging().set(false);
    };

    let on_pointerleave = move |_ev: web_sys::PointerEvent| {
        // Pointer capture keeps drag events flowing even when the cursor
        // leaves the canvas, so there's nothing to reset here.
    };

    // ── Touch event handlers (mobile) ──────────────────────────────────────────
    let on_touchstart = move |ev: web_sys::TouchEvent| {
        if state.status.viewport_zoomed().get_untracked() {
            return;
        }

        // Cancel any ongoing inertia animation immediately
        crate::components::inertia::cancel_inertia(inertia_generation);
        velocity_tracker.update_value(|t| t.reset());

        let touches = ev.touches();
        let n = touches.length();

        // Two-finger: initialize pinch-to-zoom
        if n == 2 {
            ev.prevent_default();
            use crate::components::pinch::{two_finger_geometry, PinchState};
            if let Some((mid_x, dist)) = two_finger_geometry(&touches) {
                let files = state.library.files().get_untracked();
                let idx = state.library.current_index().get_untracked();
                let file = idx.and_then(|i| files.get(i));
                let time_res = file
                    .as_ref()
                    .map(|f| f.spectrogram.time_resolution)
                    .unwrap_or(1.0);
                let duration = file
                    .as_ref()
                    .map(|f| f.audio.duration_secs)
                    .unwrap_or(f64::MAX);
                pinch_state.set(Some(PinchState::horizontal(
                    dist,
                    state.view.zoom_level().get_untracked(),
                    state.view.scroll_offset().get_untracked(),
                    mid_x,
                    time_res,
                    duration,
                    state.playback.start_mode().get_untracked().uses_from_here(),
                )));
            }
            state.interaction.is_dragging().set(false);
            return;
        }

        if n != 1 {
            return;
        }
        pinch_state.set(None);

        let touch = touches.get(0).unwrap();
        if state.interaction.canvas_tool().get_untracked() != CanvasTool::Hand {
            return;
        }
        // Always start pan drag (bookmark on tap handled in touchend)
        ev.prevent_default();
        state.interaction.is_dragging().set(true);
        hand_drag_start.set((
            touch.client_x() as f64,
            state.view.scroll_offset().get_untracked(),
        ));
    };

    let on_touchmove = move |ev: web_sys::TouchEvent| {
        if state.status.viewport_zoomed().get_untracked() {
            return;
        }

        let touches = ev.touches();
        let n = touches.length();

        // Two-finger pinch/pan
        if n == 2 {
            if let Some(ps) = pinch_state.get_untracked() {
                ev.prevent_default();
                use crate::components::pinch::{apply_pinch, two_finger_geometry};
                if let Some((mid_x, dist)) = two_finger_geometry(&touches) {
                    let Some(canvas_el) = canvas_ref.get() else {
                        return;
                    };
                    let canvas: &web_sys::HtmlCanvasElement = canvas_el.as_ref();
                    let rect = canvas.get_bounding_client_rect();
                    let cw = canvas.width() as f64;
                    let (new_zoom, new_scroll) = apply_pinch(&ps, dist, mid_x, rect.left(), cw);
                    state.suspend_follow();
                    state.view.zoom_level().set(new_zoom);
                    state.view.scroll_offset().set(new_scroll);
                }
            }
            return;
        }

        if n != 1 {
            return;
        }
        let touch = touches.get(0).unwrap();
        if !state.interaction.is_dragging().get_untracked() {
            return;
        }
        if state.interaction.canvas_tool().get_untracked() != CanvasTool::Hand {
            return;
        }
        ev.prevent_default();
        let (start_client_x, start_scroll) = hand_drag_start.get_untracked();
        let dx = touch.client_x() as f64 - start_client_x;
        let cw = state.viewmode.spectrogram_canvas_width().get_untracked();
        if cw == 0.0 {
            return;
        }
        let files = state.library.files().get_untracked();
        let idx = state.library.current_index().get_untracked();
        let file = idx.and_then(|i| files.get(i));
        let waterfall_active = (state.mic.recording().get_untracked()
            || state.mic.listening().get_untracked())
            && crate::canvas::live_waterfall::is_active();
        let time_res = if waterfall_active {
            crate::canvas::live_waterfall::time_resolution()
        } else {
            file.as_ref()
                .map(|f| f.spectrogram.time_resolution)
                .unwrap_or(1.0)
        };
        let zoom = state.view.zoom_level().get_untracked();
        let visible_time = viewport::visible_time(cw, zoom, time_res);
        let dt = -(dx / cw) * visible_time;
        state.suspend_follow();
        state.suspend_waterfall_follow(2000.0);
        let new_scroll = if waterfall_active {
            let total_time = crate::canvas::live_waterfall::total_time();
            let oldest = crate::canvas::live_waterfall::oldest_time();
            let max_scroll = (total_time - visible_time).max(oldest);
            (start_scroll + dt).clamp(oldest, max_scroll)
        } else {
            let duration = file.as_ref().map(|f| f.audio.duration_secs).unwrap_or(0.0);
            let from_here_mode = state.playback.start_mode().get_untracked().uses_from_here();
            viewport::clamp_scroll_for_mode(
                start_scroll + dt,
                duration,
                visible_time,
                from_here_mode,
            )
        };
        state.view.scroll_offset().set(new_scroll);
        // Record velocity sample for inertia
        let now = web_sys::window().unwrap().performance().unwrap().now();
        velocity_tracker.update_value(|t| t.push(now, touch.client_x() as f64));
    };

    let on_touchend = move |_ev: web_sys::TouchEvent| {
        let remaining = _ev.touches().length();
        if remaining < 2 {
            pinch_state.set(None);
        }
        if remaining == 1 {
            if let Some(touch) = _ev.touches().get(0) {
                hand_drag_start.set((
                    touch.client_x() as f64,
                    state.view.scroll_offset().get_untracked(),
                ));
                if state.interaction.canvas_tool().get_untracked() == CanvasTool::Hand {
                    state.interaction.is_dragging().set(true);
                }
            }
            return;
        }
        if remaining == 0 {
            // Hand tool: bookmark on tap (no significant drag) while playing, or launch inertia
            if state.interaction.canvas_tool().get_untracked() == CanvasTool::Hand {
                if let Some(touch) = _ev.changed_touches().get(0) {
                    let (start_x, _) = hand_drag_start.get_untracked();
                    let dx = (touch.client_x() as f64 - start_x).abs();
                    if dx < 5.0 && state.playback.is_playing().get_untracked() {
                        let t = state.playback.playhead_time().get_untracked();
                        state
                            .viewmode
                            .bookmarks()
                            .update(|bm| bm.push(crate::state::Bookmark { time: t }));
                    } else if dx >= 5.0 {
                        // Flick → launch inertia
                        let vel = velocity_tracker.with_value(|t| t.velocity_px_per_sec());
                        let cw = state.viewmode.spectrogram_canvas_width().get_untracked();
                        let files = state.library.files().get_untracked();
                        let idx = state.library.current_index().get_untracked();
                        let file = idx.and_then(|i| files.get(i));
                        let waterfall_active = (state.mic.recording().get_untracked()
                            || state.mic.listening().get_untracked())
                            && crate::canvas::live_waterfall::is_active();
                        let time_res = if waterfall_active {
                            crate::canvas::live_waterfall::time_resolution()
                        } else {
                            file.as_ref()
                                .map(|f| f.spectrogram.time_resolution)
                                .unwrap_or(1.0)
                        };
                        let duration = if waterfall_active {
                            crate::canvas::live_waterfall::total_time()
                        } else {
                            file.as_ref()
                                .map(|f| f.audio.duration_secs)
                                .unwrap_or(f64::MAX)
                        };
                        let from_here_mode =
                            state.playback.start_mode().get_untracked().uses_from_here();
                        crate::components::inertia::start_inertia(
                            state,
                            vel,
                            cw,
                            time_res,
                            duration,
                            from_here_mode,
                            inertia_generation,
                        );
                    }
                }
            }
            state.interaction.is_dragging().set(false);
        }
    };

    view! {
        <div class="waveform-container"
            style=move || {
                let ta = if state.status.viewport_zoomed().get() { "pinch-zoom" } else { "none" };
                // Time-gutter hover or active drag → `cell` cursor, mirroring
                // the spectrogram axes.
                match state.interaction.canvas_tool().get() {
                    CanvasTool::Hand => if state.interaction.is_dragging().get() {
                        format!("cursor: grabbing; touch-action: {ta};")
                    } else {
                        format!("cursor: grab; touch-action: {ta};")
                    },
                    CanvasTool::Selection => format!("cursor: crosshair; touch-action: {ta};"),
                }
            }
        >
            <div class="chart-row">
            <BandGutter/>
            <div class="chart-stage">
                <canvas
                    node_ref=canvas_ref
                    style:pointer-events=move || if state.status.viewport_zoomed().get() { "none" } else { "auto" }
                    on:wheel=on_wheel
                    on:pointerdown=on_pointerdown
                    on:pointermove=on_pointermove
                    on:pointerup=on_pointerup
                    on:pointerleave=on_pointerleave
                    on:touchstart=on_touchstart
                    on:touchmove=on_touchmove
                    on:touchend=on_touchend
                />
                <Playhead/>
            </div>
            </div>
            <div class="view-bottom-row">
                <div class="view-bottom-corner"></div>
                <TimeGutter/>
            </div>
        </div>
    }
}
