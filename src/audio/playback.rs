use leptos::prelude::*;
use crate::annotations::AnnotationKind;
use crate::state::{ActiveFocus, AppState, GainMode, Selection, PlaybackMode};
use crate::audio::streaming_playback::{self, PlaybackParams};
use crate::audio::source::{AudioSource, TimelineAudioSource};
use crate::viewport;
use std::cell::RefCell;
use std::sync::Arc;

thread_local! {
    static PLAYHEAD_HANDLE: RefCell<Option<i32>> = const { RefCell::new(None) };
    static REPLAY_TIMER: RefCell<Option<i32>> = const { RefCell::new(None) };
}

struct PlaybackTarget {
    source: Arc<dyn AudioSource>,
    sample_rate: u32,
    duration_secs: f64,
}

fn timeline_selection(state: &AppState) -> Option<Selection> {
    state.selection.get_untracked()
}

fn playback_target(state: &AppState) -> Option<PlaybackTarget> {
    let files = state.files.get_untracked();

    if let Some(timeline) = state.active_timeline.get_untracked() {
        let mut sample_rate = None;
        let mut segments = Vec::with_capacity(timeline.segments.len());

        for seg in &timeline.segments {
            let file = files.get(seg.file_index)?;
            let sr = file.audio.sample_rate;
            if let Some(expected_sr) = sample_rate {
                if expected_sr != sr {
                    state.show_error_toast("Timeline playback requires matching sample rates");
                    return None;
                }
            } else {
                sample_rate = Some(sr);
            }
            segments.push((
                file.audio.source.clone(),
                seg.timeline_offset_secs,
                seg.duration_secs,
            ));
        }

        let sr = sample_rate?;
        let source: Arc<dyn AudioSource> = Arc::new(TimelineAudioSource::new(segments, sr));
        return Some(PlaybackTarget {
            source,
            sample_rate: sr,
            duration_secs: timeline.total_duration_secs,
        });
    }

    let idx = state.current_file_index.get_untracked()?;
    let file = files.get(idx)?;
    Some(PlaybackTarget {
        source: file.audio.source.clone(),
        sample_rate: file.audio.sample_rate,
        duration_secs: file.audio.duration_secs,
    })
}

/// Resolve the effective selection for playback, checking the focused
/// selection (transient drag or annotations) based on `active_focus`.
pub fn effective_selection(state: &AppState) -> Option<Selection> {
    let focus = state.active_focus.get_untracked();

    // 1. Transient selection — only when it has focus
    if focus == Some(ActiveFocus::TransientSelection) {
        if let Some(sel) = state.selection.get_untracked() {
            return Some(sel);
        }
    }

    if state.active_timeline.get_untracked().is_some() {
        return None;
    }

    // 2. Selected annotations — only when annotations have focus
    if focus != Some(ActiveFocus::Annotations) {
        return None;
    }
    let ids = state.selected_annotation_ids.get_untracked();
    if ids.is_empty() {
        return None;
    }
    let idx = state.current_file_index.get_untracked()?;
    let store = state.annotation_store.get_untracked();
    let set = store.sets.get(idx)?.as_ref()?;

    let mut time_start = f64::MAX;
    let mut time_end = f64::MIN;
    let mut all_have_freq = true;
    let mut freq_lo = f64::MAX;
    let mut freq_hi = f64::MIN;
    let mut count = 0usize;

    for ann in &set.annotations {
        if !ids.contains(&ann.id) {
            continue;
        }
        if let AnnotationKind::Region(ref r) = ann.kind {
            time_start = time_start.min(r.time_start);
            time_end = time_end.max(r.time_end);
            match (r.freq_low, r.freq_high) {
                (Some(lo), Some(hi)) => {
                    freq_lo = freq_lo.min(lo);
                    freq_hi = freq_hi.max(hi);
                }
                _ => {
                    all_have_freq = false;
                }
            }
            count += 1;
        }
    }

    if count == 0 || time_end <= time_start {
        return None;
    }

    Some(Selection {
        time_start,
        time_end,
        freq_low: if all_have_freq { Some(freq_lo) } else { None },
        freq_high: if all_have_freq { Some(freq_hi) } else { None },
    })
}

