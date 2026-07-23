use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
pub use oversample_ipc::mic::{DeviceInfo, MicInfo, MicStatus, RecordingResult, SampleRateRange};
use serde::Serialize;
use std::collections::VecDeque;
use std::io::Cursor;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// How many seconds of audio the listening pre-roll ring retains. A long-press
/// record seeds up to this much preceding audio into the recording. Capped to
/// bound memory while listening (e.g. ~30 MB at 384 kHz i32 mono).
const PREROLL_RING_SECS: usize = 15;

#[derive(Clone, Copy, Debug, Serialize)]
#[allow(dead_code)]
pub enum NativeSampleFormat {
    I16,
    I24,
    I32,
    F32,
}

impl NativeSampleFormat {
    pub fn bits_per_sample(self) -> u16 {
        match self {
            Self::I16 => 16,
            Self::I24 => 24,
            Self::I32 => 32,
            Self::F32 => 32,
        }
    }

    pub fn is_float(self) -> bool {
        matches!(self, Self::F32)
    }
}

/// Thread-safe sample storage that keeps raw samples in their native format.
pub struct RecordingBuffer {
    pub format: NativeSampleFormat,
    pub sample_rate: u32,
    // Native-format storage (only one is active, based on format)
    pub samples_i16: Vec<i16>,
    pub samples_i32: Vec<i32>, // for I24 and I32
    pub samples_f32: Vec<f32>, // for F32 format (native)
    // f32 copies for streaming to frontend
    pub pending_f32: Vec<f32>,
    pub total_samples: usize,
    /// Raw POSIX fd for writing directly to shared storage (Android ContentResolver).
    /// Set before recording starts, consumed on stop.
    pub shared_fd: Option<i32>,
    /// Effective-bit-depth detection (I24/I32 streams only). `detect_or` is the
    /// bitwise OR of every raw i32 sample seen on the current stream; the count
    /// of low bits that are ALWAYS zero across all samples (its trailing zeros)
    /// is the number of unused LSBs, so `container_bits - trailing_zeros` is the
    /// effective depth — e.g. a 24-bit interface delivering 24-bit data in a
    /// 32-bit container leaves the low 8 bits zero (effective 24). Reset when a
    /// new stream opens. `detect_samples` gates on having enough data.
    pub detect_or: i32,
    pub detect_samples: u64,
    /// Per-device user override of the output bit depth (12/16/24/32). When set,
    /// the recording is written at this effective depth instead of the
    /// auto-detected one. `None` = Auto (use detection).
    pub force_bits: Option<u16>,
    /// Rolling pre-roll ring (native format), filled only while listening
    /// (`is_streaming && !is_recording`) and capped to [`PREROLL_RING_SECS`].
    /// Seeded into the recording buffer at record start so a long-press capture
    /// includes the preceding audio, written linearly to disk like the rest.
    /// Only the ring matching `format` is used.
    pub preroll_i16: VecDeque<i16>,
    pub preroll_i32: VecDeque<i32>,
    pub preroll_f32: VecDeque<f32>,
    /// Number of pre-roll samples seeded at the last record start (for GUANO /
    /// cue marker). Reset on `clear()`.
    pub preroll_seeded: usize,
}

impl RecordingBuffer {
    pub fn new(format: NativeSampleFormat, sample_rate: u32) -> Self {
        Self {
            format,
            sample_rate,
            samples_i16: Vec::new(),
            samples_i32: Vec::new(),
            samples_f32: Vec::new(),
            pending_f32: Vec::new(),
            total_samples: 0,
            shared_fd: None,
            detect_or: 0,
            detect_samples: 0,
            force_bits: None,
            preroll_i16: VecDeque::new(),
            preroll_i32: VecDeque::new(),
            preroll_f32: VecDeque::new(),
            preroll_seeded: 0,
        }
    }

    /// Max pre-roll ring length (mono samples) for the current sample rate.
    fn preroll_cap(&self) -> usize {
        (self.sample_rate as usize)
            .saturating_mul(PREROLL_RING_SECS)
            .max(1)
    }

    /// Append mono native samples to the pre-roll ring, trimming the oldest to
    /// stay within [`preroll_cap`]. Called only while listening.
    pub fn push_preroll_i16(&mut self, data: &[i16]) {
        self.preroll_i16.extend(data.iter().copied());
        let cap = self.preroll_cap();
        while self.preroll_i16.len() > cap {
            self.preroll_i16.pop_front();
        }
    }
    pub fn push_preroll_i32(&mut self, data: &[i32]) {
        self.preroll_i32.extend(data.iter().copied());
        let cap = self.preroll_cap();
        while self.preroll_i32.len() > cap {
            self.preroll_i32.pop_front();
        }
    }
    pub fn push_preroll_f32(&mut self, data: &[f32]) {
        self.preroll_f32.extend(data.iter().copied());
        let cap = self.preroll_cap();
        while self.preroll_f32.len() > cap {
            self.preroll_f32.pop_front();
        }
    }

