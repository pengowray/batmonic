use leptos::prelude::*;
use super::file_groups::{TrackInfo, SequenceInfo};
use crate::format_time::format_duration_compact;

/// Parse a CC license URL/string (from XC metadata "lic" field) into a short label.
/// e.g. "//creativecommons.org/licenses/by-nc-sa/4.0/" -> "CC BY-NC-SA 4.0"
pub(crate) fn parse_cc_license(lic: &str) -> Option<String> {
    let lower = lic.to_lowercase();
    if lower.contains("creativecommons.org/licenses/") {
        if let Some(idx) = lower.find("/licenses/") {
            let rest = &lic[idx + 10..];
            let parts: Vec<&str> = rest.trim_matches('/').split('/').collect();
            if parts.len() >= 2 {
                let license_type = parts[0].to_uppercase();
                let version = parts[1];
                return Some(format!("CC {} {}", license_type, version));
            } else if !parts.is_empty() {
                return Some(format!("CC {}", parts[0].to_uppercase()));
            }
        }
    }
    if lower.starts_with("cc") {
        return Some(lic.to_string());
    }
    None
}

/// Get XC metadata field value by key.
pub(crate) fn get_xc_field(metadata: &[(String, String)], key: &str) -> Option<String> {
    metadata.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
}

/// All data needed to render badges for a single file.
#[derive(Clone, PartialEq)]
pub struct FileBadgeData {
    pub sample_rate: u32,
    pub bits_per_sample: u16,
    pub is_float: bool,
    pub duration_secs: f64,
    pub is_unsaved: bool,
    pub is_streaming: bool,
    pub track: Option<TrackInfo>,
    pub sequence: Option<SequenceInfo>,
    pub cc_license: Option<String>,
    pub cc_tooltip: Option<String>,
    pub file_index: usize,
}

/// Format sample rate for display: 44100 → "44.1kHz", 192000 → "192kHz", 48000 → "48kHz"
fn format_sample_rate(sr: u32) -> String {
    let khz = sr as f64 / 1000.0;
    if sr % 1000 == 0 {
        format!("{}kHz", sr / 1000)
    } else {
        // e.g. 44100 → "44.1kHz"
        let formatted = format!("{:.1}kHz", khz);
        // Strip trailing ".0" for clean display
        if formatted.ends_with(".0kHz") {
            format!("{}kHz", sr / 1000)
        } else {
            formatted
        }
    }
}

