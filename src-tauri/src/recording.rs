use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use serde::Serialize;
use std::io::Cursor;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

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
        }
    }

    pub fn clear(&mut self) {
        self.samples_i16.clear();
        self.samples_i32.clear();
        self.samples_f32.clear();
        self.pending_f32.clear();
        self.total_samples = 0;
        // Note: shared_fd is NOT cleared here — it persists across clear()
        // because it's set before recording starts and consumed on stop.
    }

    /// Drain pending f32 samples for streaming to frontend.
    pub fn drain_pending(&mut self) -> Vec<f32> {
        std::mem::take(&mut self.pending_f32)
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
}

#[derive(Serialize)]
pub struct MicInfo {
    pub device_name: String,
    pub sample_rate: u32,
    pub bits_per_sample: u16,
    pub is_float: bool,
    pub format: String,
    pub supported_sample_rates: Vec<u32>,
    /// Audio host backend name: "Oboe", "WASAPI", "ASIO", "CoreAudio", "ALSA", "JACK", etc.
    pub host_name: String,
}

#[derive(Serialize)]
pub struct SampleRateRange {
    pub min: u32,
    pub max: u32,
    pub channels: u16,
    pub format: String,
}

#[derive(Serialize)]
pub struct DeviceInfo {
    pub name: String,
    pub is_default: bool,
    pub sample_rate_ranges: Vec<SampleRateRange>,
}

#[derive(Serialize)]
pub struct RecordingResult {
    pub filename: String,
    pub saved_path: String,
    pub sample_rate: u32,
    pub bits_per_sample: u16,
    pub is_float: bool,
    pub duration_secs: f64,
    pub num_samples: usize,
    pub samples_f32: Vec<f32>,
}