    /// Drop all pre-roll rings (e.g. when a fresh listen session starts).
    pub fn clear_preroll_rings(&mut self) {
        self.preroll_i16.clear();
        self.preroll_i32.clear();
        self.preroll_f32.clear();
    }

    /// Seed the recording buffer with the last `p` pre-roll samples (clamped to
    /// what the ring holds), prepending them to the just-cleared `samples_*`.
    /// Bumps `total_samples`, records `preroll_seeded`, and clears the rings.
    /// MUST be called right after `clear()` at record start so the pre-roll ends
    /// up at the front of the recording. Returns the number of samples seeded.
    pub fn seed_preroll(&mut self, p: usize) -> usize {
        let seeded = if p == 0 {
            0
        } else {
            // `make_contiguous` + `extend_from_slice` so the copy is a memcpy of
            // the tail rather than an element-wise iterator — this runs under the
            // buffer lock that the audio callback also takes, so keep it short to
            // avoid a capture xrun at the pre-roll seam.
            match self.format {
                NativeSampleFormat::I16 => {
                    let slice = self.preroll_i16.make_contiguous();
                    let take = p.min(slice.len());
                    let start = slice.len() - take;
                    self.samples_i16.extend_from_slice(&slice[start..]);
                    take
                }
                NativeSampleFormat::I24 | NativeSampleFormat::I32 => {
                    let slice = self.preroll_i32.make_contiguous();
                    let take = p.min(slice.len());
                    let start = slice.len() - take;
                    self.samples_i32.extend_from_slice(&slice[start..]);
                    take
                }
                NativeSampleFormat::F32 => {
                    let slice = self.preroll_f32.make_contiguous();
                    let take = p.min(slice.len());
                    let start = slice.len() - take;
                    self.samples_f32.extend_from_slice(&slice[start..]);
                    take
                }
            }
        };
        self.total_samples += seeded;
        self.preroll_seeded = seeded;
        self.clear_preroll_rings();
        seeded
    }

    /// Effective bit depth detected from the i32 sample stream so far, or `None`
    /// until enough samples have been seen (and there is real signal — silence is
    /// all zeros and would read as 0 bits). `container_bits` is the stream's
    /// container width (32 for I24/I32). Saturates at `container_bits`.
    pub fn effective_bits(&self, container_bits: u16) -> Option<u16> {
        // ~0.25 s at 192 kHz; enough to see the LSB usage settle.
        const MIN_SAMPLES: u64 = 48_000;
        if self.detect_samples < MIN_SAMPLES || self.detect_or == 0 {
            return None;
        }
        let trailing = self.detect_or.trailing_zeros().min(container_bits as u32);
        Some(container_bits.saturating_sub(trailing as u16))
    }

    pub fn clear(&mut self) {
        self.samples_i16.clear();
        self.samples_i32.clear();
        self.samples_f32.clear();
        self.pending_f32.clear();
        self.total_samples = 0;
        self.detect_or = 0;
        self.detect_samples = 0;
        self.preroll_seeded = 0;
        // Note: shared_fd is NOT cleared here — it persists across clear()
        // because it's set before recording starts and consumed on stop.
        // Note: the pre-roll rings are NOT cleared here — clear() runs at record
        // start, immediately before `seed_preroll` reads them; seed clears them.
    }

    /// Drain pending f32 samples for streaming to frontend.
    pub fn drain_pending(&mut self) -> Vec<f32> {
        std::mem::take(&mut self.pending_f32)
    }
}

/// RAII owner for the raw shared-storage (Android MediaStore) fd during a
/// recording stop. Kotlin's `detachFd()` hands Rust a bare fd that nothing else
/// will close, so any early return between taking it off the buffer and writing
/// to it would leak it (fd exhaustion after enough no-sample / skip-save /
/// finalize-error stops). Wrapping it means every `?`/return path closes it; the
/// write path calls [`take`](Self::take) to assume ownership (handing it to
/// `File::from_raw_fd`) and defuse the guard.
///
/// Off-Android there is never a shared fd (`try_create_shared_fd` is mobile-only,
/// so `shared_fd` is always `None`), hence the close is `target_os = "android"`
/// only — matching where `libc` is a dependency.
pub(crate) struct SharedFdGuard(Option<i32>);

impl SharedFdGuard {
    pub(crate) fn new(fd: Option<i32>) -> Self {
        Self(fd)
    }

    /// Assume ownership of the fd (for handoff to `File::from_raw_fd`), defusing
    /// the guard. Returns `None` if there was no fd.
    pub(crate) fn take(&mut self) -> Option<i32> {
        self.0.take()
    }
}

