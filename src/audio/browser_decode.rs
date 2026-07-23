//! Browser-native audio decoding via `AudioContext.decodeAudioData`.
//!
//! Symphonia's AAC decoder only handles AAC-LC. For HE-AAC, PS, ELD and other
//! profiles (common in consumer m4a exports), this fallback uses the webview's
//! built-in decoder which supports whatever the OS media stack handles.

use std::sync::Arc;

use js_sys::{ArrayBuffer, Uint8Array};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{AudioBuffer, AudioContext, AudioContextOptions};

use crate::audio::source::InMemorySource;
use crate::types::{AudioData, FileMetadata};

/// Decode `bytes` using the browser's native audio decoder.
/// `preferred_rate` (if supplied) attempts to create the AudioContext at the
/// source sample rate so the decoder doesn't resample to the output hardware
/// rate, which would otherwise discard frequencies above ~24 kHz.
pub async fn decode_via_audio_context(
    bytes: &[u8],
    format_label: &'static str,
    preferred_rate: Option<u32>,
) -> Result<AudioData, String> {
    let ctx = if let Some(rate) = preferred_rate {
        let opts = AudioContextOptions::new();
        opts.set_sample_rate(rate as f32);
        AudioContext::new_with_context_options(&opts)
            .or_else(|_| AudioContext::new())
            .map_err(|e| format!("AudioContext: {e:?}"))?
    } else {
        AudioContext::new().map_err(|e| format!("AudioContext: {e:?}"))?
    };

    // Copy bytes into a fresh ArrayBuffer — decodeAudioData transfers ownership.
    let ab = ArrayBuffer::new(bytes.len() as u32);
    Uint8Array::new(&ab).copy_from(bytes);

    let promise = ctx
        .decode_audio_data(&ab)
        .map_err(|e| format!("decodeAudioData rejected: {e:?}"))?;
    let js = JsFuture::from(promise)
        .await
        .map_err(|e| format!("decodeAudioData failed: {e:?}"))?;
    let buf: AudioBuffer = js
        .dyn_into()
        .map_err(|_| "decodeAudioData didn't return an AudioBuffer".to_string())?;

    let sample_rate = buf.sample_rate() as u32;
    let channels = buf.number_of_channels();
    let frames = buf.length() as usize;

    let all_samples: Vec<f32> = if channels == 1 {
        buf.get_channel_data(0)
            .map_err(|e| format!("get_channel_data: {e:?}"))?
    } else {
        // Interleave channels so the downstream pipeline can split them again.
        let mut per_channel: Vec<Vec<f32>> = Vec::with_capacity(channels as usize);
        for ch in 0..channels {
            per_channel.push(
                buf.get_channel_data(ch)
                    .map_err(|e| format!("get_channel_data({ch}): {e:?}"))?,
            );
        }
        let mut interleaved = Vec::with_capacity(frames * channels as usize);
        for f in 0..frames {
            for ch in 0..channels as usize {
                interleaved.push(per_channel[ch].get(f).copied().unwrap_or(0.0));
            }
        }
        interleaved
    };

    let _ = ctx.close();

    let (samples, source) = build_source(all_samples, channels, sample_rate);
    let duration_secs = samples.len() as f64 / sample_rate as f64;

    Ok(AudioData {
        samples,
        source,
        sample_rate,
        channels,
        duration_secs,
        metadata: FileMetadata {
            file_size: bytes.len(),
            format: format_label,
            bits_per_sample: 16,
            is_float: false,
            guano: None,
            data_offset: None,
            data_size: None,
            zc_data: None,
        },
    })
}

fn build_source(
    all_samples: Vec<f32>,
    channels: u32,
    sample_rate: u32,
) -> (Arc<Vec<f32>>, Arc<InMemorySource>) {
    if channels == 1 {
        let samples = Arc::new(all_samples);
        let source = Arc::new(InMemorySource {
            samples: samples.clone(),
            raw_samples: None,
            sample_rate,
            channels: 1,
        });
        (samples, source)
    } else {
        let raw = Arc::new(all_samples);
        let mono = mix_to_mono(&raw, channels);
        let samples = Arc::new(mono);
        let source = Arc::new(InMemorySource {
            samples: samples.clone(),
            raw_samples: Some(raw),
            sample_rate,
            channels,
        });
        (samples, source)
    }
}

fn mix_to_mono(samples: &[f32], channels: u32) -> Vec<f32> {
    let ch = channels as usize;
    samples
        .chunks_exact(ch)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect()
}
