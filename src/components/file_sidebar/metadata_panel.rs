use crate::state::store_fields::*;
use crate::state::{AppState, MetadataView};
use leptos::prelude::*;

/// Returns (section, display_key) for a GUANO field.
/// Known fields return "GUANO" as section; unknown pipe-separated keys
/// return the prefix (e.g. "BatGizmo App") as section and the last segment as display key.
fn categorize_guano_key(key: &str) -> (String, String) {
    let known = match key {
        // Pipe-separated variants
        "Loc|Lat" => Some("Latitude"),
        "Loc|Lon" => Some("Longitude"),
        "Loc|Elev" => Some("Elevation"),
        "Filter|HP" => Some("High-pass Filter"),
        "Filter|LP" => Some("Low-pass Filter"),
        "Species|Auto" => Some("Species (Auto)"),
        "Species|Manual" => Some("Species (Manual)"),
        // Standard GUANO space-separated field names
        "Loc Position" => Some("GPS Position"),
        "Loc Elevation" => Some("Elevation"),
        "Filter HP" => Some("High-pass Filter (kHz)"),
        "Filter LP" => Some("Low-pass Filter (kHz)"),
        "Species Auto ID" => Some("Species (Auto)"),
        "Species Manual ID" => Some("Species (Manual)"),
        "Temperature Int" => Some("Internal Temp"),
        "Temperature Ext" => Some("External Temp"),
        "Model" => Some("Model"),
        "Serial" => Some("Serial"),
        "Microphone" => Some("Microphone"),
        // Common fields
        "TE" => Some("Time Expansion"),
        "Samplerate" => Some("Sample Rate"),
        "Length" => Some("Length"),
        _ => None,
    };
    if let Some(display) = known {
        return ("GUANO".into(), display.into());
    }
    if let Some(pos) = key.rfind('|') {
        let prefix = &key[..pos];
        let short = &key[pos + 1..];
        (prefix.replace('|', " "), short.into())
    } else {
        ("GUANO".into(), key.into())
    }
}

/// Classify a metadata field by its display label. Drives formatted-mode
/// transforms (humanized dates, °F conversion, coordinate parsing, JSON
/// expansion) and is robust to both GUANO and XC field name variants.
#[derive(Clone, Copy, PartialEq)]
enum FieldKind {
    Date,
    Temperature,
    GpsPosition,
    License,
    None,
}

fn classify(label: &str) -> FieldKind {
    let l = label.to_ascii_lowercase();
    if l == "date" || l == "recorded" || l == "timestamp" || l == "datetime" {
        FieldKind::Date
    } else if l.contains("temp") {
        FieldKind::Temperature
    } else if l == "gps position" || l == "coordinates" || l == "gps" || l == "loc position" {
        FieldKind::GpsPosition
    } else if l == "license" || l == "licence" || l == "lic" {
        FieldKind::License
    } else {
        FieldKind::None
    }
}

/// Parse a value into (lat, lon). Accepts "lat lon", "lat,lon",
/// "lat, lon", with optional trailing junk. Returns None if anything
/// doesn't look like a decimal coordinate pair.
fn parse_lat_lon(value: &str) -> Option<(f64, f64)> {
    let s = value.trim();
    // Try comma split first, then whitespace
    let parts: Vec<&str> = if s.contains(',') {
        s.split(',').collect()
    } else {
        s.split_whitespace().collect()
    };
    if parts.len() < 2 {
        return None;
    }
    let lat: f64 = parts[0].trim().parse().ok()?;
    let lon: f64 = parts[1].trim().parse().ok()?;
    if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
        return None;
    }
    Some((lat, lon))
}

/// Parse a temperature value (string) into degrees Celsius.
/// Accepts e.g. "25", "25.5", "25C", "25 °C", "25°C". If the value
/// already contains "F", we assume Fahrenheit was intended and skip
/// the conversion.
fn parse_temp_c(value: &str) -> Option<f64> {
    let v = value.trim();
    if v.is_empty() {
        return None;
    }
    let lower = v.to_ascii_lowercase();
    if lower.contains('f') && !lower.contains('c') {
        return None;
    }
    // Strip non-numeric trailing characters
    let num: String = v
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '-' || *c == '.' || *c == '+')
        .collect();
    num.parse().ok()
}

/// Parse an ISO-ish date/time string into a JS epoch ms. Falls back to
/// js_sys::Date::parse which handles ISO 8601, RFC 2822, and a number
/// of looser shapes (incl. "2024-03-15").
fn parse_date_ms(value: &str) -> Option<f64> {
    let v = value.trim();
    if v.is_empty() {
        return None;
    }
    let ms = js_sys::Date::parse(v);
    if ms.is_nan() {
        None
    } else {
        Some(ms)
    }
}