impl Drop for SharedFdGuard {
    fn drop(&mut self) {
        #[cfg(target_os = "android")]
        if let Some(fd) = self.0.take() {
            unsafe {
                libc::close(fd);
            }
        }
    }
}

/// Wrapper to allow cpal::Stream in Tauri managed state.
/// Safe because we only store/drop the stream; we never access its internals
/// from multiple threads simultaneously.
pub(crate) struct SendStream(#[allow(dead_code)] cpal::Stream);
unsafe impl Send for SendStream {}
unsafe impl Sync for SendStream {}

/// Holds the active cpal stream and shared state.
pub struct MicState {
    #[allow(dead_code)]
    pub stream: SendStream,
    pub buffer: Arc<Mutex<RecordingBuffer>>,
    pub is_recording: Arc<AtomicBool>,
    pub is_streaming: Arc<AtomicBool>,
    pub emitter_stop: Arc<AtomicBool>,
    pub format: NativeSampleFormat,
    pub sample_rate: u32,
    pub channels: usize,
    pub device_name: String,
    pub supported_sample_rates: Vec<u32>,
    /// Crash-recovery writer + shared state. Active between
    /// `mic_start_recording` and `mic_stop_recording` on Android.
    pub recovery: crate::recovery::RecoveryHandle,
}

// `MicInfo`, `RecordingResult`, `MicStatus`, `SampleRateRange`, and `DeviceInfo`
// are shared IPC wire types — see `oversample_ipc::mic` (re-exported above).

fn detect_format(config: &cpal::SupportedStreamConfig) -> NativeSampleFormat {
    match config.sample_format() {
        cpal::SampleFormat::I16 => NativeSampleFormat::I16,
        cpal::SampleFormat::I32 => {
            // cpal reports I32 for both 24-bit and 32-bit devices.
            // Check if the config's bits per sample hint suggests 24-bit.
            // Unfortunately cpal doesn't expose this directly, so we default to I32.
            // Users with 24-bit devices will still get lossless capture since
            // 24-bit samples fit in i32.
            NativeSampleFormat::I32
        }
        cpal::SampleFormat::F32 => NativeSampleFormat::F32,
        _ => NativeSampleFormat::F32, // fallback for other formats
    }
}

/// Collect distinct supported sample rates from a device's input configs.
fn collect_supported_rates(device: &cpal::Device) -> Vec<u32> {
    let mut rates = std::collections::BTreeSet::new();
    if let Ok(configs) = device.supported_input_configs() {
        for cfg in configs {
            // Add the min and max of each range
            rates.insert(cfg.min_sample_rate());
            rates.insert(cfg.max_sample_rate());
            // Also add common rates that fall within the range
            for &r in &[44100, 48000, 96000, 192000, 256000, 384000, 500000] {
                if r >= cfg.min_sample_rate() && r <= cfg.max_sample_rate() {
                    rates.insert(r);
                }
            }
        }
    }
    rates.into_iter().collect()
}

/// Try to find a supported config at the requested rate (or highest rate <= max).
/// Returns None if no suitable config is found.
#[allow(dead_code)]
fn negotiate_sample_rate(
    device: &cpal::Device,
    requested_max_rate: u32,
) -> Option<cpal::SupportedStreamConfig> {
    let configs: Vec<_> = match device.supported_input_configs() {
        Ok(c) => c.collect(),
        Err(_) => return None,
    };
    negotiate_sample_rate_from(&configs, requested_max_rate)
}

/// Try to find a supported config from a pre-filtered list at the requested rate
/// (or highest rate <= max). Returns None if no suitable config is found.
fn negotiate_sample_rate_from(
    configs: &[cpal::SupportedStreamConfigRange],
    requested_max_rate: u32,
) -> Option<cpal::SupportedStreamConfig> {
    if configs.is_empty() {
        return None;
    }

    // Try exact requested rate first — find a config range that contains it
    for cfg in configs {
        if requested_max_rate >= cfg.min_sample_rate()
            && requested_max_rate <= cfg.max_sample_rate()
        {
            return Some(cfg.clone().with_sample_rate(requested_max_rate));
        }
    }

    // No exact match — find the config with the highest max_rate <= requested
    let mut best: Option<(u32, cpal::SupportedStreamConfig)> = None;
    for cfg in configs {
        let rate = cfg.max_sample_rate();
        if rate <= requested_max_rate {
            if best.as_ref().map_or(true, |(b, _)| rate > *b) {
                best = Some((rate, cfg.clone().with_sample_rate(rate)));
            }
        }
    }

    best.map(|(_, config)| config)
}

/// List all available input devices and their supported sample rate ranges.
pub fn list_input_devices() -> Vec<DeviceInfo> {
    let host = cpal::default_host();
    let default_name = host
        .default_input_device()
        .and_then(|d| d.description().ok().map(|desc| desc.name().to_string()))
        .unwrap_or_default();

    let mut devices: Vec<DeviceInfo> = Vec::new();
    if let Ok(input_devices) = host.input_devices() {
        for device in input_devices {
            let name = device
                .description()
                .map(|d| d.name().to_string())
                .unwrap_or_else(|_| "Unknown".into());
            let is_default = name == default_name;
            let mut ranges = Vec::new();
            if let Ok(configs) = device.supported_input_configs() {
                for cfg in configs {
                    let fmt = match cfg.sample_format() {
                        cpal::SampleFormat::I16 => "I16",
                        cpal::SampleFormat::I32 => "I32",
                        cpal::SampleFormat::F32 => "F32",
                        _ => "Other",
                    };
                    ranges.push(SampleRateRange {
                        min: cfg.min_sample_rate(),
                        max: cfg.max_sample_rate(),
                        channels: cfg.channels(),
                        format: fmt.to_string(),
                    });
                }
            }
            // Deduplicate: merge ranges into existing entry with the same name
            // (Android's Oboe backend often reports the same device multiple times)
            if let Some(existing) = devices.iter_mut().find(|d| d.name == name) {
                existing.is_default |= is_default;
                for r in ranges {
                    let dominated = existing.sample_rate_ranges.iter().any(|e| {
                        e.min == r.min
                            && e.max == r.max
                            && e.channels == r.channels
                            && e.format == r.format
                    });
                    if !dominated {
                        existing.sample_rate_ranges.push(r);
                    }
                }
            } else {
                devices.push(DeviceInfo {
                    name,
                    is_default,
                    sample_rate_ranges: ranges,
                });
            }
        }
    }
    devices
}

/// Map a user's max_bit_depth value to a preferred cpal SampleFormat.
/// Returns None for auto (0) — accept any format.
fn preferred_format_for_bit_depth(max_bit_depth: u16) -> Option<cpal::SampleFormat> {
    match max_bit_depth {
        0 => None, // auto
        1..=16 => Some(cpal::SampleFormat::I16),
        17..=32 => Some(cpal::SampleFormat::I32),
        _ => None,
    }
}

/// Open an input device and create a capture stream.
/// If `device_name` is Some, look up that device by name; otherwise use the default.
/// If `requested_max_rate` > 0, try to negotiate the highest rate up to that value.
/// If `max_bit_depth` > 0, prefer a config with matching bit depth (16 -> I16, 24/32 -> I32).
/// If `requested_channels` > 0, prefer that channel count (1=mono, 2=stereo).
pub fn open_mic(
    requested_max_rate: u32,
    device_name: Option<&str>,
    max_bit_depth: u16,
    requested_channels: u16,
) -> Result<MicState, String> {
    let host = cpal::default_host();
    let device = if let Some(name) = device_name {
        // Try to find the requested device by name
        let found = host.input_devices().ok().and_then(|mut devs| {
            devs.find(|d| {
                d.description()
                    .ok()
                    .map(|desc| desc.name() == name)
                    .unwrap_or(false)
            })
        });
        match found {
            Some(d) => d,
            None => {
                eprintln!(
                    "Requested device '{}' not found, falling back to default",
                    name
                );
                host.default_input_device()
                    .ok_or_else(|| "No microphone found. Check your audio settings.".to_string())?
            }
        }
    } else {
        host.default_input_device()
            .ok_or_else(|| "No microphone found. Check your audio settings.".to_string())?
    };

    let device_name = device
        .description()
        .map(|d| d.name().to_string())
        .unwrap_or_else(|_| "Unknown".into());
    let supported_rates = collect_supported_rates(&device);

    let pref_fmt = preferred_format_for_bit_depth(max_bit_depth);

    let config = if requested_max_rate == 0 && pref_fmt.is_none() && requested_channels == 0 {
        // Full auto mode: use the device's preferred/native config.
        // This avoids Android's Oboe backend reporting inflated max rates
        // (e.g. 192kHz for built-in mic) that trigger silent resampling.
        let default_cfg = device
            .default_input_config()
            .map_err(|e| format!("Failed to get mic config: {}", e))?;
        eprintln!(
            "Mic config negotiation: full auto, using device default {}Hz {:?} (supported rates: {:?})",
            default_cfg.sample_rate(),
            default_cfg.sample_format(),
            supported_rates
        );
        default_cfg
    } else {
        // At least one preference set — negotiate from supported configs
        let all_configs: Vec<_> = device
            .supported_input_configs()
            .map_err(|e| format!("Failed to enumerate mic configs: {}", e))?
            .collect();

        // Filter by preferred format if set
        let format_filtered: Vec<_> = if let Some(fmt) = pref_fmt {
            let filtered: Vec<_> = all_configs
                .iter()
                .filter(|c| c.sample_format() == fmt)
                .cloned()
                .collect();
            if filtered.is_empty() {
                eprintln!(
                    "Mic config: no configs with format {:?}, ignoring bit depth preference",
                    fmt
                );
                all_configs.clone()
            } else {
                filtered
            }
        } else {
            all_configs.clone()
        };

        // Filter by channel count if set
        let chan_filtered: Vec<_> = if requested_channels > 0 {
            let filtered: Vec<_> = format_filtered
                .iter()
                .filter(|c| c.channels() == requested_channels)
                .cloned()
                .collect();
            if filtered.is_empty() {
                eprintln!(
                    "Mic config: no configs with {} channels, ignoring channel preference",
                    requested_channels
                );
                format_filtered
            } else {
                filtered
            }
        } else {
            format_filtered
        };

        // Now negotiate sample rate from the filtered configs
        let negotiated = if requested_max_rate > 0 {
            negotiate_sample_rate_from(&chan_filtered, requested_max_rate)
        } else {
            // Auto rate: pick the config with the highest max rate
            chan_filtered
                .iter()
                .max_by_key(|c| c.max_sample_rate())
                .map(|c| {
                    let rate = c.max_sample_rate();
                    c.clone().with_sample_rate(rate)
                })
        };

        match negotiated {
            Some(cfg) => {
                eprintln!(
                    "Mic config negotiation: {}Hz {:?} {}ch (requested: max_rate={}, max_bits={}, channels={})",
                    cfg.sample_rate(), cfg.sample_format(), cfg.channels(),
                    requested_max_rate, max_bit_depth, requested_channels,
                );
                cfg
            }
            None => {
                eprintln!("Mic config negotiation: no matching config, using device default");
                device
                    .default_input_config()
                    .map_err(|e| format!("Failed to get mic config: {}", e))?
            }
        }
    };

    let format = detect_format(&config);
    let sample_rate = config.sample_rate();
    let stream_config: cpal::StreamConfig = config.into();
    let channels = stream_config.channels as usize;

    let buffer = Arc::new(Mutex::new(RecordingBuffer::new(format, sample_rate)));
    let is_recording = Arc::new(AtomicBool::new(false));
    let is_streaming = Arc::new(AtomicBool::new(false));
    let emitter_stop = Arc::new(AtomicBool::new(false));

    let buf = buffer.clone();
    let rec = is_recording.clone();
    let strm = is_streaming.clone();

    let err_callback = |err: cpal::StreamError| {
        eprintln!("Audio stream error: {}", err);
    };

    let stream = match format {
        NativeSampleFormat::I16 => {
            device.build_input_stream(
                &stream_config,
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    let mut buf = buf.lock().unwrap();
                    if rec.load(Ordering::Relaxed) {
                        if channels > 1 {
                            let mono: Vec<i16> = data
                                .chunks(channels)
                                .map(|frame| {
                                    (frame.iter().map(|&s| s as i32).sum::<i32>() / channels as i32)
                                        as i16
                                })
                                .collect();
                            buf.total_samples += mono.len();
                            buf.samples_i16.extend_from_slice(&mono);
                        } else {
                            buf.total_samples += data.len();
                            buf.samples_i16.extend_from_slice(data);
                        }
                    } else if strm.load(Ordering::Relaxed) {
                        // Listening: keep the rolling pre-roll ring (native fmt).
                        if channels > 1 {
                            let mono: Vec<i16> = data
                                .chunks(channels)
                                .map(|frame| {
                                    (frame.iter().map(|&s| s as i32).sum::<i32>() / channels as i32)
                                        as i16
                                })
                                .collect();
                            buf.push_preroll_i16(&mono);
                        } else {
                            buf.push_preroll_i16(data);
                        }
                    }
                    if strm.load(Ordering::Relaxed) || rec.load(Ordering::Relaxed) {
                        if channels > 1 {
                            let f32_data: Vec<f32> = data
                                .chunks(channels)
                                .map(|frame| {
                                    frame.iter().map(|&s| s as f32 / 32768.0).sum::<f32>()
                                        / channels as f32
                                })
                                .collect();
                            buf.pending_f32.extend_from_slice(&f32_data);
                        } else {
                            let f32_data: Vec<f32> =
                                data.iter().map(|&s| s as f32 / 32768.0).collect();
                            buf.pending_f32.extend_from_slice(&f32_data);
                        }
                    }
                },
                err_callback,
                None,
            )
        }
        NativeSampleFormat::I24 | NativeSampleFormat::I32 => {
            device.build_input_stream(
                &stream_config,
                move |data: &[i32], _: &cpal::InputCallbackInfo| {
                    let mut buf = buf.lock().unwrap();
                    // Accumulate effective-bit-depth detection over the raw i32
                    // samples (cheap: one OR per sample) whenever the stream is
                    // active — covers both live listen and recording.
                    if strm.load(Ordering::Relaxed) || rec.load(Ordering::Relaxed) {
                        let mut or = buf.detect_or;
                        for &s in data {
                            or |= s;
                        }
                        buf.detect_or = or;
                        buf.detect_samples += data.len() as u64;
                    }
                    if rec.load(Ordering::Relaxed) {
                        if channels > 1 {
                            let mono: Vec<i32> = data
                                .chunks(channels)
                                .map(|frame| {
                                    (frame.iter().map(|&s| s as i64).sum::<i64>() / channels as i64)
                                        as i32
                                })
                                .collect();
                            buf.total_samples += mono.len();
                            buf.samples_i32.extend_from_slice(&mono);
                        } else {
                            buf.total_samples += data.len();
                            buf.samples_i32.extend_from_slice(data);
                        }
                    } else if strm.load(Ordering::Relaxed) {
                        // Listening: keep the rolling pre-roll ring (native fmt).
                        if channels > 1 {
                            let mono: Vec<i32> = data
                                .chunks(channels)
                                .map(|frame| {
                                    (frame.iter().map(|&s| s as i64).sum::<i64>() / channels as i64)
                                        as i32
                                })
                                .collect();
                            buf.push_preroll_i32(&mono);
                        } else {
                            buf.push_preroll_i32(data);
                        }
                    }
                    if strm.load(Ordering::Relaxed) || rec.load(Ordering::Relaxed) {
                        if channels > 1 {
                            let f32_data: Vec<f32> = data
                                .chunks(channels)
                                .map(|frame| {
                                    frame.iter().map(|&s| s as f32 / 2147483648.0).sum::<f32>()
                                        / channels as f32
                                })
                                .collect();
                            buf.pending_f32.extend_from_slice(&f32_data);
                        } else {
                            let f32_data: Vec<f32> =
                                data.iter().map(|&s| s as f32 / 2147483648.0).collect();
                            buf.pending_f32.extend_from_slice(&f32_data);
                        }
                    }
                },
                err_callback,
                None,
            )
        }
        NativeSampleFormat::F32 => {
            device.build_input_stream(
                &stream_config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    let mut buf = buf.lock().unwrap();
                    if rec.load(Ordering::Relaxed) {
                        if channels > 1 {
                            let mono: Vec<f32> = data
                                .chunks(channels)
                                .map(|frame| frame.iter().sum::<f32>() / channels as f32)
                                .collect();
                            buf.total_samples += mono.len();
                            buf.samples_f32.extend_from_slice(&mono);
                        } else {
                            buf.total_samples += data.len();
                            buf.samples_f32.extend_from_slice(data);
                        }
                    } else if strm.load(Ordering::Relaxed) {
                        // Listening: keep the rolling pre-roll ring (native fmt).
                        if channels > 1 {
                            let mono: Vec<f32> = data
                                .chunks(channels)
                                .map(|frame| frame.iter().sum::<f32>() / channels as f32)
                                .collect();
                            buf.push_preroll_f32(&mono);
                        } else {
                            buf.push_preroll_f32(data);
                        }
                    }
                    if strm.load(Ordering::Relaxed) || rec.load(Ordering::Relaxed) {
                        if channels > 1 {
                            let f32_data: Vec<f32> = data
                                .chunks(channels)
                                .map(|frame| frame.iter().sum::<f32>() / channels as f32)
                                .collect();
                            buf.pending_f32.extend_from_slice(&f32_data);
                        } else {
                            buf.pending_f32.extend_from_slice(data);
                        }
                    }
                },
                err_callback,
                None,
            )
        }
    }
    .map_err(|e| format!("Failed to open microphone: {}", e))?;

    stream
        .play()
        .map_err(|e| format!("Failed to start mic stream: {}", e))?;

    eprintln!(
        "Mic opened: {} ch={} sr={} fmt={:?} supported_rates={:?}",
        device_name, channels, sample_rate, format, supported_rates
    );

    Ok(MicState {
        stream: SendStream(stream),
        buffer,
        is_recording,
        is_streaming,
        emitter_stop,
        format,
        sample_rate,
        channels,
        device_name,
        supported_sample_rates: supported_rates,
        recovery: crate::recovery::RecoveryHandle::default(),
    })
}

