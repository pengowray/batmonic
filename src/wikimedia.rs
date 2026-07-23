//! Importing audio from Wikimedia Commons and the other MediaWiki wikis.
//!
//! Unlike Xeno-Canto (see `components::xc_browser`), Wikimedia needs no API
//! key and no native proxy: `commons.wikimedia.org/w/api.php` answers
//! anonymous cross-origin requests when passed `origin=*`, and
//! `upload.wikimedia.org` serves media with `Access-Control-Allow-Origin: *`.
//! So one code path — `fetch` from WASM — covers web, desktop and Android.
//!
//! The interesting metadata is the structured-data statement
//! [P14424](https://www.wikidata.org/wiki/Property:P14424), "time expansion
//! factor". A bat detector that records in 10x time expansion uploads a file
//! whose stored sample rate is a tenth of the real one, so every frequency in
//! it reads ten times too low. When a file carries P14424 we hand the factor
//! to the loader, which reinterprets the samples at the true rate.

use crate::state::AppState;

/// Host used for structured data and as the default for bare file titles.
/// Files uploaded to a local wiki are handled by falling back to that wiki.
const COMMONS: &str = "commons.wikimedia.org";

/// Wikidata property id for "time expansion factor" (quantity).
const P_TIME_EXPANSION: &str = "P14424";

/// Wikimedia domains we'll talk to. Everything under these answers CORS
/// preflight-free GETs with `origin=*`; an arbitrary third-party MediaWiki
/// generally does not, so pasting one gets a clear error rather than an
/// opaque network failure.
const ALLOWED_SUFFIXES: &[&str] = &[
    ".wikipedia.org",
    ".wikimedia.org",
    ".wikibooks.org",
    ".wikisource.org",
    ".wikiversity.org",
    ".wikivoyage.org",
    ".wiktionary.org",
    ".wikinews.org",
    ".wikiquote.org",
    "wikidata.org",
    "wikimedia.org",
];

/// File extensions worth offering. The decoder handles WAV, FLAC, OGG
/// (Vorbis), MP3 and M4A; Opus and WebM are listed because Commons hosts
/// them and a decode failure is a clearer answer than silently hiding them.
const AUDIO_EXTS: &[&str] = &[
    "wav", "flac", "mp3", "ogg", "oga", "opus", "m4a", "mp4", "aac", "webm",
];

/// What the user's pasted text resolved to.
#[derive(Clone, Debug, PartialEq)]
pub enum WikiTarget {
    /// A single media file, e.g. `File:Bat feeding buzz.wav`.
    File { api_host: String, title: String },
    /// An article — list the audio files it uses.
    Page { api_host: String, title: String },
    /// Free text — search Commons for matching audio.
    Search { query: String },
}

/// One media file as described by the API.
#[derive(Clone, Debug, PartialEq)]
pub struct WikiFile {
    /// Full page title including namespace, e.g. `File:Bat feeding buzz.wav`.
    pub title: String,
    /// Title without the namespace prefix — used as the loaded file's name.
    pub name: String,
    /// Direct media URL on `upload.wikimedia.org`.
    pub url: String,
    pub mime: String,
    pub size: u64,
    /// Playing time as stored, i.e. before any time-expansion correction.
    pub duration_secs: Option<f64>,
    /// Human-facing file description page.
    pub description_url: String,
    /// P14424, when the file declares one.
    pub time_expansion: Option<f64>,
    /// Label/value rows for the metadata panel.
    pub fields: Vec<(String, String)>,
}

impl WikiFile {
    /// Playing time once a time-expansion correction is applied.
    pub fn corrected_duration_secs(&self) -> Option<f64> {
        let d = self.duration_secs?;
        Some(match self.time_expansion {
            Some(te) if te.is_finite() && te > 0.0 => d / te,
            _ => d,
        })
    }
}

// ── Input parsing ────────────────────────────────────────────────────────

fn host_allowed(host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    ALLOWED_SUFFIXES
        .iter()
        .any(|s| host == s.trim_start_matches('.') || host.ends_with(s))
}