/// Extract the UTC offset (in minutes) explicitly written in the
/// original timestamp string. Recognizes trailing `Z`, `+HH:MM`,
/// `-HH:MM`, `+HHMM`, `-HHMM`. Returns None if no offset is present.
fn extract_tz_offset_minutes(value: &str) -> Option<i32> {
    let v = value.trim();
    if v.is_empty() {
        return None;
    }
    if v.ends_with('Z') || v.ends_with('z') {
        return Some(0);
    }
    // A timezone offset can only appear in the *time* portion. If there is no
    // time portion at all (a bare date like "2025-05-30"), there is no offset —
    // and scanning the whole string would mis-read the date's own "-" separators
    // (this is exactly how "2025-05-30" turned into the nonsensical "UTC-30:00":
    // the "30" after the last "-" was parsed as 30 hours). So bail out unless a
    // `T` or space separator marks where the time begins.
    let time_start = match v.find('T').or_else(|| v.find(' ')) {
        Some(i) => i + 1,
        None => return None,
    };
    let tail = &v[time_start..];
    let sign_idx = tail.rfind(|c: char| c == '+' || c == '-')?;
    let sign_char = tail.as_bytes()[sign_idx] as char;
    let off = &tail[sign_idx + 1..];
    let (h, m) = if let Some((h, m)) = off.split_once(':') {
        (h.parse::<i32>().ok()?, m.parse::<i32>().ok()?)
    } else if off.len() == 4 {
        (off[..2].parse::<i32>().ok()?, off[2..].parse::<i32>().ok()?)
    } else if off.len() == 2 {
        (off.parse::<i32>().ok()?, 0)
    } else {
        return None;
    };
    // Reject implausible offsets. Real UTC offsets span −12:00..+14:00 with
    // minutes in 0..59 (e.g. +05:45 Nepal, +12:45 Chatham), so anything outside
    // that is a parse artefact rather than a real zone.
    if !(0..=14).contains(&h) || !(0..60).contains(&m) {
        return None;
    }
    let total = h * 60 + m;
    Some(if sign_char == '-' { -total } else { total })
}

fn format_tz_offset(minutes: i32) -> String {
    if minutes == 0 {
        return "UTC".into();
    }
    let sign = if minutes < 0 { '-' } else { '+' };
    let m = minutes.unsigned_abs();
    format!("UTC{sign}{:02}:{:02}", m / 60, m % 60)
}

/// Render a JS Date in a long, locale-sensitive format.
/// `tz_minutes`: if Some, render the date in that fixed UTC offset (so
/// the displayed wall-clock time matches the original timestamp string);
/// if None, render in the user's local time zone.
fn format_date_long(ms: f64, had_time: bool, tz_minutes: Option<i32>) -> String {
    let opts = js_sys::Object::new();
    if had_time {
        let _ = js_sys::Reflect::set(&opts, &"dateStyle".into(), &"long".into());
        let _ = js_sys::Reflect::set(&opts, &"timeStyle".into(), &"medium".into());
    } else {
        let _ = js_sys::Reflect::set(&opts, &"dateStyle".into(), &"long".into());
    }
    let shifted_ms = match tz_minutes {
        Some(off) => {
            // Adjust the epoch so that the UTC reading at this offset
            // matches the wall-clock time in the original zone, then
            // force the formatter into UTC.
            let _ = js_sys::Reflect::set(&opts, &"timeZone".into(), &"UTC".into());
            ms + (off as f64) * 60_000.0
        }
        None => ms,
    };
    let d = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(shifted_ms));
    d.to_locale_string("default", &opts)
        .as_string()
        .unwrap_or_default()
}

/// "4 days ago" / "in 2 hours" style relative time.
fn humanize_relative(ms: f64) -> String {
    let now = js_sys::Date::now();
    let delta_secs = (now - ms) / 1000.0;
    let abs = delta_secs.abs();
    let future = delta_secs < 0.0;
    let (n, unit) = if abs < 60.0 {
        (abs as i64, "second")
    } else if abs < 3600.0 {
        ((abs / 60.0) as i64, "minute")
    } else if abs < 86_400.0 {
        ((abs / 3600.0) as i64, "hour")
    } else if abs < 30.0 * 86_400.0 {
        ((abs / 86_400.0) as i64, "day")
    } else if abs < 365.0 * 86_400.0 {
        ((abs / (30.0 * 86_400.0)) as i64, "month")
    } else {
        ((abs / (365.0 * 86_400.0)) as i64, "year")
    };
    let plural = if n == 1 { "" } else { "s" };
    if future {
        format!("in {n} {unit}{plural}")
    } else if n == 0 {
        "just now".into()
    } else {
        format!("{n} {unit}{plural} ago")
    }
}

