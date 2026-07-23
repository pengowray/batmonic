use crate::recording::{self, NativeSampleFormat, RecordingResult};
use crate::recovery;
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
    uac_version: Option<u32>,
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
        uac_version.unwrap_or(0),
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
#[allow(clippy::too_many_arguments)]
pub fn usb_start_recording(
    app: tauri::AppHandle,
    state: tauri::State<UsbStreamMutex>,
    shared_fd: Option<i32>,
    filename: Option<String>,
    connection_type: Option<String>,
    mic_name: Option<String>,
    mic_make: Option<String>,
    device_make: Option<String>,
    device_model: Option<String>,
    app_version: Option<String>,
    loc_latitude: Option<f64>,
    loc_longitude: Option<f64>,
    loc_elevation: Option<f64>,
    loc_accuracy: Option<f64>,
    enable_recovery: Option<bool>,
    preroll_samples: Option<u32>,
) -> Result<(), String> {
    let usb = state.lock().map_err(|e| e.to_string())?;
    let s = usb.as_ref().ok_or("USB stream not open")?;
    if !s.is_streaming.load(Ordering::Relaxed) {
        return Err("USB stream is not actively streaming — cannot start recording".into());
    }
    {
        let mut buf = s.buffer.lock().unwrap();
        buf.clear();
        buf.shared_fd = shared_fd;
        // Seed the pre-roll ring into the recording, then flip is_recording while
        // still holding the buffer lock so the isochronous thread appends live
        // frames right after the pre-roll (gapless seam).
        let seeded = buf.seed_preroll(preroll_samples.unwrap_or(0) as usize);
        s.is_recording.store(true, Ordering::Relaxed);
        if seeded > 0 {
            eprintln!(
                "usb_start_recording: seeded {} pre-roll samples to disk",
                seeded
            );
        }
    }

    let args = recovery::StartArgs {
        filename,
        connection_type,
        mic_name,
        mic_make,
        device_make,
        device_model,
        app_version,
        loc_latitude,
        loc_longitude,
        loc_elevation,
        loc_accuracy,
        enable_recovery,
    };
    // USB streams are always 16-bit mono in this implementation.
    if let Some(writer) = recovery::start_writer(
        &app,
        &args,
        NativeSampleFormat::I16,
        s.sample_rate,
        1,
        "batcap",
    ) {
        if let Ok(mut guard) = s.recovery.writer.lock() {
            *guard = Some(writer);
        }
    }

    // is_recording was set under the buffer lock above (with the pre-roll seed).
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
    device_make: Option<String>,
    device_model: Option<String>,
    app_version: Option<String>,
    skip_native_save: Option<bool>,
) -> Result<RecordingResult, String> {
    let usb = state.lock().map_err(|e| e.to_string())?;
    let s = usb.as_ref().ok_or("USB stream not open")?;
    s.is_recording.store(false, Ordering::Relaxed);
    let still_streaming = s.is_streaming.load(Ordering::Relaxed);

    let (num_samples, sample_rate, shared_fd, preroll_seeded) = {
        let mut buf = s.buffer.lock().unwrap();
        (
            buf.total_samples,
            buf.sample_rate,
            buf.shared_fd.take(),
            buf.preroll_seeded,
        )
    };
    // RAII-own the shared-storage fd so every early return below closes it.
    let mut shared_fd = recording::SharedFdGuard::new(shared_fd);

    // Take the recovery writer out. Done under the writer lock so the emitter
    // can't race us and flush between here and the final tail drain below.
    let recovery_writer = s.recovery.writer.lock().ok().and_then(|mut g| g.take());
    let streaming_mode = recovery_writer.is_some();

    if num_samples == 0 {
        if let Some(writer) = recovery_writer {
            recovery::cleanup(writer);
        }
        if !still_streaming {
            return Err("No samples recorded — USB stream died during recording".into());
        }
        return Err("No samples recorded — stream was active but buffer is empty".into());
    }

    let duration_secs = num_samples as f64 / sample_rate as f64;
    let skip_save = skip_native_save.unwrap_or(false);

    if skip_save {
        // WASM side will re-encode with pre-roll; streamed partial is redundant.
        if let Some(writer) = recovery_writer {
            recovery::cleanup(writer);
        }
        drop(shared_fd); // close the unused shared fd
        return Ok(RecordingResult {
            filename: String::new(),
            saved_path: String::new(),
            sample_rate,
            bits_per_sample: 16,
            is_float: false,
            duration_secs,
            num_samples,
            has_memory_samples: false,
            file_size_bytes: 0,
        });
    }

    let now = chrono::Local::now();
    let filename = now.format("batcap_%Y%m%d_%H%M%S.wav").to_string();
    let location = match (loc_latitude, loc_longitude) {
        (Some(lat), Some(lon)) => Some(recording::RecordingLocation {
            latitude: lat,
            longitude: lon,
            elevation: loc_elevation,
            accuracy: loc_accuracy,
        }),
        _ => None,
    };
    let device_lower = s.device_name.to_lowercase();
    let interface_label = if device_lower.contains("echo meter") || device_lower.contains("emt2") {
        "USB (EMT2)".to_string()
    } else {
        match s.uac_version {
            2 => "USB (UAC2)".to_string(),
            1 => "USB (UAC1)".to_string(),
            _ => "USB (UAC)".to_string(),
        }
    };
    let is_mobile = cfg!(target_os = "android");
    let guano_params = recording::TauriGuanoParams {
        connection_type: Some(interface_label),
        location,
        device_make,
        device_model,
        mic_name: Some(s.device_name.clone()),
        mic_make: None,
        app_version: app_version.unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string()),
        is_mobile,
        preroll_secs: (preroll_seeded > 0 && sample_rate > 0)
            .then(|| preroll_seeded as f64 / sample_rate as f64),
    };
    let guano =
        recording::build_tauri_guano(sample_rate, num_samples, &filename, &now, &guano_params);
    let guano_text = guano.to_text();

    let (saved_path, file_size_bytes, has_memory_samples) = if streaming_mode {
        // Streaming-to-disk: partial file has every flushed sample. Append
        // the tail + GUANO, patch the header, then move to destination.
        let writer = recovery_writer.expect("streaming_mode implies writer");
        let final_bytes = {
            let mut buf = s.buffer.lock().unwrap();
            usb_audio::drain_usb_recovery_bytes(&mut buf)
        };
        // Everything below operates on the owned writer / bytes / paths, not the
        // USB state. Release the UsbStreamMutex before the (possibly multi-GB)
        // in-place finalize + shared-storage copy so it can't block
        // usb_stream_status or a concurrent USB command for the copy's duration.
        drop(usb);
        let finalized_path =
            recovery::finalize_in_place_and_take(writer, &final_bytes, &guano_text)
                .map_err(|e| format!("recovery finalize failed: {}", e))?;
        let final_size = finalized_path.metadata().map(|m| m.len()).unwrap_or(0) as usize;

        let saved_path = if let Some(fd) = shared_fd.take() {
            crate::cmd_mic::stream_finalized_to_shared_fd(&finalized_path, fd)?;
            let _ = std::fs::remove_file(&finalized_path);
            "shared://recording".to_string()
        } else {
            let dir = app
                .path()
                .app_data_dir()
                .map_err(|e| e.to_string())?
                .join("recordings");
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let target = dir.join(&filename);
            std::fs::rename(&finalized_path, &target)
                .map_err(|e| format!("recovery: rename to final path: {}", e))?;
            target.to_string_lossy().to_string()
        };
        (saved_path, final_size, false)
    } else {
        // To-memory mode: encode the accumulated i16 buffer; stash the samples
        // for the WASM finalizer to pull as raw bytes (mic_take_recorded_samples).
        let samples_f32 = usb_audio::get_usb_samples_f32(s);
        let mut wav_data = usb_audio::encode_usb_wav(s)?;
        // All USB-state reads are done; release the UsbStreamMutex before the
        // GUANO append + (possibly large) shared-storage write so concurrent
        // status polls / USB commands don't block for the write's duration.
        drop(usb);
        oversample_core::audio::guano::append_guano_chunk(&mut wav_data, &guano_text);
        let file_size_bytes = wav_data.len();
        let path = if let Some(fd) = shared_fd.take() {
            recording::write_wav_to_fd(fd, &wav_data)?;
            "shared://recording".to_string()
        } else {
            let dir = app
                .path()
                .app_data_dir()
                .map_err(|e| e.to_string())?
                .join("recordings");
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let full_path = dir.join(&filename);
            std::fs::write(&full_path, &wav_data).map_err(|e| e.to_string())?;
            full_path.to_string_lossy().to_string()
        };
        let has_mem = !samples_f32.is_empty();
        *app.state::<crate::RecordedMemoryMutex>()
            .inner()
            .lock()
            .unwrap() = samples_f32;
        (path, file_size_bytes, has_mem)
    };

    Ok(RecordingResult {
        filename,
        saved_path,
        sample_rate,
        bits_per_sample: 16,
        is_float: false,
        duration_secs,
        num_samples,
        has_memory_samples,
        file_size_bytes,
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
