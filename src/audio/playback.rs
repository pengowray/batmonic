use leptos::prelude::*;
use crate::types::AudioData;
use crate::state::{AppState, Selection, PlaybackMode};
use crate::audio::streaming_playback::{self, PlaybackParams};
use std::cell::RefCell;

thread_local! {
    static PLAYHEAD_HANDLE: RefCell<Option<i32>> = RefCell::new(None);
    static REPLAY_TIMER: RefCell<Option<i32>> = RefCell::new(None);
}

fn cancel_replay_timer() {
    REPLAY_TIMER.with(|t| {
        if let Some(handle) = t.borrow_mut().take() {
            let _ = web_sys::window().unwrap().clear_timeout_with_handle(handle);
        }
    });
}

pub fn stop(state: &AppState) {
    cancel_replay_timer();
    cancel_playhead();
    streaming_playback::stop_stream();
    // Restore scroll to pre-play position when stopping
    state.scroll_offset.set(state.pre_play_scroll.get_untracked());
    state.is_playing.set(false);
}

/// Continue playback from the current playhead position with fresh parameters.
/// Used for live parameter switching — mode, gain, filter, etc.
pub fn replay_live(state: &AppState) {
    if !state.is_playing.get_untracked() { return; }

    let current_time = state.playhead_time.get_untracked();
    cancel_playhead();
    streaming_playback::stop_stream();

    let files = state.files.get_untracked();
    let idx = state.current_file_index.get_untracked();
    let Some(file) = idx.and_then(|i| files.get(i)) else { return };

    let selection = state.selection.get_untracked();
    let sr = file.audio.sample_rate;
    let total = file.audio.source.total_samples() as usize;
    let sel_end = selection.map(|s| s.time_end).unwrap_or(file.audio.duration_secs);
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
        file.audio.source.clone(),
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

    start_playhead(state.clone(), current_time, remaining_duration, playback_speed);
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
    state.selection.set(None);
    state.scroll_offset.set(0.0);
    play(state);
    state.pre_play_scroll.set(pre);
}

/// Play from the current "here" time (play_from_here_time signal).
pub fn play_from_here(state: &AppState) {
    let pre = state.scroll_offset.get_untracked();
    let start_secs = state.play_from_here_time.get_untracked();
    stop(state);
    state.pre_play_scroll.set(pre);
    play_from_time(state, start_secs);
    state.pre_play_scroll.set(pre);
}

/// Play from a specific time offset in the current file.
pub fn play_from_time(state: &AppState, start_secs: f64) {
    let files = state.files.get_untracked();
    let idx = state.current_file_index.get_untracked();
    let Some(file) = idx.and_then(|i| files.get(i)) else { return };

    let sr = file.audio.sample_rate;
    let total = file.audio.source.total_samples() as usize;
    let selection = state.selection.get_untracked();
    let end_secs = selection.map(|s| s.time_end).unwrap_or(file.audio.duration_secs);
    let start_secs = start_secs.max(0.0).min(end_secs);
    let start_sample = (start_secs * sr as f64) as usize;
    let end_sample = ((end_secs * sr as f64) as usize).min(total);
    if end_sample <= start_sample { return; }

    let params = snapshot_params(state, selection, sr);
    let channel_view = state.channel_view.get_untracked();

    let Some(_) = streaming_playback::start_stream(
        file.audio.source.clone(),
        channel_view,
        sr,
        start_sample,
        end_sample,
        params,
    ) else { return };

    let play_duration = (end_sample - start_sample) as f64 / sr as f64;
    let te_factor = state.te_factor.get_untracked();
    let playback_speed = match state.playback_mode.get_untracked() {
        PlaybackMode::TimeExpansion => {
            let abs_f = te_factor.abs().max(1.0);
            if te_factor > 0.0 { 1.0 / abs_f } else { abs_f }
        }
        _ => 1.0,
    };

    state.is_playing.set(true);
    state.playhead_time.set(start_secs);
    start_playhead(state.clone(), start_secs, play_duration, playback_speed);
}

pub fn play(state: &AppState) {
    stop(state);

    let files = state.files.get_untracked();
    let idx = state.current_file_index.get_untracked();
    let Some(file) = idx.and_then(|i| files.get(i)) else { return };

    let selection = state.selection.get_untracked();
    let sr = file.audio.sample_rate;

    let (start_sample, end_sample) = extract_selection_range(&file.audio, selection);
    if end_sample <= start_sample { return; }

    let params = snapshot_params(state, selection, sr);
    let play_start_time = selection.map(|s| s.time_start).unwrap_or(0.0);
    let play_duration = (end_sample - start_sample) as f64 / sr as f64;
    let channel_view = state.channel_view.get_untracked();

    let Some(_) = streaming_playback::start_stream(
        file.audio.source.clone(),
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

    state.pre_play_scroll.set(state.scroll_offset.get_untracked());
    state.is_playing.set(true);
    state.playhead_time.set(play_start_time);
    start_playhead(state.clone(), play_start_time, play_duration, playback_speed);
}

/// Returns (start_sample, end_sample) for the current selection or full file.
fn extract_selection_range(audio: &AudioData, selection: Option<Selection>) -> (usize, usize) {
    let sr = audio.sample_rate;
    let total = audio.source.total_samples() as usize;
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
fn snapshot_params(state: &AppState, selection: Option<Selection>, sample_rate: u32) -> PlaybackParams {
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

    let cb: Rc<RefCell<Option<wasm_bindgen::closure::Closure<dyn FnMut()>>>> =
        Rc::new(RefCell::new(None));
    let cb_clone = cb.clone();

    *cb.borrow_mut() = Some(wasm_bindgen::closure::Closure::new(move || {
        if !state.is_playing.get_untracked() {
            return;
        }
        let window = web_sys::window().unwrap();
        let perf = window.performance().unwrap();
        let elapsed_ms = perf.now() - anim_start;
        let elapsed_real = elapsed_ms / 1000.0;
        let current = start_time + elapsed_real * speed;

        if current >= end_time {
            state.playhead_time.set(end_time);
            state.scroll_offset.set(state.pre_play_scroll.get_untracked());
            state.is_playing.set(false);
            // Show bookmark popup briefly if any bookmarks were made during playback
            if !state.bookmarks.get_untracked().is_empty() {
                state.show_bookmark_popup.set(true);
                let state_bm = state.clone();
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
