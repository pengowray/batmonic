use crate::audio::guano::{self, parse_guano, GuanoMetadata};
use crate::audio::source::InMemorySource;
use crate::types::{AudioData, FileMetadata};
use std::io::Cursor;
use std::sync::Arc;

/// Parsed WAV header — enough info to stream from disk without loading all samples.
#[derive(Clone, Debug)]
pub struct WavHeader {
    pub sample_rate: u32,
    pub channels: u16,
    pub bits_per_sample: u16,
    pub is_float: bool,
    pub data_offset: u64,       // byte offset of PCM "data" chunk body within file
    pub data_size: u64,         // byte length of PCM data
    pub total_frames: u64,      // data_size / (channels * bytes_per_sample / 8)
    pub guano: Option<GuanoMetadata>,
}

/// Parse only the WAV header from the given bytes (typically first 8-64KB of file).
/// Returns enough metadata to open the file for streaming without decoding all samples.
///
/// Supports both standard RIFF/WAVE and RF64/WAVE (used by recorders for files >4 GB).
///
/// If the GUANO chunk is before the data chunk, it will be included. If GUANO is after
/// the data chunk (common), the caller must provide tail bytes separately via
/// `parse_guano_from_tail()`.
pub fn parse_wav_header(header_bytes: &[u8]) -> Result<WavHeader, String> {
    parse_wav_header_with_file_size(header_bytes, None)
}

/// Like `parse_wav_header`, but accepts an optional actual file size to correct
/// u32 overflow in the `data` chunk size field for files >4 GB.
pub fn parse_wav_header_with_file_size(header_bytes: &[u8], file_size: Option<u64>) -> Result<WavHeader, String> {
    if header_bytes.len() < 12 {
        return Err("File too small for WAV header".into());
    }

    let is_rf64 = &header_bytes[0..4] == b"RF64" && &header_bytes[8..12] == b"WAVE";
    let is_riff = &header_bytes[0..4] == b"RIFF" && &header_bytes[8..12] == b"WAVE";

    if !is_riff && !is_rf64 {
        return Err("Not a RIFF/WAVE or RF64/WAVE file".into());
    }

    let mut pos = 12usize;
    let mut fmt_chunk: Option<(u16, u32, u16, u16)> = None; // (format_tag, sample_rate, channels, bits)
    let mut data_offset: Option<u64> = None;
    let mut data_size: Option<u64> = None;
    let mut guano: Option<GuanoMetadata> = None;

    // RF64: 64-bit sizes from the ds64 chunk (must appear before fmt/data)
    let mut ds64_data_size: Option<u64> = None;

    while pos + 8 <= header_bytes.len() {
        let chunk_id = &header_bytes[pos..pos + 4];
        let chunk_size = u32::from_le_bytes(
            header_bytes[pos + 4..pos + 8].try_into().map_err(|_| "Invalid chunk size")?,
        ) as u64;
        let body_start = pos + 8;
        // Use u64 to avoid usize overflow on 32-bit WASM for large chunks
        let body_end_u64 = body_start as u64 + chunk_size;
        let chunk_fits = body_end_u64 <= header_bytes.len() as u64;

        match chunk_id {
            b"ds64" => {
                // RF64 Data Size 64 chunk: provides 64-bit sizes
                // Layout: riffSize(8) + dataSize(8) + sampleCount(8) + ...
                if body_start + 24 <= header_bytes.len() {
                    let ds = &header_bytes[body_start..];
                    // bytes 0..8: RIFF size (not needed)
                    // bytes 8..16: data chunk size (64-bit)
                    ds64_data_size = Some(u64::from_le_bytes([
                        ds[8], ds[9], ds[10], ds[11], ds[12], ds[13], ds[14], ds[15],
                    ]));
                }
            }
            b"fmt " => {
                if chunk_size < 16 || !chunk_fits {
                    return Err("fmt chunk too small or truncated".into());
                }
                let body_end = body_end_u64 as usize;
                let fmt = &header_bytes[body_start..body_end];
                let format_tag = u16::from_le_bytes([fmt[0], fmt[1]]);
                let channels = u16::from_le_bytes([fmt[2], fmt[3]]);
                let sample_rate = u32::from_le_bytes([fmt[4], fmt[5], fmt[6], fmt[7]]);
                let bits_per_sample = u16::from_le_bytes([fmt[14], fmt[15]]);
                fmt_chunk = Some((format_tag, sample_rate, channels, bits_per_sample));
            }
            b"data" => {
                data_offset = Some(body_start as u64);
                // For RF64, the data chunk size field is 0xFFFFFFFF; use ds64 value
                if is_rf64 && chunk_size == 0xFFFFFFFF {
                    data_size = ds64_data_size;
                } else {
                    data_size = Some(chunk_size);
                }
                // Data chunk extends past our header bytes — stop scanning
                if guano.is_some() || !chunk_fits {
                    break;
                }
                // Skip past the data chunk to look for GUANO after it
                let aligned = ((chunk_size + 1) & !1) as usize;
                pos = body_start + aligned;
                continue;
            }
            b"guan" => {
                if chunk_fits {
                    let body_end = body_end_u64 as usize;
                    let guan_bytes = &header_bytes[body_start..body_end];
                    guano = guano::parse_guano_chunk(guan_bytes);
                }
            }
            _ => {}
        }

        // Advance to next chunk (word-aligned)
        let aligned = ((chunk_size + 1) & !1) as usize;
        match body_start.checked_add(aligned) {
            Some(next) if next > pos => pos = next,
            _ => break, // overflow or no progress — stop
        }
    }

    let (format_tag, sample_rate, channels, bits_per_sample) =
        fmt_chunk.ok_or("No fmt chunk found in WAV header")?;
    let data_offset = data_offset.ok_or("No data chunk found in WAV header")?;
    let mut data_size = data_size.ok_or("No data chunk found in WAV header")?;

    // format_tag: 1 = PCM integer, 3 = IEEE float
    let is_float = format_tag == 3;
    if format_tag != 1 && format_tag != 3 {
        return Err(format!("Unsupported WAV format tag: {}", format_tag));
    }

    let bytes_per_frame = channels as u64 * (bits_per_sample as u64 / 8);
    if bytes_per_frame == 0 {
        return Err("Invalid WAV: zero bytes per frame".into());
    }

    // Fix u32 overflow: if we know the actual file size and the data_size looks
    // suspiciously small (data extends to end of file but chunk says otherwise),
    // recalculate from file size. This handles:
    // - Standard RIFF files >4GB where the u32 data size wrapped
    // - Recorders that write 0xFFFFFFFF as data size without using RF64
    if let Some(fs) = file_size {
        let expected_data = fs.saturating_sub(data_offset);
        // If the stored data_size is much smaller than what the file contains,
        // or if it's exactly 0xFFFFFFFF (sentinel used by some writers), fix it.
        if data_size == 0xFFFFFFFF || (expected_data > data_size + 1024 && expected_data > 1_000_000) {
            // Align to whole frames
            let corrected = (expected_data / bytes_per_frame) * bytes_per_frame;
            log::info!(
                "Correcting data_size: header says {} bytes, file suggests {} bytes",
                data_size, corrected
            );
            data_size = corrected;
        }
    }

    let total_frames = data_size / bytes_per_frame;

    Ok(WavHeader {
        sample_rate,
        channels,
        bits_per_sample,
        is_float,
        data_offset,
        data_size,
        total_frames,
        guano,
    })
}

