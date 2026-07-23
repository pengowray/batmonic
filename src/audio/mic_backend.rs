//! Backend-specific microphone operations.
//!
//! Abstracts the three mic backends (Browser/Web Audio, cpal/Tauri native,
//! Raw USB) behind the `MicBackend` enum (defined in `state`) with uniform
//! open/close/record/listen methods. All thread-local state for each backend
//! lives here.

use crate::audio::playback::{apply_gain, snapshot_params};
use crate::audio::streaming_playback::{apply_filters, PlaybackParams};
use crate::dsp::heterodyne::RealtimeCombHet;
use crate::dsp::phase_vocoder::phase_vocoder_pitch_shift;
use crate::dsp::pitch_shift::pitch_shift_realtime;
use crate::dsp::zc_divide::zc_divide;
use crate::state::store_fields::*;
use crate::state::{AppState, MicAcquisitionState, MicBackend, PlaybackMode};
use crate::tauri_bridge::{
    get_tauri_internals, tauri_invoke, tauri_invoke_args, tauri_invoke_no_args,
    tauri_invoke_typed_args, tauri_invoke_typed_no_args,
};
use leptos::prelude::*;
use oversample_core::audio::live_schedule::{plan_live_schedule, DEFAULT_MAX_LOOKAHEAD_SECS};
use std::cell::RefCell;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::AudioContext;

/// Build IPC args for `mic_start_recording` / `usb_start_recording`.
/// Shared between the cpal and raw-USB paths so they can't drift.
///
/// Setting `enableRecovery: true` tells the backend to stream native-format
/// PCM to a `<app_data>/recordings/.recovery/<name>.wav.part` file as the
/// audio callback fires, and to drain the in-memory Tauri buffer after each
/// flush. This bounds Tauri-side memory to ~240 ms of samples and
/// doubles as crash-recovery. At stop the partial is finalized in-place (the
/// GUANO chunk is appended and the header is patched) and moved to the
/// final destination with no large in-memory copy.
///
/// We disable this only for `record_mode == ToMemory` (user asked to keep it all
/// in RAM, no disk write). Pre-roll now streams to disk too: the native side
/// seeds the recording with its listening pre-roll ring (`preroll_samples`), so
/// the `.wav.part` starts with the pre-roll and the whole capture is crash-safe
/// and bounded — no WASM re-encode.
fn build_start_recording_args(
    state: &AppState,
    shared_fd: Option<i32>,
) -> oversample_ipc::mic::StartRecordingArgs {
    let to_memory =
        state.playback.record_mode().get_untracked() == crate::state::RecordMode::ToMemory;
    let preroll = state.mic.preroll_samples().get_untracked();
    let stream_to_disk = state.is_tauri && !to_memory;

    // Filename mirrors the one the WASM side uses for the live file so the
    // sidecar + partial match up with the final WAV name on recovery.
    let filename = state.mic.live_file_idx().get_untracked().and_then(|idx| {
        state
            .library
            .files()
            .with_untracked(|f| f.get(idx).map(|f| f.name.clone()))
    });

    let (device_make, device_model) = if state.recording_meta.device_model_enabled().get_untracked()
    {
        (
            state.recording_meta.cached_make().get_untracked(),
            state.recording_meta.cached_model().get_untracked(),
        )
    } else {
        (None, None)
    };

    let (loc_latitude, loc_longitude, loc_elevation, loc_accuracy) =
        match state.recording_meta.location().get_untracked() {
            Some(loc) => (
                Some(loc.latitude),
                Some(loc.longitude),
                loc.elevation,
                loc.accuracy,
            ),
            None => (None, None, None, None),
        };

    // Per-device manual bit-depth override for the current device (None = Auto).
    let force_bits = state.mic.device_name().get_untracked().and_then(|dev| {
        state
            .mic
            .bit_depth_override()
            .with_untracked(|m| m.get(&dev).copied())
    });

    oversample_ipc::mic::StartRecordingArgs {
        shared_fd,
        enable_recovery: stream_to_disk,
        force_bits,
        filename,
        connection_type: state.mic.connection_type().get_untracked(),
        mic_name: state.mic.device_name().get_untracked(),
        mic_make: state.mic.manufacturer().get_untracked(),
        device_make,
        device_model,
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        loc_latitude,
        loc_longitude,
        loc_elevation,
        loc_accuracy,
        // Seed the native listening pre-roll ring into the start of the
        // recording so a long-press capture is written linearly to disk.
        preroll_samples: (preroll > 0).then_some(preroll as u32),
    }
}

/// Build IPC args for mic_stop_recording / usb_stop_recording,
/// including optional GPS location and device model fields from state.
fn build_stop_recording_args(state: &AppState) -> oversample_ipc::mic::StopRecordingArgs {
    let (loc_latitude, loc_longitude, loc_elevation, loc_accuracy) =
        match state.recording_meta.location().get_untracked() {
            Some(loc) => (
                Some(loc.latitude),
                Some(loc.longitude),
                loc.elevation,
                loc.accuracy,
            ),
            None => (None, None, None, None),
        };
    let (device_make, device_model) = if state.recording_meta.device_model_enabled().get_untracked()
    {
        (
            state.recording_meta.cached_make().get_untracked(),
            state.recording_meta.cached_model().get_untracked(),
        )
    } else {
        (None, None)
    };
    oversample_ipc::mic::StopRecordingArgs {
        loc_latitude,
        loc_longitude,
        loc_elevation,
        loc_accuracy,
        device_make,
        device_model,
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        // Pre-roll now streams to disk natively (seeded from the listening ring),
        // so the native partial is authoritative — never skip the native save.
        skip_native_save: None,
    }
}

// ── Thread-local state: Web Audio mode ──────────────────────────────────

thread_local! {
    static MIC_CTX: RefCell<Option<AudioContext>> = const { RefCell::new(None) };
    static MIC_STREAM: RefCell<Option<web_sys::MediaStream>> = const { RefCell::new(None) };
    static MIC_PROCESSOR: RefCell<Option<web_sys::ScriptProcessorNode>> = const { RefCell::new(None) };
    static MIC_BUFFER: RefCell<Vec<f32>> = const { RefCell::new(Vec::new()) };
    static MIC_HANDLER: RefCell<Option<Closure<dyn FnMut(web_sys::AudioProcessingEvent)>>> = RefCell::new(None);
    static WEB_RT_HET: RefCell<RealtimeCombHet> = RefCell::new(RealtimeCombHet::new());
    /// Overlap context state for PS/PV live listening (web).
    static WEB_LISTEN_STATE: RefCell<ListenDspState> = const { RefCell::new(ListenDspState::new()) };
}

// ── Thread-local state: Native mode (shared by cpal AND USB) ────────────

thread_local! {
    /// Whether a native mic (cpal or USB) is currently open.
    static NATIVE_MIC_OPEN: RefCell<Option<NativeMode>> = const { RefCell::new(None) };
    /// AudioContext for HET playback (output only, no mic input).
    static HET_CTX: RefCell<Option<AudioContext>> = const { RefCell::new(None) };
    /// Next scheduled playback time for HET audio buffers (the schedule cursor).
    static HET_NEXT_TIME: RefCell<f64> = const { RefCell::new(0.0) };
    /// Scheduled live-playback sources paired with their scheduled end time, so
    /// `stop_het_playback` can stop them instantly on Stop / skip-ahead and we
    /// can prune ended ones. Without this, already-scheduled buffers keep
    /// sounding for seconds after the user presses Stop.
    static HET_SOURCES: RefCell<Vec<(web_sys::AudioBufferSourceNode, f64)>> = const { RefCell::new(Vec::new()) };
    /// Count of skip-ahead (dropped-backlog) events since last read — a coarse
    /// signal that live playback is being throttled (e.g. app backgrounded).
    static HET_SKIP_COUNT: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    /// Generation counter for the native audio pull loop. Bumped each time a
    /// loop starts (on mic open) so any stale loop from a prior open exits.
    static NATIVE_PULL_GEN: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    /// Accumulated recording samples on the frontend for native modes (cpal/USB).
    static NATIVE_REC_BUFFER: RefCell<Vec<f32>> = const { RefCell::new(Vec::new()) };
    /// Realtime heterodyne processor for native modes.
    static NATIVE_RT_HET: RefCell<RealtimeCombHet> = RefCell::new(RealtimeCombHet::new());
    /// Overlap context state for PS/PV live listening (native).
    static NATIVE_LISTEN_STATE: RefCell<ListenDspState> = const { RefCell::new(ListenDspState::new()) };
}

