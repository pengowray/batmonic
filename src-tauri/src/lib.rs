mod audio_decode;
mod native_playback;
mod recording;
mod usb_audio;
mod xc;

use audio_decode::{AudioFileInfo, FullDecodeResult};
use native_playback::{NativePlayParams, PlaybackState, PlaybackStatus};
use recording::{DeviceInfo, MicInfo, MicState, MicStatus, RecordingResult};
use usb_audio::{UsbStreamInfo, UsbStreamState, UsbStreamStatus};
use std::sync::atomic::Ordering;
use std::sync::Mutex;
use tauri::Manager;

type MicMutex = Mutex<Option<MicState>>;
type PlaybackMutex = Mutex<Option<PlaybackState>>;
type UsbStreamMutex = Mutex<Option<UsbStreamState>>;

#[tauri::command]
fn save_recording(
    app: tauri::AppHandle,
    filename: String,
    data: Vec<u8>,
) -> Result<String, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("recordings");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(&filename);
    std::fs::write(&path, &data).map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
fn mic_open(
    app: tauri::AppHandle,
    state: tauri::State<MicMutex>,
    max_sample_rate: Option<u32>,
    device_name: Option<String>,
    max_bit_depth: Option<u16>,
    channels: Option<u16>,
) -> Result<MicInfo, String> {
    let mut mic = state.lock().map_err(|e| e.to_string())?;
    if mic.is_some() {
        // Already open — return current info
        let m = mic.as_ref().unwrap();
        return Ok(MicInfo {
            device_name: m.device_name.clone(),
            sample_rate: m.sample_rate,
            bits_per_sample: m.format.bits_per_sample(),
            is_float: m.format.is_float(),
            format: format!("{:?}", m.format),
            supported_sample_rates: m.supported_sample_rates.clone(),
        });
    }

    let requested = max_sample_rate.unwrap_or(0);
    let m = recording::open_mic(
        requested,
        device_name.as_deref(),
        max_bit_depth.unwrap_or(0),
        channels.unwrap_or(0),
    )?;
    let info = MicInfo {
        device_name: m.device_name.clone(),
        sample_rate: m.sample_rate,
        bits_per_sample: m.format.bits_per_sample(),
        is_float: m.format.is_float(),
        format: format!("{:?}", m.format),
        supported_sample_rates: m.supported_sample_rates.clone(),
    };

    // Start the emitter thread for streaming audio chunks to the frontend
    recording::start_emitter(app, m.buffer.clone(), m.emitter_stop.clone());

    *mic = Some(m);
    Ok(info)
}

#[tauri::command]
fn mic_list_devices() -> Vec<DeviceInfo> {
    recording::list_input_devices()
}

#[tauri::command]
fn mic_close(state: tauri::State<MicMutex>) -> Result<(), String> {
    let mut mic = state.lock().map_err(|e| e.to_string())?;
    if let Some(m) = mic.take() {
        m.emitter_stop.store(true, Ordering::Relaxed);
        m.is_recording.store(false, Ordering::Relaxed);
        m.is_streaming.store(false, Ordering::Relaxed);
        drop(m); // drops the cpal::Stream, closing the mic
    }
    Ok(())
}

#[tauri::command]
fn mic_start_recording(state: tauri::State<MicMutex>, shared_fd: Option<i32>) -> Result<(), String> {
    let mic = state.lock().map_err(|e| e.to_string())?;
    let m = mic.as_ref().ok_or("Microphone not open")?;
    {
        let mut buf = m.buffer.lock().unwrap();
        buf.clear();
        buf.shared_fd = shared_fd;
    }
    m.is_recording.store(true, Ordering::Relaxed);
    Ok(())
}