/// Try to extract a balanced JSON array/object from a string that may
/// contain a prefix or trailing junk. Returns the parsed Value if the
/// extracted slice round-trips through serde_json.
fn extract_json(value: &str) -> Option<serde_json::Value> {
    let s = value.trim();
    let start = s.find(|c: char| c == '[' || c == '{')?;
    // Find the matching closing bracket using simple depth tracking
    // (good enough for the well-behaved JSON we see in GUANO).
    let bytes = s.as_bytes();
    let open = bytes[start] as char;
    let close = if open == '[' { ']' } else { '}' };
    let mut depth = 0i32;
    let mut end = None;
    let mut in_str = false;
    let mut esc = false;
    for (i, b) in bytes.iter().enumerate().skip(start) {
        let c = *b as char;
        if in_str {
            if esc {
                esc = false;
            } else if c == '\\' {
                esc = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            x if x == open => depth += 1,
            x if x == close => {
                depth -= 1;
                if depth == 0 {
                    end = Some(i + 1);
                    break;
                }
            }
            _ => {}
        }
    }
    let end = end?;
    serde_json::from_str(&s[start..end]).ok()
}

/// Render an extracted JSON value as a small nested "k: v" block.
fn json_block(json: &serde_json::Value) -> impl IntoView {
    fn line(key: String, val: String) -> impl IntoView {
        view! {
            <div class="metadata-json-line">
                <span class="metadata-json-key">{key}</span>
                <span class="metadata-json-sep">": "</span>
                <span class="metadata-json-val">{val}</span>
            </div>
        }
    }
    fn flatten_value(v: &serde_json::Value) -> String {
        match v {
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Number(n) => n.to_string(),
            serde_json::Value::Bool(b) => b.to_string(),
            serde_json::Value::Null => "null".into(),
            other => other.to_string(),
        }
    }
    let entries: Vec<_> = match json {
        serde_json::Value::Array(arr) => arr
            .iter()
            .flat_map(|v| match v {
                serde_json::Value::Object(map) => map
                    .iter()
                    .map(|(k, v)| line(k.clone(), flatten_value(v)).into_any())
                    .collect::<Vec<_>>(),
                other => vec![line(String::new(), flatten_value(other)).into_any()],
            })
            .collect(),
        serde_json::Value::Object(map) => map
            .iter()
            .map(|(k, v)| line(k.clone(), flatten_value(v)).into_any())
            .collect(),
        other => vec![line(String::new(), flatten_value(other)).into_any()],
    };
    view! { <div class="metadata-json-block">{entries}</div> }
}

/// Parse a temperature value and return (°C string, °F string).
/// Both formatted to 0 dp when near-integer, otherwise 1 dp.
fn format_temp_c_f(value: &str) -> Option<(String, String)> {
    let c = parse_temp_c(value)?;
    let f = c * 9.0 / 5.0 + 32.0;
    let fmt = |n: f64| {
        if (n - n.round()).abs() < 0.05 {
            format!("{n:.0}")
        } else {
            format!("{n:.1}")
        }
    };
    Some((
        format!("{}\u{00B0}C", fmt(c)),
        format!("{}\u{00B0}F", fmt(f)),
    ))
}

/// Inline (single-line) metadata row — used by the File section.
fn inline_row(label: String, value: String, label_title: Option<String>) -> impl IntoView {
    let value_for_copy = value.clone();
    let value_for_title = value.clone();
    let on_copy = move |ev: web_sys::MouseEvent| {
        ev.stop_propagation();
        super::copy_to_clipboard(&value_for_copy);
    };
    view! {
        <div class="setting-row metadata-row metadata-row-inline">
            <span class="setting-label" title=label_title.unwrap_or_default()>{label}</span>
            <span class="setting-value metadata-value" title=value_for_title>{value}</span>
            <button class="copy-btn" on:click=on_copy title="Copy">{"\u{2398}"}</button>
        </div>
    }
}

/// Spacious row (key on its own line, value beneath). Used for all
/// sections other than the File summary. Honors the Formatted/Original
/// view mode for value rendering.
fn spacious_row(
    label: String,
    value: String,
    label_title: Option<String>,
    view_mode: MetadataView,
) -> impl IntoView {
    let kind = classify(&label);
    let value_for_copy = value.clone();
    let on_copy = move |ev: web_sys::MouseEvent| {
        ev.stop_propagation();
        super::copy_to_clipboard(&value_for_copy);
    };

    let body = render_value_body(&label, &value, kind, view_mode);

    view! {
        <div class="metadata-row metadata-row-spacious">
            <div class="metadata-key-line">
                <span class="metadata-key" title=label_title.unwrap_or_default()>{label}</span>
                <button class="copy-btn copy-btn-spacious" on:click=on_copy title="Copy">{"\u{2398}"}</button>
            </div>
            {body}
        </div>
    }
}

/// Render the value portion of a spacious row according to view mode
/// and detected field kind.
fn render_value_body(
    label: &str,
    value: &str,
    kind: FieldKind,
    view_mode: MetadataView,
) -> leptos::tachys::view::any_view::AnyView {
    let raw = || {
        view! {
            <div class="metadata-value-block" title=value.to_string()>{value.to_string()}</div>
        }
        .into_any()
    };

    if view_mode == MetadataView::Original {
        return raw();
    }

    match kind {
        FieldKind::Date => date_block(value).into_any(),
        FieldKind::Temperature => {
            if let Some((c, f)) = format_temp_c_f(value) {
                view! {
                    <div class="metadata-value-block">
                        {c}
                        " "
                        <span class="metadata-temp-f">"("{f}")"</span>
                    </div>
                }
                .into_any()
            } else {
                raw()
            }
        }
        FieldKind::GpsPosition => {
            if let Some((lat, lon)) = parse_lat_lon(value) {
                gps_block(lat, lon, value.to_string()).into_any()
            } else {
                raw()
            }
        }
        FieldKind::License => {
            if let Some(short) = super::file_badges::parse_cc_license(value) {
                view! {
                    <div class="metadata-value-block">{short}</div>
                    <div class="metadata-value-subtle" title=value.to_string()>{value.to_string()}</div>
                }.into_any()
            } else {
                raw()
            }
        }
        FieldKind::None => {
            // Try JSON expansion (e.g. Wildlife Acoustics "Audio settings")
            if value.trim_start().starts_with('[') || value.trim_start().starts_with('{') {
                if let Some(j) = extract_json(value) {
                    return view! {
                        <div class="metadata-value-block metadata-value-json">
                            {json_block(&j)}
                        </div>
                    }
                    .into_any();
                }
            }
            let _ = label;
            raw()
        }
    }
}

/// Render a parsed date with the original UTC offset, a localized
/// "Local: ..." line if the local interpretation differs, and a
/// humanized relative-time hint.
fn date_block(value: &str) -> impl IntoView {
    let Some(ms) = parse_date_ms(value) else {
        return view! {
            <div class="metadata-value-block" title=value.to_string()>{value.to_string()}</div>
        }
        .into_any();
    };
    let had_time = value.contains('T') || value.contains(':');
    let orig_tz = extract_tz_offset_minutes(value);
    let rel = humanize_relative(ms);

    let primary_str = match orig_tz {
        Some(off) => format!(
            "{} {}",
            format_date_long(ms, had_time, Some(off)),
            format_tz_offset(off)
        ),
        // A bare date with no explicit zone (e.g. "2025-05-30") is parsed as UTC
        // midnight; render it in UTC too so it doesn't slip to the previous day
        // for viewers west of Greenwich.
        None if !had_time => format_date_long(ms, had_time, Some(0)),
        None => format_date_long(ms, had_time, None),
    };

    // A distinct "Local:" interpretation exists only when the original timestamp
    // pinned a specific tz AND that tz differs from the viewer's local tz.
    let local_line = orig_tz.and_then(|off| {
        let local_offset =
            -(js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(ms)).get_timezone_offset() as i32);
        if local_offset == off {
            return None;
        }
        Some(format!(
            "Local: {} {}",
            format_date_long(ms, had_time, None),
            format_tz_offset(local_offset),
        ))
    });

    let Some(local_str) = local_line else {
        return view! {
            <div class="metadata-value-block">{primary_str}</div>
            <div class="metadata-value-relative">{rel}</div>
        }
        .into_any();
    };

    // The local line is shown by default only for recent recordings (within the
    // last week); older ones start hidden. Either way the primary date is a
    // toggle — click it to show/hide the local interpretation.
    let within_week = {
        let age = js_sys::Date::now() - ms; // ms; positive = in the past
        age.abs() <= 7.0 * 86_400_000.0
    };
    let show_local = RwSignal::new(within_week);

    view! {
        <div
            class="metadata-value-block metadata-date-toggle"
            title="Click to toggle local time"
            on:click=move |_| show_local.update(|v| *v = !*v)
        >{primary_str}</div>
        {move || show_local.get().then({
            let local_str = local_str.clone();
            move || view! { <div class="metadata-value-local">{local_str}</div> }
        })}
        <div class="metadata-value-relative">{rel}</div>
    }
    .into_any()
}