// ── Thread-local state: USB-specific ────────────────────────────────────

thread_local! {
    /// Keep the USB stream error event listener closure alive.
    static USB_ERROR_CLOSURE: RefCell<Option<Closure<dyn FnMut(JsValue)>>> = RefCell::new(None);
}

/// Which native mode is active (stored in NATIVE_MIC_OPEN).
#[derive(Clone, Copy, Debug, PartialEq)]
enum NativeMode {
    Cpal,
    Usb,
}

/// Result of stopping a recording.
pub enum StopResult {
    /// Browser mode: raw samples extracted from the JS callback buffer.
    Samples { samples: Vec<f32>, sample_rate: u32 },
    /// Native (cpal/USB) mode: parsed result from Tauri command.
    TauriResult(TauriRecordingResult),
    /// Recording produced no usable data.
    Empty,
    /// An error occurred while stopping.
    Error(String),
}

/// Parsed recording result returned by `mic_stop_recording` / `usb_stop_recording`.
pub struct TauriRecordingResult {
    pub filename: String,
    pub saved_path: String,
    pub sample_rate: u32,
    pub bits_per_sample: u16,
    pub is_float: bool,
    pub duration_secs: f64,
    pub samples: Vec<f32>,
    pub file_size_bytes: Option<usize>,
}

impl TauriRecordingResult {
    /// Build from the IPC metadata, fetching to-memory samples as raw bytes when
    /// the native side stashed them (`has_memory_samples`).
    pub async fn from_result(r: oversample_ipc::mic::RecordingResult) -> Option<Self> {
        // A successful recording can legitimately have no in-memory samples:
        // streaming-to-disk mode (Tauri default) writes the WAV during capture
        // and returns metadata only, and the skip-save / preroll path returns no
        // samples + empty saved_path with duration > 0 (re-encoded from the WASM
        // buffer). The Rust side returns Err when there's nothing to save, so any
        // Ok with positive duration is real; no samples AND zero duration is
        // nothing.
        if r.duration_secs <= 0.0 && !r.has_memory_samples {
            return None;
        }
        let samples = if r.has_memory_samples {
            fetch_recorded_samples().await
        } else {
            Vec::new()
        };
        Some(TauriRecordingResult {
            filename: r.filename,
            saved_path: r.saved_path,
            sample_rate: r.sample_rate,
            bits_per_sample: r.bits_per_sample,
            is_float: r.is_float,
            duration_secs: r.duration_secs,
            samples,
            file_size_bytes: Some(r.file_size_bytes),
        })
    }
}

/// Fetch the to-memory recording samples the native side stashed (raw f32
/// little-endian bytes as an ArrayBuffer) and decode them. Empty on failure or
/// when nothing was stashed.
async fn fetch_recorded_samples() -> Vec<f32> {
    match crate::tauri_bridge::tauri_invoke_no_args("mic_take_recorded_samples").await {
        Ok(val) => val
            .dyn_into::<js_sys::ArrayBuffer>()
            .ok()
            .map(|buf| js_sys::Float32Array::new(&buf).to_vec())
            .unwrap_or_default(),
        Err(e) => {
            log::warn!("mic_take_recorded_samples failed: {}", e);
            Vec::new()
        }
    }
}

// ── Live listen DSP dispatch ──────────────────────────────────────────

/// Absolute minimum context for PS/PV (must cover at least one PV FFT window).
const LISTEN_CONTEXT_FLOOR: usize = 4096;

/// Crossfade length (samples) between consecutive output chunks to
/// eliminate any residual boundary discontinuity after overlap-save.
const LISTEN_CROSSFADE: usize = 256;

/// How many recent input samples to prepend to each chunk before applying
/// the bandpass/EQ filter. The IIR filters are stateless across calls, so
/// without warmup the per-chunk transients click at chunk boundaries.
const FILTER_WARMUP_SAMPLES: usize = 1024;

/// Persistent state for overlap-save PS/PV live processing.
struct ListenDspState {
    /// Accumulated raw input samples (sliding context window).
    context: Vec<f32>,
    /// Tail of the previous returned chunk, used for crossfade.
    prev_tail: Vec<f32>,
    /// Recent input samples kept around to seed the IIR bandpass filter
    /// at the start of each chunk so its transient settles before the
    /// audible portion. Up to FILTER_WARMUP_SAMPLES long.
    filter_tail: Vec<f32>,
}

impl ListenDspState {
    const fn new() -> Self {
        Self {
            context: Vec::new(),
            prev_tail: Vec::new(),
            filter_tail: Vec::new(),
        }
    }

    fn clear(&mut self) {
        self.context.clear();
        self.prev_tail.clear();
        self.filter_tail.clear();
    }
}

/// Apply the bandpass/EQ filter step with warmup-tail continuity so IIR
/// transients don't click at chunk boundaries. Returns input unchanged when
/// no filter is active.
fn apply_live_filter(
    input: &[f32],
    sample_rate: u32,
    params: &PlaybackParams,
    dsp_state: &mut ListenDspState,
) -> Vec<f32> {
    // Fast path: no filter / notch / spectral subtraction enabled at all.
    if !params.filter_enabled && !params.notch_enabled && !params.noise_reduce_enabled {
        // Still update the warmup tail in case the filter gets enabled later.
        let take = input.len().min(FILTER_WARMUP_SAMPLES);
        dsp_state.filter_tail = input[input.len() - take..].to_vec();
        return input.to_vec();
    }

    let warmup_len = dsp_state.filter_tail.len().min(FILTER_WARMUP_SAMPLES);
    let mut buf = Vec::with_capacity(warmup_len + input.len());
    if warmup_len > 0 {
        let start = dsp_state.filter_tail.len() - warmup_len;
        buf.extend_from_slice(&dsp_state.filter_tail[start..]);
    }
    buf.extend_from_slice(input);

    let filtered = apply_filters(&buf, sample_rate, params);

    // Save the new tail (raw input, not filtered — we want the IIR transient
    // recomputed with each chunk's actual recent past).
    let take = input.len().min(FILTER_WARMUP_SAMPLES);
    dsp_state.filter_tail = input[input.len() - take..].to_vec();

    if filtered.len() > warmup_len {
        filtered[warmup_len..].to_vec()
    } else {
        filtered
    }
}