#[tauri::command]
fn mic_stop_recording(
    app: tauri::AppHandle,
    state: tauri::State<MicMutex>,
    loc_latitude: Option<f64>,
    loc_longitude: Option<f64>,
    loc_elevation: Option<f64>,
    loc_accuracy: Option<f64>,
    device_model: Option<String>,
) -> Result<RecordingResult, String> {
    let mic = state.lock().map_err(|e| e.to_string())?;
    let m = mic.as_ref().ok_or("Microphone not open")?;
    m.is_recording.store(false, Ordering::Relaxed);

    let mut buf = m.buffer.lock().unwrap();
    let num_samples = buf.total_samples;
    if num_samples == 0 {
        return Err("No samples recorded".into());
    }

    let sample_rate = buf.sample_rate;
    let duration_secs = num_samples as f64 / sample_rate as f64;
    let shared_fd = buf.shared_fd.take();

    // Generate filename
    let now = chrono::Local::now();
    let filename = now.format("batcap_%Y%m%d_%H%M%S.wav").to_string();

    // Encode WAV at native bit depth
    let mut wav_data = recording::encode_native_wav(&buf)?;

    // Get f32 samples for frontend display
    let samples_f32 = recording::get_samples_f32(&buf);

    let bits_per_sample = buf.format.bits_per_sample();
    let is_float = buf.format.is_float();

    drop(buf);

    // Build location struct if coordinates were provided
    let location = match (loc_latitude, loc_longitude) {
        (Some(lat), Some(lon)) => Some(recording::RecordingLocation {
            latitude: lat,
            longitude: lon,
            elevation: loc_elevation,
            accuracy: loc_accuracy,
        }),
        _ => None,
    };

    // Append GUANO metadata
    let guano_text = recording::build_recording_guano(
        sample_rate, num_samples, &m.device_name, &filename, &now,
        Some("Cpal"), location.as_ref(), device_model.as_deref(),
    );
    recording::append_guano_chunk(&mut wav_data, &guano_text);

    // Write to shared storage fd if available, otherwise to internal storage
    let saved_path = if let Some(fd) = shared_fd {
        recording::write_wav_to_fd(fd, &wav_data)?;
        "shared://recording".to_string() // marker: file is in shared storage
    } else {
        let dir = app
            .path()
            .app_data_dir()
            .map_err(|e| e.to_string())?
            .join("recordings");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = dir.join(&filename);
        std::fs::write(&path, &wav_data).map_err(|e| e.to_string())?;
        path.to_string_lossy().to_string()
    };

    Ok(RecordingResult {
        filename,
        saved_path,
        sample_rate,
        bits_per_sample,
        is_float,
        duration_secs,
        num_samples,
        samples_f32,
    })
}

#[tauri::command]
fn mic_set_listening(state: tauri::State<MicMutex>, listening: bool) -> Result<(), String> {
    let mic = state.lock().map_err(|e| e.to_string())?;
    let m = mic.as_ref().ok_or("Microphone not open")?;
    m.is_streaming.store(listening, Ordering::Relaxed);
    Ok(())
}

#[tauri::command]
fn mic_get_status(state: tauri::State<MicMutex>) -> MicStatus {
    let mic = state.lock().unwrap_or_else(|e| e.into_inner());
    match mic.as_ref() {
        Some(m) => {
            let samples = m.buffer.lock().map(|b| b.total_samples).unwrap_or(0);
            MicStatus {
                is_open: true,
                is_recording: m.is_recording.load(Ordering::Relaxed),
                is_streaming: m.is_streaming.load(Ordering::Relaxed),
                samples_recorded: samples,
                sample_rate: m.sample_rate,
            }
        }
        None => MicStatus {
            is_open: false,
            is_recording: false,
            is_streaming: false,
            samples_recorded: 0,
            sample_rate: 0,
        },
    }
}

// ── Noise preset commands ───────────────────────────────────────────

#[tauri::command]
fn save_noise_preset(app: tauri::AppHandle, name: String, json: String) -> Result<String, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("noise-presets");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let sanitized: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' || c == ' ' { c } else { '_' })
        .collect();
    let sanitized = sanitized.trim().to_string();
    let filename = if sanitized.is_empty() {
        "noise_profile.batm".to_string()
    } else {
        format!("{}.batm", sanitized.replace(' ', "_").to_lowercase())
    };
    let path = dir.join(&filename);
    std::fs::write(&path, &json).map_err(|e| e.to_string())?;
    Ok(filename)
}

#[tauri::command]
fn load_noise_preset(app: tauri::AppHandle, name: String) -> Result<String, String> {
    let path = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("noise-presets")
        .join(&name);
    std::fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read preset '{}': {}", name, e))
}

#[tauri::command]
fn list_noise_presets(app: tauri::AppHandle) -> Result<Vec<String>, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("noise-presets");
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut presets: Vec<String> = std::fs::read_dir(&dir)
        .map_err(|e| e.to_string())?
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(".batm") || name.ends_with(".json") { Some(name) } else { None }
        })
        .collect();
    presets.sort();
    Ok(presets)
}

#[tauri::command]
fn delete_noise_preset(app: tauri::AppHandle, name: String) -> Result<(), String> {
    let path = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("noise-presets")
        .join(&name);
    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| e.to_string())?;
    }
    Ok(())
}