/// Shared badge row component used by both file menu items and toolbar heading.
///
/// Renders: [sample_rate] [bit_depth] duration [~] #seq [track] [💾] [CC]
#[component]
pub fn FileBadgeRow(
    data: FileBadgeData,
    /// "toolbar" or "file-menu"
    #[prop(default = "file-menu")]
    context: &'static str,
    /// Controls seq/track badge visibility
    #[prop(into)]
    show_group_badges: Signal<bool>,
    /// Whether seq/track badges are clickable dropdown triggers (toolbar only)
    #[prop(default = false)]
    group_dropdowns: bool,
    /// Whether to show the download button (file-menu recordings only)
    #[prop(default = false)]
    show_download: bool,
    /// Download handler (required if show_download is true)
    #[prop(optional)]
    on_download: Option<Callback<()>>,
    /// Sequence badge click handler (toolbar dropdown)
    #[prop(optional)]
    on_seq_click: Option<Callback<()>>,
    /// Track badge click handler (toolbar dropdown)
    #[prop(optional)]
    on_track_click: Option<Callback<()>>,
) -> impl IntoView {
    let sr_label = format_sample_rate(data.sample_rate);

    // Bit depth badge: only for > 16 bit
    let bit_badge = if data.bits_per_sample > 16 {
        if data.is_float && data.bits_per_sample == 32 {
            Some("32 bit float".to_string())
        } else {
            Some(format!("{} bit", data.bits_per_sample))
        }
    } else {
        None
    };

    let dur_label = format_duration_compact(data.duration_secs);
    let is_streaming = data.is_streaming;
    let seq = data.sequence.clone();
    let track = data.track.clone();
    let is_unsaved = data.is_unsaved;
    let is_file_menu = context == "file-menu";
    let cc_license = data.cc_license.clone();
    let cc_tooltip = data.cc_tooltip.clone();

    view! {
        // 1. Sample rate badge (always)
        <span class="file-badge badge-sample-rate">{sr_label}</span>

        // 2. Bit depth badge (only > 16)
        {bit_badge.map(|label| view! {
            <span class="file-badge badge-bit-depth">{label}</span>
        })}

        // 3. Duration (plain text)
        <span class="badge-duration">{dur_label}</span>

        // 4. Streaming badge
        {is_streaming.then(|| view! {
            <span class="file-badge file-badge-streaming" title="Streaming (large file)">"[~]"</span>
        })}

        // 5. Sequence badge (conditional on show_group_badges)
        {seq.map(move |si| {
            let label = format!("#{}", si.sequence_number);
            let tooltip = si.gap_from_prev_secs
                .map(|g| format!("Gap: {}", format_duration_compact(g)))
                .unwrap_or_default();
            if group_dropdowns {
                let on_click = on_seq_click.clone();
                leptos::either::Either::Left(view! {
                    <button
                        class="file-badge file-badge-seq badge-clickable"
                        title=tooltip
                        on:click=move |e: web_sys::MouseEvent| {
                            e.stop_propagation();
                            if let Some(cb) = &on_click { cb.run(()); }
                        }
                    >
                        {label}" \u{25BE}"
                    </button>
                })
            } else {
                leptos::either::Either::Right(view! {
                    <span
                        class="file-badge file-badge-seq"
                        style:display=move || if show_group_badges.get() { "inline" } else { "none" }
                        title=tooltip
                    >{label}</span>
                })
            }
        })}

        // 6. Track badge (conditional on show_group_badges)
        {track.map(move |ti| {
            let label = format!("[{}]", ti.label);
            if group_dropdowns {
                let on_click = on_track_click.clone();
                leptos::either::Either::Left(view! {
                    <button
                        class="file-badge file-badge-track badge-clickable"
                        title="Switch track"
                        on:click=move |e: web_sys::MouseEvent| {
                            e.stop_propagation();
                            if let Some(cb) = &on_click { cb.run(()); }
                        }
                    >
                        {label}" \u{25BE}"
                    </button>
                })
            } else {
                leptos::either::Either::Right(view! {
                    <span
                        class="file-badge file-badge-track"
                        style:display=move || if show_group_badges.get() { "inline" } else { "none" }
                    >{label}</span>
                })
            }
        })}

        // 7. Download button (file-menu only, when show_download is true)
        {(is_file_menu && show_download).then(|| {
            let on_dl = on_download.clone();
            let cls = if is_unsaved {
                "badge-download-btn unsaved"
            } else {
                "badge-download-btn saved"
            };
            let title = if is_unsaved { "Download WAV (unsaved)" } else { "Download WAV" };
            view! {
                <button
                    class=cls
                    title=title
                    on:click=move |e: web_sys::MouseEvent| {
                        e.stop_propagation();
                        if let Some(cb) = &on_dl { cb.run(()); }
                    }
                >"\u{1F4BE}"</button>
            }
        })}

        // 8. CC badge (file-menu only, non-clickable)
        {(is_file_menu && cc_license.is_some()).then(|| {
            let tooltip = cc_tooltip.unwrap_or_else(|| {
                format!("Creative Commons {}", cc_license.as_deref().unwrap_or(""))
            });
            view! {
                <span class="badge-cc-mini" title=tooltip>
                    <span class="toolbar-cc-icon badge-cc-icon"></span>
                </span>
            }
        })}
    }
}
