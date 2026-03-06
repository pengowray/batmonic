use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
pub struct TrackInfo {
    pub group_key: String,
    pub label: String,
}

/// Parse a filename to extract a track/channel suffix.
///
/// Recognises patterns like:
/// - `260305_0058_1-2.wav` → group_key="260305_0058", label="1-2"
/// - `recording_Ch1.flac` → group_key="recording", label="Ch1"
/// - `260227_0055_3 my recording.wav` → group_key="260227_0055", label="3"
/// - `site_004.wav` → group_key="site", label="004"
pub fn parse_track_suffix(filename: &str) -> Option<TrackInfo> {
    // Strip extension
    let stem = filename.rsplit_once('.').map(|(s, _)| s).unwrap_or(filename);

    // Find last underscore — everything after it is the candidate segment
    let (prefix, segment) = stem.rsplit_once('_')?;

    // Don't match if prefix is empty
    if prefix.is_empty() {
        return None;
    }

    // Extract the leading "track" portion of the segment.
    // For renamed files like "3 my recording", take only the leading part.
    let track_part = segment.split_once(' ').map(|(t, _)| t).unwrap_or(segment);

    if track_part.is_empty() {
        return None;
    }

    // Pattern 1: channel range like "1-2", "3-4"
    if let Some((a, b)) = track_part.split_once('-') {
        if !a.is_empty() && a.chars().all(|c| c.is_ascii_digit())
            && !b.is_empty() && b.chars().all(|c| c.is_ascii_digit())
        {
            return Some(TrackInfo {
                group_key: prefix.to_string(),
                label: track_part.to_string(),
            });
        }
    }

    // Pattern 2: "Ch1", "ch2", "CH3" (case-insensitive)
    let lower = track_part.to_ascii_lowercase();
    if lower.starts_with("ch") && lower.len() > 2 && lower[2..].chars().all(|c| c.is_ascii_digit()) {
        return Some(TrackInfo {
            group_key: prefix.to_string(),
            label: track_part.to_string(),
        });
    }

    // Pattern 3: bare number like "3", "004"
    if track_part.chars().all(|c| c.is_ascii_digit()) {
        return Some(TrackInfo {
            group_key: prefix.to_string(),
            label: track_part.to_string(),
        });
    }

    None
}

/// Compute file groups from a list of filenames.
///
/// Returns a parallel Vec: `Some(TrackInfo)` for files that belong to a group
/// of 2+ files sharing the same `group_key`, `None` for singletons.
pub fn compute_file_groups(names: &[String]) -> Vec<Option<TrackInfo>> {
    let parsed: Vec<Option<TrackInfo>> = names.iter().map(|n| parse_track_suffix(n)).collect();

    // Count occurrences per group_key
    let mut counts: HashMap<String, usize> = HashMap::new();
    for info in &parsed {
        if let Some(ti) = info {
            *counts.entry(ti.group_key.clone()).or_insert(0) += 1;
        }
    }

    // Only keep entries where group has 2+ members
    parsed
        .into_iter()
        .map(|opt| {
            opt.filter(|ti| counts.get(&ti.group_key).copied().unwrap_or(0) >= 2)
        })
        .collect()
}