/// Parse an 8- or 14-digit `YYYYMMDD[HHMMSS]` run into an ISO string, with
/// sanity bounds on each field. Returns None if it's not a plausible date.
fn parse_ymd_run(run: &str) -> Option<String> {
    if run.len() != 8 && run.len() != 14 {
        return None;
    }
    let y: i32 = run[0..4].parse().ok()?;
    let m: u32 = run[4..6].parse().ok()?;
    let d: u32 = run[6..8].parse().ok()?;
    if !(1990..=2100).contains(&y) || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    if run.len() == 8 {
        return Some(format!("{y:04}-{m:02}-{d:02}"));
    }
    let hh: u32 = run[8..10].parse().ok()?;
    let mm: u32 = run[10..12].parse().ok()?;
    let ss: u32 = run[12..14].parse().ok()?;
    if hh > 23 || mm > 59 || ss > 59 {
        return None;
    }
    Some(format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}"))
}

/// Extract an ISO-ish datestamp embedded in a filename. Recognizes the common
/// recorder patterns `YYYYMMDD`, `YYYYMMDDHHMMSS`, and `YYYYMMDD[_-T ]HHMMSS`
/// (e.g. "PREFIX_20210630_223015.wav", "20240115.WAV"). Field bounds reject
/// most non-date digit runs. Returns the first plausible match.
fn date_from_filename(name: &str) -> Option<String> {
    let chars: Vec<char> = name.chars().collect();
    let n = chars.len();
    let is_d = |c: char| c.is_ascii_digit();
    let mut i = 0;
    while i < n {
        if !is_d(chars[i]) {
            i += 1;
            continue;
        }
        let start = i;
        while i < n && is_d(chars[i]) {
            i += 1;
        }
        let run_len = i - start;
        if run_len == 8 || run_len == 14 {
            let run: String = chars[start..i].iter().collect();
            if let Some(iso) = parse_ymd_run(&run) {
                // A bare YYYYMMDD may be followed by a separator + HHMMSS.
                if run_len == 8 && i < n && matches!(chars[i], '_' | '-' | 'T' | ' ') {
                    let t_start = i + 1;
                    let mut t_end = t_start;
                    while t_end < n && is_d(chars[t_end]) {
                        t_end += 1;
                    }
                    if t_end - t_start == 6 {
                        let t: String = chars[t_start..t_end].iter().collect();
                        let hh: u32 = t[0..2].parse().ok()?;
                        let mm: u32 = t[2..4].parse().ok()?;
                        let ss: u32 = t[4..6].parse().ok()?;
                        if hh <= 23 && mm <= 59 && ss <= 59 {
                            return Some(format!("{iso}T{hh:02}:{mm:02}:{ss:02}"));
                        }
                    }
                }
                return Some(iso);
            }
        }
    }
    None
}

/// A spacious row whose value is always rendered as a date (so the source label
/// can be descriptive, e.g. "GUANO timestamp", without relying on `classify`).
fn date_row(label: String, value: String, view_mode: MetadataView) -> impl IntoView {
    let value_for_copy = value.clone();
    let on_copy = move |ev: web_sys::MouseEvent| {
        ev.stop_propagation();
        super::copy_to_clipboard(&value_for_copy);
    };
    let body = if view_mode == MetadataView::Original {
        view! { <div class="metadata-value-block" title=value.clone()>{value.clone()}</div> }
            .into_any()
    } else {
        date_block(&value).into_any()
    };
    view! {
        <div class="metadata-row metadata-row-spacious">
            <div class="metadata-key-line">
                <span class="metadata-key">{label}</span>
                <button class="copy-btn copy-btn-spacious" on:click=on_copy title="Copy">{"\u{2398}"}</button>
            </div>
            {body}
        </div>
    }
}