fn cancel_replay_timer() {
    REPLAY_TIMER.with(|t| {
        if let Some(handle) = t.borrow_mut().take() {
            web_sys::window().unwrap().clear_timeout_with_handle(handle);
        }
    });
}

/// Check whether the current playhead position is within the visible viewport.
fn is_playhead_visible(state: &AppState) -> bool {
    let playhead = state.playhead_time.get_untracked();
    let scroll = state.scroll_offset.get_untracked();
    let zoom = state.zoom_level.get_untracked();
    let canvas_w = state.spectrogram_canvas_width.get_untracked();
    let time_res = state.current_file_index.get_untracked()
        .and_then(|i| state.files.get_untracked().get(i).cloned())
        .map(|f| f.spectrogram.time_resolution)
        .unwrap_or(1.0);
    let visible = viewport::visible_time(canvas_w, zoom, time_res);
    let rel = playhead - scroll;
    rel >= 0.0 && rel <= visible
}

/// Check whether any part of a selection overlaps the visible viewport.
pub fn is_selection_in_viewport(state: &AppState, sel: &Selection) -> bool {
    let scroll = state.scroll_offset.get_untracked();
    let zoom = state.zoom_level.get_untracked();
    let canvas_w = state.spectrogram_canvas_width.get_untracked();
    let time_res = state.current_file_index.get_untracked()
        .and_then(|i| state.files.get_untracked().get(i).cloned())
        .map(|f| f.spectrogram.time_resolution)
        .unwrap_or(1.0);
    let visible = viewport::visible_time(canvas_w, zoom, time_res);
    if visible <= 0.0 {
        return true; // can't determine viewport, assume visible
    }
    let vp_end = scroll + visible;
    sel.time_start < vp_end && sel.time_end > scroll
}

pub fn stop(state: &AppState) {
    let was_playing = state.is_playing.get_untracked();
    cancel_replay_timer();
    cancel_playhead();
    streaming_playback::stop_stream();
    state.is_buffering.set(false);
    if was_playing {
        if state.user_panned_during_playback.get_untracked()
            && !is_playhead_visible(state)
        {
            // User scrolled the playhead off-screen — don't snap back.
            // The pre-play position was pushed to nav history when playback
            // started, so the back button can still return there.
        } else {
            // Playhead is still on-screen (or user didn't pan) — snap back.
            state.scroll_offset.set(state.pre_play_scroll.get_untracked());
        }
    }
    state.is_playing.set(false);
    state.active_playback_selection.set(None);
}

/// Continue playback from the current playhead position with fresh parameters.
/// Used for live parameter switching — mode, gain, filter, etc.
pub fn replay_live(state: &AppState) {
    if !state.is_playing.get_untracked() { return; }

    let current_time = state.playhead_time.get_untracked();
    cancel_playhead();
    streaming_playback::stop_stream();

    let Some(target) = playback_target(state) else { return; };

    let selection = state.active_playback_selection.get_untracked();
    let sr = target.sample_rate;
    let total = target.source.total_samples() as usize;
    let sel_end = selection.map(|s| s.time_end).unwrap_or(target.duration_secs);
    let start_sample = ((current_time * sr as f64) as usize).min(total);
    let end_sample = ((sel_end * sr as f64) as usize).min(total);

    if end_sample <= start_sample {
        state.is_playing.set(false);
        return;
    }

    let params = snapshot_params(state, selection, sr);
    let remaining_duration = (end_sample - start_sample) as f64 / sr as f64;
    let channel_view = state.channel_view.get_untracked();

    let Some(_) = streaming_playback::start_stream(
        target.source,
        channel_view,
        sr,
        start_sample,
        end_sample,
        params,
    ) else {
        state.is_playing.set(false);
        return;
    };

    // Compute correct playback speed for the current mode
    let te_factor = state.te_factor.get_untracked();
    let playback_speed = match state.playback_mode.get_untracked() {
        PlaybackMode::TimeExpansion => {
            let abs_f = te_factor.abs().max(1.0);
            if te_factor > 0.0 { 1.0 / abs_f } else { abs_f }
        }
        _ => 1.0,
    };

    start_playhead(*state, current_time, remaining_duration, playback_speed);
}

