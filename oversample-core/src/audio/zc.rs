//! Anabat zero-crossing (`.zc`) file format parser.
//!
//! The `.zc` format is Titley Scientific's (Anabat) zero-crossing file
//! used by bat-call recorders since the late 1990s. Instead of continuous
//! waveform samples it stores a sequence of *zero-crossing intervals*
//! in microseconds, from which a time-vs-frequency dot plot is derived.
//!
//! File layout (file types 129–132, "Anabat sequence file"):
//!
//! ```text
//!   0x000  u16le  data_info_pointer        # offset of params block (usually 0x11a)
//!   0x002  u8     (pad)
//!   0x003  u8     file_type                # 129, 130, 131, or 132
//!   0x004  u16    (pad)
//!   0x006  [u8;  8] tape                   # null-padded ASCII metadata
//!   0x00e  [u8;  8] date
//!   0x016  [u8; 40] location
//!   0x03e  [u8; 50] species
//!   0x070  [u8; 16] spec
//!   0x080  [u8; 73] note1
//!   0x0c9  [u8; 80] note2
//!   ----- data_info block at data_info_pointer (commonly 0x11a) -----
//!   +0x00  u16le  data_pointer             # offset of interval data
//!   +0x02  u16le  res1                     # sample-rate; 25000 typical, used for timeFactor scaling
//!   +0x04  u8     divratio                 # frequency-divider ratio (typically 8 or 16)
//!   +0x05  u8     vres                     # vertical resolution
//!   ----- file_type >= 132 only: addl block at 0x120 -----
//!   0x120  u16le  year
//!   0x122  u8     month
//!   0x123  u8     day
//!   0x124  u8     hour
//!   0x125  u8     minute
//!   0x126  u8     second
//!   0x127  u8     second_hundredths
//!   0x128  u16le  microseconds
//!   0x12a  [u8; 6] id_code
//!   0x130  [u8;32] gps_data
//!   0x150  ...     GUANO metadata (optional, length = data_pointer - 0x150)
//!   ----- interval data from data_pointer -----
//! ```
//!
//! Interval-data encoding (variable-length per dot):
//!
//! | First byte    | Meaning                                                     |
//! |---------------|-------------------------------------------------------------|
//! | `0x00..=0x7F` | 7-bit signed delta from previous interval (two's complement)|
//! | `0x80..=0x9F` | 13-bit absolute interval; lower 8 bits from next byte        |
//! | `0xA0..=0xBF` | 21-bit absolute; next 2 bytes                                |
//! | `0xC0..=0xDF` | 29-bit absolute; next 3 bytes                                |
//! | `0xE0..=0xFF` | Status byte: low 5 bits = status code; next byte = dot count |
//!
//! Frequency at dot `i` is derived from a 2-dot rolling period:
//!
//! ```text
//!   freq[i] = divratio · 1_000_000 / (time_us[i+1] - time_us[i-1])
//! ```
//!
//! Valid frequency range is `Tmin..=Tmax` µs where `Tmin = max(48,
//! divratio*4)` and `Tmax = min(12589, divratio*250)`; periods outside
//! that range emit `freq = 0` (no detection).
//!
//! References:
//! - [riggsd/zcant `anabat.py`](https://github.com/riggsd/zcant/blob/master/zcant/anabat.py)
//! - [BioAcoustica `zcjs.js`](https://github.com/BioAcoustica/zcjs/blob/master/zcjs.js)