/// Percent-decode, then turn MediaWiki's `_` word separator back into a space.
pub fn decode_component(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
            if let Some(b) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(if bytes[i] == b'_' { b' ' } else { bytes[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Percent-encode for use as a query-string value. Everything outside the
/// unreserved set is escaped, which is stricter than `encodeURIComponent` but
/// always valid.
fn encode_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn is_file_title(title: &str) -> bool {
    // The API normalises localised namespaces for us, so only the canonical
    // English prefixes need recognising up front.
    let lower = title.to_ascii_lowercase();
    lower.starts_with("file:") || lower.starts_with("image:") || lower.starts_with("media:")
}

fn has_audio_ext(name: &str) -> bool {
    name.rsplit('.')
        .next()
        .map(|e| {
            let e = e.to_ascii_lowercase();
            AUDIO_EXTS.contains(&e.as_str())
        })
        .unwrap_or(false)
}

/// Strip a namespace prefix, giving the bare filename.
fn strip_namespace(title: &str) -> String {
    match title.split_once(':') {
        Some((ns, rest)) if is_file_title(&format!("{ns}:")) => rest.trim().to_string(),
        _ => title.trim().to_string(),
    }
}

/// Work out what the user meant by whatever they pasted.
///
/// Accepts a Commons or Wikipedia file page URL, a direct
/// `upload.wikimedia.org` media URL, an article URL, a `File:…` title, a bare
/// filename ending in an audio extension, or free text to search for.
pub fn parse_input(input: &str) -> Result<WikiTarget, String> {
    let input = input.trim();
    if input.is_empty() {
        return Err("Enter a Wikimedia file, article or search term".into());
    }

    if let Some(rest) = input
        .strip_prefix("https://")
        .or_else(|| input.strip_prefix("http://"))
    {
        let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
        let host = host.split('@').next_back().unwrap_or(host);
        let host = host.split(':').next().unwrap_or(host).to_ascii_lowercase();
        if !host_allowed(&host) {
            return Err(format!(
                "{host} isn't a Wikimedia site \u{2014} paste a Wikipedia or Wikimedia Commons link"
            ));
        }
        let (path, _query) = path.split_once('?').unwrap_or((path, ""));
        let path = path.split('#').next().unwrap_or(path);

        // Direct media URL: .../wikipedia/commons/e/e7/Bat_feeding_buzz.wav
        // and its /thumb/ variant, which appends a rendition filename.
        if host == "upload.wikimedia.org" {
            let file = path
                .split('/')
                .filter(|s| !s.is_empty())
                .find(|s| has_audio_ext(s))
                .ok_or("That upload.wikimedia.org link doesn't point at an audio file")?;
            return Ok(WikiTarget::File {
                api_host: COMMONS.to_string(),
                title: format!("File:{}", decode_component(file)),
            });
        }

        // Query-string form: /w/index.php?title=File:Foo.wav
        if let Some(q) = rest.split_once('?').map(|(_, q)| q) {
            for pair in q.split('&') {
                if let Some(v) = pair.strip_prefix("title=") {
                    let title = decode_component(v);
                    return Ok(classify(host, title));
                }
            }
        }

        // `path` here has no leading slash — it's whatever followed the host.
        let title = path
            .split_once("wiki/")
            .map(|(_, t)| t)
            .unwrap_or(path)
            .trim_matches('/');
        if title.is_empty() {
            return Err("That link has no page title in it".into());
        }
        return Ok(classify(host, decode_component(title)));
    }

    if is_file_title(input) {
        return Ok(WikiTarget::File {
            api_host: COMMONS.to_string(),
            title: normalise_file_title(&input.replace('_', " ")),
        });
    }
    if has_audio_ext(input) && !input.contains(' ') {
        return Ok(WikiTarget::File {
            api_host: COMMONS.to_string(),
            title: format!("File:{input}"),
        });
    }
    Ok(WikiTarget::Search {
        query: input.to_string(),
    })
}

/// Rewrite `Image:`/`Media:` onto the canonical `File:` prefix.
fn normalise_file_title(title: &str) -> String {
    format!("File:{}", strip_namespace(title))
}

fn classify(host: String, title: String) -> WikiTarget {
    if is_file_title(&title) {
        // Structured data for a file lives on Commons even when the link came
        // from a local wiki, so resolve there and let the caller fall back.
        WikiTarget::File {
            api_host: COMMONS.to_string(),
            title: normalise_file_title(&title),
        }
    } else {
        WikiTarget::Page {
            api_host: host,
            title,
        }
    }
}

// ── API ──────────────────────────────────────────────────────────────────

async fn api_get(host: &str, params: &[(&str, &str)]) -> Result<serde_json::Value, String> {
    let mut url =
        format!("https://{host}/w/api.php?action=query&format=json&formatversion=2&origin=*");
    for (k, v) in params {
        url.push('&');
        url.push_str(k);
        url.push('=');
        url.push_str(&encode_component(v));
    }
    let text = crate::components::file_sidebar::fetch_text(&url).await?;
    let json: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| format!("Wikimedia API returned invalid JSON: {e}"))?;
    if let Some(err) = json["error"]["info"].as_str() {
        return Err(format!("Wikimedia API error: {err}"));
    }
    Ok(json)
}

/// The media URL and description fields, available from any wiki (a local
/// wiki reports Commons-hosted files too, flagged `imagerepository: shared`).
const FILE_PARAMS: &[(&str, &str)] = &[
    ("iiprop", "url|size|mime|extmetadata"),
    (
        "iiextmetadatafilter",
        "ImageDescription|Artist|LicenseShortName|DateTimeOriginal|Credit",
    ),
];

/// The MediaInfo slot, where structured-data statements (P14424) live. Only
/// Commons has that slot — asking any other wiki for it earns a warning and
/// nothing else.
const MEDIAINFO_PARAMS: &[(&str, &str)] = &[("rvprop", "content"), ("rvslots", "mediainfo")];

/// Build a parameter list for `host`, asking for structured data only from
/// the wiki that has it.
fn params_for<'a>(host: &str, extra: &[(&'a str, &'a str)]) -> Vec<(&'a str, &'a str)> {
    let mut v: Vec<(&str, &str)> = vec![if host == COMMONS {
        ("prop", "imageinfo|revisions")
    } else {
        ("prop", "imageinfo")
    }];
    v.extend_from_slice(FILE_PARAMS);
    if host == COMMONS {
        v.extend_from_slice(MEDIAINFO_PARAMS);
    }
    v.extend_from_slice(extra);
    v
}

/// Pull P14424 out of a MediaInfo slot's JSON content.
fn time_expansion_from_mediainfo(page: &serde_json::Value) -> Option<f64> {
    let content = page["revisions"][0]["slots"]["mediainfo"]["content"]
        .as_str()
        .or_else(|| page["revisions"][0]["slots"]["mediainfo"]["*"].as_str())?;
    let mediainfo: serde_json::Value = serde_json::from_str(content).ok()?;
    let statements = mediainfo["statements"][P_TIME_EXPANSION].as_array()?;
    // Prefer a preferred-rank statement, else the first normal one; skip
    // deprecated values and `somevalue`/`novalue` snaks.
    let pick = statements
        .iter()
        .filter(|s| s["rank"].as_str() != Some("deprecated"))
        .find(|s| s["rank"].as_str() == Some("preferred"))
        .or_else(|| {
            statements
                .iter()
                .find(|s| s["rank"].as_str() != Some("deprecated"))
        })?;
    let snak = &pick["mainsnak"];
    if snak["snaktype"].as_str() != Some("value") {
        return None;
    }
    // Quantities arrive as a signed decimal string, e.g. "+10" or "+2.5".
    let amount = snak["datavalue"]["value"]["amount"].as_str()?;
    amount.trim_start_matches('+').parse::<f64>().ok()
}

/// Strip the HTML that `extmetadata` descriptions and credits are wrapped in.
fn strip_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut depth = 0usize;
    for c in s.chars() {
        match c {
            '<' => depth += 1,
            '>' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#039;", "'")
        .replace("&nbsp;", " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn parse_file_page(page: &serde_json::Value) -> Option<WikiFile> {
    let title = page["title"].as_str()?.to_string();
    // Don't test `missing` here: a local wiki marks every Commons-hosted file
    // missing (it has no local description page) while still returning full
    // imageinfo. Absent imageinfo is the real "not here" signal.
    let ii = page["imageinfo"].get(0)?;
    let url = ii["url"].as_str()?.to_string();
    let mime = ii["mime"].as_str().unwrap_or("").to_string();
    let size = ii["size"].as_u64().unwrap_or(0);
    let duration_secs = ii["duration"].as_f64();
    let description_url = ii["descriptionurl"].as_str().unwrap_or("").to_string();
    let time_expansion = time_expansion_from_mediainfo(page);

    let ext = |key: &str| {
        ii["extmetadata"][key]["value"]
            .as_str()
            .map(strip_html)
            .filter(|v| !v.trim().is_empty())
    };

    let mut fields = Vec::new();
    if let Some(v) = ext("ImageDescription") {
        fields.push(("Description".to_string(), v));
    }
    if let Some(v) = ext("Artist") {
        fields.push(("Author".to_string(), v));
    }
    if let Some(v) = ext("Credit") {
        fields.push(("Source".to_string(), v));
    }
    if let Some(v) = ext("DateTimeOriginal") {
        fields.push(("Date".to_string(), v));
    }
    if let Some(v) = ext("LicenseShortName") {
        fields.push(("License".to_string(), v));
    }
    if let Some(te) = time_expansion {
        fields.push((
            "Time expansion".to_string(),
            format!("{} (P14424)", format_factor(te)),
        ));
    }
    if !description_url.is_empty() {
        fields.push(("URL".to_string(), description_url.clone()));
    }

    Some(WikiFile {
        name: strip_namespace(&title),
        title,
        url,
        mime,
        size,
        duration_secs,
        description_url,
        time_expansion,
        fields,
    })
}

/// Render a factor the way people write it: `\u{00D7}10`, `\u{00D7}2.5`.
pub fn format_factor(te: f64) -> String {
    if (te - te.round()).abs() < 1e-9 {
        format!("\u{00D7}{}", te.round() as i64)
    } else {
        format!("\u{00D7}{te}")
    }
}

fn pages_of(json: &serde_json::Value) -> Vec<serde_json::Value> {
    json["query"]["pages"]
        .as_array()
        .cloned()
        .unwrap_or_default()
}

/// Look up one file. Structured data lives on Commons, so try there first and
/// fall back to the originating wiki for files uploaded locally.
pub async fn fetch_file(api_host: &str, title: &str) -> Result<WikiFile, String> {
    let json = api_get(COMMONS, &params_for(COMMONS, &[("titles", title)])).await?;
    if let Some(file) = pages_of(&json).first().and_then(parse_file_page) {
        return Ok(file);
    }
    if api_host != COMMONS {
        let json = api_get(api_host, &params_for(api_host, &[("titles", title)])).await?;
        if let Some(file) = pages_of(&json).first().and_then(parse_file_page) {
            return Ok(file);
        }
    }
    Err(format!("{title} isn't on Wikimedia Commons"))
}

/// How many titles one `titles=` query may name.
const TITLE_BATCH: usize = 50;

/// Re-query Commons for `files` listed by another wiki, so they pick up the
/// structured data (P14424 above all) that only Commons serves. Files Commons
/// doesn't have — local uploads — keep the description they came with.
async fn enrich_from_commons(files: Vec<WikiFile>) -> Vec<WikiFile> {
    let mut enriched = Vec::with_capacity(files.len());
    for chunk in files.chunks(TITLE_BATCH) {
        let titles = chunk
            .iter()
            .map(|f| f.title.as_str())
            .collect::<Vec<_>>()
            .join("|");
        let looked_up = match api_get(COMMONS, &params_for(COMMONS, &[("titles", &titles)])).await {
            Ok(json) => pages_of(&json)
                .iter()
                .filter_map(parse_file_page)
                .collect::<Vec<_>>(),
            Err(e) => {
                log::warn!("Commons lookup for article files failed: {e}");
                Vec::new()
            }
        };
        for file in chunk {
            let better = looked_up.iter().find(|c| c.title == file.title).cloned();
            enriched.push(better.unwrap_or_else(|| file.clone()));
        }
    }
    enriched
}

fn collect_audio(json: &serde_json::Value) -> Vec<WikiFile> {
    let mut files: Vec<WikiFile> = pages_of(json)
        .iter()
        .filter_map(parse_file_page)
        .filter(|f| f.mime.starts_with("audio/") || has_audio_ext(&f.name))
        .collect();
    files.sort_by(|a, b| a.name.cmp(&b.name));
    files
}

/// List the audio files an article uses.
pub async fn list_page_audio(api_host: &str, title: &str) -> Result<Vec<WikiFile>, String> {
    let params = params_for(
        api_host,
        &[
            ("generator", "images"),
            ("gimlimit", "500"),
            ("titles", title),
        ],
    );
    let json = api_get(api_host, &params).await?;
    let files = collect_audio(&json);
    if api_host == COMMONS || files.is_empty() {
        return Ok(files);
    }
    Ok(enrich_from_commons(files).await)
}

/// Search Commons for audio files matching free text.
pub async fn search_audio(query: &str) -> Result<Vec<WikiFile>, String> {
    let search = format!("filetype:audio {query}");
    let params = params_for(
        COMMONS,
        &[
            ("generator", "search"),
            ("gsrsearch", &search),
            ("gsrnamespace", "6"),
            ("gsrlimit", "40"),
        ],
    );
    let json = api_get(COMMONS, &params).await?;
    Ok(collect_audio(&json))
}

/// Resolve whatever the user typed. A file resolves to a single-entry list;
/// an article or a search resolves to everything that matched.
pub async fn resolve(target: &WikiTarget) -> Result<Vec<WikiFile>, String> {
    match target {
        WikiTarget::File { api_host, title } => Ok(vec![fetch_file(api_host, title).await?]),
        WikiTarget::Page { api_host, title } => list_page_audio(api_host, title).await,
        WikiTarget::Search { query } => search_audio(query).await,
    }
}

// ── Loading ──────────────────────────────────────────────────────────────

/// Download a file and hand it to the normal loader, correcting the sample
/// rate when the file declares a time expansion factor.
pub async fn load_file(state: AppState, file: &WikiFile) -> Result<(), String> {
    let load_id = state.loading_start(&file.name);
    let result = load_file_inner(state, file, load_id).await;
    state.loading_done(load_id);
    if let Err(ref e) = result {
        state.show_error_toast(format!("Could not load {}: {e}", file.name));
    } else if let Some(te) = file.time_expansion {
        state.show_info_toast(format!(
            "{} \u{2014} corrected for {} time expansion",
            file.name,
            format_factor(te)
        ));
    }
    result
}

async fn load_file_inner(state: AppState, file: &WikiFile, load_id: u64) -> Result<(), String> {
    let bytes = crate::components::file_sidebar::fetch_bytes(&file.url).await?;
    crate::components::file_sidebar::load_named_bytes(
        file.name.clone(),
        &bytes,
        Some(file.fields.clone()),
        None,
        file.time_expansion,
        Some("Wikimedia Commons".to_string()),
        state,
        load_id,
        false,
    )
    .await
}

/// Resolve and load the first match in one step — used by the URL-hash
/// deep link. Reports its own failures, since there's no UI to hand them to.
pub async fn load_from_input(state: AppState, input: &str) {
    let resolved = async {
        let target = parse_input(input)?;
        let files = resolve(&target).await?;
        files
            .into_iter()
            .next()
            .ok_or_else(|| format!("No audio found for \u{201C}{input}\u{201D}"))
    }
    .await;
    match resolved {
        // load_file surfaces its own errors.
        Ok(file) => {
            let _ = load_file(state, &file).await;
        }
        Err(e) => {
            log::error!("Wikimedia deep link {input:?} failed: {e}");
            state.show_error_toast(e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_commons_file_page_url() {
        assert_eq!(
            parse_input("https://commons.wikimedia.org/wiki/File:Bat_feeding_buzz.wav").unwrap(),
            WikiTarget::File {
                api_host: COMMONS.to_string(),
                title: "File:Bat feeding buzz.wav".to_string(),
            }
        );
    }

    #[test]
    fn file_page_on_a_local_wiki_still_resolves_against_commons() {
        assert_eq!(
            parse_input("https://en.wikipedia.org/wiki/File:Bat_feeding_buzz.wav").unwrap(),
            WikiTarget::File {
                api_host: COMMONS.to_string(),
                title: "File:Bat feeding buzz.wav".to_string(),
            }
        );
    }

    #[test]
    fn parses_direct_upload_url() {
        assert_eq!(
            parse_input("https://upload.wikimedia.org/wikipedia/commons/e/e7/Bat_feeding_buzz.wav")
                .unwrap(),
            WikiTarget::File {
                api_host: COMMONS.to_string(),
                title: "File:Bat feeding buzz.wav".to_string(),
            }
        );
    }

    #[test]
    fn parses_article_url_as_a_page() {
        assert_eq!(
            parse_input("https://en.wikipedia.org/wiki/Bat").unwrap(),
            WikiTarget::Page {
                api_host: "en.wikipedia.org".to_string(),
                title: "Bat".to_string(),
            }
        );
    }

    #[test]
    fn bare_titles_and_filenames_go_to_commons() {
        assert!(matches!(
            parse_input("File:Bat feeding buzz.wav").unwrap(),
            WikiTarget::File { .. }
        ));
        assert!(matches!(
            parse_input("Bat_feeding_buzz.wav").unwrap(),
            WikiTarget::File { .. }
        ));
    }

    #[test]
    fn free_text_becomes_a_search() {
        assert_eq!(
            parse_input("hoary bat echolocation").unwrap(),
            WikiTarget::Search {
                query: "hoary bat echolocation".to_string()
            }
        );
    }

    #[test]
    fn non_wikimedia_hosts_are_rejected() {
        let err = parse_input("https://example.com/wiki/File:Foo.wav").unwrap_err();
        assert!(err.contains("example.com"), "{err}");
    }

    #[test]
    fn empty_input_is_rejected() {
        assert!(parse_input("   ").is_err());
    }

    #[test]
    fn reads_p14424_from_a_mediainfo_slot() {
        let page = serde_json::json!({
            "revisions": [{ "slots": { "mediainfo": { "content":
                r#"{"statements":{"P14424":[{"mainsnak":{"snaktype":"value",
                   "datavalue":{"value":{"amount":"+10","unit":"1"},
                   "type":"quantity"}},"rank":"normal"}]}}"#
            }}}]
        });
        assert_eq!(time_expansion_from_mediainfo(&page), Some(10.0));
    }

    #[test]
    fn prefers_a_preferred_rank_statement_and_skips_deprecated() {
        let page = serde_json::json!({
            "revisions": [{ "slots": { "mediainfo": { "content":
                r#"{"statements":{"P14424":[
                   {"mainsnak":{"snaktype":"value","datavalue":{"value":{"amount":"+3"}}},"rank":"deprecated"},
                   {"mainsnak":{"snaktype":"value","datavalue":{"value":{"amount":"+8"}}},"rank":"normal"},
                   {"mainsnak":{"snaktype":"value","datavalue":{"value":{"amount":"+20"}}},"rank":"preferred"}
                   ]}}"#
            }}}]
        });
        assert_eq!(time_expansion_from_mediainfo(&page), Some(20.0));
    }

    #[test]
    fn missing_or_valueless_statements_yield_none() {
        let none = serde_json::json!({ "revisions": [{ "slots": { "mediainfo": {
            "content": r#"{"statements":{}}"# }}}] });
        assert_eq!(time_expansion_from_mediainfo(&none), None);

        let somevalue = serde_json::json!({ "revisions": [{ "slots": { "mediainfo": { "content":
            r#"{"statements":{"P14424":[{"mainsnak":{"snaktype":"somevalue"},"rank":"normal"}]}}"#
        }}}] });
        assert_eq!(time_expansion_from_mediainfo(&somevalue), None);
    }

    #[test]
    fn strips_html_from_descriptions() {
        assert_eq!(
            strip_html("<p>Echolocation of <i>Pipistrellus</i> &amp; friends</p>"),
            "Echolocation of Pipistrellus & friends"
        );
    }

    #[test]
    fn corrected_duration_divides_by_the_factor() {
        let mut f = WikiFile {
            title: "File:X.wav".into(),
            name: "X.wav".into(),
            url: String::new(),
            mime: "audio/wav".into(),
            size: 0,
            duration_secs: Some(30.0),
            description_url: String::new(),
            time_expansion: Some(10.0),
            fields: Vec::new(),
        };
        assert_eq!(f.corrected_duration_secs(), Some(3.0));
        f.time_expansion = None;
        assert_eq!(f.corrected_duration_secs(), Some(30.0));
    }

    #[test]
    fn formats_factors_the_way_people_write_them() {
        assert_eq!(format_factor(10.0), "\u{00D7}10");
        assert_eq!(format_factor(2.5), "\u{00D7}2.5");
    }
}
