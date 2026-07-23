use serde::{Deserialize, Serialize};

/// Multi-layered file identity for matching annotations to audio files across sessions.
/// Layers are computed progressively — Layer 1 is instant, higher layers are lazy.
///
/// Hash algorithms:
/// - Layer 2 (spot): BLAKE3 multi-point — 16 x 1MB chunks across audio data region
/// - Layer 3 (content): BLAKE3 of entire file with header bytes zeroed
/// - Layer 4 (full): BLAKE3 root hash of entire file (= bao root hash)
/// - Layer 4-alt: SHA-256 of entire file (optional, for compatibility)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FileIdentity {
    /// Layer 1: filename (basename only).
    pub filename: String,
    /// Layer 1: file size in bytes.
    pub file_size: u64,

    /// Layer 2: BLAKE3 multi-point spot hash (16 x 1MB chunks across audio data).
    /// Fast even for multi-GB files (reads at most 16 MB). None until computed.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub spot_hash_b3: Option<String>,

    /// Layer 3: Content hash — BLAKE3 of entire file with header bytes zeroed.
    /// Survives metadata/header edits while preserving audio content identity.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub content_hash: Option<String>,

    /// Layer 4: Full file BLAKE3 hash (= bao root hash). Computed on demand.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub full_blake3: Option<String>,

    /// Layer 4-alt: Full file SHA-256. For compatibility and paranoid verification.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub full_sha256: Option<String>,

    /// Legacy Layer 2 spot hash (SHA-256, 3x4KB). Read from old v2 sidecars.
    /// Not written in new sidecars.
    #[serde(skip_serializing_if = "Option::is_none", default, alias = "spot_hash")]
    pub legacy_spot_hash: Option<String>,

    /// Audio data region byte offset within the file (WAV: data chunk start).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub data_offset: Option<u64>,
    /// Audio data region byte length.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub data_size: Option<u64>,

    /// File last-modified timestamp (ms since epoch, as string).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub last_modified: Option<String>,

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
        // Layer 4: full file hash (BLAKE3 or SHA-256)
        if let (Some(a), Some(b)) = (&self.full_blake3, &other.full_blake3) {
            if a == b {
                return MatchConfidence::Certain;
            }
            return MatchConfidence::None;
        }
        if let (Some(a), Some(b)) = (&self.full_sha256, &other.full_sha256) {
            if a == b {
                return MatchConfidence::Certain;
            }
            return MatchConfidence::None;
        }
        // Layer 3: content hash (header-zeroed BLAKE3)
        if let (Some(a), Some(b)) = (&self.content_hash, &other.content_hash) {
            if a == b {
                return MatchConfidence::High;
            }
            // If content_hash mismatches but data_size is the same, the header may have
            // changed size (different zero-padding length → different content_hash).
            // Don't reject outright — fall through to spot hash check.
            // A full reconstruction can be attempted on-demand via
            // compute_content_hash_reconstructed().
            let same_data_size = matches!(
                (&self.data_size, &other.data_size),
                (Some(a), Some(b)) if *a == *b && *a > 0
            );
            if !same_data_size {
                return MatchConfidence::None;
            }
        }
        // Layer 2: BLAKE3 spot hash
        if let (Some(a), Some(b)) = (&self.spot_hash_b3, &other.spot_hash_b3) {
            if a == b {
                return MatchConfidence::Likely;
            }
            return MatchConfidence::None;
        }
        // Legacy Layer 2: SHA-256 spot hash (for old sidecars)
        if let (Some(a), Some(b)) = (&self.legacy_spot_hash, &other.legacy_spot_hash) {
            if a == b {
                return MatchConfidence::Likely;
            }
            return MatchConfidence::None;
        }
        // Layer 1: filename + size
        if self.filename == other.filename && self.file_size == other.file_size {
            return MatchConfidence::Weak;
        }
        MatchConfidence::None
    }
}

/// A unique annotation identifier (a UUID v4).
///
/// A newtype rather than a bare `String`, so an annotation id can't be silently
/// swapped with an arbitrary string or with some other id-shaped field.
/// `#[serde(transparent)]` keeps the on-disk/JSON form a plain string, so
/// existing `.batm` sidecars load unchanged.
///
/// Keep ids flowing as `AnnotationId` (not `String`) — the pattern to follow:
/// - make a fresh one with [`AnnotationId::new`];
/// - wrap a string crossing a boundary (a DOM dataset value, an FFI string)
///   with `AnnotationId::from(s)`;
/// - compare two ids with `==` (they are `Eq`);
/// - reach the underlying text only at the edges (DOM attributes, logging) via
///   [`AnnotationId::as_str`] or `Display`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AnnotationId(String);

impl AnnotationId {
    /// Generate a fresh random id (UUID v4).
    pub fn new() -> Self {
        Self(generate_uuid())
    }

    /// The underlying string. Use only at edges (DOM attributes, logging);
    /// pass `AnnotationId` itself everywhere else.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for AnnotationId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for AnnotationId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl From<String> for AnnotationId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for AnnotationId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// A time+frequency region annotation.
/// Frequency bounds are optional — present = "region" (2D box), absent = "segment" (time-only span).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Region {
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
    /// Whether this region is locked (cannot be resized/moved via handles).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub locked: Option<bool>,
}

