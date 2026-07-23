use crate::components::file_sidebar::file_groups::{self, TrackInfo};
use crate::state::LoadedFile;

/// Runtime view of a timeline — maps timeline-local time to file data.
/// Built from selected files and their recording timestamps.
#[derive(Clone, Debug)]
pub struct TimelineView {
    /// Segments sorted by timeline offset (earliest first).
    pub segments: Vec<TimelineSegment>,
    /// Total span in seconds (from origin to end of last segment).
    pub total_duration_secs: f64,
    /// Epoch ms of the timeline origin (earliest file start).
    pub origin_epoch_ms: f64,
    /// Multitrack groups available for track switching.
    pub multitrack_groups: Vec<MultitrackOption>,
}

/// A single file's placement on the timeline.
#[derive(Clone, Debug)]
pub struct TimelineSegment {
    /// Index into `AppState::files`.
    pub file_index: usize,
    /// Offset in seconds from the timeline origin.
    pub timeline_offset_secs: f64,
    /// Duration of this file in seconds.
    pub duration_secs: f64,
    /// True if this segment overlaps with another segment.
    pub has_overlap: bool,
}

/// A multitrack option available for switching in the Channel dropdown.
#[derive(Clone, Debug)]
pub struct MultitrackOption {
    pub group_id: String,
    pub label: String,
    /// Maps from primary file_index to alternate file_index for this track.
    pub alternates: Vec<(usize, usize)>,
}

impl TimelineView {
    /// Build a timeline from selected file indices.
    ///
    /// Files are positioned by their `recording_start_epoch_ms`. Files without
    /// timestamps are placed sequentially after the last positioned file.
    pub fn from_files(file_indices: &[usize], files: &[LoadedFile]) -> Option<Self> {
        if file_indices.len() < 2 {
            return None;
        }

        // Collect (file_index, start_epoch_ms_or_none, duration)
        let mut entries: Vec<(usize, Option<f64>, f64)> = file_indices
            .iter()
            .filter_map(|&i| {
                let f = files.get(i)?;
                Some((i, f.recording_start_epoch_ms(), f.audio.duration_secs))
            })
            .collect();

        if entries.is_empty() {
            return None;
        }

        // Sort: files with timestamps first (by timestamp), then files without
        entries.sort_by(|a, b| match (&a.1, &b.1) {
            (Some(ta), Some(tb)) => ta.partial_cmp(tb).unwrap_or(std::cmp::Ordering::Equal),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        });

        // Determine the origin (earliest start time)
        let origin_ms = entries
            .iter()
            .filter_map(|(_, start, _)| *start)
            .next()
            .unwrap_or(0.0);

        // Build segments
        let mut segments = Vec::new();
        let mut cursor_secs = 0.0; // For files without timestamps

        for (file_idx, start_ms, duration) in &entries {
            let offset = if let Some(ms) = start_ms {
                (ms - origin_ms) / 1000.0
            } else {
                // No timestamp: place after the current cursor
                cursor_secs
            };

            segments.push(TimelineSegment {
                file_index: *file_idx,
                timeline_offset_secs: offset,
                duration_secs: *duration,
                has_overlap: false,
            });

            let end = offset + duration;
            if end > cursor_secs {
                cursor_secs = end;
            }
        }

        // Detect overlaps
        for i in 0..segments.len() {
            for j in (i + 1)..segments.len() {
                let a_start = segments[i].timeline_offset_secs;
                let a_end = a_start + segments[i].duration_secs;
                let b_start = segments[j].timeline_offset_secs;
                let b_end = b_start + segments[j].duration_secs;

                if a_start < b_end && b_start < a_end {
                    segments[i].has_overlap = true;
                    segments[j].has_overlap = true;
                }
            }
        }

        // Total duration
        let total_duration_secs = segments
            .iter()
            .map(|s| s.timeline_offset_secs + s.duration_secs)
            .fold(0.0_f64, f64::max);

        // Detect multitrack groups from the selected files
        let all_names: Vec<String> = files.iter().map(|f| f.name.clone()).collect();
        let all_groups = file_groups::compute_file_groups(&all_names);

        let mut multitrack_groups = Vec::new();
        let mut seen_groups = std::collections::HashSet::new();

        for &idx in file_indices {
            if let Some(ti) = all_groups.get(idx).and_then(|g| g.as_ref()) {
                if seen_groups.insert(ti.group_key.clone()) {
                    // Find all tracks in this group
                    let group_members: Vec<(usize, TrackInfo)> = all_groups
                        .iter()
                        .enumerate()
                        .filter_map(|(i, g)| {
                            g.as_ref()
                                .filter(|t| t.group_key == ti.group_key)
                                .map(|t| (i, t.clone()))
                        })
                        .collect();

                    if group_members.len() >= 2 {
                        // Build alternates: map from primary (idx) to each other member
                        let alternates: Vec<(usize, usize)> = group_members
                            .iter()
                            .filter(|(i, _)| *i != idx)
                            .map(|(i, _)| (idx, *i))
                            .collect();

                        for (_, member_ti) in &group_members {
                            if !multitrack_groups
                                .iter()
                                .any(|m: &MultitrackOption| m.label == member_ti.label)
                            {
                                multitrack_groups.push(MultitrackOption {
                                    group_id: ti.group_key.clone(),
                                    label: member_ti.label.clone(),
                                    alternates: alternates.clone(),
                                });
                            }
                        }
                    }
                }
            }
        }

        Some(TimelineView {
            segments,
            total_duration_secs,
            origin_epoch_ms: origin_ms,
            multitrack_groups,
        })
    }

    /// Find which file (if any) is at the given timeline-local time.
    /// Returns `(file_index, offset_within_file)`.
    pub fn file_at_time(&self, t: f64) -> Option<(usize, f64)> {
        // Return the first segment that contains this time
        for seg in &self.segments {
            let seg_end = seg.timeline_offset_secs + seg.duration_secs;
            if t >= seg.timeline_offset_secs && t < seg_end {
                return Some((seg.file_index, t - seg.timeline_offset_secs));
            }
        }
        None
    }

    /// Get all segments that are visible in the given time range.
    pub fn segments_in_range(&self, start: f64, end: f64) -> Vec<&TimelineSegment> {
        self.segments
            .iter()
            .filter(|s| {
                let seg_end = s.timeline_offset_secs + s.duration_secs;
                s.timeline_offset_secs < end && seg_end > start
            })
            .collect()
    }

    /// Get the segment for a specific file index.
    pub fn segment_for_file(&self, file_index: usize) -> Option<&TimelineSegment> {
        self.segments.iter().find(|s| s.file_index == file_index)
    }
}
