//! Streaming playback engine.
//!
//! Instead of processing the entire selection through DSP before any audio
//! plays, this module processes and schedules audio in small chunks (~0.5s).
//! The user hears sound almost immediately while subsequent chunks are
//! processed in the background.

use web_sys::{AudioContext, AudioContextOptions};
use wasm_bindgen_futures::JsFuture;
use std::cell::RefCell;
use std::sync::Arc;

use crate::audio::source::{AudioSource, ChannelView};
use crate::audio::streaming_source;
use crate::state::{PlaybackMode, FilterQuality, GainMode};
use crate::dsp::agc::{AgcConfig, AgcProcessor};
use crate::dsp::heterodyne::heterodyne_mix;
use crate::dsp::pitch_shift::pitch_shift_realtime;
use crate::dsp::zc_divide::zc_divide;
use crate::dsp::filters::{apply_eq_filter, apply_eq_filter_fast};
use crate::audio::playback::{apply_bandpass, apply_gain};

/// Number of source samples per chunk. ~0.5s at 192kHz, ~2s at 44.1kHz.
const CHUNK_SAMPLES: usize = 96_000;

/// Extra overlap samples prepended to each chunk for IIR filter warmup.
/// This lets filters (heterodyne lowpass, bandpass) settle before the
/// actual chunk data, avoiding clicks at boundaries.
const FILTER_WARMUP: usize = 4096;

/// Overlap samples for PV HQ mode. Each chunk extends by this amount past
/// its nominal end. The trailing overlap gets a Hann fade-out, while the
/// next chunk's leading overlap (same size) gets a Hann fade-in. Web Audio
/// sums them → smooth crossfade, eliminating boundary clicks without
/// needing the warmup-trim hack.
pub(crate) const PV_HQ_OVERLAP: usize = 8192;

/// Compensatory gain boost for PhaseVocoder mode (dB).
/// PV's STFT bin-shifting inherently loses ~6-12 dB compared to PitchShift.
pub(crate) const PV_MODE_BOOST_DB: f64 = 6.0;

/// How far ahead (in seconds) to stay buffered beyond current playback time.
const LOOKAHEAD_SECS: f64 = 1.5;

/// Number of chunks to pre-buffer before starting playback.
/// This prevents initial skips/gaps while the first chunks are being processed.
const PREBUFFER_CHUNKS: usize = 5; // 3 might be enough

thread_local! {
    static STREAM_CTX: RefCell<Option<AudioContext>> = const { RefCell::new(None) };
    /// Master gain node for fade-out on stop (avoids clicks).
    static STREAM_GAIN: RefCell<Option<web_sys::GainNode>> = const { RefCell::new(None) };
    /// Monotonically increasing generation counter to detect stale streams.
    static STREAM_GEN: RefCell<u32> = const { RefCell::new(0) };
    /// Web Audio time at which the most recently scheduled buffer ends.
    /// Compared against the audio context's current_time() from the playhead
    /// animation to detect buffer underruns.
    static SCHEDULED_END: std::cell::Cell<f64> = const { std::cell::Cell::new(0.0) };
}

/// How many seconds of audio are scheduled beyond the audio context's
/// current_time. Returns None if no stream is active. Negative means the
/// scheduler has fallen behind and audio is running out (buffer underrun).
pub fn audio_buffer_ahead_secs() -> Option<f64> {
    STREAM_CTX.with(|ctx| {
        let borrow = ctx.borrow();
        let audio_ctx = borrow.as_ref()?;
        let sched = SCHEDULED_END.with(|s| s.get());
        if sched <= 0.0 { return None; }
        Some(sched - audio_ctx.current_time())
    })
}

/// Snapshot of all playback parameters, frozen at play start so that
/// parameter changes mid-playback don't cause glitches.
pub(crate) struct PlaybackParams {
    pub mode: PlaybackMode,
    pub het_freq: f64,
    pub het_cutoff: f64,
    pub te_factor: f64,
    pub ps_factor: f64,
    pub pv_factor: f64,
    pub pv_hq: bool,
    pub zc_factor: f64,
    pub gain_db: f64,
    pub gain_mode: GainMode,
    pub auto_peak_gain_db: f64,
    pub filter_enabled: bool,
    pub filter_freq_low: f64,
    pub filter_freq_high: f64,
    pub filter_db_below: f64,
    pub filter_db_selected: f64,
    pub filter_db_harmonics: f64,
    pub filter_db_above: f64,
    pub filter_band_mode: u8,
    pub filter_quality: FilterQuality,
    pub sel_freq_low: f64,
    pub sel_freq_high: f64,
    pub has_selection: bool,
    pub notch_enabled: bool,
    pub notch_bands: Vec<crate::dsp::notch::NoiseBand>,
    pub notch_harmonic_suppression: f64,
    pub noise_reduce_enabled: bool,
    pub noise_reduce_strength: f64,
    pub noise_reduce_floor: Option<crate::dsp::spectral_sub::NoiseFloor>,
}

