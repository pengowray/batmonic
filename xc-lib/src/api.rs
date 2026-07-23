use crate::types::{XcRecording, XcSearchResult};

const API_BASE: &str = "https://xeno-canto.org/api/3/recordings";

/// Parse an XC recording from API JSON.
///
/// Rejects (returns `None` for) records missing the fields that make a record
/// usable, rather than letting them through with empty values that fail opaquely
/// later:
///  - `id` must be present AND numeric — `id_num()` parses it and falls back to
///    0, which would collide as `XC0` in the cache/index filename scheme.
///  - `file` (the download URL) must be present and non-empty, else the record
///    is undownloadable and would only fail at download time.
fn parse_recording(rec: &serde_json::Value) -> Option<XcRecording> {
    let id = rec["id"].as_str()?;
    if id.parse::<u64>().is_err() {
        return None; // non-numeric id → would collide at XC0
    }
    if rec["file"].as_str().unwrap_or("").is_empty() {
        return None; // no download URL → undownloadable record
    }
    let s = |key: &str| rec[key].as_str().unwrap_or("").to_string();
    let also = rec["also"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    Some(XcRecording {
        id: s("id"),
        genus: s("gen"),
        sp: s("sp"),
        ssp: s("ssp"),
        en: s("en"),
        grp: s("grp"),
        rec: s("rec"),
        cnt: s("cnt"),
        loc: s("loc"),
        lat: s("lat"),
        lon: s("lon"),
        alt: s("alt"),
        sound_type: s("type"),
        sex: s("sex"),
        stage: s("stage"),
        method: s("method"),
        url: s("url"),
        file_url: s("file"),
        file_name: s("file-name"),
        lic: s("lic"),
        q: s("q"),
        length: s("length"),
        time: s("time"),
        date: s("date"),
        uploaded: s("uploaded"),
        also,
        rmk: s("rmk"),
        animal_seen: s("animal-seen"),
        playback_used: s("playback-used"),
        temp: s("temp"),
        regnr: s("regnr"),
        auto_rec: s("auto"),
        dvc: s("dvc"),
        mic: s("mic"),
        smp: s("smp"),
    })
}

/// Parse a search response from the XC API.
fn parse_search_response(body: &serde_json::Value) -> Result<XcSearchResult, String> {
    if let Some(err) = body.get("error") {
        return Err(format!("API error: {}", err));
    }

    let num_recordings = body["numRecordings"]
        .as_str()
        .and_then(|s| s.parse().ok())
        .or_else(|| body["numRecordings"].as_u64().map(|n| n as u32))
        .unwrap_or(0);
    let num_species = body["numSpecies"]
        .as_str()
        .and_then(|s| s.parse().ok())
        .or_else(|| body["numSpecies"].as_u64().map(|n| n as u32))
        .unwrap_or(0);
    let num_pages = body["numPages"].as_u64().map(|n| n as u32).unwrap_or(1);
    let page = body["page"].as_u64().map(|n| n as u32).unwrap_or(1);

    // Surface a schema mismatch rather than silently returning zero results: a
    // present-but-non-array `recordings` means the response shape changed.
    if let Some(v) = body.get("recordings") {
        if !v.is_array() {
            return Err("unexpected XC response: `recordings` is not an array".to_string());
        }
    }
    let recordings = body["recordings"]
        .as_array()
        .map(|arr| arr.iter().filter_map(parse_recording).collect())
        .unwrap_or_default();

    Ok(XcSearchResult {
        num_recordings,
        num_species,
        num_pages,
        page,
        recordings,
    })
}

/// Search the XC API with a query string.
///
/// The query uses XC search tag syntax, e.g.:
/// - `"grp:bats"` — all bat recordings
/// - `"grp:bats cnt:Australia"` — Australian bat recordings
/// - `"nr:928094"` — specific recording by number
/// - `"sp:Myotis dasycneme"` — species name search
///
/// Plain text without tags is automatically wrapped in `sp:"..."`,
/// since XC API v3 requires all queries to use tags.
pub async fn search(
    client: &reqwest::Client,
    api_key: &str,
    query: &str,
    page: u32,
    per_page: u32,
) -> Result<XcSearchResult, String> {
    let query = normalize_query(query);
    let url = format!(
        "{}?query={}&key={}&page={}&per_page={}",
        API_BASE,
        urlencod(&query),
        urlencod(api_key),
        page,
        per_page.clamp(50, 500),
    );

    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("HTTP request failed: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("HTTP {status}: {body}"));
    }

    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse JSON: {e}"))?;

    parse_search_response(&body)
}

/// Fetch a single recording by XC number.
pub async fn fetch_recording(
    client: &reqwest::Client,
    api_key: &str,
    id: u64,
) -> Result<XcRecording, String> {
    let result = search(client, api_key, &format!("nr:{id}"), 1, 50).await?;
    result
        .recordings
        .into_iter()
        .next()
        .ok_or_else(|| format!("No recording found for XC{id}"))
}

/// Download audio bytes for a recording.
pub async fn download_audio(client: &reqwest::Client, file_url: &str) -> Result<Vec<u8>, String> {
    let resp = client.get(file_url).send().await.map_err(|e| {
        if e.is_timeout() {
            "Download timed out — try again".to_string()
        } else if e.is_connect() {
            "Could not connect to server — check your internet connection".to_string()
        } else {
            format!("Download failed: {e}")
        }
    })?;

    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        return Err(match status {
            401 | 403 => "Access denied — check your API key".into(),
            404 => "Recording not found on server".into(),
            429 => "Too many requests — wait a moment and try again".into(),
            500..=599 => format!("Server error (HTTP {status}) — try again later"),
            _ => format!("Download failed (HTTP {status})"),
        });
    }

    resp.bytes().await.map(|b| b.to_vec()).map_err(|e| {
        if e.is_timeout() {
            "Download timed out while reading data — try again".to_string()
        } else {
            format!("Failed to read audio data: {e}")
        }
    })
}