/// Debounced version of `replay_live()`. Coalesces rapid signal changes
/// (e.g., HFR toggle setting multiple signals, or slider dragging) into
/// a single restart ~80ms after the last change.
pub fn schedule_replay_live(state: &AppState) {
    use wasm_bindgen::prelude::*;
    let state = *state;
    cancel_replay_timer();
    let cb = wasm_bindgen::closure::Closure::once(move || {
        replay_live(&state);
    });
    let handle = web_sys::window()
        .unwrap()
        .set_timeout_with_callback_and_timeout_and_arguments_0(
            cb.as_ref().unchecked_ref(),
            80,
        )
        .unwrap_or(0);
    cb.forget();
    REPLAY_TIMER.with(|t| {
        *t.borrow_mut() = Some(handle);
    });
}

/// Play from the very start of the current file (ignores selection).
pub fn play_from_start(state: &AppState) {
    let pre = state.scroll_offset.get_untracked();
    stop(state);
    state.pre_play_scroll.set(pre);
    // Use play_from_time_inner directly to avoid double-stop (play() calls stop() again,
    // which would restore scroll_offset to pre_play_scroll, undoing our scroll_offset=0).
    play_from_time_inner(state, 0.0, None);
}

/// Play from the current "here" time (play_from_here_time signal).
pub fn play_from_here(state: &AppState) {
    let pre = state.scroll_offset.get_untracked();
    let start_secs = current_play_from_here_time(state);
    stop(state);
    state.pre_play_scroll.set(pre);
    // Ignore selection for end time — "play from here" should always play to end of file.
    // If the user has a selection and the "here" marker is past the selection end,
    // this previously caused silent failure.
    play_from_time_inner(state, start_secs, None);
}

fn current_play_from_here_time(state: &AppState) -> f64 {
    let Some(target) = playback_target(state) else {
        return state.play_from_here_time.get_untracked();
    };

    let canvas_width = state.spectrogram_canvas_width.get_untracked();
    let zoom = state.zoom_level.get_untracked();
    let scroll = state.scroll_offset.get_untracked();
    let time_res = if let Some(ref tl) = state.active_timeline.get_untracked() {
        let files = state.files.get_untracked();
        tl.segments.first().and_then(|s| files.get(s.file_index))
            .map(|f| f.spectrogram.time_resolution)
            .unwrap_or(1.0)
    } else {
        let files = state.files.get_untracked();
        let idx = state.current_file_index.get_untracked();
        idx.and_then(|i| files.get(i))
            .map(|f| f.spectrogram.time_resolution)
            .unwrap_or(1.0)
    };
    let visible_time = viewport::visible_time(canvas_width, zoom, time_res);

    if visible_time <= 0.0 {
        state.play_from_here_time.get_untracked()
    } else {
        viewport::play_from_here_time(scroll, visible_time).clamp(0.0, target.duration_secs)
    }
}

/// Play from a specific time offset in the current file.
/// Uses the current selection (if any) for end time.
pub fn play_from_time(state: &AppState, start_secs: f64) {
    let selection = state.selection.get_untracked();
    play_from_time_inner(state, start_secs, selection);
}