const HEADER_LEN: usize = 281;
const DEFAULT_RES1: u32 = 25_000;
const STATUS_OFF: u8 = 1;
/// Hard cap on dot count we'll parse — guards against malformed files.
const MAX_DOTS: usize = 1_000_000;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ZcMetadata {
    pub tape: String,
    pub date: String,
    pub location: String,
    pub species: String,
    pub spec: String,
    pub note1: String,
    pub note2: String,
    pub divratio: u8,
    pub vres: u8,
    pub res1: u32,
    /// For file_type >= 132 only.
    pub timestamp: Option<ZcTimestamp>,
    /// For file_type >= 132 only.
    pub id_code: String,
    /// For file_type >= 132 only.
    pub gps: String,
    pub file_type: u8,
    /// GUANO metadata key-value pairs (file_type >= 132 with a metadata block).
    pub guano: Vec<(String, String)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ZcTimestamp {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    pub microseconds_total: u32, // hundredths * 10_000 + extra microseconds
}

#[derive(Clone, Debug, PartialEq)]
pub struct ZcData {
    /// Times (seconds since file start) of each zero-crossing dot.
    pub times_s: Vec<f64>,
    /// Inferred frequency (Hz) of each dot. `0.0` means "no detection"
    /// (period outside the divratio-determined valid range).
    pub freqs_hz: Vec<f64>,
    /// `true` for dots that were marked OFF via a status byte. Receivers
    /// usually filter these out when plotting.
    pub off_mask: Vec<bool>,
    pub metadata: ZcMetadata,
}

impl ZcData {
    /// Total recording duration in seconds (last dot's time, 0 if empty).
    pub fn duration_secs(&self) -> f64 {
        self.times_s.last().copied().unwrap_or(0.0)
    }

    /// Number of "ON" dots (excluding OFF-masked entries).
    pub fn on_dot_count(&self) -> usize {
        self.off_mask.iter().filter(|&&b| !b).count()
    }
}

/// Detect whether `bytes` looks like an Anabat ZC file.
pub fn is_zc(bytes: &[u8]) -> bool {
    if bytes.len() < HEADER_LEN {
        return false;
    }
    let file_type = bytes[3];
    if !(129..=132).contains(&file_type) {
        return false;
    }
    let data_info_pointer = u16::from_le_bytes([bytes[0], bytes[1]]) as usize;
    // data_info_pointer is typically 0x11a (282) but a few rare older
    // files might use 0x120; either way it must point inside the file
    // and at least past the header.
    data_info_pointer >= HEADER_LEN && data_info_pointer + 6 <= bytes.len()
}

pub fn parse_zc(bytes: &[u8]) -> Result<ZcData, String> {
    if bytes.len() < HEADER_LEN {
        return Err(format!(
            "File too small for Anabat header ({} < {})",
            bytes.len(),
            HEADER_LEN
        ));
    }
    let data_info_pointer = u16::from_le_bytes([bytes[0], bytes[1]]) as usize;
    let file_type = bytes[3];
    if !(129..=132).contains(&file_type) {
        return Err(format!(
            "Unsupported Anabat file_type {} (expected 129..=132)",
            file_type
        ));
    }
    if data_info_pointer < HEADER_LEN || data_info_pointer + 6 > bytes.len() {
        return Err(format!(
            "data_info_pointer 0x{:x} out of range",
            data_info_pointer
        ));
    }

    // Parse the header text fields.
    let metadata_text = ZcMetadata {
        tape: ascii_field(&bytes[0x006..0x00e]),
        date: ascii_field(&bytes[0x00e..0x016]),
        location: ascii_field(&bytes[0x016..0x03e]),
        species: ascii_field(&bytes[0x03e..0x070]),
        spec: ascii_field(&bytes[0x070..0x080]),
        note1: ascii_field(&bytes[0x080..0x0c9]),
        note2: ascii_field(&bytes[0x0c9..0x119]),
        ..Default::default()
    };

    // Parse the data_info block.
    let data_pointer = u16::from_le_bytes(
        bytes[data_info_pointer..data_info_pointer + 2]
            .try_into()
            .unwrap(),
    ) as usize;
    let res1 = u16::from_le_bytes(
        bytes[data_info_pointer + 2..data_info_pointer + 4]
            .try_into()
            .unwrap(),
    ) as u32;
    let divratio = bytes[data_info_pointer + 4];
    let vres = bytes[data_info_pointer + 5];

    if !(10_000..=60_000).contains(&res1) {
        return Err(format!(
            "Implausible res1 = {} (expected 10000..=60000)",
            res1
        ));
    }
    if divratio == 0 {
        return Err("divratio of 0 would divide by zero".into());
    }
    if data_pointer < data_info_pointer + 6 || data_pointer > bytes.len() {
        return Err(format!("data_pointer 0x{:x} out of range", data_pointer));
    }

    let mut metadata = ZcMetadata {
        divratio,
        vres,
        res1,
        file_type,
        ..metadata_text
    };

    // For v132+: pull the timestamp/id/gps block at 0x120 and GUANO at 0x150.
    if file_type >= 132 && bytes.len() >= 0x150 {
        let p = 0x120;
        let year = u16::from_le_bytes([bytes[p], bytes[p + 1]]);
        let month = bytes[p + 2];
        let day = bytes[p + 3];
        let hour = bytes[p + 4];
        let minute = bytes[p + 5];
        let second = bytes[p + 6];
        let sec_hundredths = bytes[p + 7];
        let micros = u16::from_le_bytes([bytes[p + 8], bytes[p + 9]]);
        if year >= 1990 && year <= 2200 && month >= 1 && month <= 12 && day >= 1 && day <= 31 {
            metadata.timestamp = Some(ZcTimestamp {
                year,
                month,
                day,
                hour,
                minute,
                second,
                microseconds_total: (sec_hundredths as u32) * 10_000 + (micros as u32),
            });
        }
        metadata.id_code = ascii_field(&bytes[p + 0x0a..p + 0x10]);
        metadata.gps = ascii_field(&bytes[p + 0x10..p + 0x30]);
        if data_pointer > 0x150 + 12 {
            let guano_bytes = &bytes[0x150..data_pointer.min(bytes.len())];
            metadata.guano = parse_guano_text(guano_bytes);
        }
    }

    let (intervals_us, off_mask) = decode_intervals(&bytes[data_pointer..])?;
    let time_factor = if res1 == DEFAULT_RES1 {
        1.0
    } else {
        DEFAULT_RES1 as f64 / res1 as f64
    };

    // Cumulative times in seconds (scaled by time_factor when res1 != 25000).
    let mut times_s = Vec::with_capacity(intervals_us.len());
    let mut cumulative_us = 0.0f64;
    for &iv in &intervals_us {
        cumulative_us += iv as f64 * time_factor;
        times_s.push(cumulative_us * 1e-6);
    }

    // Frequency at dot i = divratio · 1e6 / (time_us[i+1] - time_us[i-1]).
    // Tmin/Tmax (µs) clamp out implausible periods.
    let divr = divratio as u32;
    let tmin = (divr.saturating_mul(4)).max(48) as f64;
    let tmax = (divr.saturating_mul(250)).min(12_589) as f64;
    let mut freqs_hz = vec![0.0f64; times_s.len()];
    for i in 1..times_s.len().saturating_sub(1) {
        let dt_us = (times_s[i + 1] - times_s[i - 1]) * 1e6;
        if dt_us >= tmin && dt_us <= tmax {
            freqs_hz[i] = divr as f64 * 1e6 / dt_us;
        }
    }

    Ok(ZcData {
        times_s,
        freqs_hz,
        off_mask,
        metadata,
    })
}

/// Strip null/whitespace padding from an ASCII metadata field.
fn ascii_field(bytes: &[u8]) -> String {
    let s: String = bytes
        .iter()
        .take_while(|&&b| b != 0)
        .map(|&b| {
            if b.is_ascii() && !b.is_ascii_control() {
                b as char
            } else {
                '?'
            }
        })
        .collect();
    s.trim().to_string()
}

/// Parse a GUANO metadata text block into `(key, value)` pairs.
fn parse_guano_text(bytes: &[u8]) -> Vec<(String, String)> {
    // Trim trailing nulls.
    let end = bytes
        .iter()
        .rposition(|&b| b != 0)
        .map(|i| i + 1)
        .unwrap_or(0);
    let text = std::str::from_utf8(&bytes[..end]).unwrap_or("");
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() {
                return None;
            }
            line.split_once(':')
                .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        })
        .collect()
}