/// Encode the recording buffer to WAV at native bit depth.
/// The WAV format to actually write. Auto-detect down-converts a 32-bit
/// container to the smallest format that holds the detected effective bit depth:
/// 16-bit for <=16-bit data (which zero-pads a 12-bit recorder cleanly — the low
/// bits drop out in the shift), 24-bit for <=24-bit (e.g. DUO-CAPTURE EX). It's
/// lossless for a device that genuinely delivers that depth, a half/third
/// smaller, and labels the header correctly. Falls back to the container until
/// detection settles or for genuine 32-bit input. (A per-device override can
/// force a specific depth — wired separately.)
fn output_format(buffer: &RecordingBuffer) -> NativeSampleFormat {
    // A per-device override (force_bits) wins; otherwise the auto-detected depth.
    let target = buffer.force_bits.or_else(|| buffer.effective_bits(32));
    match buffer.format {
        NativeSampleFormat::I32 => match target {
            Some(b) if b <= 16 => NativeSampleFormat::I16,
            Some(b) if b <= 24 => NativeSampleFormat::I24,
            _ => NativeSampleFormat::I32,
        },
        other => other,
    }
}

pub fn encode_native_wav(buffer: &RecordingBuffer) -> Result<Vec<u8>, String> {
    let out_format = output_format(buffer);

    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: buffer.sample_rate,
        bits_per_sample: out_format.bits_per_sample(),
        sample_format: if out_format.is_float() {
            hound::SampleFormat::Float
        } else {
            hound::SampleFormat::Int
        },
    };

    let mut cursor = Cursor::new(Vec::new());
    let mut writer =
        hound::WavWriter::new(&mut cursor, spec).map_err(|e| format!("WAV writer error: {}", e))?;

    let werr = |e: hound::Error| format!("WAV write error: {}", e);
    // For a target depth that doesn't match the 16-bit WAV size (e.g. a forced or
    // detected 12-bit), zero the surplus low bits so the file is honestly N-bit
    // (12-bit zero-padded). No-op (mask = all ones) for native 16-bit output.
    let target = buffer.force_bits.or_else(|| buffer.effective_bits(32));
    let mask16: i16 = match (out_format, target) {
        (NativeSampleFormat::I16, Some(b)) if b < 16 => !((1i16 << (16 - b)) - 1),
        _ => -1,
    };
    match buffer.format {
        NativeSampleFormat::I16 => {
            for &s in &buffer.samples_i16 {
                writer.write_sample(s & mask16).map_err(werr)?;
            }
        }
        // i32 container source: write at the (possibly down-converted) out depth.
        // The shifts drop only the always-zero low bits, so this is lossless for a
        // device that genuinely delivers `out_format` bits.
        NativeSampleFormat::I24 | NativeSampleFormat::I32 => {
            for &s in &buffer.samples_i32 {
                match out_format {
                    NativeSampleFormat::I16 => writer.write_sample(((s >> 16) as i16) & mask16),
                    NativeSampleFormat::I24 => writer.write_sample(s >> 8),
                    _ => writer.write_sample(s),
                }
                .map_err(werr)?;
            }
        }
        NativeSampleFormat::F32 => {
            for &s in &buffer.samples_f32 {
                writer.write_sample(s).map_err(werr)?;
            }
        }
    }

    writer
        .finalize()
        .map_err(|e| format!("WAV finalize error: {}", e))?;
    Ok(cursor.into_inner())
}

