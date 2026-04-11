/// Parser and writer for GUANO metadata embedded in WAV files.
/// GUANO (Grand Unified Acoustic Notation Ontology) stores text metadata
/// as a "guan" subchunk in the RIFF structure.

#[derive(Clone, Debug, Default)]
pub struct GuanoMetadata {
    pub fields: Vec<(String, String)>,
}

impl GuanoMetadata {
    pub fn new() -> Self {
        Self { fields: Vec::new() }
    }

    pub fn add(&mut self, key: &str, value: &str) -> &mut Self {
        self.fields.push((key.to_string(), value.to_string()));
        self
    }

    /// Build the GUANO text representation (key: value lines).
    pub fn to_text(&self) -> String {
        build_guano_text(&self.fields)
    }
}

/// Build GUANO text from key-value pairs.
pub fn build_guano_text(fields: &[(String, String)]) -> String {
    let mut text = String::new();
    for (key, value) in fields {
        text.push_str(key);
        text.push_str(": ");
        text.push_str(value);
        text.push('\n');
    }
    text
}

/// Append a GUANO "guan" RIFF subchunk to WAV bytes in-place.
/// Updates the RIFF header file size at bytes[4..8].
pub fn append_guano_chunk(wav_bytes: &mut Vec<u8>, guano_text: &str) {
    let text_bytes = guano_text.as_bytes();
    let chunk_size = text_bytes.len() as u32;

    // Append chunk: "guan" + size (LE u32) + text data
    wav_bytes.extend_from_slice(b"guan");
    wav_bytes.extend_from_slice(&chunk_size.to_le_bytes());
    wav_bytes.extend_from_slice(text_bytes);

    // RIFF word-alignment: pad with a zero byte if chunk data size is odd
    if !text_bytes.len().is_multiple_of(2) {
        wav_bytes.push(0);
    }

    // Update RIFF header file size at bytes[4..8]
    // RIFF file size = total file size - 8 (for "RIFF" + size field itself)
    let riff_size = (wav_bytes.len() - 8) as u32;
    wav_bytes[4..8].copy_from_slice(&riff_size.to_le_bytes());
}

/// Search raw WAV bytes for a "guan" RIFF subchunk and parse GUANO metadata.
pub fn parse_guano(bytes: &[u8]) -> Option<GuanoMetadata> {
    // Must be RIFF/WAVE or RF64/WAVE
    if bytes.len() < 12 || &bytes[8..12] != b"WAVE" {
        return None;
    }
    let magic = &bytes[0..4];
    if magic != b"RIFF" && magic != b"RF64" {
        return None;
    }

    let mut pos = 12;
    while pos + 8 <= bytes.len() {
        let chunk_id = &bytes[pos..pos + 4];
        let chunk_size = u32::from_le_bytes([
            bytes[pos + 4],
            bytes[pos + 5],
            bytes[pos + 6],
            bytes[pos + 7],
        ]) as usize;
        let data_start = pos + 8;
        let data_end = data_start + chunk_size;

        if chunk_id == b"guan" && data_end <= bytes.len() {
            let text = std::str::from_utf8(&bytes[data_start..data_end])
                .ok()?;
            return Some(parse_guano_text(text));
        }

        // Chunks are word-aligned (padded to even size)
        pos = data_start + ((chunk_size + 1) & !1);
    }

    None
}

/// Parse GUANO metadata from raw chunk body bytes (without the "guan" chunk header).
pub fn parse_guano_chunk(chunk_body: &[u8]) -> Option<GuanoMetadata> {
    let text = std::str::from_utf8(chunk_body).ok()?;
    Some(parse_guano_text(text))
}

