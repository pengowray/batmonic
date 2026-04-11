use crate::native_playback::{self, NativePlayParams, PlaybackStatus};
use crate::PlaybackMutex;

#[tauri::command]
pub fn native_play(
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
pub fn native_stop(state: tauri::State<PlaybackMutex>) -> Result<(), String> {
    let mut pb = state.lock().map_err(|e| e.to_string())?;
    native_playback::stop(&mut pb);
    Ok(())
}

#[tauri::command]
pub fn native_playback_status(state: tauri::State<PlaybackMutex>) -> PlaybackStatus {
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