/// Parsed FLAC header — enough info to stream without loading all samples.
#[derive(Clone, Debug)]
pub struct FlacHeader {
    pub sample_rate: u32,
    pub channels: u16,
    pub bits_per_sample: u16,
    pub total_frames: u64,       // total per-channel sample frames
    pub first_frame_offset: u64, // byte offset where audio frames begin (after all metadata blocks)
    pub max_frame_size: u32,     // from STREAMINFO, 0 if unknown
}

/// Parse FLAC header from the given bytes (typically first 64KB of file).
/// Returns format metadata and the byte offset where audio frames begin.
pub fn parse_flac_header(header_bytes: &[u8]) -> Result<FlacHeader, String> {
    if header_bytes.len() < 42 {
        return Err("File too small for FLAC header".into());
    }
    if &header_bytes[0..4] != b"fLaC" {
        return Err("Not a FLAC file".into());
    }

    // First metadata block must be STREAMINFO
    let block_header = header_bytes[4];
    let is_last = (block_header & 0x80) != 0;
    let block_type = block_header & 0x7F;
    if block_type != 0 {
        return Err("First FLAC metadata block is not STREAMINFO".into());
    }
    let block_len = ((header_bytes[5] as u32) << 16)
        | ((header_bytes[6] as u32) << 8)
        | (header_bytes[7] as u32);
    if block_len < 34 || header_bytes.len() < 8 + block_len as usize {
        return Err("STREAMINFO block too small or truncated".into());
    }

    let si = &header_bytes[8..8 + 34]; // STREAMINFO is exactly 34 bytes

    // min/max block size: si[0..2], si[2..4]
    // min/max frame size: si[4..7], si[7..10]
    let max_frame_size = ((si[7] as u32) << 16) | ((si[8] as u32) << 8) | (si[9] as u32);

    // Bytes 10-17 contain packed fields:
    // sample_rate: 20 bits, channels-1: 3 bits, bps-1: 5 bits, total_samples: 36 bits
    let sr = ((si[10] as u32) << 12) | ((si[11] as u32) << 4) | ((si[12] as u32) >> 4);
    let ch = ((si[12] >> 1) & 0x07) + 1;
    let bps = (((si[12] & 0x01) as u16) << 4) | ((si[13] >> 4) as u16);
    let bps = bps + 1;
    let total_samples = ((si[13] as u64 & 0x0F) << 32)
        | ((si[14] as u64) << 24)
        | ((si[15] as u64) << 16)
        | ((si[16] as u64) << 8)
        | (si[17] as u64);

    if sr == 0 {
        return Err("FLAC: sample rate is 0".into());
    }

    // Walk metadata blocks to find first_frame_offset
    let mut pos = 4u64; // after "fLaC"
    let mut last = is_last;
    while !last {
        let p = pos as usize;
        if p + 4 > header_bytes.len() {
            break;
        }
        let hdr = header_bytes[p];
        last = (hdr & 0x80) != 0;
        let len = ((header_bytes[p + 1] as u32) << 16)
            | ((header_bytes[p + 2] as u32) << 8)
            | (header_bytes[p + 3] as u32);
        pos += 4 + len as u64;
    }

    Ok(FlacHeader {
        sample_rate: sr,
        channels: ch as u16,
        bits_per_sample: bps,
        total_frames: total_samples,
        first_frame_offset: pos,
        max_frame_size,
    })
}