impl Region {
    pub fn is_locked(&self) -> bool {
        self.locked.unwrap_or(false)
    }
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
    Region(Region),
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
    /// User-defined tags (e.g. species, call type).
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub tags: Vec<String>,
    /// True when the label was auto-generated (e.g. "Region 3") rather than
    /// set by the user. UI renders default labels in italic. Omitted/None
    /// means the label (if any) is user-authored.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub label_default: Option<bool>,
}

/// Human-readable kind name used as the prefix for auto-generated labels
/// ("Region 1", "Segment 2", etc.).
pub fn kind_name(kind: &AnnotationKind) -> &'static str {
    match kind {
        AnnotationKind::Region(r) if r.freq_low.is_some() && r.freq_high.is_some() => "Region",
        AnnotationKind::Region(_) => "Segment",
        AnnotationKind::Marker(_) => "Marker",
        AnnotationKind::Group(_) => "Group",
        AnnotationKind::Measurement(_) => "Measurement",
    }
}

fn label_of(kind: &AnnotationKind) -> Option<&str> {
    match kind {
        AnnotationKind::Region(r) => r.label.as_deref(),
        AnnotationKind::Marker(m) => m.label.as_deref(),
        AnnotationKind::Group(g) => g.label.as_deref(),
        AnnotationKind::Measurement(m) => m.label.as_deref(),
    }
}

/// Generate a default label like "Region 4" by finding the highest existing
/// number among annotations whose label matches "{prefix} N" (for any kind —
/// so numbering is shared per prefix). Leaves gaps so deletions don't renumber
/// the remaining annotations. `exclude_id` skips a specific annotation (useful
/// when regenerating a label for an annotation already in the set).
pub fn generate_default_label(
    annotations: &[Annotation],
    kind: &AnnotationKind,
    exclude_id: Option<&str>,
) -> String {
    let prefix = kind_name(kind);
    let mut max_n: u32 = 0;
    for a in annotations {
        if exclude_id == Some(a.id.as_str()) {
            continue;
        }
        let Some(label) = label_of(&a.kind) else {
            continue;
        };
        let Some(rest) = label.strip_prefix(prefix) else {
            continue;
        };
        let rest = rest.trim_start();
        if let Ok(n) = rest.parse::<u32>() {
            if n > max_n {
                max_n = n;
            }
        }
    }
    format!("{} {}", prefix, max_n + 1)
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
    /// Byte offset of audio data within the file (WAV: data chunk start).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub data_offset: Option<u64>,
    /// Byte length of audio data region.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub data_size: Option<u64>,
    /// Peak level (dBFS) of first 30s of unmodified audio.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub peak_db_30s: Option<f64>,
    /// Peak level (dBFS) of entire unmodified audio file.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub peak_db_full: Option<f64>,
}

/// Current `.batm` sidecar on-disk format version. A file whose `version`
/// exceeds this was written by a newer app: still loaded best-effort (serde fills
/// defaults), but unknown fields may be dropped or misread.
pub const ANNOTATION_FORMAT_VERSION: u32 = 3;