// ── Annotation sidecar commands ──────────────────────────────────────

#[tauri::command]
fn read_sidecar(path: String) -> Result<Option<String>, String> {
    let sidecar = format!("{}.batm", path);
    if std::path::Path::new(&sidecar).exists() {
        std::fs::read_to_string(&sidecar)
            .map(Some)
            .map_err(|e| format!("Failed to read sidecar: {e}"))
    } else {
        Ok(None)
    }
}

#[tauri::command]
fn write_sidecar(path: String, yaml: String) -> Result<(), String> {
    let sidecar = format!("{}.batm", path);
    // Atomic write: write to temp, then rename
    let tmp = format!("{}.batm.tmp", path);
    std::fs::write(&tmp, &yaml).map_err(|e| format!("Failed to write sidecar: {e}"))?;
    std::fs::rename(&tmp, &sidecar).map_err(|e| format!("Failed to rename sidecar: {e}"))?;
    Ok(())
}

#[tauri::command]
fn read_central_annotations(app: tauri::AppHandle, file_key: String) -> Result<Option<String>, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("annotations");
    let path = dir.join(format!("{}.batm", file_key));
    if path.exists() {
        std::fs::read_to_string(&path)
            .map(Some)
            .map_err(|e| format!("Failed to read annotations: {e}"))
    } else {
        Ok(None)
    }
}

#[tauri::command]
fn write_central_annotations(app: tauri::AppHandle, file_key: String, yaml: String) -> Result<(), String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("annotations");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("{}.batm", file_key));
    let tmp = dir.join(format!("{}.batm.tmp", file_key));
    std::fs::write(&tmp, &yaml).map_err(|e| format!("Failed to write annotations: {e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("Failed to rename annotations: {e}"))?;
    Ok(())
}

/// Show a native save dialog and export annotations to the chosen path.
/// Returns the saved path, or empty string if cancelled.
#[cfg(not(target_os = "android"))]
#[tauri::command]
async fn export_annotations_file(filename: String, yaml: String) -> Result<String, String> {
    let handle = rfd::AsyncFileDialog::new()
        .set_file_name(&filename)
        .add_filter("Oversample annotations", &["batm"])
        .add_filter("YAML files", &["yaml", "yml"])
        .set_title("Export annotations")
        .save_file()
        .await;
    match handle {
        Some(file) => {
            let path = file.path().to_string_lossy().to_string();
            std::fs::write(file.path(), &yaml)
                .map_err(|e| format!("Failed to write export: {e}"))?;
            Ok(path)
        }
        None => Ok(String::new()), // cancelled
    }
}

#[cfg(target_os = "android")]
#[tauri::command]
async fn export_annotations_file(_filename: String, _yaml: String) -> Result<String, String> {
    Err("File export dialog not supported on Android".into())
}

/// Show a native file-open dialog and return the selected paths.
#[cfg(not(target_os = "android"))]
#[tauri::command]
async fn open_file_dialog() -> Result<Vec<String>, String> {
    let handle = rfd::AsyncFileDialog::new()
        .add_filter("Audio files", &["wav", "w4v", "flac", "ogg", "mp3"])
        .add_filter("All files", &["*"])
        .set_title("Open audio files")
        .pick_files()
        .await;
    match handle {
        Some(files) => Ok(files.iter().map(|f| f.path().to_string_lossy().to_string()).collect()),
        None => Ok(Vec::new()), // cancelled
    }
}

#[cfg(target_os = "android")]
#[tauri::command]
async fn open_file_dialog() -> Result<Vec<String>, String> {
    Err("File open dialog not supported on Android".into())
}

// ── Audio file decoding commands ─────────────────────────────────────

#[tauri::command]
fn audio_file_info(path: String) -> Result<AudioFileInfo, String> {
    audio_decode::file_info(&path)
}

#[tauri::command]
fn audio_decode_full(path: String) -> Result<FullDecodeResult, String> {
    audio_decode::decode_full(&path)
}

/// Read raw file bytes — returns binary data via efficient IPC (no JSON serialization).
#[tauri::command]
fn read_file_bytes(path: String) -> Result<tauri::ipc::Response, String> {
    let bytes = std::fs::read(&path)
        .map_err(|e| format!("Failed to read file '{}': {}", path, e))?;
    Ok(tauri::ipc::Response::new(bytes))
}

/// Read a byte range from a file — for streaming large files without loading entirely.
#[tauri::command]
fn read_file_range(path: String, offset: u64, length: u64) -> Result<tauri::ipc::Response, String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(&path)
        .map_err(|e| format!("Failed to open '{}': {}", path, e))?;
    f.seek(SeekFrom::Start(offset))
        .map_err(|e| format!("Seek failed: {}", e))?;
    let mut buf = vec![0u8; length as usize];
    f.read_exact(&mut buf)
        .map_err(|e| format!("Read failed: {}", e))?;
    Ok(tauri::ipc::Response::new(buf))
}