/// Decode the variable-length interval section. Returns interval values
/// in *microseconds before time_factor scaling*, plus a per-dot OFF mask.
fn decode_intervals(data: &[u8]) -> Result<(Vec<u32>, Vec<bool>), String> {
    let mut intervals: Vec<u32> = Vec::with_capacity(4096);
    let mut off_intervals: Vec<(usize, u8)> = Vec::new();
    let mut last_diff: i64 = 0;
    let mut i = 0usize;

    while i < data.len() && intervals.len() < MAX_DOTS {
        let b = data[i];
        match b {
            0x00..=0x7F => {
                // 7-bit signed delta from current `last_diff`.
                let mut offset = b as i64;
                if offset > 63 {
                    offset -= 128;
                }
                last_diff = last_diff.saturating_add(offset);
                if last_diff < 0 {
                    last_diff = 0;
                }
                intervals.push(last_diff as u32);
                i += 1;
            }
            0x80..=0x9F => {
                if i + 1 >= data.len() {
                    break;
                }
                let acc = (((b & 0x1F) as u32) << 8) | data[i + 1] as u32;
                last_diff = acc as i64;
                intervals.push(acc);
                i += 2;
            }
            0xA0..=0xBF => {
                if i + 2 >= data.len() {
                    break;
                }
                let acc =
                    (((b & 0x1F) as u32) << 16) | ((data[i + 1] as u32) << 8) | data[i + 2] as u32;
                last_diff = acc as i64;
                intervals.push(acc);
                i += 3;
            }
            0xC0..=0xDF => {
                if i + 3 >= data.len() {
                    break;
                }
                let acc = (((b & 0x1F) as u32) << 24)
                    | ((data[i + 1] as u32) << 16)
                    | ((data[i + 2] as u32) << 8)
                    | data[i + 3] as u32;
                last_diff = acc as i64;
                intervals.push(acc);
                i += 4;
            }
            0xE0..=0xFF => {
                // Status byte + dotcount.
                let status = b & 0x1F;
                if i + 1 >= data.len() {
                    break;
                }
                let dotcount = data[i + 1];
                if status == STATUS_OFF {
                    off_intervals.push((intervals.len(), dotcount));
                }
                // Other status codes (e.g., main / out-of-range markers)
                // are ignored — they affect classification rather than
                // dot positioning.
                i += 2;
            }
        }
    }

    let mut off_mask = vec![false; intervals.len()];
    for (start, count) in off_intervals {
        let end = (start + count as usize).min(off_mask.len());
        for slot in &mut off_mask[start..end] {
            *slot = true;
        }
    }
    Ok((intervals, off_mask))
}