fn selection_bandpass_active(sample_rate: u32, params: &PlaybackParams) -> bool {
    params.has_selection
        && matches!(
            params.mode,
            PlaybackMode::Normal
                | PlaybackMode::TimeExpansion
                | PlaybackMode::PitchShift
                | PlaybackMode::PhaseVocoder
                | PlaybackMode::ZeroCrossing
        )
        && (params.sel_freq_low > 0.0
            || params.sel_freq_high < (sample_rate as f64 / 2.0))
}

/// Duration of fade-in when starting playback (milliseconds).
const FADE_IN_MS: f64 = 30.0;

/// Stop any active streaming playback with a short fade-out to avoid clicks.
///
/// The AudioContext is kept alive for reuse — only the gain node is disconnected.
/// This avoids exhausting the browser's AudioContext limit (~6 in Chrome) when
/// the user presses play repeatedly.
pub(crate) fn stop_stream() {
    STREAM_GEN.with(|g| {
        let mut generation = g.borrow_mut();
        *generation = generation.wrapping_add(1);
    });
    SCHEDULED_END.with(|s| s.set(0.0));

    // Take the gain node but keep the context alive for reuse.
    let gain = STREAM_GAIN.with(|g| g.borrow_mut().take());
    let ctx = STREAM_CTX.with(|c| c.borrow().clone());

    if let (Some(gain_node), Some(ref old_ctx)) = (gain, ctx) {
        let now = old_ctx.current_time();
        let param = gain_node.gain();
        // Mute immediately to avoid overlap with new stream
        let _ = param.cancel_scheduled_values(now);
        let _ = param.set_value_at_time(0.0, now);
        // Disconnect gain node to release all scheduled buffer sources.
        // The generation counter prevents the chunk loop from scheduling more.
        let _ = gain_node.disconnect();
    }
}

/// Returns true if streaming playback is currently active.
#[allow(dead_code)]
pub(crate) fn is_streaming() -> bool {
    STREAM_CTX.with(|c| c.borrow().is_some())
}

/// Start streaming playback of a sample range.
///
/// `source` provides sample data; `channel_view` selects which channel(s) to play.
/// Returns the final playback sample rate (may differ from source for TE mode),
/// or `None` if AudioContext creation failed.
pub(crate) fn start_stream(
    source: Arc<dyn AudioSource>,
    channel_view: ChannelView,
    sample_rate: u32,
    start_sample: usize,
    end_sample: usize,
    params: PlaybackParams,
) -> Option<u32> {
    stop_stream();

    let final_rate = match params.mode {
        PlaybackMode::TimeExpansion => {
            let abs_f = params.te_factor.abs().max(1.0);
            let rate = if params.te_factor > 0.0 {
                sample_rate as f64 / abs_f
            } else {
                sample_rate as f64 * abs_f
            };
            (rate as u32).clamp(8000, 384_000)
        }
        _ => sample_rate,
    };

    // Stereo output: stereo source + Stereo view (all modes, not just Normal)
    let stereo_out = source.channel_count() >= 2
        && channel_view == ChannelView::Stereo;

    // Reuse existing AudioContext if its sample rate matches; otherwise create new.
    let ctx = STREAM_CTX.with(|c| {
        let existing = c.borrow().clone();
        if let Some(ref ctx) = existing {
            if (ctx.sample_rate() - final_rate as f32).abs() < 1.0 {
                let _ = ctx.resume();
                return Some(ctx.clone());
            }
            // Sample rate changed — must close old and create new
            let _ = ctx.close();
            *c.borrow_mut() = None;
        }
        let opts = AudioContextOptions::new();
        opts.set_sample_rate(final_rate as f32);
        let new_ctx = AudioContext::new_with_context_options(&opts)
            .or_else(|_| AudioContext::new())
            .ok()?;
        *c.borrow_mut() = Some(new_ctx.clone());
        Some(new_ctx)
    });
    let Some(ctx) = ctx else {
        web_sys::console::warn_1(&"Playback failed: could not create AudioContext".into());
        return None;
    };

    // Create a master gain node for fade-out on stop
    let gain_node = ctx.create_gain().unwrap();
    let _ = gain_node.connect_with_audio_node(&ctx.destination());

    let generation = STREAM_GEN.with(|g| *g.borrow());
    STREAM_GAIN.with(|g| *g.borrow_mut() = Some(gain_node.clone()));

    // Spawn the async chunk-processing loop
    wasm_bindgen_futures::spawn_local(chunk_loop(
        ctx,
        gain_node,
        generation,
        source,
        channel_view,
        stereo_out,
        sample_rate,
        final_rate,
        start_sample,
        end_sample,
        params,
    ));

    Some(final_rate)
}