/// Inner implementation: play from `start_secs` to `sel_end` (or end of file).
fn play_from_time_inner(state: &AppState, start_secs: f64, selection: Option<Selection>) {
    let Some(target) = playback_target(state) else { return; };

    let sr = target.sample_rate;
    let total = target.source.total_samples() as usize;
    let end_secs = selection.map(|s| s.time_end).unwrap_or(target.duration_secs);
    let start_secs = start_secs.max(0.0).min(end_secs);
    let start_sample = (start_secs * sr as f64) as usize;
    let end_sample = ((end_secs * sr as f64) as usize).min(total);
    if end_sample <= start_sample {
        web_sys::console::warn_1(&format!(
            "Playback: nothing to play (start={start_secs:.3}s, end={end_secs:.3}s, total={total})"
        ).into());
        return;
    }

    let params = snapshot_params(state, selection, sr);
    let channel_view = state.channel_view.get_untracked();

    let Some(_) = streaming_playback::start_stream(
        target.source,
        channel_view,
        sr,
        start_sample,
        end_sample,
        params,
    ) else {
        web_sys::console::warn_1(&"Playback failed: could not start audio stream".into());
        return;
    };

    let play_duration = (end_sample - start_sample) as f64 / sr as f64;
    let te_factor = state.te_factor.get_untracked();
    let playback_speed = match state.playback_mode.get_untracked() {
        PlaybackMode::TimeExpansion => {
            let abs_f = te_factor.abs().max(1.0);
            if te_factor > 0.0 { 1.0 / abs_f } else { abs_f }
        }
        _ => 1.0,
    };

    state.active_playback_selection.set(selection);
    state.is_playing.set(true);
    state.playhead_time.set(start_secs);
    start_playhead(*state, start_secs, play_duration, playback_speed);
}

pub fn play(state: &AppState) {
    stop(state);

    let Some(target) = playback_target(state) else { return; };

    let selection = if state.active_timeline.get_untracked().is_some() {
        timeline_selection(state)
    } else {
        let sel = effective_selection(state);
        // If the selection is entirely off-screen, ignore it and play from "here"
        if let Some(ref s) = sel {
            if !is_selection_in_viewport(state, s) {
                play_from_here(state);
                return;
            }
        }
        sel
    };
    let sr = target.sample_rate;

    let (start_sample, end_sample) = extract_selection_range(sr, target.source.total_samples() as usize, selection);
    if end_sample <= start_sample { return; }

    let params = snapshot_params(state, selection, sr);
    let play_start_time = selection.map(|s| s.time_start).unwrap_or(0.0);
    let play_duration = (end_sample - start_sample) as f64 / sr as f64;
    let channel_view = state.channel_view.get_untracked();

    let Some(_) = streaming_playback::start_stream(
        target.source,
        channel_view,
        sr,
        start_sample,
        end_sample,
        params,
    ) else { return };

    let te_factor = state.te_factor.get_untracked();
    let playback_speed = match state.playback_mode.get_untracked() {
        PlaybackMode::TimeExpansion => {
            let abs_f = te_factor.abs().max(1.0);
            if te_factor > 0.0 { 1.0 / abs_f } else { abs_f }
        }
        _ => 1.0,
    };

    state.push_nav(); // save pre-play position so the back button can return here
    state.pre_play_scroll.set(state.scroll_offset.get_untracked());
    state.user_panned_during_playback.set(false);
    state.active_playback_selection.set(selection);
    state.is_playing.set(true);
    state.playhead_time.set(play_start_time);
    start_playhead(*state, play_start_time, play_duration, playback_speed);
}

/// Returns (start_sample, end_sample) for the current selection or full file.
fn extract_selection_range(sample_rate: u32, total: usize, selection: Option<Selection>) -> (usize, usize) {
    let sr = sample_rate;
    if let Some(sel) = selection {
        let start = ((sel.time_start * sr as f64) as usize).min(total);
        let end = ((sel.time_end * sr as f64) as usize).min(total);
        if end > start {
            return (start, end);
        }
    }
    (0, total)
}