/// Parsed MP3 header — enough info to decide whether streaming is needed.
#[derive(Clone, Debug)]
pub struct Mp3Header {
    pub sample_rate: u32,
    pub channels: u16,
    pub estimated_total_frames: u64, // from Xing/LAME header or bitrate estimate
    pub data_offset: u64,            // byte offset where audio frames begin (after ID3v2 tags)
}

/// Parse MP3 metadata from the given bytes (typically first 64KB of file).
/// Uses symphonia to probe the format and extract codec parameters.
/// `file_size` is needed to estimate duration when no Xing/LAME header is present.
pub fn parse_mp3_header(header_bytes: &[u8], file_size: u64) -> Result<Mp3Header, String> {
    use symphonia::core::codecs::CODEC_TYPE_NULL;
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    use symphonia::core::probe::Hint;

    let cursor = Cursor::new(header_bytes.to_vec());
    let mss = MediaSourceStream::new(Box::new(cursor), Default::default());

    let mut hint = Hint::new();
    hint.with_extension("mp3");

    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| format!("MP3 probe error: {e}"))?;

    let format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or("No audio track found in MP3")?;

    let sample_rate = track
        .codec_params
        .sample_rate
        .ok_or("MP3 missing sample rate")?;
    let channels = track
        .codec_params
        .channels
        .ok_or("MP3 missing channel info")?
        .count() as u16;

    // Try to get exact frame count from Xing/LAME header
    let estimated_total_frames = if let Some(n_frames) = track.codec_params.n_frames {
        n_frames
    } else {
        // Estimate from file size and bitrate.
        // Typical MP3: bitrate = file_size * 8 / duration, so duration = file_size * 8 / bitrate.
        // Default to 128 kbps if we can't determine bitrate.
        let bitrate = track.codec_params.bits_per_coded_sample.unwrap_or(0) as u64;
        let bitrate = if bitrate > 0 { bitrate * 1000 } else { 128_000 };
        // frames = duration * sample_rate = (file_size * 8 / bitrate) * sample_rate
        file_size * 8 * sample_rate as u64 / bitrate
    };

    // Detect ID3v2 tag to determine audio data offset
    let data_offset = if header_bytes.len() >= 10 && &header_bytes[0..3] == b"ID3" {
        // ID3v2 size is stored as a 28-bit synchsafe integer in bytes 6-9
        let size = ((header_bytes[6] as u64 & 0x7F) << 21)
            | ((header_bytes[7] as u64 & 0x7F) << 14)
            | ((header_bytes[8] as u64 & 0x7F) << 7)
            | (header_bytes[9] as u64 & 0x7F);
        10 + size // 10-byte ID3v2 header + tag body
    } else {
        0
    };

    Ok(Mp3Header {
        sample_rate,
        channels,
        estimated_total_frames,
        data_offset,
    })
}