/// Async loop that processes and schedules audio chunks.
async fn chunk_loop(
    ctx: AudioContext,
    gain_node: web_sys::GainNode,
    generation: u32,
    source: Arc<dyn AudioSource>,
    channel_view: ChannelView,
    stereo_out: bool,
    source_rate: u32,
    final_rate: u32,
    start_sample: usize,
    end_sample: usize,
    params: PlaybackParams,
) {
    let mut pos = start_sample;
    let pv_hq_mode = params.pv_hq && matches!(params.mode, PlaybackMode::PhaseVocoder | PlaybackMode::PitchShift);

    // Gain computation depends on mode:
    // - Off: no gain at all (0 dB)
    // - Manual: gain_db slider only
    // - AutoPeak: pre-scan peak normalization + gain_db slider on top
    // - Adaptive: AGC leveler with smooth envelope following (applied in process_one_chunk)
    let is_adaptive = params.gain_mode == GainMode::Adaptive;
    let manual_boost = params.gain_db; // slider value, additive for all modes

    let auto_peak_gain: f64 = params.auto_peak_gain_db;

    // AGC processor for Adaptive mode — persists across chunks for smooth gain transitions
    let agc = if is_adaptive {
        Some(RefCell::new(AgcProcessor::new(AgcConfig::default(), final_rate)))
    } else {
        None
    };

    let mode_boost = match params.mode {
        PlaybackMode::PhaseVocoder => PV_MODE_BOOST_DB,
        _ => 0.0,
    };

    let global_gain = match params.gain_mode {
        GainMode::Off => mode_boost,
        GainMode::Manual => manual_boost + mode_boost,
        GainMode::AutoPeak => auto_peak_gain + manual_boost + mode_boost,
        GainMode::Adaptive => manual_boost + mode_boost, // AGC applied per-sample in process_one_chunk
    };

    // ── Pre-buffer phase ─────────────────────────────────────────────────────
    // Process PREBUFFER_CHUNKS chunks before scheduling anything, so Web Audio
    // has a comfortable lead and the first audible chunk doesn't skip.
    struct PreBuf {
        samples: Vec<f32>,
        left: Option<Vec<f32>>,
        right: Option<Vec<f32>>,
    }
    let mut prebuf: Vec<PreBuf> = Vec::with_capacity(PREBUFFER_CHUNKS);

    for _ in 0..PREBUFFER_CHUNKS {
        if pos >= end_sample { break; }
        let current_gen = STREAM_GEN.with(|g| *g.borrow());
        if current_gen != generation { return; }

        let (final_samples, left, right, new_pos) = process_one_chunk(
            &source, channel_view, stereo_out, source_rate, &params,
            global_gain, agc.as_ref(),
            pos, start_sample, end_sample,
        ).await;
        pos = new_pos;

        prebuf.push(PreBuf { samples: final_samples, left, right });

        // Yield between pre-buffer chunks so the UI stays responsive
        yield_to_browser().await;
    }

    // Now schedule all pre-buffered chunks at once, starting slightly in the future
    let mut scheduled_time = ctx.current_time() + 0.02;
    SCHEDULED_END.with(|s| s.set(scheduled_time));

    // Fade in from silence to avoid click when crossfading with old stream
    {
        let param = gain_node.gain();
        let _ = param.set_value_at_time(0.0, scheduled_time);
        let _ = param.linear_ramp_to_value_at_time(1.0, scheduled_time + FADE_IN_MS / 1000.0);
    }

    // In PV HQ mode, stride duration is CHUNK_SAMPLES/rate (the overlap is
    // extra audio that blends with the next chunk via crossfade).
    let stride_duration = CHUNK_SAMPLES as f64 / final_rate as f64;

    for buf in prebuf {
        if !buf.samples.is_empty() {
            if stereo_out {
                if let (Some(ref left), Some(ref right)) = (buf.left, buf.right) {
                    schedule_buffer_stereo(&ctx, &gain_node, left, right, final_rate, scheduled_time);
                    if pv_hq_mode {
                        scheduled_time += stride_duration;
                    } else {
                        let chunk_duration = left.len() as f64 / final_rate as f64;
                        scheduled_time += chunk_duration;
                    }
                }
            } else {
                schedule_buffer(&ctx, &gain_node, &buf.samples, final_rate, scheduled_time);
                if pv_hq_mode {
                    // Advance by stride only; the trailing overlap will be summed
                    // with the next chunk's fade-in by Web Audio.
                    scheduled_time += stride_duration;
                } else {
                    let chunk_duration = buf.samples.len() as f64 / final_rate as f64;
                    scheduled_time += chunk_duration;
                }
            }
            SCHEDULED_END.with(|s| s.set(scheduled_time));
        }
    }

    // ── Streaming phase ──────────────────────────────────────────────────────
    // Continue processing remaining chunks with lookahead throttling.
    while pos < end_sample {
        let current_gen = STREAM_GEN.with(|g| *g.borrow());
        if current_gen != generation { break; }

        let (final_samples, left, right, new_pos) = process_one_chunk(
            &source, channel_view, stereo_out, source_rate, &params,
            global_gain, agc.as_ref(),
            pos, start_sample, end_sample,
        ).await;
        pos = new_pos;

        if !final_samples.is_empty() {
            if stereo_out {
                if let (Some(ref left), Some(ref right)) = (left, right) {
                    schedule_buffer_stereo(&ctx, &gain_node, left, right, final_rate, scheduled_time);
                    if pv_hq_mode {
                        scheduled_time += stride_duration;
                    } else {
                        let chunk_duration = left.len() as f64 / final_rate as f64;
                        scheduled_time += chunk_duration;
                    }
                }
            } else {
                schedule_buffer(&ctx, &gain_node, &final_samples, final_rate, scheduled_time);
                if pv_hq_mode {
                    scheduled_time += stride_duration;
                } else {
                    let chunk_duration = final_samples.len() as f64 / final_rate as f64;
                    scheduled_time += chunk_duration;
                }
            }
            SCHEDULED_END.with(|s| s.set(scheduled_time));
        }

        // Yield to browser so UI stays responsive
        yield_to_browser().await;

        // If we're well ahead of playback, sleep before processing next chunk
        let now = ctx.current_time();
        if scheduled_time - now > LOOKAHEAD_SECS {
            let sleep_ms = ((scheduled_time - now - LOOKAHEAD_SECS * 0.5) * 1000.0) as u32;
            if sleep_ms > 10 {
                sleep(sleep_ms).await;
            }
        }
    }
}