#[derive(Serialize)]
pub struct MicStatus {
    pub is_open: bool,
    pub is_recording: bool,
    pub is_streaming: bool,
    pub samples_recorded: usize,
    pub sample_rate: u32,
}

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
            let name = device.description().map(|d| d.name().to_string()).unwrap_or_else(|_| "Unknown".into());
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
                        e.min == r.min && e.max == r.max && e.channels == r.channels && e.format == r.format
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
        0 => None,        // auto
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
        let found = host
            .input_devices()
            .ok()
            .and_then(|mut devs| devs.find(|d| d.description().ok().map(|desc| desc.name() == name).unwrap_or(false)));
        match found {
            Some(d) => d,
            None => {
                eprintln!("Requested device '{}' not found, falling back to default", name);
                host.default_input_device()
                    .ok_or_else(|| "No microphone found. Check your audio settings.".to_string())?
            }
        }
    } else {
        host.default_input_device()
            .ok_or_else(|| "No microphone found. Check your audio settings.".to_string())?
    };

    let device_name = device.description().map(|d| d.name().to_string()).unwrap_or_else(|_| "Unknown".into());
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
            let filtered: Vec<_> = all_configs.iter()
                .filter(|c| c.sample_format() == fmt)
                .cloned()
                .collect();
            if filtered.is_empty() {
                eprintln!("Mic config: no configs with format {:?}, ignoring bit depth preference", fmt);
                all_configs.clone()
            } else {
                filtered
            }
        } else {
            all_configs.clone()
        };

        // Filter by channel count if set
        let chan_filtered: Vec<_> = if requested_channels > 0 {
            let filtered: Vec<_> = format_filtered.iter()
                .filter(|c| c.channels() == requested_channels)
                .cloned()
                .collect();
            if filtered.is_empty() {
                eprintln!("Mic config: no configs with {} channels, ignoring channel preference", requested_channels);
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
            chan_filtered.iter()
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
                eprintln!(
                    "Mic config negotiation: no matching config, using device default"
                );
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
                            let mono: Vec<i16> = data.chunks(channels)
                                .map(|frame| (frame.iter().map(|&s| s as i32).sum::<i32>() / channels as i32) as i16)
                                .collect();
                            buf.total_samples += mono.len();
                            buf.samples_i16.extend_from_slice(&mono);
                        } else {
                            buf.total_samples += data.len();
                            buf.samples_i16.extend_from_slice(data);
                        }
                    }
                    if strm.load(Ordering::Relaxed) || rec.load(Ordering::Relaxed) {
                        if channels > 1 {
                            let f32_data: Vec<f32> = data.chunks(channels)
                                .map(|frame| frame.iter().map(|&s| s as f32 / 32768.0).sum::<f32>() / channels as f32)
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
                    if rec.load(Ordering::Relaxed) {
                        if channels > 1 {
                            let mono: Vec<i32> = data.chunks(channels)
                                .map(|frame| (frame.iter().map(|&s| s as i64).sum::<i64>() / channels as i64) as i32)
                                .collect();
                            buf.total_samples += mono.len();
                            buf.samples_i32.extend_from_slice(&mono);
                        } else {
                            buf.total_samples += data.len();
                            buf.samples_i32.extend_from_slice(data);
                        }
                    }
                    if strm.load(Ordering::Relaxed) || rec.load(Ordering::Relaxed) {
                        if channels > 1 {
                            let f32_data: Vec<f32> = data.chunks(channels)
                                .map(|frame| frame.iter().map(|&s| s as f32 / 2147483648.0).sum::<f32>() / channels as f32)
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
                            let mono: Vec<f32> = data.chunks(channels)
                                .map(|frame| frame.iter().sum::<f32>() / channels as f32)
                                .collect();
                            buf.total_samples += mono.len();
                            buf.samples_f32.extend_from_slice(&mono);
                        } else {
                            buf.total_samples += data.len();
                            buf.samples_f32.extend_from_slice(data);
                        }
                    }
                    if strm.load(Ordering::Relaxed) || rec.load(Ordering::Relaxed) {
                        if channels > 1 {
                            let f32_data: Vec<f32> = data.chunks(channels)
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
    })
}

/// Encode the recording buffer to WAV at native bit depth.
pub fn encode_native_wav(buffer: &RecordingBuffer) -> Result<Vec<u8>, String> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: buffer.sample_rate,
        bits_per_sample: buffer.format.bits_per_sample(),
        sample_format: if buffer.format.is_float() {
            hound::SampleFormat::Float
        } else {
            hound::SampleFormat::Int
        },
    };

    let mut cursor = Cursor::new(Vec::new());
    let mut writer =
        hound::WavWriter::new(&mut cursor, spec).map_err(|e| format!("WAV writer error: {}", e))?;

    match buffer.format {
        NativeSampleFormat::I16 => {
            for &s in &buffer.samples_i16 {
                writer
                    .write_sample(s)
                    .map_err(|e| format!("WAV write error: {}", e))?;
            }
        }
        NativeSampleFormat::I24 => {
            for &s in &buffer.samples_i32 {
                // Mask to 24-bit range
                let s24 = (s >> 8) as i32;
                writer
                    .write_sample(s24)
                    .map_err(|e| format!("WAV write error: {}", e))?;
            }
        }
        NativeSampleFormat::I32 => {
            for &s in &buffer.samples_i32 {
                writer
                    .write_sample(s)
                    .map_err(|e| format!("WAV write error: {}", e))?;
            }
        }
        NativeSampleFormat::F32 => {
            for &s in &buffer.samples_f32 {
                writer
                    .write_sample(s)
                    .map_err(|e| format!("WAV write error: {}", e))?;
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

    let is_usb = params.connection_type.as_deref()
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
        mic_make: if is_usb { params.mic_make.clone() } else { None },
        loc_position: params.location.as_ref().map(|l| (l.latitude, l.longitude)),
        loc_elevation: params.location.as_ref().and_then(|l| l.elevation),
        loc_accuracy: params.location.as_ref().and_then(|l| l.accuracy),
        device_make: if params.is_mobile { params.device_make.clone() } else { None },
        device_model: if params.is_mobile { params.device_model.clone() } else { None },
    };

    guano::build_recording_guano(
        sample_rate, duration_secs, filename,
        true, // is_tauri
        params.is_mobile,
        &extra, &ts, &params.app_version,
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
    use std::os::unix::io::FromRawFd;
    use std::io::Write;

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
pub fn start_emitter(
    app: tauri::AppHandle,
    buffer: Arc<Mutex<RecordingBuffer>>,
    stop_flag: Arc<AtomicBool>,
) {
    std::thread::spawn(move || {
        use tauri::Emitter;
        while !stop_flag.load(Ordering::Relaxed) {
            std::thread::sleep(std::time::Duration::from_millis(80));
            let chunks = {
                let mut buf = buffer.lock().unwrap();
                buf.drain_pending()
            };
            if !chunks.is_empty() {
                let _ = app.emit("mic-audio-chunk", &chunks);
            }
        }
    });
}