/// Optional GPS location for GUANO metadata.
pub struct RecordingLocation {
    pub latitude: f64,
    pub longitude: f64,
    pub elevation: Option<f64>,
    pub accuracy: Option<f64>,
}

/// Parameters for building GUANO metadata in the Tauri backend.
/// Consolidates location, device model, mic info, and app version.
pub struct TauriGuanoParams {
    pub connection_type: Option<String>,
    pub location: Option<RecordingLocation>,
    pub device_make: Option<String>,
    pub device_model: Option<String>,
    pub mic_name: Option<String>,
    pub mic_make: Option<String>,
    pub app_version: String,
    pub is_mobile: bool,
    /// Pre-roll duration (seconds) seeded at record start, for the GUANO
    /// `OVRS|Preroll` field. `None` when no pre-roll was used.
    pub preroll_secs: Option<f64>,
}

/// Build GUANO metadata for a Tauri-side recording using the shared builder.
/// `timestamp` is chrono::DateTime<chrono::Local> from the stop time;
/// the actual recording start is computed by subtracting duration.
pub fn build_tauri_guano(
    sample_rate: u32,
    num_samples: usize,
    filename: &str,
    timestamp: &chrono::DateTime<chrono::Local>,
    params: &TauriGuanoParams,
) -> oversample_core::audio::guano::GuanoMetadata {
    use oversample_core::audio::guano::{self, RecordingGuanoExtra};

    let duration_secs = num_samples as f64 / sample_rate as f64;
    let start_time = *timestamp - chrono::Duration::milliseconds((duration_secs * 1000.0) as i64);
    let ts = start_time.format("%Y-%m-%dT%H:%M:%S%:z").to_string();

    let is_usb = params
        .connection_type
        .as_deref()
        .map(|c| c.contains("USB"))
        .unwrap_or(false);

    // For non-USB (internal) mics, mic_name = "Internal"
    let mic_name = if is_usb {
        params.mic_name.clone()
    } else {
        Some("Internal".to_string())
    };

    let extra = RecordingGuanoExtra {
        mic_interface: params.connection_type.clone(),
        mic_name,
        mic_audio_device: None, // Web Audio API only — not applicable to native
        mic_make: if is_usb {
            params.mic_make.clone()
        } else {
            None
        },
        loc_position: params.location.as_ref().map(|l| (l.latitude, l.longitude)),
        loc_elevation: params.location.as_ref().and_then(|l| l.elevation),
        loc_accuracy: params.location.as_ref().and_then(|l| l.accuracy),
        device_make: if params.is_mobile {
            params.device_make.clone()
        } else {
            None
        },
        device_model: if params.is_mobile {
            params.device_model.clone()
        } else {
            None
        },
        preroll_secs: params.preroll_secs,
    };

    guano::build_recording_guano(
        sample_rate,
        duration_secs,
        filename,
        true, // is_tauri
        params.is_mobile,
        &extra,
        &ts,
        &params.app_version,
    )
}

