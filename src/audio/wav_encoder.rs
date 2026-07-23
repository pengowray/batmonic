// SPDX-License-Identifier: GPL-3.0-only OR MIT OR Apache-2.0
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

use crate::types::WavMarker;

/// Build WAV `cue ` and `LIST`/`adtl` chunks for the given markers.
/// Returns the raw bytes to insert into the RIFF stream (before the GUANO chunk).
pub fn encode_wav_cue_chunks(markers: &[WavMarker]) -> Vec<u8> {
    if markers.is_empty() {
        return Vec::new();
    }

    let mut buf = Vec::new();

    // ── cue chunk ──
    // cue chunk body: u32 num_cue_points, then per point:
    //   u32 id, u32 position, 4-byte data_id ("data"), u32 chunk_start, u32 block_start, u32 sample_offset
    let num_points = markers.len() as u32;
    let cue_body_size = 4 + num_points * 24;
    buf.extend_from_slice(b"cue ");
    buf.extend_from_slice(&cue_body_size.to_le_bytes());
    buf.extend_from_slice(&num_points.to_le_bytes());
    for m in markers {
        buf.extend_from_slice(&m.id.to_le_bytes()); // id
        buf.extend_from_slice(&(m.position as u32).to_le_bytes()); // position
        buf.extend_from_slice(b"data"); // fcc_chunk
        buf.extend_from_slice(&0u32.to_le_bytes()); // chunk_start
        buf.extend_from_slice(&0u32.to_le_bytes()); // block_start
        buf.extend_from_slice(&(m.position as u32).to_le_bytes()); // sample_offset
    }

    // ── LIST/adtl chunk with labl and note sub-chunks ──
    let mut adtl_body = Vec::new();
    adtl_body.extend_from_slice(b"adtl");
    for m in markers {
        if let Some(ref label) = m.label {
            let text_bytes = label.as_bytes();
            // labl sub-chunk: u32 cue_id + null-terminated string
            let sub_size = 4 + text_bytes.len() as u32 + 1; // +1 for null terminator
            adtl_body.extend_from_slice(b"labl");
            adtl_body.extend_from_slice(&sub_size.to_le_bytes());
            adtl_body.extend_from_slice(&m.id.to_le_bytes());
            adtl_body.extend_from_slice(text_bytes);
            adtl_body.push(0); // null terminator
                               // RIFF word-alignment padding
            if sub_size % 2 != 0 {
                adtl_body.push(0);
            }
        }
        if let Some(ref note) = m.note {
            let text_bytes = note.as_bytes();
            let sub_size = 4 + text_bytes.len() as u32 + 1;
            adtl_body.extend_from_slice(b"note");
            adtl_body.extend_from_slice(&sub_size.to_le_bytes());
            adtl_body.extend_from_slice(&m.id.to_le_bytes());
            adtl_body.extend_from_slice(text_bytes);
            adtl_body.push(0);
            if sub_size % 2 != 0 {
                adtl_body.push(0);
            }
        }
    }

    // Only write the LIST chunk if there are labl/note sub-chunks
    if adtl_body.len() > 4 {
        let list_size = adtl_body.len() as u32;
        buf.extend_from_slice(b"LIST");
        buf.extend_from_slice(&list_size.to_le_bytes());
        buf.extend_from_slice(&adtl_body);
        // Word-align the LIST chunk
        if list_size % 2 != 0 {
            buf.push(0);
        }
    }

    buf
}

/// Insert cue marker chunks into an already-encoded WAV byte buffer.
/// The chunks are inserted just before the GUANO chunk (if present) or at end of RIFF.
/// Also updates the RIFF file size field.
pub fn insert_cue_chunks(wav: &mut Vec<u8>, cue_bytes: &[u8]) {
    if cue_bytes.is_empty() || wav.len() < 44 {
        return;
    }

    // Find insertion point: just before "guan" chunk, or at end
    let mut insert_pos = wav.len();
    let mut pos = 12; // skip RIFF header
    while pos + 8 <= wav.len() {
        let chunk_id = &wav[pos..pos + 4];
        let chunk_size = u32::from_le_bytes(wav[pos + 4..pos + 8].try_into().unwrap()) as usize;
        if chunk_id == b"guan" {
            insert_pos = pos;
            break;
        }
        pos += 8 + ((chunk_size + 1) & !1);
    }

    wav.splice(insert_pos..insert_pos, cue_bytes.iter().copied());

    // Update RIFF file size (bytes 4..8)
    let riff_size = (wav.len() - 8) as u32;
    wav[4..8].copy_from_slice(&riff_size.to_le_bytes());
}

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
    buf.extend_from_slice(&1u16.to_le_bytes()); // PCM
    buf.extend_from_slice(&1u16.to_le_bytes()); // mono
    buf.extend_from_slice(&sample_rate.to_le_bytes());
    buf.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    buf.extend_from_slice(&2u16.to_le_bytes()); // block align
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

