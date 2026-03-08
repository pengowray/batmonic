use serde::{Serialize, Deserialize};

/// Multi-layered file identity for matching annotations to audio files across sessions.
/// Layers are computed progressively — Layer 1 is instant, higher layers are lazy.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FileIdentity {
    /// Layer 1: filename (basename only).
    pub filename: String,
    /// Layer 1: file size in bytes.
    pub file_size: u64,

    /// Layer 2: Spot-check hash. SHA-256 of (first 4KB + middle 4KB + last 4KB).
    /// Fast even for multi-GB files. None until computed.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub spot_hash: Option<String>,

    /// Layer 3: Audio-data-only hash. SHA-256 of just PCM/audio bytes (no headers).
    /// Survives metadata edits. Lazy — computed on demand.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub audio_hash: Option<String>,

    /// Layer 4: Full file SHA-256. Computed lazily on explicit request.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub full_sha256: Option<String>,

    /// Original file path (Tauri only). Used for sidecar file placement and re-finding.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub file_path: Option<String>,
}

/// Confidence level when matching a FileIdentity against another.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum MatchConfidence {
    /// No match at all.
    None,
    /// Only filename + size match (fragile).
    Weak,
    /// Spot-check hash matches (very likely the same file).
    Likely,
    /// Audio-data hash matches (same audio content, possibly different metadata).
    High,
    /// Full SHA-256 matches (identical files).
    Certain,
}

impl FileIdentity {
    /// Compare two identities and return the highest confidence match.
    pub fn match_confidence(&self, other: &FileIdentity) -> MatchConfidence {
        // Check from highest confidence downward
        if let (Some(a), Some(b)) = (&self.full_sha256, &other.full_sha256) {
            if a == b {
                return MatchConfidence::Certain;
            }
            // Different full hashes = definitely different files
            return MatchConfidence::None;
        }
        if let (Some(a), Some(b)) = (&self.audio_hash, &other.audio_hash) {
            if a == b {
                return MatchConfidence::High;
            }
            return MatchConfidence::None;
        }
        if let (Some(a), Some(b)) = (&self.spot_hash, &other.spot_hash) {
            if a == b {
                return MatchConfidence::Likely;
            }
            return MatchConfidence::None;
        }
        // Fall back to filename + size
        if self.filename == other.filename && self.file_size == other.file_size {
            return MatchConfidence::Weak;
        }
        MatchConfidence::None
    }
}

/// Unique annotation identifier (UUID v4 as string).
pub type AnnotationId = String;

/// A saved time+frequency region selection.
/// Frequency bounds are optional — a time-only selection has no freq_low/freq_high.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SavedSelection {
    pub time_start: f64,
    pub time_end: f64,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub freq_low: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub freq_high: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub color: Option<String>,
}

/// A time-position marker (future).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Marker {
    pub time: f64,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub color: Option<String>,
}

/// A measurement between two time+frequency points (future).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Measurement {
    pub start_time: f64,
    pub start_freq: f64,
    pub end_time: f64,
    pub end_freq: f64,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub label: Option<String>,
}

/// A named group that can contain other annotations or nested groups.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Group {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub color: Option<String>,
    /// Whether the group is collapsed in the UI.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub collapsed: Option<bool>,
}

/// Tagged annotation kind — extensible for future types.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AnnotationKind {
    Selection(SavedSelection),
    Marker(Marker),
    Measurement(Measurement),
    Group(Group),
}

/// A single annotation with metadata.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Annotation {
    pub id: AnnotationId,
    #[serde(flatten)]
    pub kind: AnnotationKind,
    pub created_at: String,
    pub modified_at: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub notes: Option<String>,
    /// Parent group id. None = root level.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub parent_id: Option<AnnotationId>,
    /// Sort order within parent. Lower values sort first.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub sort_order: Option<f64>,
}

/// Basic audio file metadata stored in the sidecar for reference.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AudioFileMetadata {
    pub sample_rate: u32,
    pub total_samples: u64,
    pub channels: u32,
    pub duration_secs: f64,
    pub format: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub bits_per_sample: Option<u16>,
}

/// Per-file annotation collection — serialized to .batm sidecar files (YAML).
/// Field order matters: serde serializes in declaration order, and we want
/// noise_profile (with its large bin_magnitudes) at the end for readability.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AnnotationSet {
    /// Sidecar format version.
    pub version: u32,
    /// Unique ID for this sidecar file (UUID v4).
    #[serde(default = "generate_uuid")]
    pub id: String,
    /// App version that last wrote this file.
    #[serde(default)]
    pub app_version: String,
    /// When this sidecar was created.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub created_at: Option<String>,
    /// When this sidecar was last modified.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub modified_at: Option<String>,

    pub file_identity: FileIdentity,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub audio_metadata: Option<AudioFileMetadata>,

    #[serde(default)]
    pub annotations: Vec<Annotation>,

    /// Noise reduction profile (notch bands + spectral floor). Kept near the end
    /// because noise_floor.bin_magnitudes can be very long.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub noise_profile: Option<crate::dsp::notch::NoiseProfile>,
}

impl AnnotationSet {
    /// Create a new empty AnnotationSet for a file.
    pub fn new(file_identity: FileIdentity) -> Self {
        Self {
            version: 1,
            id: generate_uuid(),
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            created_at: Some(now_iso8601()),
            modified_at: Some(now_iso8601()),
            file_identity,
            audio_metadata: None,
            annotations: Vec::new(),
            noise_profile: None,
        }
    }

