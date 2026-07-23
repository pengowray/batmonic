use crate::recording::{self, MicInfo, MicStatus, RecordingResult};
use crate::recovery;
use crate::MicMutex;
use oversample_ipc::mic::DeviceListResult;
use std::sync::atomic::Ordering;
use tauri::Manager;

/// Get the human-readable cpal audio host name for the current platform.
/// Returns names like "Oboe", "WASAPI", "ASIO", "CoreAudio", "ALSA", "JACK".
fn cpal_host_name() -> String {
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

/// Copy the finalized `.wav.part` → `shared_fd` (Android MediaStore) using
/// `std::io::copy`, so even a multi-GB file streams without a big in-memory
/// blob. On non-Android platforms `shared_fd` should never be set; returns an
/// explicit error if it is.
#[cfg(target_os = "android")]
pub fn stream_finalized_to_shared_fd(
    finalized_path: &std::path::Path,
    fd: i32,
) -> Result<(), String> {
    use std::os::unix::io::FromRawFd;
    let mut src = std::fs::File::open(finalized_path)
        .map_err(|e| format!("recovery: open finalized for copy: {}", e))?;
    let mut dst = unsafe { std::fs::File::from_raw_fd(fd) };
    std::io::copy(&mut src, &mut dst).map_err(|e| format!("recovery: copy to shared fd: {}", e))?;
    Ok(())
}

#[cfg(not(target_os = "android"))]
pub fn stream_finalized_to_shared_fd(
    _finalized_path: &std::path::Path,
    _fd: i32,
) -> Result<(), String> {
    Err("shared_fd only supported on Android".into())
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
    // (also does best-effort disk flushing for crash-recovery).
    recording::start_emitter(m.buffer.clone(), m.emitter_stop.clone(), m.recovery.clone());

    *mic = Some(m);
    Ok(info)
}

#[tauri::command]
pub fn mic_list_devices() -> DeviceListResult {
    DeviceListResult {
        devices: recording::list_input_devices(),
        host_name: cpal_host_name(),
    }
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
#[allow(clippy::too_many_arguments)]
pub fn mic_start_recording(
    app: tauri::AppHandle,
    state: tauri::State<MicMutex>,
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
    force_bits: Option<u16>,
    preroll_samples: Option<u32>,
) -> Result<(), String> {
    let mic = state.lock().map_err(|e| e.to_string())?;
    let m = mic.as_ref().ok_or("Microphone not open")?;
    {
        let mut buf = m.buffer.lock().unwrap();
        buf.clear();
        buf.shared_fd = shared_fd;
        buf.force_bits = force_bits;
        // Seed the just-cleared buffer with the listening pre-roll ring, then
        // flip is_recording while still holding the buffer lock so the next
        // audio callback appends live frames immediately after the pre-roll —
        // no gap, no duplication. The recovery writer is installed just below;
        // the emitter only flushes once it exists, so nothing reaches disk
        // before the placeholder header.
        let seeded = buf.seed_preroll(preroll_samples.unwrap_or(0) as usize);
        m.is_recording.store(true, Ordering::Relaxed);
        if seeded > 0 {
            eprintln!(
                "mic_start_recording: seeded {} pre-roll samples to disk",
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
    if let Some(writer) = recovery::start_writer(
        &app,
        &args,
        m.format,
        m.sample_rate,
        m.channels as u16,
        "batcap",
    ) {
        if let Ok(mut guard) = m.recovery.writer.lock() {
            *guard = Some(writer);
        }
    }

    // is_recording was set under the buffer lock above (with the pre-roll seed).
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
    skip_native_save: Option<bool>,
) -> Result<RecordingResult, String> {
    let mic = state.lock().map_err(|e| e.to_string())?;
    let m = mic.as_ref().ok_or("Microphone not open")?;
    m.is_recording.store(false, Ordering::Relaxed);

    // Snapshot totals before we drain the tail — we want the sample count as
    // captured, not the remainder-in-memory count.
    let (num_samples, sample_rate, shared_fd, preroll_seeded) = {
        let mut buf = m.buffer.lock().unwrap();
        (
            buf.total_samples,
            buf.sample_rate,
            buf.shared_fd.take(),
            buf.preroll_seeded,
        )
    };
    // RAII-own the shared-storage fd so every early return below (no-samples,
    // skip-save, finalize error) closes it instead of leaking it.
    let mut shared_fd = recording::SharedFdGuard::new(shared_fd);
    let bits_per_sample = m.format.bits_per_sample();
    let is_float = m.format.is_float();
    let duration_secs = num_samples as f64 / sample_rate as f64;
    let skip_save = skip_native_save.unwrap_or(false);

    // Take the recovery writer out. Done INSIDE the writer lock to race-safely
    // handle the emitter — while we hold the lock the emitter can't flush.
    let recovery_writer = m.recovery.writer.lock().ok().and_then(|mut g| g.take());
    let streaming_mode = recovery_writer.is_some();

    if num_samples == 0 {
        if let Some(writer) = recovery_writer {
            recovery::cleanup(writer);
        }
        return Err("No samples recorded".into());
    }

    // Pre-roll capture will re-encode the WAV on the WASM side, so whatever
    // we've streamed to the partial file is redundant — throw it away.
    if skip_save {
        if let Some(writer) = recovery_writer {
            recovery::cleanup(writer);
        }
        return Ok(RecordingResult {
            filename: String::new(),
            saved_path: String::new(),
            sample_rate,
            bits_per_sample,
            is_float,
            duration_secs,
            num_samples,
            has_memory_samples: false,
            file_size_bytes: 0,
        });
    }

    // Build the GUANO chunk for either path below.
    let now = chrono::Local::now();
    let filename_ts = now.format("batcap_%Y%m%d_%H%M%S.wav").to_string();
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
    let guano_params = recording::TauriGuanoParams {
        connection_type: Some(host_name),
        location,
        device_make,
        device_model,
        mic_name: Some("Internal".to_string()),
        mic_make: None,
        app_version: app_version.unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string()),
        is_mobile,
        preroll_secs: (preroll_seeded > 0 && sample_rate > 0)
            .then(|| preroll_seeded as f64 / sample_rate as f64),
    };
    let guano =
        recording::build_tauri_guano(sample_rate, num_samples, &filename_ts, &now, &guano_params);
    let guano_text = guano.to_text();

    let (saved_path, file_size_bytes, has_memory_samples) = if streaming_mode {
        // Streaming-to-disk: the partial file already contains every sample
        // that was flushed. Take whatever's still in the tail buffer, append
        // it + the GUANO chunk, patch the header, then move the finished file
        // to the destination. No big in-memory blob involved.
        let writer = recovery_writer.expect("streaming_mode implies writer");
        let final_bytes = {
            let mut buf = m.buffer.lock().unwrap();
            recovery::drain_cpal_bytes(&mut buf)
        };
        // Everything below operates on the owned writer / paths, not the mic
        // state. Release the MicMutex before the (potentially large) in-place
        // finalize + shared-storage copy so it can't block mic_get_status or a
        // concurrent mic command for the duration of the copy.
        drop(mic);
        let finalized_path =
            recovery::finalize_in_place_and_take(writer, &final_bytes, &guano_text)
                .map_err(|e| format!("recovery finalize failed: {}", e))?;

        let final_size = finalized_path.metadata().map(|m| m.len()).unwrap_or(0);

        let saved_path = if let Some(fd) = shared_fd.take() {
            // Copy internal → shared fd (streaming, no big RAM blob), then
            // drop the internal copy. On Android this writes to the MediaStore
            // URI the frontend reserved at record start.
            stream_finalized_to_shared_fd(&finalized_path, fd)?;
            let _ = std::fs::remove_file(&finalized_path);
            "shared://recording".to_string()
        } else {
            // Move .wav.part → recordings/<name>.wav
            let dir = app
                .path()
                .app_data_dir()
                .map_err(|e| e.to_string())?
                .join("recordings");
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let target = dir.join(&filename_ts);
            std::fs::rename(&finalized_path, &target)
                .map_err(|e| format!("recovery: rename to final path: {}", e))?;
            target.to_string_lossy().to_string()
        };
        (saved_path, final_size as usize, false)
    } else {
        // To-memory mode: encode from the accumulated in-memory samples (no
        // streaming happened). The captured samples are stashed on the native
        // side for the WASM finalizer to fetch as raw f32 bytes via
        // `mic_take_recorded_samples` (instead of a giant inline JSON array).
        let buf = m.buffer.lock().unwrap();
        let samples_f32 = recording::get_samples_f32(&buf);
        let mut wav_data = recording::encode_native_wav(&buf)?;
        drop(buf);
        // All mic-state reads are done; release the MicMutex before the GUANO
        // append + (possibly large) shared-storage write, mirroring the
        // streaming branch above so mic_get_status / other mic commands don't
        // block for the write's duration.
        drop(mic);
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
            let full_path = dir.join(&filename_ts);
            std::fs::write(&full_path, &wav_data).map_err(|e| e.to_string())?;
            full_path.to_string_lossy().to_string()
        };
        // Stash the captured samples for the WASM finalizer to pull as raw bytes.
        let has_mem = !samples_f32.is_empty();
        *app.state::<crate::RecordedMemoryMutex>()
            .inner()
            .lock()
            .unwrap() = samples_f32;
        (path, file_size_bytes, has_mem)
    };

    Ok(RecordingResult {
        filename: filename_ts,
        saved_path,
        sample_rate,
        bits_per_sample,
        is_float,
        duration_secs,
        num_samples,
        has_memory_samples,
        file_size_bytes,
    })
}

/// Scan the crash-recovery directory and finalize any leftover `.wav.part`
/// files. Call once on app startup. Returns the list of recovered recordings;
/// files have already been moved into the recordings directory.
#[tauri::command]
pub fn mic_recover_recordings(app: tauri::AppHandle) -> Vec<recovery::RecoveredRecording> {
    let app_data = match app.path().app_data_dir() {
        Ok(p) => p,
        Err(_) => return Vec::new(),
    };
    recovery::recover_leftover_recordings(&app_data)
}

#[tauri::command]
pub fn mic_set_listening(state: tauri::State<MicMutex>, listening: bool) -> Result<(), String> {
    let mic = state.lock().map_err(|e| e.to_string())?;
    let m = mic.as_ref().ok_or("Microphone not open")?;
    m.is_streaming.store(listening, Ordering::Relaxed);
    // Start the pre-roll ring fresh each time listening begins so a long-press
    // record can't pick up stale audio from a previous listen session.
    if listening {
        if let Ok(mut buf) = m.buffer.lock() {
            buf.clear_preroll_rings();
        }
    }
    Ok(())
}

/// Drain the pending streamed samples of the source the frontend is actively
/// using (`source == "usb"` → USB engine, else the cpal mic), returned to the
/// pull loop as raw little-endian f32 bytes (an ArrayBuffer). Replaces the JSON
/// `mic-audio-chunk` event push (an array of numbers is ~5x the bytes and parses
/// element-by-element in WASM).
///
/// Draining the explicitly-selected source (rather than guessing by which mutex
/// is `Some`) avoids reading a stale device's buffer if a previous backend
/// wasn't fully torn down during a device switch. Returns an empty buffer when
/// nothing is pending or the selected source isn't open.
#[tauri::command]
pub fn mic_pull_audio(
    source: String,
    mic: tauri::State<MicMutex>,
    usb: tauri::State<crate::UsbStreamMutex>,
) -> tauri::ipc::Response {
    let samples: Vec<f32> = if source == "usb" {
        usb.lock()
            .ok()
            .and_then(|g| {
                g.as_ref()
                    .and_then(|u| u.buffer.lock().ok().map(|mut b| b.drain_pending()))
            })
            .unwrap_or_default()
    } else {
        mic.lock()
            .ok()
            .and_then(|g| {
                g.as_ref()
                    .and_then(|m| m.buffer.lock().ok().map(|mut b| b.drain_pending()))
            })
            .unwrap_or_default()
    };

    let mut bytes = Vec::with_capacity(samples.len() * 4);
    for s in samples {
        bytes.extend_from_slice(&s.to_le_bytes());
    }
    tauri::ipc::Response::new(bytes)
}

/// Take the to-memory recording samples stashed by `mic_stop_recording` /
/// `usb_stop_recording`, returned as raw little-endian f32 bytes (an
/// ArrayBuffer to the frontend) and clearing the stash. Empty when no
/// to-memory recording is awaiting pickup.
#[tauri::command]
pub fn mic_take_recorded_samples(
    recorded: tauri::State<crate::RecordedMemoryMutex>,
) -> tauri::ipc::Response {
    let samples = recorded
        .lock()
        .map(|mut g| std::mem::take(&mut *g))
        .unwrap_or_default();
    let mut bytes = Vec::with_capacity(samples.len() * 4);
    for s in samples {
        bytes.extend_from_slice(&s.to_le_bytes());
    }
    tauri::ipc::Response::new(bytes)
}

#[tauri::command]
pub fn mic_get_status(state: tauri::State<MicMutex>) -> MicStatus {
    let mic = state.lock().unwrap_or_else(|e| e.into_inner());
    match mic.as_ref() {
        Some(m) => {
            use crate::recording::NativeSampleFormat;
            let (samples, effective_bits) = m
                .buffer
                .lock()
                .map(|b| {
                    // Detection only applies to i32 (24/32-bit) container streams.
                    let eff = matches!(b.format, NativeSampleFormat::I24 | NativeSampleFormat::I32)
                        .then(|| b.effective_bits(32))
                        .flatten();
                    (b.total_samples, eff)
                })
                .unwrap_or((0, None));
            MicStatus {
                is_open: true,
                is_recording: m.is_recording.load(Ordering::Relaxed),
                is_streaming: m.is_streaming.load(Ordering::Relaxed),
                samples_recorded: samples,
                sample_rate: m.sample_rate,
                effective_bits,
            }
        }
        None => MicStatus {
            is_open: false,
            is_recording: false,
            is_streaming: false,
            samples_recorded: 0,
            sample_rate: 0,
            effective_bits: None,
        },
    }
}
