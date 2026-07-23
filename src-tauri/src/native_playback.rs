//! Native cpal-based audio playback for Tauri.
//!
//! Streams decoded + processed audio through cpal output, bypassing
//! Web Audio entirely. A producer thread decodes and processes chunks
//! into a ring buffer; the cpal callback drains it.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use serde::Deserialize;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use tauri::Emitter;

/// Playback parameters sent from the frontend.
#[derive(Deserialize, Clone, Debug)]
pub struct NativePlayParams {
    pub path: String,
    pub start_sample: usize,
    pub end_sample: usize,
    #[allow(dead_code)]
    pub sample_rate: u32,
    pub mode: String, // "Normal", "Heterodyne", "TimeExpansion", "PitchShift", "ZeroCrossing"
    pub het_freq: f64,
    pub het_cutoff: f64,
    pub te_factor: f64,
    pub gain_db: f64,
    pub auto_gain: bool,
}

/// Wraps cpal::Stream to be Send (cpal::Stream is Send on desktop platforms).
/// The inner stream is only kept alive for its RAII drop side-effect (it stops
/// playback when dropped); it is never read directly.
struct SendStream(#[allow(dead_code)] cpal::Stream);
unsafe impl Send for SendStream {}

#[allow(dead_code)]
pub struct PlaybackState {
    _stream: SendStream,
    ring: Arc<Mutex<VecDeque<f32>>>,
    is_playing: Arc<AtomicBool>,
    playhead_bits: Arc<AtomicU64>,
    producer_stop: Arc<AtomicBool>,
    emitter_stop: Arc<AtomicBool>,
}

/// Number of source samples per processing chunk (~0.5s at 192kHz).
const CHUNK_SAMPLES: usize = 96_000;

/// Ring buffer capacity in samples (2 seconds at 48kHz).
const RING_CAPACITY: usize = 96_000 * 2;

/// Start native audio playback.
pub fn start(params: NativePlayParams, app: tauri::AppHandle) -> Result<PlaybackState, String> {
    // Decode the file via the shared oversample-core decoder (single source of
    // truth; the browser/WASM frontend uses the same path). `samples` is already
    // an `Arc<Vec<f32>>`, so no extra copy is needed here.
    let bytes = std::fs::read(&params.path)
        .map_err(|e| format!("Failed to read '{}': {e}", params.path))?;
    let audio = oversample_core::audio::loader::load_audio(&bytes)?;
    let all_samples = audio.samples;
    let source_rate = audio.sample_rate;

    let start_sample = params.start_sample.min(all_samples.len());
    let end_sample = params.end_sample.min(all_samples.len());
    if end_sample <= start_sample {
        return Err("Empty sample range".into());
    }

    // Determine output sample rate
    let output_rate = match params.mode.as_str() {
        "TimeExpansion" => ((source_rate as f64 / params.te_factor) as u32).max(8000),
        _ => source_rate,
    };

    // Open default output device
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or("No output audio device found")?;

    // Try to get a config matching our desired sample rate
    let config = cpal::StreamConfig {
        channels: 1,
        sample_rate: output_rate,
        buffer_size: cpal::BufferSize::Default,
    };

    let ring: Arc<Mutex<VecDeque<f32>>> =
        Arc::new(Mutex::new(VecDeque::with_capacity(RING_CAPACITY)));
    let is_playing = Arc::new(AtomicBool::new(true));
    let playhead_bits = Arc::new(AtomicU64::new(0u64));
    let producer_stop = Arc::new(AtomicBool::new(false));
    let emitter_stop = Arc::new(AtomicBool::new(false));

    // cpal output callback: drain ring buffer into output frames
    let ring_out = ring.clone();
    let is_playing_out = is_playing.clone();
    let stream = device
        .build_output_stream(
            &config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                let mut ring = ring_out.lock().unwrap();
                for sample in data.iter_mut() {
                    *sample = ring.pop_front().unwrap_or(0.0);
                }
                // If ring is empty and producer is done, signal playback complete
                if ring.is_empty() && !is_playing_out.load(Ordering::Relaxed) {
                    // Already stopped
                }
            },
            |err| {
                eprintln!("cpal output error: {err}");
            },
            None,
        )
        .map_err(|e| format!("Failed to build output stream: {e}"))?;

    stream
        .play()
        .map_err(|e| format!("Failed to start playback: {e}"))?;

    // Producer thread: decode chunks, apply DSP, push to ring
    let ring_prod = ring.clone();
    let is_playing_prod = is_playing.clone();
    let playhead_prod = playhead_bits.clone();
    let producer_stop_flag = producer_stop.clone();
    let params_clone = params.clone();

    std::thread::spawn(move || {
        let start_time_secs = start_sample as f64 / source_rate as f64;
        let mut pos = start_sample;

        // For auto-gain: pre-scan up to ~15s so quiet intros don't cause
        // excessive gain, without stalling on very long files.
        let cached_gain: Option<f64> = if params_clone.auto_gain {
            let max_scan = (source_rate as usize) * 15;
            let scan_end = end_sample.min(start_sample + max_scan);
            let peak = all_samples[start_sample..scan_end]
                .iter()
                .fold(0.0f32, |mx, s| mx.max(s.abs()));
            if peak < 1e-10 {
                Some(0.0)
            } else {
                let peak_db = 20.0 * (peak as f64).log10();
                Some((-3.0 - peak_db).min(30.0))
            }
        } else {
            None
        };

        while pos < end_sample && !producer_stop_flag.load(Ordering::Relaxed) {
            let chunk_end = (pos + CHUNK_SAMPLES).min(end_sample);
            let chunk = &all_samples[pos..chunk_end];

            // Apply DSP
            let processed = match params_clone.mode.as_str() {
                "Heterodyne" => heterodyne_mix(
                    chunk,
                    source_rate,
                    params_clone.het_freq,
                    params_clone.het_cutoff,
                ),
                "Normal" | "TimeExpansion" => chunk.to_vec(),
                // PitchShift and ZeroCrossing: pass through for now (to be added)
                _ => chunk.to_vec(),
            };

            // Apply gain
            let mut final_samples = processed;
            let gain = cached_gain.unwrap_or(params_clone.gain_db);
            apply_gain(&mut final_samples, gain);

            // Push to ring buffer, waiting if it's full
            let mut pushed = 0;
            while pushed < final_samples.len() && !producer_stop_flag.load(Ordering::Relaxed) {
                {
                    let mut ring = ring_prod.lock().unwrap();
                    let space = RING_CAPACITY.saturating_sub(ring.len());
                    let take = space.min(final_samples.len() - pushed);
                    ring.extend(&final_samples[pushed..pushed + take]);
                    pushed += take;
                }
                if pushed < final_samples.len() {
                    // Ring is full, sleep briefly and retry
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
            }

            // Update playhead
            let current_secs =
                start_time_secs + (chunk_end - start_sample) as f64 / source_rate as f64;
            playhead_prod.store(current_secs.to_bits(), Ordering::Relaxed);

            pos = chunk_end;
        }

        // Wait for ring buffer to drain
        loop {
            if producer_stop_flag.load(Ordering::Relaxed) {
                break;
            }
            let remaining = ring_prod.lock().unwrap().len();
            if remaining == 0 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }

        is_playing_prod.store(false, Ordering::Relaxed);
    });

    // Emitter thread: sends playback-position events to frontend
    let playhead_emit = playhead_bits.clone();
    let is_playing_emit = is_playing.clone();
    let emitter_stop_flag = emitter_stop.clone();

    std::thread::spawn(move || {
        while !emitter_stop_flag.load(Ordering::Relaxed) && is_playing_emit.load(Ordering::Relaxed)
        {
            let secs = f64::from_bits(playhead_emit.load(Ordering::Relaxed));
            let _ = app.emit("playback-position", secs);
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    });

    Ok(PlaybackState {
        _stream: SendStream(stream),
        ring,
        is_playing,
        playhead_bits,
        producer_stop,
        emitter_stop,
    })
}

/// Stop native playback.
pub fn stop(state: &mut Option<PlaybackState>) {
    if let Some(s) = state.take() {
        s.producer_stop.store(true, Ordering::Relaxed);
        s.emitter_stop.store(true, Ordering::Relaxed);
        s.is_playing.store(false, Ordering::Relaxed);
        // Stream and threads are dropped, stopping playback
    }
}

// ── Minimal DSP functions (native-side copies) ──────────────────────

/// Simple heterodyne: ring-modulate then cascaded lowpass.
fn heterodyne_mix(samples: &[f32], sample_rate: u32, freq: f64, cutoff: f64) -> Vec<f32> {
    let sr = sample_rate as f64;
    let mut result: Vec<f32> = samples
        .iter()
        .enumerate()
        .map(|(i, &s)| {
            let t = i as f64 / sr;
            let carrier = (2.0 * std::f64::consts::PI * freq * t).cos() as f32;
            s * carrier * 2.0
        })
        .collect();

    // Cascaded lowpass (4 passes)
    let cutoff_hz = cutoff.min(sr / 2.0 - 1.0).max(20.0);
    for _ in 0..4 {
        lowpass_inplace(&mut result, cutoff_hz, sample_rate);
    }
    result
}

fn lowpass_inplace(buf: &mut [f32], cutoff_hz: f64, sample_rate: u32) {
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

fn apply_gain(samples: &mut [f32], gain_db: f64) {
    if gain_db.abs() < 0.001 {
        return;
    }
    let gain_linear = 10.0_f64.powf(gain_db / 20.0) as f32;
    for s in samples.iter_mut() {
        *s *= gain_linear;
    }
}