// ── USB audio streaming commands ────────────────────────────────────

#[tauri::command]
fn usb_start_stream(
    app: tauri::AppHandle,
    state: tauri::State<UsbStreamMutex>,
    fd: i32,
    endpoint_address: u32,
    max_packet_size: u32,
    sample_rate: u32,
    num_channels: u32,
    device_name: String,
    interface_number: Option<u32>,
    alternate_setting: Option<u32>,
) -> Result<UsbStreamInfo, String> {
    let usb = state.lock().map_err(|e| e.to_string())?;
    // Stop existing stream if any
    if let Some(existing) = usb.as_ref() {
        usb_audio::stop_usb_stream(existing);
    }
    drop(usb);

    let stream_state = usb_audio::start_usb_stream(
        fd,
        endpoint_address,
        max_packet_size,
        sample_rate,
        num_channels,
        device_name.clone(),
        app,
        interface_number.unwrap_or(0),
        alternate_setting.unwrap_or(0),
    )?;

    let info = UsbStreamInfo {
        sample_rate,
        device_name,
    };

    let mut usb = state.lock().map_err(|e| e.to_string())?;
    *usb = Some(stream_state);
    Ok(info)
}

#[tauri::command]
fn usb_stop_stream(state: tauri::State<UsbStreamMutex>) -> Result<(), String> {
    {
        let usb = state.lock().map_err(|e| e.to_string())?;
        if let Some(s) = usb.as_ref() {
            usb_audio::stop_usb_stream(s);
        }
    }
    // Give the streaming thread a moment to wind down
    std::thread::sleep(std::time::Duration::from_millis(100));
    let mut usb = state.lock().map_err(|e| e.to_string())?;
    *usb = None;
    Ok(())
}

#[tauri::command]
fn usb_start_recording(state: tauri::State<UsbStreamMutex>, shared_fd: Option<i32>) -> Result<(), String> {
    let usb = state.lock().map_err(|e| e.to_string())?;
    let s = usb.as_ref().ok_or("USB stream not open")?;
    if !s.is_streaming.load(Ordering::Relaxed) {
        return Err("USB stream is not actively streaming — cannot start recording".into());
    }
    usb_audio::clear_usb_buffer(s);
    {
        let mut buf = s.buffer.lock().unwrap();
        buf.shared_fd = shared_fd;
    }
    s.is_recording.store(true, Ordering::Relaxed);
    Ok(())
}

#[tauri::command]
fn usb_stop_recording(
    app: tauri::AppHandle,
    state: tauri::State<UsbStreamMutex>,
    loc_latitude: Option<f64>,
    loc_longitude: Option<f64>,
    loc_elevation: Option<f64>,
    loc_accuracy: Option<f64>,
    device_model: Option<String>,
) -> Result<RecordingResult, String> {
    let usb = state.lock().map_err(|e| e.to_string())?;
    let s = usb.as_ref().ok_or("USB stream not open")?;
    s.is_recording.store(false, Ordering::Relaxed);
    let still_streaming = s.is_streaming.load(Ordering::Relaxed);

    let (num_samples, sample_rate, shared_fd) = {
        let mut buf = s.buffer.lock().unwrap();
        (buf.total_samples, buf.sample_rate, buf.shared_fd.take())
    };

    if num_samples == 0 {
        if !still_streaming {
            return Err("No samples recorded — USB stream died during recording".into());
        }
        return Err("No samples recorded — stream was active but buffer is empty".into());
    }

    let duration_secs = num_samples as f64 / sample_rate as f64;

    let now = chrono::Local::now();
    let filename = now.format("batcap_%Y%m%d_%H%M%S.wav").to_string();

    let mut wav_data = usb_audio::encode_usb_wav(s)?;
    let samples_f32 = usb_audio::get_usb_samples_f32(s);

    let location = match (loc_latitude, loc_longitude) {
        (Some(lat), Some(lon)) => Some(recording::RecordingLocation {
            latitude: lat,
            longitude: lon,
            elevation: loc_elevation,
            accuracy: loc_accuracy,
        }),
        _ => None,
    };

    // Append GUANO metadata
    let guano_text = recording::build_recording_guano(
        sample_rate, num_samples, &s.device_name, &filename, &now,
        Some("USB (Raw)"), location.as_ref(), device_model.as_deref(),
    );
    recording::append_guano_chunk(&mut wav_data, &guano_text);

    // Write to shared storage fd if available, otherwise to internal storage
    let saved_path = if let Some(fd) = shared_fd {
        recording::write_wav_to_fd(fd, &wav_data)?;
        "shared://recording".to_string()
    } else {
        let dir = app
            .path()
            .app_data_dir()
            .map_err(|e| e.to_string())?
            .join("recordings");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = dir.join(&filename);
        std::fs::write(&path, &wav_data).map_err(|e| e.to_string())?;
        path.to_string_lossy().to_string()
    };

    Ok(RecordingResult {
        filename,
        saved_path,
        sample_rate,
        bits_per_sample: 16,
        is_float: false,
        duration_secs,
        num_samples,
        samples_f32,
    })
}