/// Apply the live DSP pipeline: bandpass/EQ filter → playback-mode transform.
///
/// Reads everything from the unified HFR signals via `params`. `mute_output`
/// short-circuits to silence (used for "Mic warm-up / Ready" state). Modes
/// that don't apply to live audio (TimeExpansion) fall through to passthrough.
fn process_live_audio(
    input: &[f32],
    sample_rate: u32,
    rt_het: &mut RealtimeCombHet,
    dsp_state: &mut ListenDspState,
    context_samples: usize,
    params: &PlaybackParams,
    mute_output: bool,
) -> Vec<f32> {
    if mute_output {
        dsp_state.clear();
        return vec![0.0f32; input.len()];
    }

    let filtered = apply_live_filter(input, sample_rate, params, dsp_state);

    let mut result = match params.mode {
        PlaybackMode::Heterodyne => {
            dsp_state.context.clear();
            dsp_state.prev_tail.clear();
            let carriers = crate::audio::streaming_playback::het_carriers(
                params.het_freq,
                params.het_comb_spacing,
                params.het_comb_count,
            );
            let mut out = vec![0.0f32; filtered.len()];
            rt_het.process(
                &filtered,
                &mut out,
                sample_rate,
                &carriers,
                params.het_cutoff,
            );
            out
        }
        PlaybackMode::PitchShift | PlaybackMode::PhaseVocoder => {
            // Accumulate input into sliding context window
            dsp_state.context.extend_from_slice(&filtered);
            let max_ctx = LISTEN_CONTEXT_FLOOR.max(context_samples);
            if dsp_state.context.len() > max_ctx {
                let excess = dsp_state.context.len() - max_ctx;
                dsp_state.context.drain(..excess);
            }

            // Run the batch DSP on the full context
            let full_output = if params.mode == PlaybackMode::PitchShift {
                pitch_shift_realtime(&dsp_state.context, params.ps_factor)
            } else {
                let mut out = phase_vocoder_pitch_shift(&dsp_state.context, params.pv_factor);
                let boost =
                    10.0f32.powf(crate::audio::streaming_playback::PV_MODE_BOOST_DB as f32 / 20.0);
                for s in &mut out {
                    *s *= boost;
                }
                out
            };

            // Extract the last chunk_len samples (deep interior of OLA = clean)
            let chunk_len = input.len();
            let extracted = if full_output.len() >= chunk_len {
                &full_output[full_output.len() - chunk_len..]
            } else {
                &full_output[..]
            };
            let mut result = extracted.to_vec();

            // Crossfade start of this chunk with end of previous chunk
            let fade = LISTEN_CROSSFADE
                .min(result.len())
                .min(dsp_state.prev_tail.len());
            for i in 0..fade {
                let t = i as f32 / fade as f32; // 0→1
                result[i] = dsp_state.prev_tail[i] * (1.0 - t) + result[i] * t;
            }

            // Save tail of current result for next crossfade
            dsp_state.prev_tail.clear();
            let tail_start = result.len().saturating_sub(LISTEN_CROSSFADE);
            dsp_state.prev_tail.extend_from_slice(&result[tail_start..]);

            result
        }
        PlaybackMode::ZeroCrossing => {
            dsp_state.context.clear();
            dsp_state.prev_tail.clear();
            zc_divide(
                &filtered,
                sample_rate,
                params.zc_factor as u32,
                params.filter_enabled,
            )
        }
        // Normal and TimeExpansion both pass through. TE is unimplementable
        // for live audio (it relies on the AudioContext sample rate change,
        // which would buffer indefinitely). The UI shows a toast when TE is
        // selected during live listening.
        PlaybackMode::Normal | PlaybackMode::TimeExpansion => {
            dsp_state.context.clear();
            dsp_state.prev_tail.clear();
            filtered
        }
    };

    // Live monitoring gain — separate from playback gain so the user can
    // tune live volume independently. AGC/AutoPeak don't apply here (no
    // file to scan, and a live AGC pass would be a future feature).
    apply_gain(&mut result, params.live_gain_db);
    result
}

// ── Public API on MicBackend ─────────────────────────────────────────

impl MicBackend {
    /// Check if this backend's mic is currently open.
    pub fn is_open(&self) -> bool {
        match self {
            MicBackend::Browser => MIC_CTX.with(|c| c.borrow().is_some()),
            MicBackend::Cpal => NATIVE_MIC_OPEN.with(|o| *o.borrow() == Some(NativeMode::Cpal)),
            MicBackend::RawUsb => NATIVE_MIC_OPEN.with(|o| *o.borrow() == Some(NativeMode::Usb)),
        }
    }

    /// Clear the live sample buffer for this backend.
    pub fn clear_buffer(&self) {
        match self {
            MicBackend::Browser => MIC_BUFFER.with(|buf| buf.borrow_mut().clear()),
            MicBackend::Cpal | MicBackend::RawUsb => {
                NATIVE_REC_BUFFER.with(|buf| buf.borrow_mut().clear());
            }
        }
    }

    /// Reset DSP state (heterodyne, phase vocoder, listen overlap buffers)
    /// to prevent stale audio from a previous session leaking into a new one.
    pub fn clear_dsp_state(&self) {
        match self {
            MicBackend::Browser => {
                WEB_RT_HET.with(|h| h.borrow_mut().reset());
                WEB_LISTEN_STATE.with(|s| s.borrow_mut().clear());
            }
            MicBackend::Cpal | MicBackend::RawUsb => {
                NATIVE_RT_HET.with(|h| h.borrow_mut().reset());
                NATIVE_LISTEN_STATE.with(|s| s.borrow_mut().clear());
            }
        }
    }

    /// Open the mic. Returns true on success.
    pub async fn open(&self, state: &AppState) -> bool {
        match self {
            MicBackend::Browser => open_web(state).await,
            MicBackend::Cpal => open_cpal(state).await,
            MicBackend::RawUsb => open_usb(state).await,
        }
    }

    /// Close the mic unconditionally.
    pub async fn close(&self, state: &AppState) {
        match self {
            MicBackend::Browser => close_web(state),
            MicBackend::Cpal => close_cpal(state).await,
            MicBackend::RawUsb => close_usb(state).await,
        }
    }

    /// Close only if not recording and not listening.
    pub async fn maybe_close(&self, state: &AppState) {
        if !state.mic.listening().get_untracked() && !state.mic.recording().get_untracked() {
            self.close(state).await;
        }
    }

    /// Signal the backend to start recording. For browser mode this is a no-op
    /// because the ScriptProcessorNode callback is already accumulating samples.
    /// On mobile Tauri, acquires a ContentResolver fd for direct shared storage write.
    pub async fn start_recording(&self, state: &AppState) -> Result<(), String> {
        match self {
            MicBackend::Browser => Ok(()),
            MicBackend::Cpal => {
                let fd = try_create_shared_fd(state).await;
                tauri_invoke_args(
                    "mic_start_recording",
                    &build_start_recording_args(state, fd),
                )
                .await
            }
            MicBackend::RawUsb => {
                let fd = try_create_shared_fd(state).await;
                tauri_invoke_args(
                    "usb_start_recording",
                    &build_start_recording_args(state, fd),
                )
                .await
            }
        }
    }

    /// Stop recording and return the result. For browser mode, extracts samples
    /// from the JS callback buffer. For native modes, calls the Tauri command.
    pub async fn stop_recording(&self, state: &AppState) -> StopResult {
        match self {
            MicBackend::Browser => {
                state.mic.recording().set(false);
                state.mic.recording_start_time().set(None);
                let sample_rate = state.mic.sample_rate().get_untracked();
                let samples = MIC_BUFFER.with(|buf| std::mem::take(&mut *buf.borrow_mut()));
                state.mic.samples_recorded().set(0);
                if samples.is_empty() || sample_rate == 0 {
                    log::warn!("No samples recorded (web)");
                    StopResult::Empty
                } else {
                    log::info!(
                        "Recording stopped: {} samples ({:.2}s at {} Hz)",
                        samples.len(),
                        samples.len() as f64 / sample_rate as f64,
                        sample_rate
                    );
                    StopResult::Samples {
                        samples,
                        sample_rate,
                    }
                }
            }
            MicBackend::Cpal => {
                match tauri_invoke_typed_args::<_, oversample_ipc::mic::RecordingResult>(
                    "mic_stop_recording",
                    &build_stop_recording_args(state),
                )
                .await
                {
                    Ok(result) => match TauriRecordingResult::from_result(result).await {
                        Some(r) => {
                            if r.saved_path.starts_with("shared://") {
                                finalize_shared_entry().await;
                            }
                            StopResult::TauriResult(r)
                        }
                        None => {
                            cancel_shared_entry().await;
                            StopResult::Empty
                        }
                    },
                    Err(e) => {
                        cancel_shared_entry().await;
                        StopResult::Error(e)
                    }
                }
            }
            MicBackend::RawUsb => {
                match tauri_invoke_typed_args::<_, oversample_ipc::mic::RecordingResult>(
                    "usb_stop_recording",
                    &build_stop_recording_args(state),
                )
                .await
                {
                    Ok(result) => match TauriRecordingResult::from_result(result).await {
                        Some(r) => {
                            if r.saved_path.starts_with("shared://") {
                                finalize_shared_entry().await;
                            }
                            StopResult::TauriResult(r)
                        }
                        None => {
                            cancel_shared_entry().await;
                            StopResult::Empty
                        }
                    },
                    Err(e) => {
                        cancel_shared_entry().await;
                        StopResult::Error(e)
                    }
                }
            }
        }
    }