/// Process a single chunk: prefetch, read, filter, DSP, trim, gain.
/// Returns (mono_samples, optional_left, optional_right, next_pos).
async fn process_one_chunk(
    source: &Arc<dyn AudioSource>,
    channel_view: ChannelView,
    stereo_out: bool,
    source_rate: u32,
    params: &PlaybackParams,
    global_gain: f64,
    agc: Option<&RefCell<AgcProcessor>>,
    pos: usize,
    start_sample: usize,
    end_sample: usize,
) -> (Vec<f32>, Option<Vec<f32>>, Option<Vec<f32>>, usize) {
    let pv_hq_mode = params.pv_hq && matches!(params.mode, PlaybackMode::PhaseVocoder | PlaybackMode::PitchShift);

    let warmup_start = pos.saturating_sub(FILTER_WARMUP);
    let chunk_end = (pos + CHUNK_SAMPLES).min(end_sample);
    let warmup_len = pos - warmup_start;

    // In PV HQ mode, extend the chunk past its nominal end by PV_HQ_OVERLAP.
    // The trailing overlap will be crossfaded with the next chunk's leading
    // overlap instead of being trimmed.
    let trailing_end = if pv_hq_mode {
        (chunk_end + PV_HQ_OVERLAP).min(end_sample)
    } else if matches!(params.mode, PlaybackMode::PitchShift | PlaybackMode::PhaseVocoder) {
        (chunk_end + FILTER_WARMUP).min(end_sample)
    } else {
        chunk_end
    };
    let trailing_len = trailing_end - chunk_end;

    // Prefetch for streaming sources
    streaming_source::prefetch_streaming(source.as_ref(), warmup_start as u64, trailing_end - warmup_start).await;

    let chunk_with_warmup = source.read_region(channel_view, warmup_start as u64, trailing_end - warmup_start);
    let filtered = apply_filters(&chunk_with_warmup, source_rate, params);
    let processed = apply_dsp_mode(&filtered, source_rate, params);

    // Helper: process a channel through filters + DSP (same pipeline as mono)
    let process_ch = |cv: ChannelView| -> Vec<f32> {
        let raw = source.read_region(cv, warmup_start as u64, trailing_end - warmup_start);
        let filtered = apply_filters(&raw, source_rate, params);
        apply_dsp_mode(&filtered, source_rate, params)
    };

    if pv_hq_mode {
        // HQ mode: trim warmup but keep trailing overlap with crossfade envelope.
        let trim_start = warmup_len;
        let core_len = chunk_end - pos; // nominal chunk length (without overlap)

        // Apply PV HQ fading to a processed buffer
        let apply_pv_hq_fading = |buf: &mut Vec<f32>| {
            // Hann fade-in on leading overlap (first chunk's start is clean)
            if pos > start_sample {
                let fade_in_len = PV_HQ_OVERLAP.min(core_len).min(buf.len());
                for (i, sample) in buf.iter_mut().enumerate().take(fade_in_len) {
                    let t = i as f32 / fade_in_len as f32;
                    let w = 0.5 * (1.0 - (std::f32::consts::PI * t).cos());
                    *sample *= w;
                }
            }
            // Hann fade-out on trailing overlap
            if trailing_len > 0 {
                let fade_out_start = buf.len().saturating_sub(trailing_len);
                let fade_out_len = buf.len() - fade_out_start;
                for i in 0..fade_out_len {
                    let t = i as f32 / fade_out_len as f32;
                    let w = 0.5 * (1.0 + (std::f32::consts::PI * t).cos());
                    buf[fade_out_start + i] *= w;
                }
            }
        };

        let mut final_samples = if trim_start < processed.len() {
            processed[trim_start..].to_vec()
        } else {
            processed.to_vec()
        };
        apply_pv_hq_fading(&mut final_samples);

        apply_gain(&mut final_samples, global_gain);
        if let Some(agc_cell) = agc {
            agc_cell.borrow_mut().process(&mut final_samples);
        }

        let (left, right) = if stereo_out {
            let l_proc = process_ch(ChannelView::Channel(0));
            let r_proc = process_ch(ChannelView::Channel(1));
            let mut l = if trim_start < l_proc.len() { l_proc[trim_start..].to_vec() } else { l_proc };
            let mut r = if trim_start < r_proc.len() { r_proc[trim_start..].to_vec() } else { r_proc };
            apply_pv_hq_fading(&mut l);
            apply_pv_hq_fading(&mut r);
            apply_gain(&mut l, global_gain);
            apply_gain(&mut r, global_gain);
            if let Some(agc_cell) = agc {
                agc_cell.borrow_mut().process_stereo(&mut l, &mut r);
            }
            (Some(l), Some(r))
        } else {
            (None, None)
        };

        (final_samples, left, right, chunk_end)
    } else {
        // Standard mode: trim warmup and trailing
        let trim_start = warmup_len;
        let trim_end = processed.len().saturating_sub(trailing_len);
        let trimmed = if trim_start < trim_end {
            &processed[trim_start..trim_end]
        } else {
            &processed[..]
        };

        let mut final_samples = trimmed.to_vec();

        apply_gain(&mut final_samples, global_gain);
        if let Some(agc_cell) = agc {
            agc_cell.borrow_mut().process(&mut final_samples);
        }

        let (left, right) = if stereo_out {
            let l_proc = process_ch(ChannelView::Channel(0));
            let r_proc = process_ch(ChannelView::Channel(1));
            let l_trim_end = l_proc.len().saturating_sub(trailing_len);
            let r_trim_end = r_proc.len().saturating_sub(trailing_len);
            let mut l = if trim_start < l_trim_end { l_proc[trim_start..l_trim_end].to_vec() } else { l_proc };
            let mut r = if trim_start < r_trim_end { r_proc[trim_start..r_trim_end].to_vec() } else { r_proc };
            apply_gain(&mut l, global_gain);
            apply_gain(&mut r, global_gain);
            if let Some(agc_cell) = agc {
                agc_cell.borrow_mut().process_stereo(&mut l, &mut r);
            }
            (Some(l), Some(r))
        } else {
            (None, None)
        };

        (final_samples, left, right, chunk_end)
    }
}