/// "Date" section: every date we can pull from the file's metadata, in rough
/// order of trust. Filesystem (created/modified) timestamps and the XC download
/// date need platform plumbing and are a follow-up.
fn date_section(f: &crate::state::LoadedFile, view_mode: MetadataView) -> impl IntoView {
    let mut rows: Vec<(String, String)> = Vec::new();

    // 1. Datestamp embedded in the filename.
    if let Some(d) = date_from_filename(&f.name) {
        rows.push(("Filename".to_string(), d));
    }

    // 2. GUANO timestamp + any other date/time fields.
    if let Some(guano) = &f.audio.metadata.guano {
        for (k, v) in &guano.fields {
            if v.trim().is_empty() {
                continue;
            }
            let kl = k.to_ascii_lowercase();
            if kl == "timestamp" {
                rows.push(("GUANO timestamp".to_string(), v.clone()));
            } else if kl.contains("date") {
                rows.push((format!("GUANO {k}"), v.clone()));
            }
        }
    }

    // 3. Recording date (+ time) and upload date from the external source the
    // file came from — Xeno-Canto, Wikimedia Commons, …
    if let Some(xc) = &f.xc_metadata {
        let get = |key: &str| {
            xc.iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(key))
                .map(|(_, v)| v.clone())
                .filter(|v| !v.trim().is_empty())
        };
        let date_label = format!("{} date", f.source_label.as_deref().unwrap_or("Xeno-canto"));
        match (get("Date"), get("Time")) {
            (Some(d), Some(t)) => rows.push((date_label, format!("{d}T{t}"))),
            (Some(d), None) => rows.push((date_label, d)),
            _ => {}
        }
        if let Some(u) = get("Uploaded").or_else(|| get("Upload date")) {
            rows.push(("Uploaded".to_string(), u));
        }
    }

    let any = !rows.is_empty();
    let items: Vec<_> = rows
        .into_iter()
        .map(|(label, value)| date_row(label, value, view_mode).into_any())
        .collect();
    if any {
        view! {
            <div class="setting-group">
                <div class="setting-group-title setting-group-title-major">"Date"</div>
                {items}
            </div>
        }
        .into_any()
    } else {
        view! { <span></span> }.into_any()
    }
}

/// World-map block with a pin and out-links. Coordinates are in WGS84
/// decimal degrees. The map asset lives at `world-map.png` (declared
/// as a Trunk `copy-file` in index.html, relative path so it works
/// under both the root domain and the /oversample/ subpath build) and
/// is expected to be an equirectangular projection (lon −180→+180 maps
/// linearly across width, lat +90→−90 across height).
fn gps_block(lat: f64, lon: f64, raw_value: String) -> impl IntoView {
    let pin_left_pct = (lon + 180.0) / 360.0 * 100.0;
    let pin_top_pct = (90.0 - lat) / 180.0 * 100.0;
    let osm = format!("https://www.openstreetmap.org/?mlat={lat}&mlon={lon}#map=10/{lat}/{lon}");
    let gmaps = format!("https://www.google.com/maps?q={lat},{lon}");
    let coord_text = format!("{lat:.5}, {lon:.5}");
    view! {
        <div class="metadata-value-block">{coord_text}</div>
        <div class="metadata-map-wrap">
            <img class="metadata-map-img" src="world-map.png" alt="World map" />
            <div class="metadata-map-pin"
                 style=format!("left: {pin_left_pct:.3}%; top: {pin_top_pct:.3}%;")
                 title=raw_value.clone()></div>
        </div>
        <div class="metadata-map-links">
            <a href=osm target="_blank" rel="noopener noreferrer">"OpenStreetMap"</a>
            <span class="metadata-map-link-sep">"\u{00B7}"</span>
            <a href=gmaps target="_blank" rel="noopener noreferrer">"Google Maps"</a>
        </div>
    }
}

/// Hash row with match indicator. Spacious layout.
fn hash_row(
    label: &str,
    hash: &str,
    reference: Option<&str>,
    from_reference: bool,
) -> impl IntoView {
    let hash_for_copy = hash.to_string();
    let label = label.to_string();
    let on_copy = move |ev: web_sys::MouseEvent| {
        ev.stop_propagation();
        super::copy_to_clipboard(&hash_for_copy);
    };
    let (indicator, indicator_class) = match reference {
        Some(expected) if expected == hash => ("\u{2713}", "hash-indicator-inline match"),
        Some(_) => ("\u{2717}", "hash-indicator-inline mismatch"),
        None => ("", "hash-indicator-inline"),
    };
    let value_class = if from_reference {
        "metadata-value-block metadata-hash hash-from-ref"
    } else {
        "metadata-value-block metadata-hash"
    };
    let hash_display = hash.to_string();
    view! {
        <div class="metadata-row metadata-row-spacious">
            <div class="metadata-key-line">
                <span class="metadata-key">{label}</span>
                <span class=indicator_class>{indicator}</span>
                <button class="copy-btn copy-btn-spacious" on:click=on_copy title="Copy">{"\u{2398}"}</button>
            </div>
            <div class=value_class>{hash_display}</div>
        </div>
    }
}

fn zc_header_section(f: &crate::state::LoadedFile, view_mode: MetadataView) -> impl IntoView {
    let Some(zc) = f.audio.metadata.zc_data.as_ref() else {
        return view! { <span></span> }.into_any();
    };
    let md = &zc.metadata;

    let mut rows: Vec<(String, String)> = Vec::new();
    if !md.location.is_empty() {
        rows.push(("Location".into(), md.location.clone()));
    }
    if !md.species.is_empty() {
        rows.push(("Species".into(), md.species.clone()));
    }
    if !md.tape.is_empty() {
        rows.push(("Tape".into(), md.tape.clone()));
    }
    if !md.date.is_empty() {
        rows.push(("Date".into(), md.date.clone()));
    }
    if !md.spec.is_empty() {
        rows.push(("Spec".into(), md.spec.clone()));
    }
    if !md.note1.is_empty() {
        rows.push(("Note 1".into(), md.note1.clone()));
    }
    if !md.note2.is_empty() {
        rows.push(("Note 2".into(), md.note2.clone()));
    }
    if !md.id_code.is_empty() {
        rows.push(("ID".into(), md.id_code.clone()));
    }
    if !md.gps.is_empty() {
        rows.push(("GPS".into(), md.gps.clone()));
    }
    if let Some(ts) = md.timestamp {
        rows.push((
            "Recorded".into(),
            format!(
                "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:06}",
                ts.year, ts.month, ts.day, ts.hour, ts.minute, ts.second, ts.microseconds_total
            ),
        ));
    }

    if rows.is_empty() {
        return view! { <span></span> }.into_any();
    }

    let items: Vec<_> = rows
        .into_iter()
        .map(|(k, v)| spacious_row(k, v, None, view_mode).into_any())
        .collect();

    view! {
        <div class="setting-group">
            <div class="setting-group-title setting-group-title-major"
                 title="Fixed-header text fields embedded in the Anabat .zc binary.">
                "Header metadata"
            </div>
            {items}
        </div>
    }
    .into_any()
}

