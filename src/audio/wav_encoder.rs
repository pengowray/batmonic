use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

/// Encode f32 samples as a 16-bit PCM WAV file (web mode fallback).
pub fn encode_wav(samples: &[f32], sample_rate: u32) -> Vec<u8> {
    let num_samples = samples.len();
    let data_size = num_samples * 2;
    let file_size = 36 + data_size;

    let mut buf = Vec::with_capacity(44 + data_size);

    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&(file_size as u32).to_le_bytes());
    buf.extend_from_slice(b"WAVE");

    buf.extend_from_slice(b"fmt ");
    buf.extend_from_slice(&16u32.to_le_bytes());
    buf.extend_from_slice(&1u16.to_le_bytes());  // PCM
    buf.extend_from_slice(&1u16.to_le_bytes());  // mono
    buf.extend_from_slice(&sample_rate.to_le_bytes());
    buf.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    buf.extend_from_slice(&2u16.to_le_bytes());  // block align
    buf.extend_from_slice(&16u16.to_le_bytes()); // bits per sample

    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&(data_size as u32).to_le_bytes());
    for &sample in samples {
        let clamped = sample.clamp(-1.0, 1.0);
        let val = (clamped * 32767.0) as i16;
        buf.extend_from_slice(&val.to_le_bytes());
    }

    buf
}

/// Encode WAV with GUANO metadata for a new recording.
pub(crate) fn encode_wav_with_guano(samples: &[f32], sample_rate: u32, filename: &str) -> Vec<u8> {
    use crate::audio::guano;
    let mut wav_data = encode_wav(samples, sample_rate);

    let now = js_sys::Date::new_0();
    let duration_secs = samples.len() as f64 / sample_rate as f64;
    // Approximate recording start time
    let start_ms = now.get_time() - (duration_secs * 1000.0);
    let start = js_sys::Date::new(&JsValue::from_f64(start_ms));
    let timestamp = format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        start.get_full_year(),
        start.get_month() + 1,
        start.get_date(),
        start.get_hours(),
        start.get_minutes(),
        start.get_seconds(),
    );
    let version = env!("CARGO_PKG_VERSION");

    let mut guano_meta = guano::GuanoMetadata::new();
    guano_meta.add("GUANO|Version", "1.0");
    guano_meta.add("Timestamp", &timestamp);
    guano_meta.add("Length", &format!("{:.6}", duration_secs));
    guano_meta.add("Samplerate", &sample_rate.to_string());
    guano_meta.add("Make", "batmonic");
    guano_meta.add("Firmware Version", version);
    guano_meta.add("Original Filename", filename);
    guano_meta.add("Note", &format!("Recorded with batmonic v{} (browser)", version));

    guano::append_guano_chunk(&mut wav_data, &guano_meta.to_text());
    wav_data
}

/// Trigger a browser download of WAV data.
pub fn download_wav(samples: &[f32], sample_rate: u32, filename: &str) {
    let wav_data = encode_wav_with_guano(samples, sample_rate, filename);

    let array = js_sys::Uint8Array::new_with_length(wav_data.len() as u32);
    array.copy_from(&wav_data);

    let parts = js_sys::Array::new();
    parts.push(&array.buffer());

    let blob = match web_sys::Blob::new_with_u8_array_sequence(&parts) {
        Ok(b) => b,
        Err(e) => {
            log::error!("Failed to create Blob: {:?}", e);
            return;
        }
    };

    let url = match web_sys::Url::create_object_url_with_blob(&blob) {
        Ok(u) => u,
        Err(e) => {
            log::error!("Failed to create object URL: {:?}", e);
            return;
        }
    };

    let window = web_sys::window().unwrap();
    let document = window.document().unwrap();
    let a: web_sys::HtmlAnchorElement = document
        .create_element("a").unwrap()
        .dyn_into().unwrap();
    a.set_href(&url);
    a.set_download(filename);
    a.set_attribute("style", "display:none").ok();
    document.body().unwrap().append_child(&a).ok();
    a.click();
    document.body().unwrap().remove_child(&a).ok();
    web_sys::Url::revoke_object_url(&url).ok();
}

/// Try to save recording via Tauri IPC (web mode). Returns true on success.
pub(crate) async fn try_tauri_save(wav_data: &[u8], filename: &str) -> bool {
    use crate::tauri_bridge::get_tauri_internals;

    let tauri = match get_tauri_internals() {
        Some(t) => t,
        None => return false,
    };

    let invoke = match js_sys::Reflect::get(&tauri, &JsValue::from_str("invoke")) {
        Ok(f) if f.is_function() => js_sys::Function::from(f),
        _ => return false,
    };

    let args = js_sys::Object::new();
    js_sys::Reflect::set(&args, &JsValue::from_str("filename"), &JsValue::from_str(filename)).ok();

    let array = js_sys::Uint8Array::new_with_length(wav_data.len() as u32);
    array.copy_from(wav_data);
    js_sys::Reflect::set(&args, &JsValue::from_str("data"), &array).ok();

    let result = invoke.call2(&tauri, &JsValue::from_str("save_recording"), &args);
    match result {
        Ok(promise_val) => {
            if let Ok(promise) = promise_val.dyn_into::<js_sys::Promise>() {
                match JsFuture::from(promise).await {
                    Ok(path) => {
                        log::info!("Saved recording to: {:?}", path.as_string());
                        true
                    }
                    Err(e) => {
                        log::error!("Tauri save failed: {:?}", e);
                        false
                    }
                }
            } else {
                false
            }
        }
        Err(e) => {
            log::error!("Tauri invoke failed: {:?}", e);
            false
        }
    }
}