pub(crate) fn apply_filters(samples: &[f32], sample_rate: u32, params: &PlaybackParams) -> Vec<f32> {
    let mut result = if params.filter_enabled {
        match params.filter_quality {
            FilterQuality::Fast => apply_eq_filter_fast(
                samples, sample_rate,
                params.filter_freq_low, params.filter_freq_high,
                params.filter_db_below, params.filter_db_selected,
                params.filter_db_harmonics, params.filter_db_above,
                params.filter_band_mode,
            ),
            FilterQuality::Spectral => apply_eq_filter(
                samples, sample_rate,
                params.filter_freq_low, params.filter_freq_high,
                params.filter_db_below, params.filter_db_selected,
                params.filter_db_harmonics, params.filter_db_above,
                params.filter_band_mode,
            ),
        }
    } else if selection_bandpass_active(sample_rate, params) {
        apply_bandpass(samples, sample_rate, params.sel_freq_low, params.sel_freq_high)
    } else {
        samples.to_vec()
    };

    // Apply notch filters after EQ/bandpass
    if params.notch_enabled && !params.notch_bands.is_empty() {
        result = crate::dsp::notch::apply_notch_filters(
            &result, sample_rate, &params.notch_bands,
            params.notch_harmonic_suppression,
        );
    }

    // Apply spectral subtraction after notch
    if params.noise_reduce_enabled {
        if let Some(ref floor) = params.noise_reduce_floor {
            result = crate::dsp::spectral_sub::apply_spectral_subtraction(
                &result, sample_rate, floor, params.noise_reduce_strength, 0.05,
                params.notch_harmonic_suppression,
            );
        }
    }

    result
}

