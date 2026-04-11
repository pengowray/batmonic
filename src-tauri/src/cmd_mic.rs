use crate::recording::{self, DeviceInfo, MicInfo, MicStatus, RecordingResult};
use crate::MicMutex;
use std::sync::atomic::Ordering;
use tauri::Manager;

/// Get the human-readable cpal audio host name for the current platform.
/// Returns names like "Oboe", "WASAPI", "ASIO", "CoreAudio", "ALSA", "JACK".
fn cpal_host_name() -> String {
    use cpal::traits::HostTrait;
    let raw = format!("{:?}", cpal::default_host().id());
    // Normalize common host names to match GUANO conventions
    match raw.as_str() {
        "Wasapi" => "WASAPI".to_string(),
        "Asio" => "ASIO".to_string(),
        "Alsa" => "ALSA".to_string(),
        "Jack" => "JACK".to_string(),
        other => other.to_string(), // "Oboe", "CoreAudio", etc. already good
    }
}

#[tauri::command]
pub fn save_recording(
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
pub fn mic_open(
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
            host_name: cpal_host_name(),
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
        host_name: cpal_host_name(),
    };

    // Start the emitter thread for streaming audio chunks to the frontend
    recording::start_emitter(app, m.buffer.clone(), m.emitter_stop.clone());

    *mic = Some(m);
    Ok(info)
}

#[tauri::command]
pub fn mic_list_devices() -> Vec<DeviceInfo> {
    recording::list_input_devices()
}

#[tauri::command]
pub fn mic_close(state: tauri::State<MicMutex>) -> Result<(), String> {
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
pub fn mic_start_recording(state: tauri::State<MicMutex>, shared_fd: Option<i32>) -> Result<(), String> {
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
pub fn mic_stop_recording(
    app: tauri::AppHandle,
    state: tauri::State<MicMutex>,
    loc_latitude: Option<f64>,
    loc_longitude: Option<f64>,
    loc_elevation: Option<f64>,
    loc_accuracy: Option<f64>,
    device_make: Option<String>,
    device_model: Option<String>,
    app_version: Option<String>,
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

    let is_mobile = cfg!(target_os = "android");
    let host_name = cpal_host_name();

    // Append GUANO metadata using shared builder
    let guano_params = recording::TauriGuanoParams {
        connection_type: Some(host_name),
        location,
        device_make,
        device_model,
        mic_name: Some("Internal".to_string()),
        mic_make: None,
        app_version: app_version.unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string()),
        is_mobile,
    };
    let guano = recording::build_tauri_guano(
        sample_rate, num_samples, &filename, &now, &guano_params,
    );
    oversample_core::audio::guano::append_guano_chunk(&mut wav_data, &guano.to_text());

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
pub fn mic_set_listening(state: tauri::State<MicMutex>, listening: bool) -> Result<(), String> {
    let mic = state.lock().map_err(|e| e.to_string())?;
    let m = mic.as_ref().ok_or("Microphone not open")?;
    m.is_streaming.store(listening, Ordering::Relaxed);
    Ok(())
}

#[tauri::command]
pub fn mic_get_status(state: tauri::State<MicMutex>) -> MicStatus {
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