/// Per-file annotation collection — serialized to .batm sidecar files (YAML).
/// Field order matters: serde serializes in declaration order, and we want
/// noise_profile (with its large bin_magnitudes) at the end for readability.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AnnotationSet {
    /// Sidecar format version. See [`ANNOTATION_FORMAT_VERSION`].
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
    /// Warn (don't fail) if this sidecar was written by a newer app whose format
    /// version exceeds what we support — fields may be dropped or misread.
    pub fn warn_if_future_version(&self) {
        if self.version > ANNOTATION_FORMAT_VERSION {
            log::warn!(
                "Annotation sidecar {} is format version {} but this app supports {}; \
                 loading best-effort.",
                self.id,
                self.version,
                ANNOTATION_FORMAT_VERSION
            );
        }
    }

    /// Create a new empty AnnotationSet for a file.
    pub fn new(file_identity: FileIdentity) -> Self {
        Self {
            version: ANNOTATION_FORMAT_VERSION,
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
    pub fn new_with_metadata(
        file_identity: FileIdentity,
        audio: &crate::types::AudioData,
        peak_db_30s: Option<f64>,
        peak_db_full: Option<f64>,
    ) -> Self {
        let mut set = Self::new(file_identity);
        set.audio_metadata = Some(AudioFileMetadata {
            sample_rate: audio.sample_rate,
            total_samples: audio.source.total_samples(),
            channels: audio.channels,
            duration_secs: audio.duration_secs,
            format: audio.metadata.format.to_string(),
            bits_per_sample: Some(audio.metadata.bits_per_sample),
            data_offset: audio.metadata.data_offset,
            data_size: audio.metadata.data_size,
            peak_db_30s,
            peak_db_full,
        });
        set
    }

    /// Touch the modified_at timestamp and app_version.
    pub fn touch(&mut self) {
        self.modified_at = Some(now_iso8601());
        self.app_version = env!("CARGO_PKG_VERSION").to_string();
    }
}

/// In-memory annotation store, keyed by each `LoadedFile`'s stable `id`
/// (a process-unique, never-reused u64 — NOT the file's list position).
/// Keying by id means annotations always track their own file across list
/// reordering, insertion, and removal, instead of silently shifting onto a
/// neighbouring file when the list changes (the old parallel-Vec design did
/// the latter, which let annotations leak between recordings).
#[derive(Clone, Debug, Default)]
pub struct AnnotationStore {
    pub sets: std::collections::HashMap<u64, AnnotationSet>,
}

impl AnnotationStore {
    /// The annotation set for file `id`, if any.
    pub fn get(&self, id: u64) -> Option<&AnnotationSet> {
        self.sets.get(&id)
    }

    /// Mutable annotation set for file `id`, if any.
    pub fn get_mut(&mut self, id: u64) -> Option<&mut AnnotationSet> {
        self.sets.get_mut(&id)
    }

    /// Whether file `id` has an annotation set.
    pub fn contains(&self, id: u64) -> bool {
        self.sets.contains_key(&id)
    }

    /// Insert/replace the annotation set for file `id`.
    pub fn insert(&mut self, id: u64, set: AnnotationSet) {
        self.sets.insert(id, set);
    }

    /// Remove (and return) the annotation set for file `id`.
    pub fn remove(&mut self, id: u64) -> Option<AnnotationSet> {
        self.sets.remove(&id)
    }

    /// Get the set for file `id`, inserting one built by `f` if absent.
    pub fn entry_or_insert_with(
        &mut self,
        id: u64,
        f: impl FnOnce() -> AnnotationSet,
    ) -> &mut AnnotationSet {
        self.sets.entry(id).or_insert_with(f)
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
    let id_set: std::collections::HashSet<&str> =
        annotations.iter().map(|a| a.id.as_str()).collect();

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
            oa.partial_cmp(&ob)
                .unwrap_or(std::cmp::Ordering::Equal)
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
        children
            .iter()
            .map(|a| {
                let kids = build_children(Some(a.id.as_str()), children_map, depth + 1);
                AnnotationNode {
                    annotation: (*a).clone(),
                    children: kids,
                    depth,
                }
            })
            .collect()
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
pub fn collect_descendants(
    annotations: &[Annotation],
    parent_id: &AnnotationId,
) -> Vec<AnnotationId> {
    let mut result = Vec::new();
    for a in annotations {
        if a.parent_id.as_ref() == Some(parent_id) {
            result.push(a.id.clone());
            result.extend(collect_descendants(annotations, &a.id));
        }
    }
    result
}

/// Assign consecutive sort_order values to annotations at a given parent level.
pub fn renumber_children(annotations: &mut [Annotation], parent_id: Option<&AnnotationId>) {
    let mut indices: Vec<usize> = annotations
        .iter()
        .enumerate()
        .filter(|(_, a)| a.parent_id.as_ref() == parent_id)
        .map(|(i, _)| i)
        .collect();
    // Sort by existing sort_order then created_at
    indices.sort_by(|&i, &j| {
        let a = &annotations[i];
        let b = &annotations[j];
        let oa = a.sort_order.unwrap_or(f64::MAX);
        let ob = b.sort_order.unwrap_or(f64::MAX);
        oa.partial_cmp(&ob)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.created_at.cmp(&b.created_at))
    });
    for (rank, &idx) in indices.iter().enumerate() {
        annotations[idx].sort_order = Some(rank as f64);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `AnnotationId` must serialize as a BARE string (serde `transparent`), so
    /// `.batm` sidecars written before the newtype existed still load unchanged.
    #[test]
    fn annotation_id_is_serde_transparent() {
        let id = AnnotationId::from("11111111-2222-4333-8444-555555555555");
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "\"11111111-2222-4333-8444-555555555555\"");

        // A pre-newtype bare string deserializes straight into the newtype.
        let back: AnnotationId = serde_json::from_str("\"abc-123\"").unwrap();
        assert_eq!(back, AnnotationId::from("abc-123"));
        assert_eq!(back.as_str(), "abc-123");
    }

    /// The forms ids actually persist in (`Option` for parent_id, `Vec` for
    /// selection) also stay bare strings / arrays of bare strings.
    #[test]
    fn optional_and_listed_ids_stay_bare() {
        let parent: Option<AnnotationId> = Some(AnnotationId::from("p1"));
        assert_eq!(serde_json::to_string(&parent).unwrap(), "\"p1\"");
        assert_eq!(
            serde_json::to_string(&Option::<AnnotationId>::None).unwrap(),
            "null"
        );

        let ids = vec![AnnotationId::from("a"), AnnotationId::from("b")];
        assert_eq!(serde_json::to_string(&ids).unwrap(), "[\"a\",\"b\"]");
        let back: Vec<AnnotationId> = serde_json::from_str("[\"a\",\"b\"]").unwrap();
        assert_eq!(back, ids);
    }
}