/// Build a PlaybackParams snapshot from current AppState.
pub(crate) fn snapshot_params(state: &AppState, selection: Option<Selection>, sample_rate: u32) -> PlaybackParams {
    PlaybackParams {
        mode: state.playback_mode.get_untracked(),
        het_freq: state.het_frequency.get_untracked(),
        het_cutoff: state.het_cutoff.get_untracked(),
        te_factor: state.te_factor.get_untracked(),
        ps_factor: state.ps_factor.get_untracked(),
        pv_factor: state.pv_factor.get_untracked(),
        pv_hq: state.pv_hq.get_untracked(),
        zc_factor: state.zc_factor.get_untracked(),
        gain_db: state.gain_db.get_untracked(),
        gain_mode: state.gain_mode.get_untracked(),
        auto_peak_gain_db: if state.gain_mode.get_untracked() == GainMode::AutoPeak {
            state.compute_auto_gain()
        } else {
            0.0
        },
        filter_enabled: state.filter_enabled.get_untracked(),
        filter_freq_low: state.filter_freq_low.get_untracked(),
        filter_freq_high: state.filter_freq_high.get_untracked(),
        filter_db_below: state.filter_db_below.get_untracked(),
        filter_db_selected: state.filter_db_selected.get_untracked(),
        filter_db_harmonics: state.filter_db_harmonics.get_untracked(),
        filter_db_above: state.filter_db_above.get_untracked(),
        filter_band_mode: state.filter_band_mode.get_untracked(),
        filter_quality: state.filter_quality.get_untracked(),
        sel_freq_low: selection.and_then(|s| s.freq_low).unwrap_or(0.0),
        sel_freq_high: selection
            .and_then(|s| s.freq_high)
            .unwrap_or(sample_rate as f64 / 2.0),
        has_selection: selection.is_some(),
        notch_enabled: state.notch_enabled.get_untracked(),
        notch_bands: state.notch_bands.get_untracked(),
        notch_harmonic_suppression: state.notch_harmonic_suppression.get_untracked(),
        noise_reduce_enabled: state.noise_reduce_enabled.get_untracked(),
        noise_reduce_strength: state.noise_reduce_strength.get_untracked(),
        noise_reduce_floor: state.noise_reduce_floor.get_untracked(),
    }
}

pub(crate) fn apply_bandpass(samples: &[f32], sample_rate: u32, freq_low: f64, freq_high: f64) -> Vec<f32> {
    let mut result = samples.to_vec();
    if freq_low > 0.0 {
        let lp = cascaded_lowpass(&result, freq_low, sample_rate, 4);
        for (r, l) in result.iter_mut().zip(lp.iter()) {
            *r -= l;
        }
    }
    if freq_high < (sample_rate as f64 / 2.0) {
        result = cascaded_lowpass(&result, freq_high, sample_rate, 4);
    }
    result
}

fn lowpass_filter_inplace(buf: &mut [f32], cutoff_hz: f64, sample_rate: u32) {
    if buf.is_empty() {
        return;
    }
    let dt = 1.0 / sample_rate as f64;
    let rc = 1.0 / (2.0 * std::f64::consts::PI * cutoff_hz);
    let alpha = (dt / (rc + dt)) as f32;
    let mut prev = buf[0];
    for s in buf[1..].iter_mut() {
        let y = alpha * *s + (1.0 - alpha) * prev;
        *s = y;
        prev = y;
    }
}

fn cascaded_lowpass(samples: &[f32], cutoff: f64, sample_rate: u32, passes: usize) -> Vec<f32> {
    let mut result = samples.to_vec();
    for _ in 0..passes {
        lowpass_filter_inplace(&mut result, cutoff, sample_rate);
    }
    result
}

pub(crate) fn apply_gain(samples: &mut [f32], gain_db: f64) {
    if gain_db.abs() < 0.001 {
        return;
    }
    let gain_linear = 10.0_f64.powf(gain_db / 20.0) as f32;
    for s in samples.iter_mut() {
        *s *= gain_linear;
    }
}

