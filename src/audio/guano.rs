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

/// Build GUANO metadata for a recording (WASM side).
/// Consolidates the duplicated inline GUANO construction from wav_encoder,
/// live_recording, etc. into a single shared function.
pub fn build_recording_guano(
    sample_rate: u32,
    duration_secs: f64,
    filename: &str,
    is_tauri: bool,
    mic_device_name: Option<&str>,
) -> GuanoMetadata {
    let now = js_sys::Date::new_0();
    let start_ms = now.get_time() - (duration_secs * 1000.0);
    let start = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(start_ms));
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
    let model = if is_tauri { "Desktop" } else { "Web" };

    let mut g = GuanoMetadata::new();
    g.add("GUANO|Version", "1.0");
    g.add("Timestamp", &timestamp);
    g.add("Length", &format!("{:.6}", duration_secs));
    g.add("Samplerate", &sample_rate.to_string());
    g.add("Make", "Oversample");
    g.add("Model", model);
    g.add("Firmware Version", version);
    g.add("TE", "1");
    g.add("Original Filename", filename);
    if let Some(mic) = mic_device_name {
        if !mic.is_empty() {
            g.add("Microphone", mic);
        }
    }
    let platform = if is_tauri { "Tauri" } else { "browser" };
    g.add("Note", &format!("Recorded with Oversample v{} ({})", version, platform));
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