    /// Create a new AnnotationSet with audio metadata populated from a LoadedFile.
    pub fn new_with_metadata(file_identity: FileIdentity, audio: &crate::types::AudioData) -> Self {
        let mut set = Self::new(file_identity);
        set.audio_metadata = Some(AudioFileMetadata {
            sample_rate: audio.sample_rate,
            total_samples: audio.source.total_samples(),
            channels: audio.channels,
            duration_secs: audio.duration_secs,
            format: audio.metadata.format.to_string(),
            bits_per_sample: Some(audio.metadata.bits_per_sample),
        });
        set
    }

    /// Touch the modified_at timestamp and app_version.
    pub fn touch(&mut self) {
        self.modified_at = Some(now_iso8601());
        self.app_version = env!("CARGO_PKG_VERSION").to_string();
    }
}

/// In-memory annotation store, indexed parallel to AppState::files.
#[derive(Clone, Debug, Default)]
pub struct AnnotationStore {
    pub sets: Vec<Option<AnnotationSet>>,
}

impl AnnotationStore {
    /// Ensure the store has at least `len` slots (push None for new files).
    pub fn ensure_len(&mut self, len: usize) {
        while self.sets.len() < len {
            self.sets.push(None);
        }
    }

    /// Remove the entry at `index`, shifting subsequent entries.
    pub fn remove(&mut self, index: usize) {
        if index < self.sets.len() {
            self.sets.remove(index);
        }
    }
}

/// Generate a UUID v4 string using js_sys::Math::random().
pub fn generate_uuid() -> String {
    let random_bytes: Vec<u8> = (0..16)
        .map(|_| (js_sys::Math::random() * 256.0) as u8)
        .collect();

    // Set version (4) and variant (10xx) bits
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&random_bytes);
    bytes[6] = (bytes[6] & 0x0f) | 0x40; // version 4
    bytes[8] = (bytes[8] & 0x3f) | 0x80; // variant 10xx

    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3],
        bytes[4], bytes[5],
        bytes[6], bytes[7],
        bytes[8], bytes[9],
        bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]
    )
}

/// Get the current time as an ISO 8601 string using js_sys::Date.
pub fn now_iso8601() -> String {
    let date = js_sys::Date::new_0();
    date.to_iso_string().as_string().unwrap_or_default()
}

/// A node in the annotation tree (used for rendering).
#[derive(Clone, Debug)]
pub struct AnnotationNode {
    pub annotation: Annotation,
    pub children: Vec<AnnotationNode>,
    pub depth: usize,
}

/// Build a tree from a flat annotation list using parent_id references.
/// Annotations with unknown parent_ids are placed at root.
pub fn build_annotation_tree(annotations: &[Annotation]) -> Vec<AnnotationNode> {
    use std::collections::HashMap;

    // Index annotations by id
    let id_set: std::collections::HashSet<&str> = annotations.iter().map(|a| a.id.as_str()).collect();

    // Collect children per parent_id (None = root)
    let mut children_map: HashMap<Option<&str>, Vec<&Annotation>> = HashMap::new();
    for a in annotations {
        // If parent_id references a non-existent id, treat as root
        let parent = match &a.parent_id {
            Some(pid) if id_set.contains(pid.as_str()) => Some(pid.as_str()),
            _ => None,
        };
        children_map.entry(parent).or_default().push(a);
    }

    // Sort each group by sort_order then by created_at
    for list in children_map.values_mut() {
        list.sort_by(|a, b| {
            let oa = a.sort_order.unwrap_or(f64::MAX);
            let ob = b.sort_order.unwrap_or(f64::MAX);
            oa.partial_cmp(&ob).unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.created_at.cmp(&b.created_at))
        });
    }

    fn build_children(
        parent_key: Option<&str>,
        children_map: &HashMap<Option<&str>, Vec<&Annotation>>,
        depth: usize,
    ) -> Vec<AnnotationNode> {
        let Some(children) = children_map.get(&parent_key) else {
            return Vec::new();
        };
        children.iter().map(|a| {
            let kids = build_children(Some(a.id.as_str()), children_map, depth + 1);
            AnnotationNode {
                annotation: (*a).clone(),
                children: kids,
                depth,
            }
        }).collect()
    }

    build_children(None, &children_map, 0)
}

/// Flatten a tree back into a depth-first ordered list (for display).
pub fn flatten_tree(nodes: &[AnnotationNode]) -> Vec<(AnnotationId, usize)> {
    let mut out = Vec::new();
    for node in nodes {
        out.push((node.annotation.id.clone(), node.depth));
        out.extend(flatten_tree(&node.children));
    }
    out
}

/// Collect all descendant ids of an annotation (for recursive deletion / grouping).
pub fn collect_descendants(annotations: &[Annotation], parent_id: &str) -> Vec<AnnotationId> {
    let mut result = Vec::new();
    for a in annotations {
        if a.parent_id.as_deref() == Some(parent_id) {
            result.push(a.id.clone());
            result.extend(collect_descendants(annotations, &a.id));
        }
    }
    result
}

/// Assign consecutive sort_order values to annotations at a given parent level.
pub fn renumber_children(annotations: &mut [Annotation], parent_id: Option<&str>) {
    let mut indices: Vec<usize> = annotations.iter().enumerate()
        .filter(|(_, a)| a.parent_id.as_deref() == parent_id)
        .map(|(i, _)| i)
        .collect();
    // Sort by existing sort_order then created_at
    indices.sort_by(|&i, &j| {
        let a = &annotations[i];
        let b = &annotations[j];
        let oa = a.sort_order.unwrap_or(f64::MAX);
        let ob = b.sort_order.unwrap_or(f64::MAX);
        oa.partial_cmp(&ob).unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.created_at.cmp(&b.created_at))
    });
    for (rank, &idx) in indices.iter().enumerate() {
        annotations[idx].sort_order = Some(rank as f64);
    }
}