/// Ensure the query uses tag syntax required by XC API v3.
///
/// If the query already contains tags (e.g. `gen:Myotis`, `grp:bats`),
/// it is returned as-is. Otherwise the plain text is wrapped in `sp:"..."`
/// — the v3 replacement for v2's tag-less species search.
fn normalize_query(query: &str) -> String {
    let q = query.trim();
    if q.is_empty() {
        return q.to_string();
    }
    // Check if query already has at least one tag (alphabetic word followed by ':')
    let has_tag = q.split_whitespace().any(|tok| {
        tok.find(':').map_or(false, |pos| {
            pos > 0 && tok[..pos].chars().all(|c| c.is_ascii_alphabetic())
        })
    });
    if has_tag {
        return q.to_string();
    }
    // Wrap plain text as species name search; quote multi-word values
    if q.contains(' ') {
        format!("sp:\"{q}\"")
    } else {
        format!("sp:{q}")
    }
}

/// Minimal URL encoding for query parameters.
fn urlencod(s: &str) -> String {
    s.replace(' ', "%20")
        .replace('"', "%22")
        .replace('&', "%26")
        .replace('=', "%3D")
        .replace('+', "%2B")
        .replace('#', "%23")
}

/// Parse an XC number from various input formats:
/// "928094", "XC928094", "xc928094", "https://xeno-canto.org/928094"
pub fn parse_xc_number(input: &str) -> Result<u64, String> {
    let s = input.trim();

    if let Ok(n) = s.parse::<u64>() {
        return Ok(n);
    }

    if let Some(rest) = s.strip_prefix("XC").or_else(|| s.strip_prefix("xc")) {
        return rest
            .parse::<u64>()
            .map_err(|_| format!("Invalid XC number: {s}"));
    }

    if s.contains("xeno-canto.org/") {
        if let Some(last) = s.trim_end_matches('/').rsplit('/').next() {
            if let Ok(n) = last.parse::<u64>() {
                return Ok(n);
            }
        }
    }

    Err(format!("Can't parse XC number from: {s}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_recording_rejects_missing_critical_fields() {
        // No id → rejected.
        assert!(parse_recording(&json!({ "file": "https://x/a.mp3" })).is_none());
        // Non-numeric id → rejected (would collide at XC0).
        assert!(parse_recording(&json!({ "id": "abc", "file": "https://x/a.mp3" })).is_none());
        // Missing / empty download URL → rejected (would fail opaquely later).
        assert!(parse_recording(&json!({ "id": "123" })).is_none());
        assert!(parse_recording(&json!({ "id": "123", "file": "" })).is_none());
    }

    #[test]
    fn parse_recording_accepts_valid() {
        let rec = parse_recording(&json!({
            "id": "928094", "file": "https://x/a.mp3", "gen": "Myotis", "sp": "daubentonii"
        }));
        let rec = rec.expect("valid record");
        assert_eq!(rec.id, "928094");
        assert_eq!(rec.id_num(), 928094);
        assert_eq!(rec.file_url, "https://x/a.mp3");
    }

    #[test]
    fn parse_search_response_flags_schema_mismatch() {
        // `recordings` present but not an array → error, not silent empty.
        assert!(parse_search_response(&json!({ "recordings": "oops" })).is_err());
        // A top-level error is surfaced.
        assert!(parse_search_response(&json!({ "error": "bad key" })).is_err());
        // A well-formed empty result is fine.
        let ok = parse_search_response(&json!({ "numRecordings": "0", "recordings": [] }));
        assert_eq!(ok.unwrap().recordings.len(), 0);
    }
}