/// Extra recording metadata for GUANO beyond the core fields.
#[derive(Default)]
pub struct RecordingGuanoExtra {
    /// Mic interface type: "Oboe", "WASAPI", "USB (UAC2)", etc.
    pub mic_interface: Option<String>,
    /// Mic name/description: USB device name, "Internal", or web API device label.
    pub mic_name: Option<String>,
    /// USB mic manufacturer (for GUANO Make field).
    pub mic_make: Option<String>,
    /// GPS location: (latitude, longitude) in WGS84 decimal degrees.
    pub loc_position: Option<(f64, f64)>,
    /// Elevation in meters above mean sea level.
    pub loc_elevation: Option<f64>,
    /// Horizontal accuracy in meters.
    pub loc_accuracy: Option<f64>,
    /// Android device manufacturer (e.g. "samsung"). Privacy-controlled.
    pub device_make: Option<String>,
    /// Android device model (e.g. "SM-A556E"). Privacy-controlled.
    pub device_model: Option<String>,
}

/// Build GUANO metadata for a recording.
///
/// Field ordering follows the GUANO spec: GUANO namespace first, then standard
/// fields, then app-specific (Oversample|*) fields.
///
/// `timestamp` should be an ISO 8601 string with T separator and UTC offset
/// (e.g. "2024-03-15T10:30:00+10:00").
/// `version` should be the main Oversample app version (from the root crate).
pub fn build_recording_guano(
    sample_rate: u32,
    duration_secs: f64,
    filename: &str,
    is_tauri: bool,
    is_mobile: bool,
    extra: &RecordingGuanoExtra,
    timestamp: &str,
    version: &str,
) -> GuanoMetadata {
    let platform = if is_tauri && is_mobile {
        "Android"
    } else if is_tauri {
        "Desktop"
    } else {
        "Web"
    };

    // Make/Model: only for external mics (USB). Never use for internal/phone mic.
    let is_external_mic = extra.mic_interface.as_deref()
        .map(|i| i.contains("USB"))
        .unwrap_or(false);

    let mut g = GuanoMetadata::new();

    // ── GUANO namespace (must come first per spec) ──────────────────────
    g.add("GUANO|Version", "1.0");

    // ── Standard GUANO fields ───────────────────────────────────────────
    g.add("Timestamp", timestamp);
    g.add("Length", &format!("{:.6}", duration_secs));
    g.add("Samplerate", &sample_rate.to_string());

    // Make/Model reflect the recording hardware (mic), not the app/phone.
    // Only populated for external (USB) mics.
    if is_external_mic {
        if let Some(ref make) = extra.mic_make {
            if !make.is_empty() {
                g.add("Make", make);
            }
        }
        if let Some(ref name) = extra.mic_name {
            if !name.is_empty() {
                g.add("Model", name);
            }
        }
    }

    g.add("Original Filename", filename);

    // Location fields
    if let Some((lat, lon)) = extra.loc_position {
        g.add("Loc Position", &format!("{} {}", lat, lon));
    }
    if let Some(elev) = extra.loc_elevation {
        g.add("Loc Elevation", &format!("{:.1}", elev));
    }
    if let Some(acc) = extra.loc_accuracy {
        g.add("Loc Accuracy", &format!("{:.1}", acc));
    }

    // ── Oversample-specific fields (after standard ones) ────────────────
    g.add("Oversample|App|Version", version);
    g.add("Oversample|App|Platform", platform);

    // Device info (Android only, privacy-controlled)
    if let Some(ref make) = extra.device_make {
        if !make.is_empty() {
            g.add("Oversample|Device|Make", make);
        }
    }
    if let Some(ref model) = extra.device_model {
        if !model.is_empty() {
            g.add("Oversample|Device|Model", model);
        }
    }

    // Mic info
    if let Some(ref interface) = extra.mic_interface {
        if !interface.is_empty() {
            g.add("Oversample|Mic|Interface", interface);
        }
    }
    if let Some(ref name) = extra.mic_name {
        if !name.is_empty() {
            g.add("Oversample|Mic|Name", name);
        }
    }

    g
}

fn parse_guano_text(text: &str) -> GuanoMetadata {
    let mut fields = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some((key, value)) = line.split_once(':') {
            fields.push((key.trim().to_string(), value.trim().to_string()));
        }
    }
    GuanoMetadata { fields }
}