fn format_file_size(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

fn file_identity_section(f: &crate::state::LoadedFile) -> impl IntoView {
    let state = expect_context::<AppState>();
    let identity = f.identity.clone();
    let has_file_handle = f.file_handle.is_some();
    let verify_outcome = f.verify_outcome.clone();
    let all_verified = f.all_hashes_verified;

    // Key off this section's own file id (not the current-file index) so the
    // sidecar hashes always match the file being rendered.
    let sidecar_identity = state
        .annotations
        .store()
        .with_untracked(|store| store.get(f.id).map(|set| set.file_identity.clone()));
    let xc = &f.xc_hashes;
    let ref_blake3 = xc.as_ref().and_then(|h| h.blake3.clone()).or_else(|| {
        sidecar_identity
            .as_ref()
            .and_then(|s| s.full_blake3.clone())
    });
    let ref_sha256 = xc.as_ref().and_then(|h| h.sha256.clone()).or_else(|| {
        sidecar_identity
            .as_ref()
            .and_then(|s| s.full_sha256.clone())
    });
    let ref_spot = xc
        .as_ref()
        .and_then(|h| h.spot_hash_b3.clone())
        .or_else(|| {
            sidecar_identity
                .as_ref()
                .and_then(|s| s.spot_hash_b3.clone())
        });
    let ref_content = xc
        .as_ref()
        .and_then(|h| h.content_hash.clone())
        .or_else(|| {
            sidecar_identity
                .as_ref()
                .and_then(|s| s.content_hash.clone())
        });
    let ref_file_size = xc
        .as_ref()
        .and_then(|h| h.file_size)
        .or_else(|| sidecar_identity.as_ref().map(|s| s.file_size));

    let mut items: Vec<leptos::tachys::view::any_view::AnyView> = Vec::new();

    let actual_size = identity.as_ref().map(|id| id.file_size);
    let display_size = actual_size.or(ref_file_size);
    let is_small = display_size
        .map(|s| s < crate::file_identity::SMALL_FILE_THRESHOLD)
        .unwrap_or(true);

    if let Some(size) = display_size {
        if size > 0 {
            let size_ref = match (actual_size, ref_file_size) {
                (Some(actual), Some(expected)) => Some(if actual == expected {
                    actual.to_string()
                } else {
                    format!("{expected}")
                }),
                _ => None,
            };
            items.push(
                hash_row(
                    "File size (bytes)",
                    &size.to_string(),
                    size_ref.as_deref(),
                    false,
                )
                .into_any(),
            );
        }
    }

    if let Some(ref id) = identity {
        if let Some(ref hash) = id.spot_hash_b3 {
            let reference = if !is_small || all_verified {
                ref_spot.as_deref()
            } else {
                None
            };
            items.push(hash_row("Spot hash", hash, reference, false).into_any());
        } else {
            items.push(
                spacious_row(
                    "Spot hash".into(),
                    "computing...".into(),
                    None,
                    MetadataView::Original,
                )
                .into_any(),
            );
        }

        if let Some(ref hash) = id.full_blake3 {
            let reference = if is_small || all_verified {
                ref_blake3.as_deref()
            } else {
                None
            };
            items.push(hash_row("Full BLAKE3", hash, reference, false).into_any());
        } else if let Some(ref known) = ref_blake3 {
            items.push(hash_row("Full BLAKE3", known, None, true).into_any());
        }

        if let Some(ref hash) = id.content_hash {
            let reference =
                if verify_outcome == crate::state::VerifyOutcome::ContentMatch || all_verified {
                    ref_content.as_deref()
                } else {
                    None
                };
            items.push(hash_row("Content hash", hash, reference, false).into_any());
        } else if let Some(ref known) = ref_content {
            items.push(hash_row("Content hash", known, None, true).into_any());
        }

        if let Some(ref hash) = id.full_sha256 {
            let reference = if all_verified {
                ref_sha256.as_deref()
            } else {
                None
            };
            items.push(hash_row("Full SHA-256", hash, reference, false).into_any());
        } else if let Some(ref known) = ref_sha256 {
            items.push(hash_row("Full SHA-256", known, None, true).into_any());
        }
    } else {
        if let Some(ref known) = ref_spot {
            items.push(hash_row("Spot hash", known, None, true).into_any());
        }
        if let Some(ref known) = ref_blake3 {
            items.push(hash_row("Full BLAKE3", known, None, true).into_any());
        }
        if let Some(ref known) = ref_content {
            items.push(hash_row("Content hash", known, None, true).into_any());
        }
        if let Some(ref known) = ref_sha256 {
            items.push(hash_row("Full SHA-256", known, None, true).into_any());
        }
    }

    if verify_outcome == crate::state::VerifyOutcome::ContentMatch {
        items.push(
            view! {
                <div class="metadata-row metadata-row-spacious">
                    <div class="hash-note">"Header changed \u{2014} audio content verified"</div>
                </div>
            }
            .into_any(),
        );
    }

    if has_file_handle && !all_verified {
        let computing = state.status.hash_computing().get();
        let on_calc_all = move |_: web_sys::MouseEvent| {
            if let Some(idx) = state.library.current_index().get_untracked() {
                crate::file_identity::start_full_hash_computation(state, idx, true);
            }
        };
        let label = if computing {
            "Computing..."
        } else {
            "Calculate all hashes"
        };
        items.push(view! {
            <div class="metadata-row metadata-row-spacious">
                <button class="hash-calc-btn" on:click=on_calc_all disabled=computing>{label}</button>
            </div>
        }.into_any());
    }

    if items.is_empty() {
        view! { <span></span> }.into_any()
    } else {
        view! {
            <div class="setting-group">
                <div class="setting-group-title setting-group-title-major">"File Identity"</div>
                {items}
            </div>
        }
        .into_any()
    }
}

#[component]
pub(crate) fn MetadataPanel() -> impl IntoView {
    let state = expect_context::<AppState>();
    let view_mode = state.panels.metadata_view();

    view! {
        <div class="sidebar-panel">
            <div class="metadata-view-toggle">
                <button
                    class=move || if view_mode.get() == MetadataView::Formatted {
                        "psd-btn psd-btn-active"
                    } else {
                        "psd-btn"
                    }
                    on:click=move |_| view_mode.set(MetadataView::Formatted)
                    title="Pretty-print JSON, localize dates, show \u{00B0}F"
                >
                    "Formatted"
                </button>
                <button
                    class=move || if view_mode.get() == MetadataView::Original {
                        "psd-btn psd-btn-active"
                    } else {
                        "psd-btn"
                    }
                    on:click=move |_| view_mode.set(MetadataView::Original)
                    title="Show raw values as stored in the file"
                >
                    "Original"
                </button>
            </div>
            {move || {
                let files = state.library.files().get();
                let idx = state.library.current_index().get();
                let file = idx.and_then(|i| files.get(i));
                let view_mode = view_mode.get();

                match file {
                    None => view! {
                        <div class="sidebar-panel-empty">"No file selected"</div>
                    }.into_any(),
                    Some(f) => {
                        let meta = &f.audio.metadata;
                        let (size_str, size_label) = if meta.file_size > 0 {
                            (format_file_size(meta.file_size), "File size".to_string())
                        } else if f.audio.duration_secs > 0.0 {
                            let bytes_per_sample = (meta.bits_per_sample as usize).max(16) / 8;
                            let num_samples = (f.audio.duration_secs * f.audio.sample_rate as f64).ceil() as usize;
                            let estimated = 44 + num_samples * f.audio.channels as usize * bytes_per_sample;
                            (format!("~{}", format_file_size(estimated)), "File size (est.)".to_string())
                        } else {
                            ("0 B".to_string(), "File size".to_string())
                        };
                        let xc_fields: Vec<_> = f.xc_metadata.clone().unwrap_or_default();
                        let source_label = f.source_label.clone()
                            .unwrap_or_else(|| "Xeno-canto".to_string());
                        let has_xc = !xc_fields.is_empty();
                        let guano_fields: Vec<_> = meta.guano.as_ref()
                            .map(|g| g.fields.clone())
                            .unwrap_or_default();
                        let has_guano = !guano_fields.is_empty();

                        view! {
                            <div class="setting-group">
                                <div class="setting-group-title setting-group-title-major">"File"</div>
                                {inline_row("Name".into(), f.name.clone(), None)}
                                {inline_row("Format".into(), meta.format.to_string(), None)}
                                {inline_row("Duration".into(), crate::format_time::format_duration(f.audio.duration_secs, 3), None)}
                                {inline_row("Sample rate".into(), format!("{} kHz", f.audio.sample_rate / 1000), None)}
                                {inline_row("Channels".into(), f.audio.channels.to_string(), None)}
                                {inline_row("Bit depth".into(), format!("{}-bit", meta.bits_per_sample), None)}
                                {inline_row(size_label, size_str, None)}
                            </div>
                            {date_section(f, view_mode)}
                            {zc_header_section(f, view_mode)}
                            {if has_xc {
                                let items: Vec<_> = xc_fields.into_iter().map(|(label, value)| {
                                    spacious_row(label, value, None, view_mode).into_any()
                                }).collect();
                                view! {
                                    <div class="setting-group">
                                        <div class="setting-group-title setting-group-title-major">{source_label}</div>
                                        {items}
                                    </div>
                                }.into_any()
                            } else {
                                view! { <span></span> }.into_any()
                            }}
                            {if has_guano {
                                let is_guano_source = matches!(meta.format, "WAV" | "W4V");
                                let default_section: &str = if is_guano_source {
                                    "Guano metadata"
                                } else {
                                    "Audio file metadata"
                                };
                                let mut items: Vec<leptos::tachys::view::any_view::AnyView> = Vec::new();
                                let mut current_section: Option<String> = None;
                                let mut first_heading = true;
                                for (k, v) in guano_fields {
                                    let (section, display_key) = if is_guano_source {
                                        let (s, d) = categorize_guano_key(&k);
                                        let s = if s == "GUANO" { default_section.to_string() } else { s };
                                        (s, d)
                                    } else {
                                        (default_section.to_string(), k.clone())
                                    };
                                    if current_section.as_ref() != Some(&section) {
                                        let heading = section.clone();
                                        let show_badge = is_guano_source && heading != default_section;
                                        let heading_class = if first_heading {
                                            "setting-group-title setting-group-title-major"
                                        } else {
                                            "setting-group-title setting-group-title-sub"
                                        };
                                        items.push(view! {
                                            <div class=heading_class>
                                                {heading}
                                                {if show_badge {
                                                    view! { <span class="metadata-source-badge">"GUANO"</span> }.into_any()
                                                } else {
                                                    view! { <span></span> }.into_any()
                                                }}
                                            </div>
                                        }.into_any());
                                        current_section = Some(section);
                                        first_heading = false;
                                    }
                                    items.push(spacious_row(display_key, v, Some(k), view_mode).into_any());
                                }
                                view! {
                                    <div class="setting-group">
                                        {items}
                                    </div>
                                }.into_any()
                            } else {
                                view! { <span></span> }.into_any()
                            }}
                            {if !f.is_recording {
                                file_identity_section(f).into_any()
                            } else {
                                view! { <span></span> }.into_any()
                            }}
                        }.into_any()
                    }
                }
            }}
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::{date_from_filename, extract_tz_offset_minutes, format_tz_offset};

    #[test]
    fn filename_date_patterns() {
        assert_eq!(
            date_from_filename("20240115.WAV").as_deref(),
            Some("2024-01-15")
        );
        assert_eq!(
            date_from_filename("PREFIX_20210630_223015.wav").as_deref(),
            Some("2021-06-30T22:30:15"),
        );
        assert_eq!(
            date_from_filename("20210630223015.wav").as_deref(),
            Some("2021-06-30T22:30:15"),
        );
        assert_eq!(
            date_from_filename("AUDIOMOTH-20231101T080000.wav").as_deref(),
            Some("2023-11-01T08:00:00"),
        );
    }

    #[test]
    fn filename_date_rejects_non_dates() {
        // Too-large month/day, out-of-range year, random digit runs.
        assert_eq!(date_from_filename("track123456789.wav"), None);
        assert_eq!(date_from_filename("99999999.wav"), None);
        assert_eq!(date_from_filename("18000101.wav"), None); // year < 1990
        assert_eq!(date_from_filename("XC1008337.wav"), None); // XC id, not a date
        assert_eq!(date_from_filename("recording.wav"), None);
    }

    #[test]
    fn tz_offset_bare_date_is_none() {
        // The XC1008337 regression: a bare date must NOT yield a timezone
        // offset (the "-30" in "2025-05-30" was being read as UTC-30:00).
        assert_eq!(extract_tz_offset_minutes("2025-05-30"), None);
        assert_eq!(extract_tz_offset_minutes("1999-12-31"), None);
        assert_eq!(extract_tz_offset_minutes("2025-01-05"), None);
    }

    #[test]
    fn tz_offset_zulu() {
        assert_eq!(extract_tz_offset_minutes("2025-05-30T10:30:45Z"), Some(0));
        assert_eq!(extract_tz_offset_minutes("2025-05-30T10:30:45z"), Some(0));
    }

    #[test]
    fn tz_offset_colon_forms() {
        assert_eq!(
            extract_tz_offset_minutes("2025-05-30T10:30:45+05:00"),
            Some(300)
        );
        assert_eq!(
            extract_tz_offset_minutes("2025-05-30T10:30:45-05:00"),
            Some(-300)
        );
        assert_eq!(
            extract_tz_offset_minutes("2025-05-30T10:30:45+05:30"),
            Some(330)
        );
        assert_eq!(
            extract_tz_offset_minutes("2025-05-30T10:30:45-09:30"),
            Some(-570)
        );
        // Space separator instead of 'T'.
        assert_eq!(
            extract_tz_offset_minutes("2025-05-30 10:30:45-05:00"),
            Some(-300)
        );
        // Odd-but-real three-quarter-hour zone (Chatham Islands, +12:45).
        assert_eq!(
            extract_tz_offset_minutes("2025-05-30T10:30:45+12:45"),
            Some(765)
        );
    }

    #[test]
    fn tz_offset_compact_forms() {
        assert_eq!(
            extract_tz_offset_minutes("2025-05-30T10:30:45+0530"),
            Some(330)
        );
        assert_eq!(
            extract_tz_offset_minutes("2025-05-30T10:30:45-0530"),
            Some(-330)
        );
        assert_eq!(
            extract_tz_offset_minutes("2025-05-30T10:30:45+05"),
            Some(300)
        );
        assert_eq!(
            extract_tz_offset_minutes("2025-05-30T10:30:45-08"),
            Some(-480)
        );
    }

    #[test]
    fn tz_offset_no_offset_present() {
        assert_eq!(extract_tz_offset_minutes("2025-05-30T10:30:45"), None);
        assert_eq!(extract_tz_offset_minutes("2025-05-30T10:30"), None);
    }

    #[test]
    fn tz_offset_invalid_or_empty() {
        assert_eq!(extract_tz_offset_minutes(""), None);
        assert_eq!(extract_tz_offset_minutes("   "), None);
        assert_eq!(extract_tz_offset_minutes("not a date"), None);
        // Implausible offsets are rejected rather than displayed.
        assert_eq!(extract_tz_offset_minutes("2025-05-30T10:30:45+30:00"), None);
        assert_eq!(extract_tz_offset_minutes("2025-05-30T10:30:45-99:99"), None);
        assert_eq!(extract_tz_offset_minutes("2025-05-30T10:30:45+05:99"), None);
    }

    #[test]
    fn format_offset_roundtrips() {
        assert_eq!(format_tz_offset(0), "UTC");
        assert_eq!(format_tz_offset(300), "UTC+05:00");
        assert_eq!(format_tz_offset(-300), "UTC-05:00");
        assert_eq!(format_tz_offset(330), "UTC+05:30");
        assert_eq!(format_tz_offset(-570), "UTC-09:30");
        assert_eq!(format_tz_offset(765), "UTC+12:45");
    }
}