/// Animate the playhead using requestAnimationFrame
fn start_playhead(state: AppState, start_time: f64, duration: f64, speed: f64) {
    let window = web_sys::window().unwrap();
    let perf = window.performance().unwrap();
    let anim_start = perf.now();
    let end_time = start_time + duration;

    use std::rc::Rc;
    use wasm_bindgen::prelude::*;
    use crate::audio::streaming_playback::audio_buffer_ahead_secs;

    let cb: Rc<RefCell<Option<wasm_bindgen::closure::Closure<dyn FnMut()>>>> =
        Rc::new(RefCell::new(None));
    let cb_clone = cb.clone();

    // Mutable across animation ticks: total wall-clock time spent frozen
    // while the audio scheduler was draining. Subtracted from elapsed so the
    // playhead tracks actual audio position, not wall-clock time.
    let buffering_total = Rc::new(RefCell::new(0.0_f64));
    let buffering_started_at: Rc<RefCell<Option<f64>>> = Rc::new(RefCell::new(None));

    // Underrun hysteresis: enter buffering when the scheduled-ahead budget
    // drops near empty, exit once it's comfortably refilled. Tuned against
    // LOOKAHEAD_SECS (1.5s) in streaming_playback.
    const BUFFER_UNDERRUN_THRESHOLD: f64 = 0.05; // seconds of audio remaining
    const BUFFER_RECOVER_THRESHOLD: f64 = 0.35;  // need this much ahead to resume

    *cb.borrow_mut() = Some(wasm_bindgen::closure::Closure::new(move || {
        if !state.is_playing.get_untracked() {
            state.is_buffering.set(false);
            return;
        }
        let window = web_sys::window().unwrap();
        let perf = window.performance().unwrap();
        let now_ms = perf.now();

        // Check streaming buffer health (None for non-streaming sources →
        // treat as always healthy).
        let ahead = audio_buffer_ahead_secs();
        let is_buffering = match ahead {
            Some(a) => {
                let currently = state.is_buffering.get_untracked();
                if currently {
                    a < BUFFER_RECOVER_THRESHOLD
                } else {
                    a < BUFFER_UNDERRUN_THRESHOLD
                }
            }
            None => false,
        };

        // Manage the buffering signal + accumulated-freeze timer.
        let was_buffering = state.is_buffering.get_untracked();
        if is_buffering && !was_buffering {
            *buffering_started_at.borrow_mut() = Some(now_ms);
            state.is_buffering.set(true);
        } else if !is_buffering && was_buffering {
            if let Some(started) = buffering_started_at.borrow_mut().take() {
                *buffering_total.borrow_mut() += now_ms - started;
            }
            state.is_buffering.set(false);
        }

        // Effective elapsed time excludes any wall-clock spent frozen.
        let freeze_so_far = *buffering_total.borrow()
            + buffering_started_at.borrow().map(|s| now_ms - s).unwrap_or(0.0);
        let elapsed_ms = now_ms - anim_start - freeze_so_far;
        let elapsed_real = elapsed_ms.max(0.0) / 1000.0;
        let current = start_time + elapsed_real * speed;

        if current >= end_time {
            state.playhead_time.set(end_time);
            state.is_buffering.set(false);
            if !(state.user_panned_during_playback.get_untracked()
                && !is_playhead_visible(&state))
            {
                state.scroll_offset.set(state.pre_play_scroll.get_untracked());
            }
            state.is_playing.set(false);
            // Show bookmark popup briefly if any bookmarks were made during playback
            if !state.bookmarks.get_untracked().is_empty() {
                state.show_bookmark_popup.set(true);
                let state_bm = state;
                let cb = wasm_bindgen::closure::Closure::once(move || {
                    state_bm.show_bookmark_popup.set(false);
                });
                let _ = web_sys::window()
                    .unwrap()
                    .set_timeout_with_callback_and_timeout_and_arguments_0(
                        cb.as_ref().unchecked_ref(),
                        6000,
                    );
                cb.forget();
            }
            return;
        }

        state.playhead_time.set(current);

        let handle = window
            .request_animation_frame(
                cb_clone.borrow().as_ref().unwrap().as_ref().unchecked_ref(),
            )
            .unwrap();
        PLAYHEAD_HANDLE.with(|h| {
            *h.borrow_mut() = Some(handle);
        });
    }));

    let handle = window
        .request_animation_frame(cb.borrow().as_ref().unwrap().as_ref().unchecked_ref())
        .unwrap();
    PLAYHEAD_HANDLE.with(|h| {
        *h.borrow_mut() = Some(handle);
    });
}

fn cancel_playhead() {
    PLAYHEAD_HANDLE.with(|h| {
        if let Some(handle) = h.borrow_mut().take() {
            let _ = web_sys::window().unwrap().cancel_animation_frame(handle);
        }
    });
}
