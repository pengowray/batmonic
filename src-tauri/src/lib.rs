mod audio_decode;
mod cmd_annotations;
mod cmd_audio_files;
mod cmd_mic;
mod cmd_noise_presets;
mod cmd_playback;
mod cmd_usb;
mod native_playback;
mod recording;
mod usb_audio;
mod xc;

use native_playback::PlaybackState;
use recording::MicState;
use std::sync::Mutex;
use tauri::Manager;
use usb_audio::UsbStreamState;

pub(crate) type MicMutex = Mutex<Option<MicState>>;
pub(crate) type PlaybackMutex = Mutex<Option<PlaybackState>>;
pub(crate) type UsbStreamMutex = Mutex<Option<UsbStreamState>>;

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
            cmd_mic::save_recording,
            cmd_mic::mic_open,
            cmd_mic::mic_close,
            cmd_mic::mic_start_recording,
            cmd_mic::mic_stop_recording,
            cmd_mic::mic_set_listening,
            cmd_mic::mic_get_status,
            cmd_mic::mic_list_devices,
            cmd_audio_files::audio_file_info,
            cmd_audio_files::audio_decode_full,
            cmd_audio_files::read_file_bytes,
            cmd_audio_files::read_file_range,
            cmd_playback::native_play,
            cmd_playback::native_stop,
            cmd_playback::native_playback_status,
            xc::xc_set_api_key,
            xc::xc_get_api_key,
            xc::xc_browse_group,
            xc::xc_refresh_taxonomy,
            xc::xc_taxonomy_age,
            xc::xc_search,
            xc::xc_species_recordings,
            xc::xc_download,
            xc::xc_is_cached,
            cmd_usb::usb_start_stream,
            cmd_usb::usb_stop_stream,
            cmd_usb::usb_start_recording,
            cmd_usb::usb_stop_recording,
            cmd_usb::usb_stream_status,
            cmd_noise_presets::save_noise_preset,
            cmd_noise_presets::load_noise_preset,
            cmd_noise_presets::list_noise_presets,
            cmd_noise_presets::delete_noise_preset,
            cmd_annotations::read_sidecar,
            cmd_annotations::write_sidecar,
            cmd_annotations::read_central_annotations,
            cmd_annotations::write_central_annotations,
            cmd_annotations::export_annotations_file,
            cmd_annotations::open_file_dialog,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