    /// Enable or disable live listening. For browser mode this is a no-op
    /// (the ScriptProcessorNode callback checks the signal). For cpal, issues
    /// the `mic_set_listening` command. For USB, no backend command needed.
    pub async fn set_listening(&self, _state: &AppState, enabled: bool) {
        match self {
            MicBackend::Browser => { /* callback checks mic_listening signal */ }
            MicBackend::Cpal => {
                let _ = tauri_invoke_args(
                    "mic_set_listening",
                    &oversample_ipc::mic::SetListeningArgs { listening: enabled },
                )
                .await;
            }
            MicBackend::RawUsb => { /* USB streams continuously once open */ }
        }
    }
}

// ── Public helpers ──────────────────────────────────────────────────────

thread_local! {
    /// Absolute sample index of the live capture ring's first sample — i.e. the
    /// total samples trimmed off the front so far. The live processing loop keeps
    /// this in sync with its hop-aligned trims, letting the waveform map ring
    /// samples to absolute positions WITHOUT the (lagging, fluctuating) waterfall
    /// processed clock — which otherwise made the live waveform jump.
    static LIVE_RING_BASE: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Set the live ring's absolute base sample index (see [`live_ring_base`]).
pub fn set_live_ring_base(base: u64) {
    LIVE_RING_BASE.with(|c| c.set(base));
}

/// Absolute sample index of the live capture ring's first sample.
pub fn live_ring_base() -> u64 {
    LIVE_RING_BASE.with(|c| c.get())
}

/// Borrow the live recording buffer and call `f` with a reference to the samples.
/// Works for both web (MIC_BUFFER) and Tauri (NATIVE_REC_BUFFER) modes.
pub fn with_live_samples<R>(is_tauri: bool, f: impl FnOnce(&[f32]) -> R) -> R {
    if is_tauri {
        NATIVE_REC_BUFFER.with(|buf| f(&buf.borrow()))
    } else {
        MIC_BUFFER.with(|buf| f(&buf.borrow()))
    }
}

/// Borrow the live recording buffer mutably (e.g. for circular-buffer trimming).
pub fn with_live_samples_mut<R>(is_tauri: bool, f: impl FnOnce(&mut Vec<f32>) -> R) -> R {
    if is_tauri {
        NATIVE_REC_BUFFER.with(|buf| f(&mut buf.borrow_mut()))
    } else {
        MIC_BUFFER.with(|buf| f(&mut buf.borrow_mut()))
    }
}

/// Extract samples from the native buffer (for error-path finalization).
pub fn take_native_buffer() -> Vec<f32> {
    NATIVE_REC_BUFFER.with(|buf| std::mem::take(&mut *buf.borrow_mut()))
}

// ── ContentResolver fd-passing (mobile) ─────────────────────────────────

/// On mobile Tauri, ask the MediaStore plugin to create a pending recording entry
/// and return the raw POSIX fd. Returns `None` on non-mobile, or if the call fails.
async fn try_create_shared_fd(state: &AppState) -> Option<i32> {
    if !state.status.is_mobile().get_untracked() {
        return None;
    }
    // Build filename from the live file if available, otherwise generate one
    let filename = state
        .mic
        .live_file_idx()
        .get_untracked()
        .and_then(|idx| {
            state
                .library
                .files()
                .with_untracked(|f| f.get(idx).map(|f| f.name.clone()))
        })
        .unwrap_or_else(|| {
            let now = js_sys::Date::new_0();
            format!(
                "batcap_{:04}{:02}{:02}_{:02}{:02}{:02}.wav",
                now.get_full_year(),
                now.get_month() + 1,
                now.get_date(),
                now.get_hours(),
                now.get_minutes(),
                now.get_seconds(),
            )
        });

    match tauri_invoke_typed_args::<_, oversample_ipc::plugins::CreateRecordingEntryResult>(
        "plugin:media-store|createRecordingEntry",
        &oversample_ipc::plugins::CreateRecordingEntryArgs {
            filename: filename.clone(),
        },
    )
    .await
    {
        Ok(result) => {
            log::info!(
                "Got shared storage fd={} uri={} for {}",
                result.fd,
                result.uri,
                filename
            );
            // Keep the content:// URI so the finalizer can read the saved file
            // back for display (the recording lives only in public storage).
            state.mic.pending_shared_uri().set(Some(result.uri));
            Some(result.fd)
        }
        Err(e) => {
            log::warn!(
                "createRecordingEntry failed (will fall back to internal storage): {}",
                e
            );
            state.mic.pending_shared_uri().set(None);
            None
        }
    }
}

/// Tell the MediaStore plugin to finalize (set IS_PENDING=0) the recording entry.
async fn finalize_shared_entry() {
    let args = js_sys::Object::new();
    match tauri_invoke("plugin:media-store|finalizeRecordingEntry", &args.into()).await {
        Ok(_) => log::info!("Finalized shared storage recording entry"),
        Err(e) => log::warn!("finalizeRecordingEntry failed: {}", e),
    }
}

/// Cancel a pending MediaStore recording entry (delete the IS_PENDING=1 row).
/// Called on error paths to avoid orphaned entries. Safe to call when no entry
/// is pending (no-op on the Kotlin side).
pub(crate) async fn cancel_shared_entry() {
    let args = js_sys::Object::new();
    match tauri_invoke("plugin:media-store|cancelRecordingEntry", &args.into()).await {
        Ok(_) => log::info!("Cancelled pending shared storage entry"),
        Err(e) => log::warn!("cancelRecordingEntry failed: {}", e),
    }
}

// ── Tauri event listeners (private) ─────────────────────────────────────

/// Subscribe to a USB stream error event (separate thread-local from tauri_listen).
fn tauri_listen_usb_error(event_name: &str, callback: Closure<dyn FnMut(JsValue)>) -> Option<()> {
    let tauri = get_tauri_internals()?;

    let transform_fn =
        js_sys::Reflect::get(&tauri, &JsValue::from_str("transformCallback")).ok()?;
    let transform_fn = js_sys::Function::from(transform_fn);
    let handler_id = transform_fn
        .call1(&tauri, callback.as_ref().unchecked_ref())
        .ok()?;

    let invoke_fn = js_sys::Reflect::get(&tauri, &JsValue::from_str("invoke")).ok()?;
    let invoke_fn = js_sys::Function::from(invoke_fn);

    let args = js_sys::Object::new();
    js_sys::Reflect::set(&args, &"event".into(), &JsValue::from_str(event_name)).ok();
    let target = js_sys::Object::new();
    js_sys::Reflect::set(&target, &"kind".into(), &JsValue::from_str("Any")).ok();
    js_sys::Reflect::set(&args, &"target".into(), &target).ok();
    js_sys::Reflect::set(&args, &"handler".into(), &handler_id).ok();

    invoke_fn
        .call2(&tauri, &JsValue::from_str("plugin:event|listen"), &args)
        .ok();

    USB_ERROR_CLOSURE.with(|c| *c.borrow_mut() = Some(callback));
    Some(())
}

// ── Shared native helpers (used by both cpal and USB) ───────────────────

/// Create and resume the HET playback AudioContext, resetting the scheduling state.
async fn setup_het_context(state: &AppState) -> bool {
    let het_ctx = match AudioContext::new() {
        Ok(c) => c,
        Err(e) => {
            log::error!("Failed to create HET AudioContext: {:?}", e);
            state
                .status
                .message()
                .set(Some("Failed to initialize audio output".into()));
            return false;
        }
    };
    if let Ok(promise) = het_ctx.resume() {
        let _ = JsFuture::from(promise).await;
    }
    HET_CTX.with(|c| *c.borrow_mut() = Some(het_ctx));
    HET_NEXT_TIME.with(|t| *t.borrow_mut() = 0.0);
    HET_SOURCES.with(|s| s.borrow_mut().clear());
    NATIVE_RT_HET.with(|h| h.borrow_mut().reset());
    NATIVE_LISTEN_STATE.with(|s| s.borrow_mut().clear());
    true
}

/// Stop and drop all tracked live-playback sources. Does not touch the schedule
/// cursor — used internally by the skip-ahead path (which resets the cursor via
/// the normal scheduling math) and by `stop_het_playback`.
// `AudioBufferSourceNode::stop_with_when` is marked deprecated in web-sys (the
// canonical method lives on the AudioScheduledSourceNode parent), but it's the
// available binding and works; the parallel `start_with_when` we already use is
// not flagged. Allow it rather than pulling in another web-sys feature.
#[allow(deprecated)]
fn stop_het_sources() {
    HET_SOURCES.with(|sources| {
        for (source, _end) in sources.borrow_mut().drain(..) {
            // `when = 0` is in the past, so playback stops immediately.
            let _ = source.stop_with_when(0.0);
            let _ = source.disconnect();
        }
    });
}

/// Stop all scheduled live playback immediately and reset the schedule cursor.
/// Call on every stop/disable path and at each listen start so a previous
/// session's backlog can neither keep sounding nor carry over into a new one.
pub fn stop_het_playback() {
    stop_het_sources();
    HET_NEXT_TIME.with(|t| *t.borrow_mut() = 0.0);
}

/// Resume the live-playback (HET) and browser-mic AudioContexts if the OS
/// suspended or interrupted them (e.g. after the app was backgrounded). Safe to
/// call when they're already running or absent.
pub fn resume_playback_context() {
    HET_CTX.with(|c| {
        if let Some(ctx) = c.borrow().as_ref() {
            if ctx.state() != web_sys::AudioContextState::Running {
                let _ = ctx.resume();
            }
        }
    });
    MIC_CTX.with(|c| {
        if let Some(ctx) = c.borrow().as_ref() {
            if ctx.state() != web_sys::AudioContextState::Running {
                let _ = ctx.resume();
            }
        }
    });
}

/// Read and reset the live-playback skip-ahead counter. A nonzero value means
/// playback fell behind and had to drop a backlog (throttling signal).
pub fn take_het_skip_count() -> u32 {
    HET_SKIP_COUNT.with(|c| {
        let n = c.get();
        c.set(0);
        n
    })
}

/// Current time of the live-playback (HET) AudioContext, if one exists. The
/// background watchdog compares this across a hide/show interval to tell whether
/// audible output kept advancing (vs. the OS having suspended the context).
pub fn het_context_time() -> Option<f64> {
    HET_CTX.with(|c| c.borrow().as_ref().map(|ctx| ctx.current_time()))
}

/// Process one chunk of native mic samples: accumulate them in
/// NATIVE_REC_BUFFER for the live waterfall and, while listening, run them
/// through the DSP and schedule HET playback. Called from the frontend audio
/// pull loop (`start_audio_pull_loop`) with samples drained from the native
/// side as raw f32 bytes (replacing the old JSON `mic-audio-chunk` event).
fn process_native_chunk(state_cb: AppState, input_data: Vec<f32>) {
    let len = input_data.len();
    if len == 0 {
        return;
    }

    // Accumulate samples for live waterfall display during recording OR listening
    if state_cb.mic.recording().get_untracked() || state_cb.mic.listening().get_untracked() {
        NATIVE_REC_BUFFER.with(|buf| buf.borrow_mut().extend_from_slice(&input_data));
        if state_cb.mic.recording().get_untracked() {
            state_cb.mic.samples_recorded().update(|n| *n += len);
        }
    }

    // Listen mode: process input through selected DSP and play through speakers
    if state_cb.mic.listening().get_untracked() {
        let sr = state_cb.mic.sample_rate().get_untracked();
        let params = snapshot_params(&state_cb, None, sr);
        let mute = state_cb.mic.mute_output().get_untracked();
        let ctx_samples = state_cb.mic.listen_context_samples().get_untracked();
        let out_data = NATIVE_RT_HET.with(|h| {
            NATIVE_LISTEN_STATE.with(|s| {
                process_live_audio(
                    &input_data,
                    sr,
                    &mut h.borrow_mut(),
                    &mut s.borrow_mut(),
                    ctx_samples,
                    &params,
                    mute,
                )
            })
        });

        // Schedule playback via AudioBuffer with bounded look-ahead so a
        // backgrounded-then-resumed burst of queued chunks can't build a
        // multi-second backlog (which would lag real time and keep sounding
        // after Stop). See `plan_live_schedule`.
        let out_len = out_data.len();
        HET_CTX.with(|ctx_cell| {
            let ctx_ref = ctx_cell.borrow();
            let Some(ctx) = ctx_ref.as_ref() else { return };

            // If the OS suspended/interrupted the context (e.g. the app was
            // backgrounded), its clock is frozen — scheduling against it would
            // pile up and never play. Kick a resume and drop this chunk;
            // subsequent chunks schedule normally once it's running again.
            if ctx.state() != web_sys::AudioContextState::Running {
                let _ = ctx.resume();
                return;
            }

            let current_time = ctx.current_time();
            let next_time = HET_NEXT_TIME.with(|t| *t.borrow());
            let decision = plan_live_schedule(current_time, next_time, DEFAULT_MAX_LOOKAHEAD_SECS);

            if decision.dropped_backlog {
                // Fell behind (throttled): stop the stale scheduled tail so it
                // never sounds, jump to "now", and count the skip. Within a
                // synchronous burst the context clock is frozen, so sources
                // stopped before control returns never produce audio — the
                // audible result is "jump to the newest chunk ≈ now".
                stop_het_sources();
                HET_SKIP_COUNT.with(|c| c.set(c.get().saturating_add(1)));
            }

            let Ok(buffer) = ctx.create_buffer(1, out_len as u32, sr as f32) else {
                return;
            };
            let _ = buffer.copy_to_channel(&out_data, 0);
            let Ok(source) = ctx.create_buffer_source() else {
                return;
            };
            source.set_buffer(Some(&buffer));
            let _ = source.connect_with_audio_node(&ctx.destination());

            let start = decision.start;
            let _ = source.start_with_when(start);

            let duration = out_len as f64 / sr as f64;
            let end_time = start + duration;
            HET_NEXT_TIME.with(|t| *t.borrow_mut() = end_time);

            // Track the source so Stop can silence it instantly; drop refs to
            // ones that have already finished playing.
            HET_SOURCES.with(|sources| {
                let mut v = sources.borrow_mut();
                v.retain(|(_, end)| *end > current_time);
                v.push((source, end_time));
            });
        });
    }
}

/// Target wall-clock interval between native audio pulls. Matches the old 80 ms
/// event-emit cadence so each chunk holds ~the same number of samples — heavy
/// modes (pitch-shift / phase-vocoder) were tuned around that chunk size, and
/// smaller/more-frequent chunks raised their per-chunk overhead enough to stutter.
const AUDIO_PULL_INTERVAL_MS: f64 = 80.0;
/// Floor on the post-processing sleep so the loop can't busy-spin when the DSP
/// nearly fills (or exceeds) the interval.
const AUDIO_PULL_MIN_SLEEP_MS: f64 = 5.0;

/// Drive the native audio stream by polling `mic_pull_audio` for raw f32 bytes
/// (an ArrayBuffer) and feeding each chunk to [`process_native_chunk`]. Runs
/// until the mic closes (NATIVE_MIC_OPEN cleared) or a newer loop supersedes it.
/// Replaces the JSON `mic-audio-chunk` event listener.
fn start_audio_pull_loop(state: AppState) {
    let gen = NATIVE_PULL_GEN.with(|c| {
        let g = c.get().wrapping_add(1);
        c.set(g);
        g
    });
    wasm_bindgen_futures::spawn_local(async move {
        let mut poll_tick = 0u32;
        loop {
            // Superseded by a newer loop, or mic closed → exit.
            if NATIVE_PULL_GEN.with(|c| c.get()) != gen {
                break;
            }
            let Some(mode) = NATIVE_MIC_OPEN.with(|o| *o.borrow()) else {
                break;
            };
            // Drain the source we actually opened, so a not-fully-torn-down
            // previous device can't shadow it.
            let source = if mode == NativeMode::Usb {
                "usb"
            } else {
                "cpal"
            };
            let t0 = js_sys::Date::now();
            let pull_args = js_sys::Object::new();
            let _ = js_sys::Reflect::set(
                &pull_args,
                &JsValue::from_str("source"),
                &JsValue::from_str(source),
            );
            match crate::tauri_bridge::tauri_invoke("mic_pull_audio", &pull_args.into()).await {
                Ok(val) => {
                    if let Ok(buf) = val.dyn_into::<js_sys::ArrayBuffer>() {
                        let samples = js_sys::Float32Array::new(&buf).to_vec();
                        if !samples.is_empty() {
                            process_native_chunk(state, samples);
                        }
                    }
                }
                Err(e) => log::warn!("mic_pull_audio failed: {}", e),
            }

            // ~once a second, poll the mic status for the auto-detected effective
            // bit depth and remember it per-device (persisted), so the chooser can
            // show "appears to be N-bit" now and ahead of time next session.
            poll_tick = poll_tick.wrapping_add(1);
            if poll_tick.is_multiple_of(12) {
                if let Ok(status) = crate::tauri_bridge::tauri_invoke_typed_no_args::<
                    oversample_ipc::mic::MicStatus,
                >("mic_get_status")
                .await
                {
                    if let (Some(bits), Some(dev)) = (
                        status.effective_bits,
                        state.mic.device_name().get_untracked(),
                    ) {
                        let changed = state
                            .mic
                            .bit_depths()
                            .with_untracked(|m| m.get(&dev) != Some(&bits));
                        if changed {
                            state.mic.bit_depths().update(|m| {
                                m.insert(dev, bits);
                            });
                            let map = state.mic.bit_depths().get_untracked();
                            crate::settings::set_mic_bit_depths(&map);
                        }
                    }
                }
            }

            // Hold a steady wall-clock cadence: subtract the time spent
            // pulling + running the DSP so a heavy chunk (e.g. pitch-shift /
            // phase-vocoder) doesn't push the next pull out and starve the
            // playback buffer. Under the old event-push the native side emitted
            // every 80 ms regardless of frontend processing time; this restores
            // that decoupling for the pull model.
            let elapsed = js_sys::Date::now() - t0;
            let remaining = (AUDIO_PULL_INTERVAL_MS - elapsed).max(AUDIO_PULL_MIN_SLEEP_MS);
            crate::web_util::sleep_ms(remaining as i32).await;
        }
    });
}

/// Clean up all native thread-local state (HET context, buffer).
fn cleanup_native_state() {
    // Stop any still-scheduled live playback before tearing down the context.
    stop_het_playback();

    HET_CTX.with(|c| {
        if let Some(ctx) = c.borrow_mut().take() {
            let _ = ctx.close();
        }
    });

    NATIVE_RT_HET.with(|h| h.borrow_mut().reset());
    NATIVE_LISTEN_STATE.with(|s| s.borrow_mut().clear());
    NATIVE_REC_BUFFER.with(|buf| buf.borrow_mut().clear());
    crate::canvas::live_waterfall::clear();
}

// ── Browser (Web Audio) backend ─────────────────────────────────────────

async fn open_web(state: &AppState) -> bool {
    if MIC_CTX.with(|c| c.borrow().is_some()) {
        return true;
    }

    state.log_debug("info", "open_web: opening browser mic...");

    let window = match web_sys::window() {
        Some(w) => w,
        None => {
            state.log_debug("error", "open_web: no window object");
            return false;
        }
    };
    let navigator = window.navigator();
    let media_devices = match navigator.media_devices() {
        Ok(md) => md,
        Err(e) => {
            state.log_debug("error", format!("open_web: no media devices: {:?}", e));
            state
                .status
                .message()
                .set(Some("Microphone not available on this device".into()));
            return false;
        }
    };

    let constraints = web_sys::MediaStreamConstraints::new();
    let audio_opts = js_sys::Object::new();
    js_sys::Reflect::set(&audio_opts, &"echoCancellation".into(), &JsValue::FALSE).ok();
    js_sys::Reflect::set(&audio_opts, &"noiseSuppression".into(), &JsValue::FALSE).ok();
    js_sys::Reflect::set(&audio_opts, &"autoGainControl".into(), &JsValue::FALSE).ok();
    // If a specific browser device was selected, constrain to that deviceId
    if let Some(device_id) = state.mic.selected_device().get_untracked() {
        if state.mic.backend().get_untracked() == Some(MicBackend::Browser) && !device_id.is_empty()
        {
            let exact = js_sys::Object::new();
            js_sys::Reflect::set(&exact, &"exact".into(), &JsValue::from_str(&device_id)).ok();
            js_sys::Reflect::set(&audio_opts, &"deviceId".into(), &exact.into()).ok();
        }
    }
    constraints.set_audio(&audio_opts.into());

    let promise = match media_devices.get_user_media_with_constraints(&constraints) {
        Ok(p) => p,
        Err(e) => {
            log::error!("getUserMedia failed: {:?}", e);
            state
                .status
                .message()
                .set(Some("Microphone not available".into()));
            return false;
        }
    };

    state.log_debug("info", "open_web: calling getUserMedia...");
    let stream_js = match JsFuture::from(promise).await {
        Ok(s) => {
            state.log_debug("info", "open_web: getUserMedia succeeded");
            s
        }
        Err(e) => {
            state.log_debug("error", format!("open_web: getUserMedia denied: {:?}", e));
            state
                .status
                .message()
                .set(Some("Microphone permission denied".into()));
            return false;
        }
    };

    let stream: web_sys::MediaStream = match stream_js.dyn_into() {
        Ok(s) => s,
        Err(_) => {
            log::error!("Failed to cast MediaStream");
            return false;
        }
    };

    let ctx = match AudioContext::new() {
        Ok(c) => c,
        Err(e) => {
            log::error!("Failed to create AudioContext: {:?}", e);
            state
                .status
                .message()
                .set(Some("Failed to initialize audio".into()));
            return false;
        }
    };

    if let Ok(promise) = ctx.resume() {
        let _ = JsFuture::from(promise).await;
    }

    let sample_rate = ctx.sample_rate() as u32;
    state.mic.sample_rate().set(sample_rate);
    // Only report a device name when the user explicitly selected a specific
    // device via the mic chooser (mic_selected_device is Some with a non-empty id).
    // "Browser default" and direct browser mode leave mic_device_name as None.
    let has_specific_device = state
        .mic
        .selected_device()
        .get_untracked()
        .as_ref()
        .is_some_and(|id| !id.is_empty());
    let dev_name = if has_specific_device {
        state
            .mic
            .device_info()
            .get_untracked()
            .map(|info| info.name.clone())
    } else {
        None
    };
    state.mic.device_name().set(dev_name);
    state.mic.manufacturer().set(None);
    state
        .mic
        .connection_type()
        .set(Some("Web Audio API".to_string()));
    let source = match ctx.create_media_stream_source(&stream) {
        Ok(s) => s,
        Err(e) => {
            log::error!("Failed to create MediaStreamSource: {:?}", e);
            return false;
        }
    };

    let processor = match ctx.create_script_processor_with_buffer_size_and_number_of_input_channels_and_number_of_output_channels(4096, 1, 1) {
        Ok(p) => p,
        Err(e) => {
            log::error!("Failed to create ScriptProcessorNode: {:?}", e);
            return false;
        }
    };

    if let Err(e) = source.connect_with_audio_node(&processor) {
        log::error!("Failed to connect source -> processor: {:?}", e);
        return false;
    }
    if let Err(e) = processor.connect_with_audio_node(&ctx.destination()) {
        log::error!("Failed to connect processor -> destination: {:?}", e);
        return false;
    }

    WEB_RT_HET.with(|h| h.borrow_mut().reset());
    WEB_LISTEN_STATE.with(|s| s.borrow_mut().clear());

    let state_cb = *state;
    let handler = Closure::<dyn FnMut(web_sys::AudioProcessingEvent)>::new(
        move |ev: web_sys::AudioProcessingEvent| {
            let input_buffer = match ev.input_buffer() {
                Ok(b) => b,
                Err(_) => return,
            };
            let output_buffer = match ev.output_buffer() {
                Ok(b) => b,
                Err(_) => return,
            };

            let input_data = match input_buffer.get_channel_data(0) {
                Ok(d) => d,
                Err(_) => return,
            };

            if state_cb.mic.listening().get_untracked() {
                let sr = state_cb.mic.sample_rate().get_untracked();
                let params = snapshot_params(&state_cb, None, sr);
                let mute = state_cb.mic.mute_output().get_untracked();
                let ctx_samples = state_cb.mic.listen_context_samples().get_untracked();
                let out_data = WEB_RT_HET.with(|h| {
                    WEB_LISTEN_STATE.with(|s| {
                        process_live_audio(
                            &input_data,
                            sr,
                            &mut h.borrow_mut(),
                            &mut s.borrow_mut(),
                            ctx_samples,
                            &params,
                            mute,
                        )
                    })
                });
                let _ = output_buffer.copy_to_channel(&out_data, 0);
            } else {
                let zeros = vec![0.0f32; input_data.len()];
                let _ = output_buffer.copy_to_channel(&zeros, 0);
            }

            // Accumulate samples for live waterfall display during recording OR listening
            if state_cb.mic.recording().get_untracked() || state_cb.mic.listening().get_untracked()
            {
                MIC_BUFFER.with(|buf| {
                    buf.borrow_mut().extend_from_slice(&input_data);
                    if state_cb.mic.recording().get_untracked() {
                        state_cb.mic.samples_recorded().set(buf.borrow().len());
                    }
                });
            }
        },
    );

    processor.set_onaudioprocess(Some(handler.as_ref().unchecked_ref()));

    MIC_CTX.with(|c| *c.borrow_mut() = Some(ctx));
    MIC_STREAM.with(|s| *s.borrow_mut() = Some(stream));
    MIC_PROCESSOR.with(|p| *p.borrow_mut() = Some(processor));
    MIC_HANDLER.with(|h| *h.borrow_mut() = Some(handler));

    log::info!("Web mic opened at {} Hz", sample_rate);
    true
}

fn close_web(state: &AppState) {
    MIC_STREAM.with(|s| {
        if let Some(stream) = s.borrow_mut().take() {
            let tracks = stream.get_tracks();
            for i in 0..tracks.length() {
                let track_js = tracks.get(i);
                if let Ok(track) = track_js.dyn_into::<web_sys::MediaStreamTrack>() {
                    track.stop();
                }
            }
        }
    });

    MIC_PROCESSOR.with(|p| {
        if let Some(proc) = p.borrow_mut().take() {
            proc.set_onaudioprocess(None);
            let _ = proc.disconnect();
        }
    });

    MIC_HANDLER.with(|h| {
        h.borrow_mut().take();
    });

    MIC_CTX.with(|c| {
        if let Some(ctx) = c.borrow_mut().take() {
            let _ = ctx.close();
        }
    });

    MIC_BUFFER.with(|buf| buf.borrow_mut().clear());
    WEB_RT_HET.with(|h| h.borrow_mut().reset());
    WEB_LISTEN_STATE.with(|s| s.borrow_mut().clear());
    crate::canvas::live_waterfall::clear();

    state.mic.samples_recorded().set(0);
    log::info!("Web mic closed");
}

// ── cpal (Tauri native) backend ─────────────────────────────────────────

async fn open_cpal(state: &AppState) -> bool {
    if NATIVE_MIC_OPEN.with(|o| *o.borrow() == Some(NativeMode::Cpal)) {
        return true;
    }

    let max_sr = state.mic.max_sample_rate().get_untracked();
    let max_bits = state.mic.max_bit_depth().get_untracked();
    let channel_mode = state.mic.channel_mode().get_untracked();
    let selected_device = state.mic.selected_device().get_untracked();
    let channels: u16 = {
        use crate::state::ChannelMode;
        match channel_mode {
            ChannelMode::Mono => 1,
            ChannelMode::Stereo => 2,
        }
    };
    let open_args = oversample_ipc::mic::MicOpenArgs {
        max_sample_rate: (max_sr > 0).then_some(max_sr),
        device_name: selected_device,
        max_bit_depth: (max_bits > 0).then_some(max_bits as u16),
        channels: Some(channels),
    };
    let info =
        match tauri_invoke_typed_args::<_, oversample_ipc::mic::MicInfo>("mic_open", &open_args)
            .await
        {
            Ok(v) => v,
            Err(e) => {
                log::warn!("Native mic failed: {}", e);
                state
                    .status
                    .message()
                    .set(Some(format!("Native mic unavailable: {}", e)));
                return false;
            }
        };

    let sample_rate = info.sample_rate;
    let bits_per_sample = info.bits_per_sample;
    let device_name = if info.device_name.is_empty() {
        "Unknown".to_string()
    } else {
        info.device_name
    };
    let host_label: Option<String> = (!info.host_name.is_empty()).then_some(info.host_name);
    let supported_rates = info.supported_sample_rates;
    if !supported_rates.is_empty() {
        state.mic.supported_rates().set(supported_rates);
    }

    state.mic.sample_rate().set(sample_rate);
    state.mic.bits_per_sample().set(bits_per_sample);
    state.mic.device_name().set(Some(device_name.clone()));
    state.mic.manufacturer().set(None);
    // Determine mic interface from the cpal host backend.
    // The host_label comes from the Tauri backend via mic_info.
    let conn_type = if device_name.to_lowercase().contains("usb") {
        "USB"
    } else if device_name.to_lowercase().contains("bluetooth")
        || device_name.to_lowercase().contains("bt ")
    {
        "Bluetooth"
    } else {
        // Use the audio host name for native audio interfaces
        host_label.as_deref().unwrap_or("Internal")
    };
    state.mic.connection_type().set(Some(conn_type.to_string()));

    // Setup HET playback AudioContext and chunk handler
    if !setup_het_context(state).await {
        return false;
    }

    // Mark open BEFORE starting the pull loop so its first tick sees the mic
    // as open (the loop exits when NATIVE_MIC_OPEN is cleared on close).
    NATIVE_MIC_OPEN.with(|o| *o.borrow_mut() = Some(NativeMode::Cpal));
    start_audio_pull_loop(*state);
    log::info!(
        "Native mic opened: {} at {} Hz, {}-bit",
        device_name,
        sample_rate,
        bits_per_sample
    );
    true
}

async fn close_cpal(state: &AppState) {
    if let Err(e) = tauri_invoke_no_args("mic_close").await {
        log::error!("mic_close failed: {}", e);
    }

    cleanup_native_state();
    NATIVE_MIC_OPEN.with(|o| *o.borrow_mut() = None);

    state.mic.samples_recorded().set(0);
    log::info!("Native mic closed");
}

// ── Raw USB backend ─────────────────────────────────────────────────────

async fn open_usb(state: &AppState) -> bool {
    if NATIVE_MIC_OPEN.with(|o| *o.borrow() == Some(NativeMode::Usb)) {
        return true;
    }

    // Step 1: list USB devices via the Kotlin plugin and pick the audio device.
    let list = match tauri_invoke_typed_no_args::<oversample_ipc::plugins::UsbDeviceListResult>(
        "plugin:usb-audio|listUsbDevices",
    )
    .await
    {
        Ok(v) => v,
        Err(e) => {
            log::warn!("USB device listing failed: {}", e);
            state.status.message().set(Some(format!("USB: {}", e)));
            return false;
        }
    };
    let Some(audio_dev) = list.devices.into_iter().find(|d| d.is_audio_device) else {
        state
            .status
            .message()
            .set(Some("No USB audio device found".into()));
        return false;
    };
    let device_name = audio_dev.device_name.clone();
    // `manufacturerName` is emitted by listUsbDevices (NOT by openUsbDevice), so
    // capture it here; the Kotlin "Unknown" placeholder counts as absent.
    let manufacturer_name = (!audio_dev.manufacturer_name.is_empty()
        && audio_dev.manufacturer_name != "Unknown")
        .then_some(audio_dev.manufacturer_name.clone());

    // Step 2: request permission if needed.
    if !audio_dev.has_permission {
        match tauri_invoke_typed_args::<_, oversample_ipc::plugins::UsbPermissionResult>(
            "plugin:usb-audio|requestUsbPermission",
            &oversample_ipc::plugins::UsbDeviceNameArgs {
                device_name: device_name.clone(),
            },
        )
        .await
        {
            Ok(result) => {
                if !result.granted {
                    state
                        .status
                        .message()
                        .set(Some("USB permission denied".into()));
                    return false;
                }
            }
            Err(e) => {
                state
                    .status
                    .message()
                    .set(Some(format!("USB permission error: {}", e)));
                return false;
            }
        }
    }

    // Step 3: open the device via the Kotlin plugin.
    let max_sr = state.mic.max_sample_rate().get_untracked();
    let info = match tauri_invoke_typed_args::<_, oversample_ipc::plugins::UsbOpenResult>(
        "plugin:usb-audio|openUsbDevice",
        &oversample_ipc::plugins::UsbOpenArgs {
            device_name: device_name.clone(),
            sample_rate: max_sr as i32,
        },
    )
    .await
    {
        Ok(v) => v,
        Err(e) => {
            state
                .status
                .message()
                .set(Some(format!("USB open failed: {}", e)));
            return false;
        }
    };

    let sample_rate = info.sample_rate as u32;
    // Friendly product name, falling back to the device path only if the device
    // reported no product string. Used for both the app display and the native
    // GUANO mic name so they match (the raw "/dev/bus/usb/…" path is not useful).
    let product_name = if info.product_name.is_empty() {
        device_name.clone()
    } else {
        info.product_name.clone()
    };
    if info.fd < 0 || info.endpoint_address == 0 || info.max_packet_size == 0 {
        state
            .status
            .message()
            .set(Some("USB device: invalid fd or endpoint".into()));
        return false;
    }

    // Step 4: start the USB stream in the Rust backend.
    let stream_args = oversample_ipc::plugins::UsbStartStreamArgs {
        fd: info.fd,
        endpoint_address: info.endpoint_address as u32,
        max_packet_size: info.max_packet_size as u32,
        sample_rate,
        num_channels: info.num_channels as u32,
        // Friendly name (not the /dev/bus/usb/… path) so the native GUANO mic
        // name matches what the app displays.
        device_name: product_name.clone(),
        interface_number: info.interface_number as u32,
        alternate_setting: info.alternate_setting as u32,
        uac_version: info.uac_version as u32,
    };
    if let Err(e) = tauri_invoke_args("usb_start_stream", &stream_args).await {
        state
            .status
            .message()
            .set(Some(format!("USB stream failed: {}", e)));
        let _ = tauri_invoke_no_args("plugin:usb-audio|closeUsbDevice").await;
        return false;
    }

    state.mic.sample_rate().set(sample_rate);
    // Fix: openUsbDevice emits `bitResolution`; the old code read `bitDepth`
    // (never present) so usb_bits was always stuck at the 16 default.
    let usb_bits = if info.bit_resolution > 0 {
        info.bit_resolution as u16
    } else {
        16
    };
    state.mic.bits_per_sample().set(usb_bits);

    // Setup HET playback AudioContext and chunk handler (same as cpal)
    if !setup_het_context(state).await {
        return false;
    }

    // Listen for USB stream errors (disconnect / ENODEV)
    let state_err = *state;
    let error_handler = Closure::<dyn FnMut(JsValue)>::new(move |event: JsValue| {
        let msg = js_sys::Reflect::get(&event, &JsValue::from_str("payload"))
            .ok()
            .and_then(|v| v.as_string())
            .unwrap_or_else(|| "USB stream error".into());

        state_err.log_debug("error", format!("USB stream error: {}", msg));
        state_err.show_error_toast(&msg);

        let was_recording = state_err.mic.recording().get_untracked();
        state_err.mic.recording().set(false);
        state_err.mic.recording_start_time().set(None);
        state_err.mic.listening().set(false);
        state_err.mic.usb_connected().set(false);
        state_err.mic.backend().set(None);
        state_err
            .mic
            .acquisition_state()
            .set(MicAcquisitionState::Failed);

        NATIVE_MIC_OPEN.with(|o| *o.borrow_mut() = None);

        // Cancel any pending shared storage entry (fd was never fully written)
        wasm_bindgen_futures::spawn_local(async { cancel_shared_entry().await });

        // Finalize any in-progress recording with whatever samples we have
        if was_recording {
            let sr = state_err.mic.sample_rate().get_untracked();
            let samples = take_native_buffer();
            if !samples.is_empty() && sr > 0 {
                crate::audio::live_recording::finalize_recording(
                    crate::audio::live_recording::FinalizeParams {
                        samples,
                        sample_rate: sr,
                        bits_per_sample: state_err.mic.bits_per_sample().get_untracked(),
                        is_float: false,
                        saved_path: String::new(),
                        file_size: None,
                    },
                    state_err,
                );
            }
        }

        // Clean up HET context (stop scheduled playback first)
        stop_het_playback();
        HET_CTX.with(|c| {
            if let Some(ctx) = c.borrow_mut().take() {
                let _ = ctx.close();
            }
        });
        NATIVE_RT_HET.with(|h| h.borrow_mut().reset());
        NATIVE_LISTEN_STATE.with(|s| s.borrow_mut().clear());
        NATIVE_REC_BUFFER.with(|buf| buf.borrow_mut().clear());
    });
    tauri_listen_usb_error("usb-stream-error", error_handler);

    // Mark open before starting the pull loop (see the cpal path).
    NATIVE_MIC_OPEN.with(|o| *o.borrow_mut() = Some(NativeMode::Usb));
    start_audio_pull_loop(*state);
    state.mic.device_name().set(Some(product_name.clone()));
    state.mic.manufacturer().set(manufacturer_name);
    state
        .mic
        .connection_type()
        .set(Some("USB (Raw)".to_string()));
    log::info!("USB mic opened: {} at {} Hz", product_name, sample_rate);
    true
}

async fn close_usb(state: &AppState) {
    if let Err(e) = tauri_invoke_no_args("usb_stop_stream").await {
        log::error!("usb_stop_stream failed: {}", e);
    }

    let _ = tauri_invoke(
        "plugin:usb-audio|closeUsbDevice",
        &js_sys::Object::new().into(),
    )
    .await;

    // Also clean up USB error closure
    USB_ERROR_CLOSURE.with(|c| {
        c.borrow_mut().take();
    });

    cleanup_native_state();
    NATIVE_MIC_OPEN.with(|o| *o.borrow_mut() = None);

    state.mic.samples_recorded().set(0);
    log::info!("USB mic closed");
}