/// Load audio from raw file bytes. Detects WAV, FLAC, OGG, or MP3 by header magic bytes.
pub fn load_audio(bytes: &[u8]) -> Result<AudioData, String> {
    if bytes.len() < 4 {
        return Err("File too small".into());
    }

    match &bytes[0..4] {
        b"RIFF" | b"RF64" => load_wav(bytes),
        b"fLaC" => load_flac(bytes),
        b"OggS" => load_ogg(bytes),
        _ if is_mp3(bytes) => load_mp3(bytes),
        _ => Err("Unknown file format (expected WAV, FLAC, OGG, or MP3)".into()),
    }
}

pub fn is_mp3(bytes: &[u8]) -> bool {
    // ID3v2 tag header
    if bytes.len() >= 3 && &bytes[0..3] == b"ID3" {
        return true;
    }
    // MPEG sync word: 0xFF followed by 0xE0–0xFF
    if bytes.len() >= 2 && bytes[0] == 0xFF && (bytes[1] & 0xE0) == 0xE0 {
        return true;
    }
    false
}

/// Rebuild a minimal RIFF/WAVE with only the `fmt` and `data` chunks.
/// Hound 3.5 doesn't handle RIFF word-alignment padding on odd-length chunks
/// (e.g. a 651-byte `bext` chunk), so we strip extraneous chunks and produce
/// a clean WAV that hound can always parse.
fn normalize_riff(bytes: &[u8]) -> Option<Vec<u8>> {
    if bytes.len() < 12 || &bytes[8..12] != b"WAVE" {
        return None;
    }
    let magic = &bytes[0..4];
    if magic != b"RIFF" && magic != b"RF64" {
        return None;
    }

    let mut pos = 12usize;
    let mut fmt_data: Option<&[u8]> = None;
    let mut audio_data: Option<&[u8]> = None;

    while pos + 8 <= bytes.len() {
        let chunk_id = &bytes[pos..pos + 4];
        let chunk_size = u32::from_le_bytes(
            bytes[pos + 4..pos + 8].try_into().ok()?,
        ) as usize;
        let data_start = pos + 8;
        let data_end = data_start + chunk_size;
        if data_end > bytes.len() {
            break;
        }

        match chunk_id {
            b"fmt " => fmt_data = Some(&bytes[data_start..data_end]),
            b"data" => {
                audio_data = Some(&bytes[data_start..data_end]);
                break; // data is always last useful chunk
            }
            _ => {}
        }

        // Advance with RIFF word-alignment (same as guano.rs)
        pos = data_start + ((chunk_size + 1) & !1);
    }

    let fmt = fmt_data?;
    let data = audio_data?;

    // WAVE + fmt chunk header + fmt body + data chunk header + data body
    let riff_body_len = 4 + 8 + fmt.len() + 8 + data.len();
    let mut out = Vec::with_capacity(12 + riff_body_len - 4);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(riff_body_len as u32).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&(fmt.len() as u32).to_le_bytes());
    out.extend_from_slice(fmt);
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(data);
    Some(out)
}

fn load_wav(bytes: &[u8]) -> Result<AudioData, String> {
    // Parse original header for data_offset/data_size before normalization
    let (orig_data_offset, orig_data_size) = parse_wav_header_with_file_size(bytes, Some(bytes.len() as u64))
        .map(|h| (Some(h.data_offset), Some(h.data_size)))
        .unwrap_or((None, None));

    let normalized;
    let wav_bytes = match normalize_riff(bytes) {
        Some(clean) => { normalized = clean; &normalized[..] }
        None => bytes,
    };
    let cursor = Cursor::new(wav_bytes);
    let reader = hound::WavReader::new(cursor).map_err(|e| format!("WAV error: {e}"))?;
    let spec = reader.spec();
    let sample_rate = spec.sample_rate;
    let channels = spec.channels as u32;
    let bits_per_sample = spec.bits_per_sample;

    let is_float = matches!(spec.sample_format, hound::SampleFormat::Float);
    let all_samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .into_samples::<f32>()
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("WAV sample error: {e}"))?,
        hound::SampleFormat::Int => {
            let max_val = (1u32 << (bits_per_sample - 1)) as f32;
            reader
                .into_samples::<i32>()
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| format!("WAV sample error: {e}"))?
                .into_iter()
                .map(|s| s as f32 / max_val)
                .collect()
        }
    };

    let guano = parse_guano(bytes);

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
            format: "WAV",
            bits_per_sample,
            is_float,
            guano,
            data_offset: orig_data_offset,
            data_size: orig_data_size,
        },
    })
}