/// Encode a complete WAV file from samples, GUANO metadata, and WAV markers.
/// This is the single source of truth for building recording WAVs — ensures
/// the downloaded/saved file matches the metadata shown in the UI.
pub fn encode_wav_complete(
    samples: &[f32],
    sample_rate: u32,
    guano: Option<&crate::audio::guano::GuanoMetadata>,
    markers: &[WavMarker],
) -> Vec<u8> {
    let mut wav_data = encode_wav(samples, sample_rate);

    // Insert cue markers (before GUANO)
    if !markers.is_empty() {
        let cue_bytes = encode_wav_cue_chunks(markers);
        insert_cue_chunks(&mut wav_data, &cue_bytes);
    }

    // Append GUANO metadata
    if let Some(g) = guano {
        let text = g.to_text();
        if !text.is_empty() {
            crate::audio::guano::append_guano_chunk(&mut wav_data, &text);
        }
    }

    wav_data
}

/// Trigger a browser download of raw WAV bytes.
pub(crate) fn trigger_browser_wav_download(wav_data: &[u8], filename: &str) {
    let array = js_sys::Uint8Array::new_with_length(wav_data.len() as u32);
    array.copy_from(wav_data);

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
    let a: web_sys::HtmlAnchorElement = document.create_element("a").unwrap().dyn_into().unwrap();
    a.set_href(&url);
    a.set_download(filename);
    a.set_attribute("style", "display:none").ok();
    document.body().unwrap().append_child(&a).ok();
    a.click();
    document.body().unwrap().remove_child(&a).ok();
    web_sys::Url::revoke_object_url(&url).ok();
}

/// Build and download a recording WAV preserving all metadata (GUANO + cue markers).
pub fn download_recording_wav(
    samples: &[f32],
    sample_rate: u32,
    filename: &str,
    guano: Option<&crate::audio::guano::GuanoMetadata>,
    markers: &[WavMarker],
) {
    let wav_data = encode_wav_complete(samples, sample_rate, guano, markers);
    trigger_browser_wav_download(&wav_data, filename);
}

/// Save WAV bytes directly to shared storage (Recordings/Oversample)
/// via the Kotlin MediaStore plugin. Skips internal storage entirely.
/// Only meaningful on Android.
pub(crate) async fn save_wav_to_shared(wav_data: &[u8], filename: &str) {
    use crate::tauri_bridge::tauri_invoke_typed;

    let args = js_sys::Object::new();
    js_sys::Reflect::set(
        &args,
        &JsValue::from_str("filename"),
        &JsValue::from_str(filename),
    )
    .ok();

    let array = js_sys::Uint8Array::new_with_length(wav_data.len() as u32);
    array.copy_from(wav_data);
    js_sys::Reflect::set(&args, &JsValue::from_str("data"), &array).ok();

    match tauri_invoke_typed::<oversample_ipc::plugins::SavePathResult>(
        "plugin:media-store|saveWavBytes",
        &args.into(),
    )
    .await
    {
        Ok(result) => {
            log::info!("Saved to shared storage: {}", result.path);
        }
        Err(e) => {
            log::warn!("Failed to save to shared storage: {}", e);
        }
    }
}

/// Try to save recording via Tauri IPC (web mode).
/// Returns the saved path on success, or None on failure.
pub(crate) async fn try_tauri_save(wav_data: &[u8], filename: &str) -> Option<String> {
    use crate::tauri_bridge::get_tauri_internals;

    let tauri = get_tauri_internals()?;

    let invoke = match js_sys::Reflect::get(&tauri, &JsValue::from_str("invoke")) {
        Ok(f) if f.is_function() => js_sys::Function::from(f),
        _ => return None,
    };

    let args = js_sys::Object::new();
    js_sys::Reflect::set(
        &args,
        &JsValue::from_str("filename"),
        &JsValue::from_str(filename),
    )
    .ok();

    let array = js_sys::Uint8Array::new_with_length(wav_data.len() as u32);
    array.copy_from(wav_data);
    js_sys::Reflect::set(&args, &JsValue::from_str("data"), &array).ok();

    let result = invoke.call2(&tauri, &JsValue::from_str("save_recording"), &args);
    match result {
        Ok(promise_val) => {
            if let Ok(promise) = promise_val.dyn_into::<js_sys::Promise>() {
                match JsFuture::from(promise).await {
                    Ok(path) => {
                        let path_str = path.as_string();
                        log::info!("Saved recording to: {:?}", path_str);
                        path_str
                    }
                    Err(e) => {
                        log::error!("Tauri save failed: {:?}", e);
                        None
                    }
                }
            } else {
                None
            }
        }
        Err(e) => {
            log::error!("Tauri invoke failed: {:?}", e);
            None
        }
    }
}
