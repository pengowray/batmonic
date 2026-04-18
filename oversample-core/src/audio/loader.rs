use crate::audio::guano::{self, parse_guano, GuanoMetadata};
use crate::audio::source::InMemorySource;
use crate::types::{AudioData, FileMetadata, WavMarker};
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
    /// Cue-point markers from `cue ` + `LIST`/`adtl` chunks, if present.
    pub wav_markers: Vec<WavMarker>,
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
    let mut cue_points: Vec<(u32, u64)> = Vec::new(); // (id, sample_position)
    let mut labels: Vec<(u32, String)> = Vec::new();   // (cue_id, text)
    let mut notes: Vec<(u32, String)> = Vec::new();    // (cue_id, text)

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
            b"cue " => {
                if chunk_fits && chunk_size >= 4 {
                    let body_end = body_end_u64 as usize;
                    let cue_data = &header_bytes[body_start..body_end];
                    let num_points = u32::from_le_bytes([cue_data[0], cue_data[1], cue_data[2], cue_data[3]]);
                    let mut cp = 4usize;
                    for _ in 0..num_points {
                        if cp + 24 > cue_data.len() { break; }
                        let id = u32::from_le_bytes(cue_data[cp..cp + 4].try_into().unwrap());
                        // sample_offset is at offset 20 within the cue point struct
                        let sample_offset = u32::from_le_bytes(cue_data[cp + 20..cp + 24].try_into().unwrap());
                        cue_points.push((id, sample_offset as u64));
                        cp += 24;
                    }
                }
            }
            b"LIST" => {
                if chunk_fits && chunk_size >= 4 {
                    let body_end = body_end_u64 as usize;
                    let list_data = &header_bytes[body_start..body_end];
                    let list_type = &list_data[0..4];
                    if list_type == b"adtl" {
                        parse_adtl_subchunks(&list_data[4..], &mut labels, &mut notes);
                    }
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

    // Build WAV markers from parsed cue points + labels/notes
    let wav_markers: Vec<WavMarker> = cue_points.iter().map(|&(id, position)| {
        let label = labels.iter().find(|(cid, _)| *cid == id).map(|(_, t)| t.clone());
        let note = notes.iter().find(|(cid, _)| *cid == id).map(|(_, t)| t.clone());
        WavMarker { id, position, label, note }
    }).collect();

    Ok(WavHeader {
        sample_rate,
        channels,
        bits_per_sample,
        is_float,
        data_offset,
        data_size,
        total_frames,
        guano,
        wav_markers,
    })
}

/// Parse `labl` and `note` sub-chunks from a LIST/adtl body.
fn parse_adtl_subchunks(data: &[u8], labels: &mut Vec<(u32, String)>, notes: &mut Vec<(u32, String)>) {
    let mut pos = 0;
    while pos + 8 <= data.len() {
        let sub_id = &data[pos..pos + 4];
        let sub_size = u32::from_le_bytes(data[pos + 4..pos + 8].try_into().unwrap()) as usize;
        let body_start = pos + 8;
        let body_end = (body_start + sub_size).min(data.len());
        if body_end - body_start >= 4 {
            let cue_id = u32::from_le_bytes(data[body_start..body_start + 4].try_into().unwrap());
            // Text follows the cue_id, null-terminated
            let text_bytes = &data[body_start + 4..body_end];
            let text = std::str::from_utf8(text_bytes)
                .unwrap_or("")
                .trim_end_matches('\0')
                .to_string();
            match sub_id {
                b"labl" => labels.push((cue_id, text)),
                b"note" => notes.push((cue_id, text)),
                _ => {}
            }
        }
        // Advance (word-aligned)
        pos = body_start + ((sub_size + 1) & !1);
    }
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

/// Load audio from raw file bytes. Detects WAV, W4V, FLAC, OGG, or MP3 by header magic bytes.
/// Extract WAV markers from raw file bytes (for non-streaming loads).
/// Returns an empty Vec for non-WAV files or files without cue markers.
pub fn parse_wav_markers(bytes: &[u8]) -> Vec<WavMarker> {
    if bytes.len() < 12 { return Vec::new(); }
    match &bytes[0..4] {
        b"RIFF" | b"RF64" => {}
        _ => return Vec::new(),
    }
    parse_wav_header_with_file_size(bytes, None)
        .map(|h| h.wav_markers)
        .unwrap_or_default()
}

pub fn load_audio(bytes: &[u8]) -> Result<AudioData, String> {
    if bytes.len() < 4 {
        return Err("File too small".into());
    }

    match &bytes[0..4] {
        b"RIFF" | b"RF64" if is_w4v(bytes) => load_w4v(bytes),
        b"RIFF" | b"RF64" => load_wav(bytes),
        b"fLaC" => load_flac(bytes),
        b"OggS" => load_ogg(bytes),
        _ if is_m4a(bytes) => load_m4a(bytes),
        _ if is_mp3(bytes) => load_mp3(bytes),
        _ => Err("Unknown file format (expected WAV, W4V, FLAC, OGG, MP3, or M4A)".into()),
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

/// Return the total size of the ID3v2 tag (header + body) in bytes, or 0 if none.
/// Only needs the first 10 bytes of the file.
pub fn id3v2_tag_size(bytes: &[u8]) -> u64 {
    if bytes.len() >= 10 && &bytes[0..3] == b"ID3" {
        // ID3v2 size is a 28-bit synchsafe integer in bytes 6-9
        let size = ((bytes[6] as u64 & 0x7F) << 21)
            | ((bytes[7] as u64 & 0x7F) << 14)
            | ((bytes[8] as u64 & 0x7F) << 7)
            | (bytes[9] as u64 & 0x7F);
        10 + size // 10-byte ID3v2 header + tag body
    } else {
        0
    }
}

pub fn is_ogg(bytes: &[u8]) -> bool {
    bytes.len() >= 4 && &bytes[0..4] == b"OggS"
}

/// Detect MPEG-4 / M4A / M4B / MP4 container (ISO BMFF).
/// Looks for the `ftyp` box at byte offset 4.
pub fn is_m4a(bytes: &[u8]) -> bool {
    bytes.len() >= 12 && &bytes[4..8] == b"ftyp"
}

/// Wildlife Acoustics W4V format tag (0x5741 = "AW" in ASCII).
const W4V_FORMAT_TAG: u16 = 0x5741;
/// Samples per W4V compressed block.
const W4V_BLOCK_SAMPLES: usize = 512;
/// Size of the per-block header (predictor i16 + scale u8 + 5 reserved bytes).
const W4V_BLOCK_HEADER: usize = 8;

/// Check if RIFF/WAVE bytes use the W4V (Wildlife Acoustics) format tag.
pub fn is_w4v(bytes: &[u8]) -> bool {
    if bytes.len() < 12 || &bytes[8..12] != b"WAVE" {
        return false;
    }
    // Scan for fmt chunk and check format tag
    let mut pos = 12usize;
    while pos + 8 <= bytes.len() {
        let chunk_id = &bytes[pos..pos + 4];
        let chunk_size = u32::from_le_bytes(
            bytes[pos + 4..pos + 8].try_into().unwrap_or([0; 4]),
        ) as usize;
        if chunk_id == b"fmt " && chunk_size >= 2 && pos + 10 <= bytes.len() {
            let format_tag = u16::from_le_bytes([bytes[pos + 8], bytes[pos + 9]]);
            return format_tag == W4V_FORMAT_TAG;
        }
        pos = pos + 8 + ((chunk_size + 1) & !1);
    }
    false
}

/// Parsed W4V header.
#[derive(Clone, Debug)]
pub struct W4vHeader {
    pub sample_rate: u32,
    pub channels: u16,
    pub block_align: u16,
    pub bits_per_coded_sample: u8,
    pub data_offset: u64,
    pub data_size: u64,
    pub total_frames: u64,
    pub guano: Option<GuanoMetadata>,
}

/// Parse W4V (Wildlife Acoustics compressed WAV) header.
pub fn parse_w4v_header(bytes: &[u8]) -> Result<W4vHeader, String> {
    if bytes.len() < 12 || &bytes[8..12] != b"WAVE" {
        return Err("Not a RIFF/WAVE file".into());
    }

    let mut pos = 12usize;
    let mut fmt_info: Option<(u32, u16, u16)> = None; // (sample_rate, channels, block_align)
    let mut data_offset: Option<u64> = None;
    let mut data_size: Option<u64> = None;
    let mut fact_samples: Option<u64> = None;
    let mut guano: Option<GuanoMetadata> = None;

    while pos + 8 <= bytes.len() {
        let chunk_id = &bytes[pos..pos + 4];
        let chunk_size = u32::from_le_bytes(
            bytes[pos + 4..pos + 8].try_into().map_err(|_| "Invalid chunk size")?,
        ) as u64;
        let body_start = pos + 8;
        let body_end_u64 = body_start as u64 + chunk_size;
        let chunk_fits = body_end_u64 <= bytes.len() as u64;

        match chunk_id {
            b"fmt " => {
                if chunk_size < 14 || !chunk_fits {
                    return Err("fmt chunk too small or truncated".into());
                }
                let body_end = body_end_u64 as usize;
                let fmt = &bytes[body_start..body_end];
                let format_tag = u16::from_le_bytes([fmt[0], fmt[1]]);
                if format_tag != W4V_FORMAT_TAG {
                    return Err(format!("Not a W4V file (format tag 0x{:04X})", format_tag));
                }
                let channels = u16::from_le_bytes([fmt[2], fmt[3]]);
                let sample_rate = u32::from_le_bytes([fmt[4], fmt[5], fmt[6], fmt[7]]);
                let block_align = u16::from_le_bytes([fmt[12], fmt[13]]);
                fmt_info = Some((sample_rate, channels, block_align));
            }
            b"fact" => {
                if chunk_size >= 4 && chunk_fits {
                    let f = &bytes[body_start..];
                    fact_samples = Some(u32::from_le_bytes([f[0], f[1], f[2], f[3]]) as u64);
                }
            }
            b"data" => {
                data_offset = Some(body_start as u64);
                data_size = Some(chunk_size);
                if guano.is_some() || !chunk_fits {
                    break;
                }
                let aligned = ((chunk_size + 1) & !1) as usize;
                pos = body_start + aligned;
                continue;
            }
            b"guan" => {
                if chunk_fits {
                    let body_end = body_end_u64 as usize;
                    guano = guano::parse_guano_chunk(&bytes[body_start..body_end]);
                }
            }
            _ => {}
        }
        let aligned = ((chunk_size + 1) & !1) as usize;
        match body_start.checked_add(aligned) {
            Some(next) if next > pos => pos = next,
            _ => break,
        }
    }

    let (sample_rate, channels, block_align) =
        fmt_info.ok_or("No fmt chunk found in W4V header")?;
    let data_offset = data_offset.ok_or("No data chunk found in W4V header")?;
    let data_size = data_size.ok_or("No data chunk found in W4V header")?;

    if block_align as usize <= W4V_BLOCK_HEADER {
        return Err("W4V block_align too small".into());
    }
    let data_bytes_per_block = block_align as usize - W4V_BLOCK_HEADER;
    let bits_per_coded_sample = (data_bytes_per_block * 8 / W4V_BLOCK_SAMPLES) as u8;
    if bits_per_coded_sample < 2 || bits_per_coded_sample > 16 {
        return Err(format!("W4V: unexpected bits per coded sample: {}", bits_per_coded_sample));
    }

    let total_frames = if let Some(n) = fact_samples {
        n
    } else {
        let num_blocks = data_size / block_align as u64;
        num_blocks * W4V_BLOCK_SAMPLES as u64
    };

    Ok(W4vHeader {
        sample_rate,
        channels,
        block_align,
        bits_per_coded_sample,
        data_offset,
        data_size,
        total_frames,
        guano,
    })
}

/// Decode W4V compressed audio blocks to f32 samples.
/// W4V uses block floating-point quantization: each block has a DC predictor
/// and a scale factor, with N-bit two's complement coded values.
/// Bit packing is MSB-first within each block's data section.
fn decode_w4v_blocks(bytes: &[u8], header: &W4vHeader) -> Vec<f32> {
    let block_align = header.block_align as usize;
    let bits = header.bits_per_coded_sample as usize;
    let num_blocks = header.data_size as usize / block_align;
    let data_start = header.data_offset as usize;
    let max_val = 32768.0f32;
    let sign_bit = 1usize << (bits - 1);
    let mask = (1usize << bits) - 1;

    let mut samples = Vec::with_capacity(num_blocks * W4V_BLOCK_SAMPLES);

    for bi in 0..num_blocks {
        let block_off = data_start + bi * block_align;
        if block_off + block_align > bytes.len() {
            break;
        }
        let block = &bytes[block_off..block_off + block_align];

        // 8-byte header: i16 predictor, u8 scale, 5 reserved bytes
        let predictor = i16::from_le_bytes([block[0], block[1]]) as i32;
        let scale = block[2] as i32;
        let data = &block[W4V_BLOCK_HEADER..];

        // Extract N-bit values, MSB-first packing
        let mut bit_pos = 0usize;
        for _ in 0..W4V_BLOCK_SAMPLES {
            let byte_idx = bit_pos / 8;
            let bit_off = bit_pos % 8;

            // Read enough bits from MSB-first packed data
            let raw = if byte_idx + 1 < data.len() {
                let two_bytes = ((data[byte_idx] as usize) << 8) | (data[byte_idx + 1] as usize);
                let shift = 16 - bits - bit_off;
                if shift < 16 {
                    (two_bytes >> shift) & mask
                } else if byte_idx + 2 < data.len() {
                    let three_bytes = (two_bytes << 8) | (data[byte_idx + 2] as usize);
                    (three_bytes >> (24 - bits - bit_off)) & mask
                } else {
                    0
                }
            } else if byte_idx < data.len() {
                let shift = 8usize.wrapping_sub(bits + bit_off);
                if shift < 8 {
                    (data[byte_idx] as usize >> shift) & mask
                } else {
                    0
                }
            } else {
                0
            };
            bit_pos += bits;

            // Two's complement: if sign bit set, value is negative
            let signed_val = if raw & sign_bit != 0 {
                raw as i32 - (1i32 << bits)
            } else {
                raw as i32
            };

            let sample_i16 = (predictor + signed_val * scale).clamp(-32768, 32767);
            samples.push(sample_i16 as f32 / max_val);
        }
    }

    samples
}

pub struct OggHeader {
    pub sample_rate: u32,
    pub channels: u16,
    pub estimated_total_frames: u64,
}

/// Parse OGG/Vorbis metadata from the given bytes (typically first 64KB of file).
/// Uses symphonia to probe the format and extract codec parameters.
/// `file_size` is needed to estimate duration when n_frames is unavailable.
pub fn parse_ogg_header(header_bytes: &[u8], file_size: u64) -> Result<OggHeader, String> {
    use symphonia::core::codecs::CODEC_TYPE_NULL;
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    use symphonia::core::probe::Hint;

    let cursor = Cursor::new(header_bytes.to_vec());
    let mss = MediaSourceStream::new(Box::new(cursor), Default::default());

    let mut hint = Hint::new();
    hint.with_extension("ogg");

    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| format!("OGG probe error: {e}"))?;

    let format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or("No audio track found in OGG")?;

    let sample_rate = track
        .codec_params
        .sample_rate
        .ok_or("OGG missing sample rate")?;
    let channels = track
        .codec_params
        .channels
        .ok_or("OGG missing channel info")?
        .count() as u16;

    let estimated_total_frames = if let Some(n_frames) = track.codec_params.n_frames {
        n_frames
    } else {
        // Rough estimate: Vorbis typically ~128-192 kbps.
        // Use 160 kbps as default estimate.
        let bitrate = 160_000u64;
        file_size * 8 * sample_rate as u64 / bitrate
    };

    Ok(OggHeader {
        sample_rate,
        channels,
        estimated_total_frames,
    })
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

fn load_w4v(bytes: &[u8]) -> Result<AudioData, String> {
    let header = parse_w4v_header(bytes)?;
    let all_samples = decode_w4v_blocks(bytes, &header);
    let channels = header.channels as u32;
    let sample_rate = header.sample_rate;

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
            format: "W4V",
            bits_per_sample: 16, // original uncompressed depth
            is_float: false,
            guano: header.guano,
            data_offset: Some(header.data_offset),
            data_size: Some(header.data_size),
        },
    })
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

/// Size of the trailing MP3 metadata (ID3v1 + Lyrics3v2 + APEv2) at the end of `bytes`.
/// Used so `data_size = file_size - data_offset - trailer` covers only audio frames.
pub fn mp3_trailer_size(bytes: &[u8]) -> u64 {
    let mut end = bytes.len();
    if end >= 128 && &bytes[end - 128..end - 125] == b"TAG" {
        end -= 128;
    }
    if end >= 15 && &bytes[end - 9..end] == b"LYRICS200" {
        if let Ok(s) = std::str::from_utf8(&bytes[end - 15..end - 9]) {
            if let Ok(sz) = s.parse::<usize>() {
                end = end.saturating_sub(sz + 15);
            }
        }
    }
    if end >= 32 && &bytes[end - 32..end - 24] == b"APETAGEX" {
        let tag_size = u32::from_le_bytes([
            bytes[end - 20], bytes[end - 19], bytes[end - 18], bytes[end - 17],
        ]) as usize;
        let flags = u32::from_le_bytes([
            bytes[end - 12], bytes[end - 11], bytes[end - 10], bytes[end - 9],
        ]);
        let has_header = (flags & 0x8000_0000) != 0;
        let total = if has_header { tag_size + 32 } else { tag_size };
        end = end.saturating_sub(total);
    }
    (bytes.len() - end) as u64
}

/// Return the byte range `(offset, size)` covered by complete Ogg pages.
/// Returns `(None, None)` if the file doesn't start with a valid Ogg page.
pub fn ogg_page_region(bytes: &[u8]) -> (Option<u64>, Option<u64>) {
    if bytes.len() < 27 || &bytes[0..4] != b"OggS" {
        return (None, None);
    }
    let mut pos = 0usize;
    let mut last_end = 0usize;
    while pos + 27 <= bytes.len() && &bytes[pos..pos + 4] == b"OggS" {
        let n_segs = bytes[pos + 26] as usize;
        let table_end = pos + 27 + n_segs;
        if table_end > bytes.len() {
            break;
        }
        let segs_sum: usize = bytes[pos + 27..table_end].iter().map(|&b| b as usize).sum();
        let page_end = table_end + segs_sum;
        if page_end > bytes.len() {
            break;
        }
        last_end = page_end;
        pos = page_end;
    }
    if last_end == 0 {
        (None, None)
    } else {
        (Some(0), Some(last_end as u64))
    }
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
            data_offset: ogg_page_region(bytes).0,
            data_size: ogg_page_region(bytes).1,
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

    // Parse header for data_offset and estimated size before decoding
    let mp3_header = parse_mp3_header(bytes, bytes.len() as u64).ok();
    let mp3_data_offset = mp3_header.as_ref().map(|h| h.data_offset).unwrap_or(0);

    // Safety: reject files whose decoded size would exceed WASM's 32-bit address space.
    // ~1.5 billion f32 samples ≈ 6 GB — well beyond the ~4 GB WASM limit.
    if let Some(ref h) = mp3_header {
        let estimated_samples = h.estimated_total_frames as u128 * h.channels as u128;
        if estimated_samples > 1_500_000_000 {
            let hours = h.estimated_total_frames as f64 / h.sample_rate as f64 / 3600.0;
            return Err(format!(
                "MP3 too large for in-memory decode (~{:.1} hours, ~{:.1} GB decoded). \
                 This file should use the streaming path.",
                hours,
                estimated_samples as f64 * 4.0 / 1_073_741_824.0,
            ));
        }
    }

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
            data_size: Some(
                (bytes.len() as u64)
                    .saturating_sub(mp3_data_offset)
                    .saturating_sub(mp3_trailer_size(bytes)),
            ),
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

/// Parsed M4A (MP4 container) header — format metadata extracted by symphonia.
#[derive(Clone, Debug)]
pub struct M4aHeader {
    pub sample_rate: u32,
    pub channels: u16,
    pub codec: &'static str,
    /// Per-channel total frames, from mdhd/tkhd duration. 0 if unknown.
    pub total_frames: u64,
    /// Text tags (title, artist, album, etc.) stored in the ilst/meta box.
    /// Reused as a GuanoMetadata container since both are just `Vec<(String, String)>`.
    pub tags: GuanoMetadata,
    /// Nero-style chapter markers from the `chpl` atom, if present.
    /// `position` holds the sample index (computed from chapter time × sample_rate).
    pub chapters: Vec<WavMarker>,
}

/// Parse MP4 tags and chapters from the full file bytes.
/// `sample_rate` is used to convert chapter times (seconds) to sample positions.
pub fn parse_m4a_chapters(bytes: &[u8], sample_rate: u32) -> Vec<WavMarker> {
    find_chpl_chapters(bytes)
        .map(|entries| {
            entries
                .into_iter()
                .enumerate()
                .map(|(i, (secs, title))| WavMarker {
                    id: (i + 1) as u32,
                    position: (secs * sample_rate as f64).max(0.0) as u64,
                    label: if title.is_empty() { None } else { Some(title) },
                    note: None,
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Parse iTunes-style metadata (`moov/udta/meta/ilst`) from raw bytes.
/// Returns key/value pairs where keys are the fourcc (e.g. "©nam" for title,
/// "©ART" for artist) rendered as UTF-8 where possible. Empty if nothing found.
pub fn parse_m4a_tags(bytes: &[u8]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let Some(moov) = find_top_level_atom(bytes, *b"moov") else { return out; };
    let Some(udta) = find_child_atom(moov, *b"udta") else { return out; };
    let Some(meta) = find_child_atom(udta, *b"meta") else { return out; };
    // `meta` is a full box: 1 byte version + 3 bytes flags, then children.
    if meta.len() < 4 { return out; }
    let meta_body = &meta[4..];
    let Some(ilst) = find_child_atom(meta_body, *b"ilst") else { return out; };

    // Walk ilst children: each child has a fourcc key + nested `data` atom.
    let mut pos = 0usize;
    while pos + 8 <= ilst.len() {
        let size_bytes: [u8; 4] = match ilst[pos..pos + 4].try_into() {
            Ok(b) => b,
            Err(_) => break,
        };
        let size = u32::from_be_bytes(size_bytes) as u64;
        let key_bytes = &ilst[pos + 4..pos + 8];
        let Some((body_start, body_end)) = mp4_box_body_range(ilst, pos, size) else { break; };
        if let Some(data) = find_child_atom(&ilst[body_start..body_end], *b"data") {
            // `data` body: 1 byte version + 3 bytes type_indicator + 4 bytes locale + payload.
            if data.len() > 8 {
                let type_indicator = u32::from_be_bytes(data[0..4].try_into().unwrap_or([0; 4])) & 0x00FF_FFFF;
                let payload = &data[8..];
                let key_raw: [u8; 4] = key_bytes.try_into().unwrap_or([0; 4]);
                let key = friendly_ilst_key(key_raw).unwrap_or_else(|| render_fourcc(key_bytes));
                let value = render_ilst_value(type_indicator, key_raw, payload);
                if !value.is_empty() {
                    out.push((key, value));
                }
            }
        }
        pos = body_end;
    }

    out
}

/// Render an iTunes ilst value according to its type_indicator:
/// 1 = UTF-8 text, 21 = signed integer BE, 0 = implicit (guess).
/// Some keys use structured binary payloads (`trkn`, `disk`) that we format
/// specially.
fn render_ilst_value(type_indicator: u32, key: [u8; 4], payload: &[u8]) -> String {
    // `trkn` and `disk` payloads are 8 bytes: 2 reserved + u16 BE index + u16 BE total.
    if (&key == b"trkn" || &key == b"disk") && payload.len() >= 6 {
        let index = u16::from_be_bytes(payload[2..4].try_into().unwrap_or([0; 2]));
        let total = u16::from_be_bytes(payload[4..6].try_into().unwrap_or([0; 2]));
        return if total > 0 { format!("{index}/{total}") } else { index.to_string() };
    }
    // `gnre` (legacy genre): 2-byte BE index into ID3 genre list.
    if &key == b"gnre" && payload.len() >= 2 {
        let idx = u16::from_be_bytes(payload[0..2].try_into().unwrap_or([0; 2]));
        return format!("Genre #{idx}");
    }
    match type_indicator {
        1 => String::from_utf8_lossy(payload).into_owned(),
        21 | 22 => {
            match payload.len() {
                1 => (payload[0] as i8).to_string(),
                2 => i16::from_be_bytes(payload.try_into().unwrap_or([0; 2])).to_string(),
                4 => i32::from_be_bytes(payload.try_into().unwrap_or([0; 4])).to_string(),
                8 => i64::from_be_bytes(payload.try_into().unwrap_or([0; 8])).to_string(),
                _ => String::new(),
            }
        }
        _ => {
            if payload.iter().all(|&b| b != 0 && b.is_ascii() || b >= 0x20) {
                String::from_utf8_lossy(payload).into_owned()
            } else {
                String::new()
            }
        }
    }
}

/// Translate a well-known iTunes fourcc to a friendlier display label.
/// Returns `None` for unknown keys so the caller can fall back to the raw fourcc.
fn friendly_ilst_key(key: [u8; 4]) -> Option<String> {
    let name = match &key {
        b"\xA9nam" => "Title",
        b"\xA9ART" => "Artist",
        b"aART"   => "Album Artist",
        b"\xA9alb" => "Album",
        b"\xA9day" => "Year",
        b"\xA9gen" => "Genre",
        b"gnre"   => "Genre",
        b"\xA9cmt" => "Comment",
        b"\xA9wrt" => "Composer",
        b"\xA9too" => "Encoder",
        b"\xA9grp" => "Grouping",
        b"\xA9lyr" => "Lyrics",
        b"trkn"   => "Track",
        b"disk"   => "Disc",
        b"cprt"   => "Copyright",
        b"desc"   => "Description",
        b"ldes"   => "Long Description",
        b"tvsh"   => "TV Show",
        b"tven"   => "TV Episode ID",
        b"tvsn"   => "TV Season",
        b"tves"   => "TV Episode",
        b"pcst"   => "Podcast",
        b"catg"   => "Category",
        b"keyw"   => "Keywords",
        b"purd"   => "Purchase Date",
        b"rtng"   => "Rating",
        b"stik"   => "Media Kind",
        _ => return None,
    };
    Some(name.to_string())
}

/// Best-effort fourcc rendering — replaces the leading `©` byte (0xA9) so
/// keys display as `©nam`/`©ART` etc. Other non-ASCII bytes become `?`.
fn render_fourcc(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(4);
    for &b in bytes.iter().take(4) {
        if b == 0xA9 {
            s.push('©');
        } else if (0x20..=0x7E).contains(&b) {
            s.push(b as char);
        } else {
            s.push('?');
        }
    }
    s
}

/// Walk ISO BMFF atoms to find `moov/udta/chpl` (Nero chapter list).
/// Returns `Vec<(start_time_seconds, title)>` or `None` if not found.
fn find_chpl_chapters(bytes: &[u8]) -> Option<Vec<(f64, String)>> {
    let moov = find_top_level_atom(bytes, *b"moov")?;
    let udta = find_child_atom(moov, *b"udta")?;
    let chpl = find_child_atom(udta, *b"chpl")?;
    parse_chpl_atom(chpl)
}

/// Parse a Nero `chpl` atom body. Layout:
/// - 1 byte version + 3 bytes flags
/// - (version >= 1): 4 bytes reserved
/// - 1 byte chapter count (some writers use 4 bytes — accept both)
/// - entries: u64 BE (100-ns units) + u8 title length + UTF-8 title
fn parse_chpl_atom(body: &[u8]) -> Option<Vec<(f64, String)>> {
    if body.len() < 5 { return None; }
    let version = body[0];
    let mut pos = 4; // skip version + flags
    if version >= 1 {
        if body.len() < pos + 4 { return None; }
        pos += 4; // reserved
    }
    // Count: try u8 first. If a u32 BE count makes more sense (when u8 count
    // would underflow the buffer), fall back to u32.
    let (count, after_count) = if pos < body.len() {
        let c = body[pos] as usize;
        // Quick sanity: if the u8 count × 9 exceeds remaining bytes, try u32.
        let remaining = body.len() - pos - 1;
        if c > 0 && c * 9 <= remaining + c * 256 {
            (c, pos + 1)
        } else if body.len() >= pos + 4 {
            let c32 = u32::from_be_bytes(body[pos..pos + 4].try_into().ok()?) as usize;
            (c32, pos + 4)
        } else {
            (c, pos + 1)
        }
    } else {
        return None;
    };
    pos = after_count;

    let mut out = Vec::with_capacity(count.min(1024));
    for _ in 0..count {
        if pos + 9 > body.len() { break; }
        let time100ns = u64::from_be_bytes(body[pos..pos + 8].try_into().ok()?);
        let title_len = body[pos + 8] as usize;
        pos += 9;
        if pos + title_len > body.len() { break; }
        let title = String::from_utf8_lossy(&body[pos..pos + title_len]).into_owned();
        pos += title_len;
        let secs = time100ns as f64 / 10_000_000.0;
        out.push((secs, title));
    }
    Some(out)
}

/// Find a top-level atom in the MP4 file with the given fourcc.
/// Returns the atom body (excluding the 8-byte header).
fn find_top_level_atom(bytes: &[u8], fourcc: [u8; 4]) -> Option<&[u8]> {
    let mut pos = 0usize;
    while pos + 8 <= bytes.len() {
        let size = u32::from_be_bytes(bytes[pos..pos + 4].try_into().ok()?) as u64;
        let id = &bytes[pos + 4..pos + 8];
        let (body_start, body_end) = mp4_box_body_range(bytes, pos, size)?;
        if id == fourcc {
            return Some(&bytes[body_start..body_end]);
        }
        pos = body_end;
    }
    None
}

/// Find a child atom within a parent's body. Most container atoms start their
/// children immediately; `meta` is the exception — it has a 4-byte full-box
/// header (version+flags) before its children.
fn find_child_atom(parent: &[u8], fourcc: [u8; 4]) -> Option<&[u8]> {
    let mut pos = 0usize;
    while pos + 8 <= parent.len() {
        let size = u32::from_be_bytes(parent[pos..pos + 4].try_into().ok()?) as u64;
        let id = &parent[pos + 4..pos + 8];
        let (body_start, body_end) = mp4_box_body_range(parent, pos, size)?;
        if id == fourcc {
            return Some(&parent[body_start..body_end]);
        }
        pos = body_end;
    }
    None
}

/// Given an atom header at `pos` with declared `size`, return (body_start, body_end).
/// Handles size==1 (64-bit extended size) and size==0 (extends to end of container).
fn mp4_box_body_range(container: &[u8], pos: usize, size: u64) -> Option<(usize, usize)> {
    let end = if size == 1 {
        if pos + 16 > container.len() { return None; }
        let large = u64::from_be_bytes(container[pos + 8..pos + 16].try_into().ok()?);
        pos as u64 + large
    } else if size == 0 {
        container.len() as u64
    } else {
        pos as u64 + size
    };
    let body_start = if size == 1 { pos + 16 } else { pos + 8 };
    let end_usize = end.min(container.len() as u64) as usize;
    if body_start > end_usize { return None; }
    Some((body_start, end_usize))
}

fn load_m4a(bytes: &[u8]) -> Result<AudioData, String> {
    use symphonia::core::audio::SampleBuffer;
    use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
    use symphonia::core::errors::Error as SymphoniaError;
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    use symphonia::core::probe::Hint;

    let cursor = Cursor::new(bytes.to_vec());
    let mss = MediaSourceStream::new(Box::new(cursor), Default::default());

    let mut hint = Hint::new();
    hint.with_extension("m4a");

    let mut probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| format!("M4A probe error: {e}"))?;

    let format = &mut probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or("No audio track found in M4A")?;

    let sample_rate = track.codec_params.sample_rate.ok_or("M4A missing sample rate")?;
    let channels = track.codec_params.channels.ok_or("M4A missing channel info")?.count() as u32;
    let track_id = track.id;

    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| format!("M4A decoder error: {e}"))?;

    // Collect iTunes-style tags from probe metadata + any format-level metadata.
    let mut tags = GuanoMetadata::new();
    if let Some(rev) = probed.metadata.get().as_ref().and_then(|m| m.current().cloned()) {
        for t in rev.tags() {
            tags.add(&t.key, &t.value.to_string());
        }
    }
    if let Some(rev) = format.metadata().current() {
        for t in rev.tags() {
            tags.add(&t.key, &t.value.to_string());
        }
    }

    let mut all_samples: Vec<f32> = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymphoniaError::ResetRequired) => { decoder.reset(); continue; }
            Err(SymphoniaError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(format!("M4A packet error: {e}")),
        };
        if packet.track_id() != track_id { continue; }
        match decoder.decode(&packet) {
            Ok(decoded) => {
                let spec = *decoded.spec();
                let mut buf = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
                buf.copy_interleaved_ref(decoded);
                all_samples.extend_from_slice(buf.samples());
            }
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(e) => return Err(format!("M4A decode error: {e}")),
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
            format: "M4A",
            bits_per_sample: 16,
            is_float: false,
            guano: if tags.fields.is_empty() { None } else { Some(tags) },
            data_offset: None,
            data_size: None,
        },
    })
}