/// Get f32 version of all recorded samples (for frontend spectrogram/display).
pub fn get_samples_f32(buffer: &RecordingBuffer) -> Vec<f32> {
    match buffer.format {
        NativeSampleFormat::I16 => buffer
            .samples_i16
            .iter()
            .map(|&s| s as f32 / 32768.0)
            .collect(),
        NativeSampleFormat::I24 | NativeSampleFormat::I32 => buffer
            .samples_i32
            .iter()
            .map(|&s| s as f32 / 2147483648.0)
            .collect(),
        NativeSampleFormat::F32 => buffer.samples_f32.clone(),
    }
}

/// Write WAV data to a raw POSIX file descriptor (from Android ContentResolver).
/// Closes the fd after writing. Only used on Android.
#[cfg(target_os = "android")]
pub fn write_wav_to_fd(fd: i32, wav_data: &[u8]) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::io::FromRawFd;

    let mut file = unsafe { std::fs::File::from_raw_fd(fd) };
    file.write_all(wav_data)
        .map_err(|e| format!("Failed to write WAV to fd {}: {}", fd, e))?;
    // File is dropped here, which closes the fd
    Ok(())
}

/// Stub for non-Android platforms (fd-passing is Android-only).
#[cfg(not(target_os = "android"))]
pub fn write_wav_to_fd(_fd: i32, _wav_data: &[u8]) -> Result<(), String> {
    Err("write_wav_to_fd is only supported on Android".into())
}