/// Synthesise a continuous waveform from a ZC recording by treating the
/// dot frequencies as the instantaneous frequency of a phase-coherent
/// oscillator. Useful for displaying the file in a spectrogram view and
/// for letting users listen to a recording reconstruction (after
/// time-expansion at playback time, since the original calls are
/// ultrasonic).
///
/// Algorithm:
/// - Walk through output samples at `1 / output_sample_rate` increments.
/// - At each output sample time `t`, find the dot index `i` such that
///   `times_s[i] <= t < times_s[i+1]`. Linearly interpolate between the
///   surrounding dot frequencies to get the instantaneous frequency.
/// - Advance the phase by `2π · f(t) · dt` and emit `sin(phase)`.
/// - OFF dots and dots with `freq = 0` (period out of range) emit
///   silence — the oscillator stays at zero amplitude across them.
/// - A short raised-cosine fade at each ON-region boundary prevents
///   audible clicks at the silence-to-tone transitions.
pub fn synthesise_waveform(zc: &ZcData, output_sample_rate: u32) -> Vec<f32> {
    if zc.times_s.is_empty() {
        return Vec::new();
    }
    let dt = 1.0 / output_sample_rate as f64;
    let total_t = zc.duration_secs();
    if total_t <= 0.0 {
        return Vec::new();
    }
    let n_samples = (total_t * output_sample_rate as f64).ceil() as usize;
    let mut samples = Vec::with_capacity(n_samples);
    let mut phase = 0.0f64;
    let mut dot_i: usize = 0;
    const TAU: f64 = std::f64::consts::TAU;
    // 1 ms fade in/out around silent regions — eliminates clicks without
    // smearing the spectrogram visibly.
    let fade_samples = (output_sample_rate as f64 * 0.001).round() as usize;

    // Pre-compute amplitude envelope: 1.0 where the surrounding dot
    // window has a valid ON frequency, with raised-cosine ramps near
    // transitions. This is simpler than a sample-by-sample envelope
    // tracker and gives perceptually clean transitions.
    let dot_active = |i: usize| -> bool {
        i < zc.freqs_hz.len()
            && zc.freqs_hz[i] > 0.0
            && !zc.off_mask.get(i).copied().unwrap_or(false)
    };

    for s in 0..n_samples {
        let t = s as f64 * dt;
        // Advance dot_i so times_s[dot_i] <= t < times_s[dot_i+1].
        while dot_i + 1 < zc.times_s.len() && zc.times_s[dot_i + 1] <= t {
            dot_i += 1;
        }

        // Instantaneous freq: linear interp between adjacent active dots.
        let freq = if dot_active(dot_i) {
            if dot_i + 1 < zc.times_s.len() && dot_active(dot_i + 1) {
                let t0 = zc.times_s[dot_i];
                let t1 = zc.times_s[dot_i + 1];
                if t1 > t0 {
                    let alpha = ((t - t0) / (t1 - t0)).clamp(0.0, 1.0);
                    zc.freqs_hz[dot_i] * (1.0 - alpha) + zc.freqs_hz[dot_i + 1] * alpha
                } else {
                    zc.freqs_hz[dot_i]
                }
            } else {
                zc.freqs_hz[dot_i]
            }
        } else {
            0.0
        };

        // Phase always advances by the instantaneous frequency so that
        // tones stay phase-coherent across dot boundaries within an ON
        // region.
        phase += TAU * freq * dt;
        if phase > TAU * 1024.0 {
            // Keep phase bounded to avoid f64 precision loss.
            phase = phase.rem_euclid(TAU);
        }

        // Amplitude envelope: 1 inside ON regions, 0 in OFF regions,
        // raised-cosine ramps near boundaries.
        let amp = if freq > 0.0 {
            // Distance to the nearest silent boundary in samples.
            let mut dist_to_silence: usize = fade_samples + 1;
            for look in 1..=fade_samples {
                let ahead = s + look;
                let t_ahead = ahead as f64 * dt;
                let dot_ahead = match zc.times_s.binary_search_by(|v| {
                    v.partial_cmp(&t_ahead).unwrap_or(std::cmp::Ordering::Equal)
                }) {
                    Ok(k) => k,
                    Err(k) => k.saturating_sub(1),
                };
                if !dot_active(dot_ahead) {
                    dist_to_silence = look;
                    break;
                }
                if s >= look {
                    let t_behind = (s - look) as f64 * dt;
                    let dot_behind = match zc.times_s.binary_search_by(|v| {
                        v.partial_cmp(&t_behind)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    }) {
                        Ok(k) => k,
                        Err(k) => k.saturating_sub(1),
                    };
                    if !dot_active(dot_behind) {
                        dist_to_silence = look;
                        break;
                    }
                }
            }
            if dist_to_silence <= fade_samples {
                let phase_in_fade = dist_to_silence as f64 / fade_samples as f64;
                0.5 * (1.0 - (std::f64::consts::PI * (1.0 - phase_in_fade)).cos())
            } else {
                1.0
            }
        } else {
            0.0
        };

        samples.push((phase.sin() * amp * 0.7) as f32);
    }
    samples
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a synthetic minimum-viable v129 .zc file with a single
    /// absolute interval + two 7-bit deltas.
    fn synth_zc_minimal() -> Vec<u8> {
        let mut buf = vec![0u8; 0x120]; // header + small data_info area + small data area
                                        // u16le data_info_pointer = 0x11a
        buf[0] = 0x1a;
        buf[1] = 0x01;
        buf[3] = 129;
        // header text fields stay null
        // data_info @ 0x11a: data_pointer = 0x120, res1 = 25000, divratio = 8, vres = 0
        let data_pointer: u16 = 0x120;
        buf[0x11a] = data_pointer as u8;
        buf[0x11a + 1] = (data_pointer >> 8) as u8;
        buf[0x11a + 2] = 0xa8;
        buf[0x11a + 3] = 0x61; // 25000 = 0x61a8
        buf[0x11a + 4] = 8; // divratio
        buf[0x11a + 5] = 0; // vres
                            // data starting at 0x120:
                            //   0x80 0x32 -> 13-bit absolute = 0x0032 = 50 µs
                            //   0x10      -> +16 delta -> last_diff = 66 µs
                            //   0x10      -> +16 delta -> last_diff = 82 µs
        buf.push(0x80);
        buf.push(0x32);
        buf.push(0x10);
        buf.push(0x10);
        buf
    }

    #[test]
    fn detects_synthetic_minimal() {
        let bytes = synth_zc_minimal();
        assert!(is_zc(&bytes), "is_zc should accept synthetic file");
        let r = parse_zc(&bytes).expect("parse should succeed");
        assert_eq!(r.metadata.file_type, 129);
        assert_eq!(r.metadata.divratio, 8);
        assert_eq!(r.metadata.res1, 25_000);
        assert_eq!(r.times_s.len(), 3);
        assert!(
            (r.times_s[0] - 50e-6).abs() < 1e-9,
            "first time: {}",
            r.times_s[0]
        );
        assert!(
            (r.times_s[1] - (50.0 + 66.0) * 1e-6).abs() < 1e-9,
            "second time: {}",
            r.times_s[1]
        );
        assert!(
            (r.times_s[2] - (50.0 + 66.0 + 82.0) * 1e-6).abs() < 1e-9,
            "third time: {}",
            r.times_s[2]
        );
    }

    #[test]
    fn rejects_non_anabat() {
        let bytes = vec![b'R', b'I', b'F', b'F', 0, 0, 0, 0, b'W', b'A', b'V', b'E'];
        assert!(!is_zc(&bytes));
        assert!(parse_zc(&bytes).is_err());
    }

    #[test]
    fn rejects_tiny_file() {
        let bytes = vec![0u8; 100];
        assert!(!is_zc(&bytes));
        assert!(parse_zc(&bytes).is_err());
    }

    #[test]
    fn off_status_marks_subsequent_dots() {
        // Build a file where the OFF status appears BETWEEN dots so it
        // actually masks something. Data section:
        //   0x80 0x32         -> absolute 50 µs              (dot 0)
        //   0xE1 0x02         -> OFF status, mark 2 subsequent dots
        //   0x10              -> +16 delta -> 66 µs          (dot 1, OFF)
        //   0x10              -> +16 delta -> 82 µs          (dot 2, OFF)
        //   0x10              -> +16 delta -> 98 µs          (dot 3, ON again)
        let mut buf = synth_zc_minimal();
        // Replace the trailing [0x10, 0x10] with [0xE1, 0x02, 0x10, 0x10, 0x10].
        let abs_end = buf.len() - 2;
        buf.truncate(abs_end);
        buf.extend_from_slice(&[0xE1, 0x02, 0x10, 0x10, 0x10]);
        let r = parse_zc(&buf).unwrap();
        assert_eq!(
            r.times_s.len(),
            4,
            "expected 4 dots, got {}",
            r.times_s.len()
        );
        assert!(!r.off_mask[0], "dot 0 should be ON");
        assert!(r.off_mask[1], "dot 1 should be OFF (status start)");
        assert!(r.off_mask[2], "dot 2 should be OFF (status count)");
        assert!(!r.off_mask[3], "dot 3 should be ON again");
    }

    #[test]
    fn synthesise_waveform_matches_dot_frequency() {
        // Build a ZC where a single dot has a known frequency ~50 kHz
        // (period 160 µs at divratio=8). Synthesise at 384 kHz and
        // confirm the output has roughly the right per-sample phase
        // increment in the middle of the ON region.
        let bytes = synth_zc_minimal();
        let zc = parse_zc(&bytes).unwrap();
        let out = synthesise_waveform(&zc, 384_000);
        // Duration is ~198 µs → at 384 kHz that's ~76 samples; very short.
        // Just sanity-check we got non-empty output with finite values
        // and at least one non-zero sample in the middle of the run.
        assert!(!out.is_empty(), "synthesis returned empty");
        assert!(
            out.iter().any(|&s| s.abs() > 0.01),
            "synthesis is silent throughout"
        );
        assert!(
            out.iter().all(|&s| s.is_finite() && s.abs() <= 1.0),
            "synthesis produced non-finite or out-of-range samples"
        );
    }

    #[test]
    fn synthesise_off_dots_silent() {
        // Modify the synthetic stream so every dot is OFF — output
        // should be entirely silent.
        let mut buf = synth_zc_minimal();
        // Inject OFF status applying to lots of subsequent dots.
        let abs_end = buf.len() - 2;
        buf.truncate(abs_end);
        buf.extend_from_slice(&[0xE1, 0xFF, 0x10, 0x10]);
        let zc = parse_zc(&buf).unwrap();
        let out = synthesise_waveform(&zc, 384_000);
        assert!(
            out.iter().all(|&s| s == 0.0),
            "OFF-only stream should synthesise to silence"
        );
    }

    #[test]
    fn frequency_calc_clamps_out_of_range() {
        let bytes = synth_zc_minimal();
        let r = parse_zc(&bytes).unwrap();
        // Times: 50, 116, 198 µs. Period between 0 and 2 = 198 - 50 = 148 µs.
        // Valid range for divratio=8: Tmin=48, Tmax=2000 µs. So freq[1] valid.
        // freq[1] = 8 * 1e6 / 148e-6_µs = 8e6 / 148 = 54054 Hz
        assert!(r.freqs_hz[0] == 0.0, "freq[0] must be 0 (edge)");
        assert!(r.freqs_hz[2] == 0.0, "freq[end] must be 0 (edge)");
        assert!(
            (r.freqs_hz[1] - 8e6 / 148.0).abs() < 1.0,
            "expected ~{}, got {}",
            8e6 / 148.0,
            r.freqs_hz[1]
        );
    }
}
