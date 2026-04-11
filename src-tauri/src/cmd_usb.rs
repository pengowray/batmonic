use crate::recording::{self, RecordingResult};
use crate::usb_audio::{self, UsbStreamInfo, UsbStreamStatus};
use crate::UsbStreamMutex;
use std::sync::atomic::Ordering;
use tauri::Manager;

#[tauri::command]
pub fn usb_start_stream(
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
pub fn usb_stop_stream(state: tauri::State<UsbStreamMutex>) -> Result<(), String> {
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
pub fn usb_start_recording(state: tauri::State<UsbStreamMutex>, shared_fd: Option<i32>) -> Result<(), String> {
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
pub fn usb_stop_recording(
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
pub fn usb_stream_status(state: tauri::State<UsbStreamMutex>) -> UsbStreamStatus {
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