fn load_flac(bytes: &[u8]) -> Result<AudioData, String> {
    // Parse header for data_offset before using claxon
    let (flac_data_offset, flac_data_size) = parse_flac_header(bytes)
        .map(|h| (Some(h.first_frame_offset), Some((bytes.len() as u64).saturating_sub(h.first_frame_offset))))
        .unwrap_or((None, None));

    let cursor = Cursor::new(bytes);
    let mut reader = claxon::FlacReader::new(cursor).map_err(|e| format!("FLAC error: {e}"))?;
    let info = reader.streaminfo();
    let sample_rate = info.sample_rate;
    let channels = info.channels;
    let bits = info.bits_per_sample;
    let max_val = (1u32 << (bits - 1)) as f32;

    let all_samples: Vec<f32> = reader
        .samples()
        .map(|s| s.map(|v| v as f32 / max_val))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("FLAC sample error: {e}"))?;

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
            format: "FLAC",
            bits_per_sample: bits as u16,
            is_float: false,
            guano: None,
            data_offset: flac_data_offset,
            data_size: flac_data_size,
        },
    })
}

fn load_ogg(bytes: &[u8]) -> Result<AudioData, String> {
    use lewton::inside_ogg::OggStreamReader;

    let cursor = Cursor::new(bytes);
    let mut reader = OggStreamReader::new(cursor).map_err(|e| format!("OGG error: {e}"))?;

    let sample_rate = reader.ident_hdr.audio_sample_rate;
    let channels = reader.ident_hdr.audio_channels as u32;

    let mut all_samples: Vec<f32> = Vec::new();
    loop {
        match reader.read_dec_packet_itl() {
            Ok(Some(packet)) => {
                all_samples.extend(packet.iter().map(|&s| s as f32 / 32768.0));
            }
            Ok(None) => break,
            Err(e) => return Err(format!("OGG decode error: {e}")),
        }
    }

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
            format: "OGG",
            bits_per_sample: 16,
            is_float: false,
            guano: None,
            data_offset: None,
            data_size: None,
        },
    })
}

fn load_mp3(bytes: &[u8]) -> Result<AudioData, String> {
    use symphonia::core::audio::SampleBuffer;
    use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
    use symphonia::core::errors::Error as SymphoniaError;
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    use symphonia::core::probe::Hint;

    // Parse header for data_offset before decoding
    let mp3_data_offset = parse_mp3_header(bytes, bytes.len() as u64)
        .map(|h| h.data_offset)
        .unwrap_or(0);

    let cursor = Cursor::new(bytes.to_vec());
    let mss = MediaSourceStream::new(Box::new(cursor), Default::default());

    let hint = Hint::new();
    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| format!("MP3 probe error: {e}"))?;

    let mut format = probed.format;

    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or("No audio track found in MP3")?;

    let sample_rate = track
        .codec_params
        .sample_rate
        .ok_or("MP3 missing sample rate")?;
    let channels = track
        .codec_params
        .channels
        .ok_or("MP3 missing channel info")?
        .count() as u32;
    let track_id = track.id;

    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| format!("MP3 decoder error: {e}"))?;

    let mut all_samples: Vec<f32> = Vec::new();

    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymphoniaError::ResetRequired) => {
                decoder.reset();
                continue;
            }
            Err(SymphoniaError::IoError(e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(e) => return Err(format!("MP3 packet error: {e}")),
        };

        if packet.track_id() != track_id {
            continue;
        }

        match decoder.decode(&packet) {
            Ok(decoded) => {
                let spec = *decoded.spec();
                let mut buf = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
                buf.copy_interleaved_ref(decoded);
                all_samples.extend_from_slice(buf.samples());
            }
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(e) => return Err(format!("MP3 decode error: {e}")),
        }
    }

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
            format: "MP3",
            bits_per_sample: 16,
            is_float: false,
            guano: None,
            data_offset: Some(mp3_data_offset),
            data_size: Some((bytes.len() as u64).saturating_sub(mp3_data_offset)),
        },
    })
}

/// Build mono-mixed samples and an InMemorySource from decoded interleaved samples.
/// For mono files, raw_samples is None (saves memory by sharing the Arc).
fn build_source(all_samples: Vec<f32>, channels: u32, sample_rate: u32) -> (Arc<Vec<f32>>, Arc<InMemorySource>) {
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