/// Start the background emitter thread that sends audio chunks to the frontend.
///
/// The thread also does best-effort disk flushing for crash-recovery: when a
/// `RecoveryWriter` is installed (by `mic_start_recording`), any native-format
/// samples appended since the last tick are written to the `.wav.part` file.
/// Disk I/O happens outside the buffer lock to avoid stalling the audio callback.
pub fn start_emitter(
    buffer: Arc<Mutex<RecordingBuffer>>,
    stop_flag: Arc<AtomicBool>,
    recovery: crate::recovery::RecoveryHandle,
) {
    std::thread::spawn(move || {
        let mut tick: u32 = 0;
        while !stop_flag.load(Ordering::Relaxed) {
            std::thread::sleep(std::time::Duration::from_millis(80));
            // Streamed samples are pulled by the frontend via the
            // `mic_pull_audio` command (raw f32 bytes) instead of being pushed
            // as a JSON "mic-audio-chunk" event: `pending_f32` accumulates until
            // the frontend drains it. This thread only does crash-recovery disk
            // flushing below.

            // Flush new samples to disk every ~240 ms (every 3rd tick) to
            // amortize open/write cost. Only happens when a recovery writer
            // is installed (i.e. streaming-to-disk mode). We hold the writer
            // lock across the drain+write so the stop command can't race us
            // and consume the writer mid-flush (which would silently drop
            // the bytes we just drained).
            tick = tick.wrapping_add(1);
            if tick % 3 == 0 {
                if let Ok(mut wg) = recovery.writer.lock() {
                    if wg.is_some() {
                        let bytes = {
                            let mut buf = match buffer.lock() {
                                Ok(b) => b,
                                Err(_) => continue,
                            };
                            crate::recovery::drain_cpal_bytes(&mut buf)
                        };
                        if !bytes.is_empty() {
                            if let Some(writer) = wg.as_mut() {
                                if let Err(e) = writer.append_bytes(&bytes) {
                                    eprintln!("recovery flush failed: {}", e);
                                }
                            }
                        }
                    }
                }
            }
        }
    });
}