#[tauri::command]
fn usb_stream_status(state: tauri::State<UsbStreamMutex>) -> UsbStreamStatus {
    let usb = state.lock().unwrap_or_else(|e| e.into_inner());
    match usb.as_ref() {
        Some(s) => usb_audio::get_usb_status(s),
        None => UsbStreamStatus {
            is_open: false,
            is_streaming: false,
            samples_recorded: 0,
            sample_rate: 0,
        },
    }
}

// ── Native playback commands ────────────────────────────────────────

#[tauri::command]
fn native_play(
    app: tauri::AppHandle,
    state: tauri::State<PlaybackMutex>,
    params: NativePlayParams,
) -> Result<(), String> {
    let mut pb = state.lock().map_err(|e| e.to_string())?;
    // Stop existing playback
    native_playback::stop(&mut pb);
    // Start new playback
    let new_state = native_playback::start(params, app)?;
    *pb = Some(new_state);
    Ok(())
}

#[tauri::command]
fn native_stop(state: tauri::State<PlaybackMutex>) -> Result<(), String> {
    let mut pb = state.lock().map_err(|e| e.to_string())?;
    native_playback::stop(&mut pb);
    Ok(())
}

#[tauri::command]
fn native_playback_status(state: tauri::State<PlaybackMutex>) -> PlaybackStatus {
    let pb = state.lock().unwrap_or_else(|e| e.into_inner());
    match pb.as_ref() {
        Some(s) => PlaybackStatus {
            is_playing: s.is_playing(),
            playhead_secs: s.playhead_secs(),
        },
        None => PlaybackStatus {
            is_playing: false,
            playhead_secs: 0.0,
        },
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri::plugin::Builder::<_, ()>::new("usb-audio").build())
        .plugin(tauri::plugin::Builder::<_, ()>::new("media-store").build())
        .plugin(tauri::plugin::Builder::<_, ()>::new("geolocation").build())
        .manage(Mutex::new(None::<MicState>))
        .manage(Mutex::new(None::<PlaybackState>))
        .manage(Mutex::new(None::<UsbStreamState>))
        .setup(|app| {
            let cache_root = app
                .path()
                .app_data_dir()
                .map(|d| d.join("xc-cache"))
                .unwrap_or_else(|_| std::path::PathBuf::from("xc-cache"));
            let _ = std::fs::create_dir_all(&cache_root);
            app.manage(Mutex::new(xc::XcState {
                client: reqwest::Client::new(),
                cache_root,
            }));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            save_recording,
            mic_open,
            mic_close,
            mic_start_recording,
            mic_stop_recording,
            mic_set_listening,
            mic_get_status,
            mic_list_devices,
            audio_file_info,
            audio_decode_full,
            read_file_bytes,
            read_file_range,
            native_play,
            native_stop,
            native_playback_status,
            xc::xc_set_api_key,
            xc::xc_get_api_key,
            xc::xc_browse_group,
            xc::xc_refresh_taxonomy,
            xc::xc_taxonomy_age,
            xc::xc_search,
            xc::xc_species_recordings,
            xc::xc_download,
            xc::xc_is_cached,
            usb_start_stream,
            usb_stop_stream,
            usb_start_recording,
            usb_stop_recording,
            usb_stream_status,
            save_noise_preset,
            load_noise_preset,
            list_noise_presets,
            delete_noise_preset,
            read_sidecar,
            write_sidecar,
            read_central_annotations,
            write_central_annotations,
            export_annotations_file,
            open_file_dialog,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