pub(crate) fn apply_dsp_mode(samples: &[f32], sample_rate: u32, params: &PlaybackParams) -> Vec<f32> {
    match params.mode {
        PlaybackMode::Normal => samples.to_vec(),
        PlaybackMode::Heterodyne => {
            let effective_lo =
                if params.has_selection && (params.sel_freq_low > 0.0 || params.sel_freq_high > 0.0)
                {
                    (params.sel_freq_low + params.sel_freq_high) / 2.0
                } else {
                    params.het_freq
                };
            heterodyne_mix(samples, sample_rate, effective_lo, params.het_cutoff)
        }
        PlaybackMode::TimeExpansion => {
            // Rate change handled by AudioContext sample rate, not sample transform
            samples.to_vec()
        }
        PlaybackMode::PitchShift => pitch_shift_realtime(samples, params.ps_factor),
        PlaybackMode::PhaseVocoder => crate::dsp::phase_vocoder::phase_vocoder_pitch_shift(samples, params.pv_factor),
        PlaybackMode::ZeroCrossing => {
            zc_divide(samples, sample_rate, params.zc_factor as u32, params.filter_enabled)
        }
    }
}

fn schedule_buffer(ctx: &AudioContext, dest: &web_sys::GainNode, samples: &[f32], sample_rate: u32, when: f64) {
    let Ok(buffer) = ctx.create_buffer(1, samples.len() as u32, sample_rate as f32) else {
        return;
    };
    let _ = buffer.copy_to_channel(samples, 0);
    let Ok(source) = ctx.create_buffer_source() else {
        return;
    };
    source.set_buffer(Some(&buffer));
    let _ = source.connect_with_audio_node(dest);
    let _ = source.start_with_when(when);
}

fn schedule_buffer_stereo(ctx: &AudioContext, dest: &web_sys::GainNode, left: &[f32], right: &[f32], sample_rate: u32, when: f64) {
    let len = left.len().min(right.len());
    let Ok(buffer) = ctx.create_buffer(2, len as u32, sample_rate as f32) else {
        return;
    };
    let _ = buffer.copy_to_channel(&left[..len], 0);
    let _ = buffer.copy_to_channel(&right[..len], 1);
    let Ok(source) = ctx.create_buffer_source() else {
        return;
    };
    source.set_buffer(Some(&buffer));
    let _ = source.connect_with_audio_node(dest);
    let _ = source.start_with_when(when);
}

async fn yield_to_browser() {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        if let Some(w) = web_sys::window() {
            let _ = w.set_timeout_with_callback(&resolve);
        }
    });
    let _ = JsFuture::from(promise).await;
}

async fn sleep(ms: u32) {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        if let Some(w) = web_sys::window() {
            let _ = w.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms as i32);
        }
    });
    let _ = JsFuture::from(promise).await;
}
