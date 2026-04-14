use leptos::prelude::*;
use crate::audio::source::ChannelView;
use crate::canvas::spectrogram_renderer::Colormap;
use crate::canvas::flow::FlowAlgo;
use crate::annotations::AnnotationKind;
use crate::types::{AudioData, PreviewImage, SpectrogramData};
use crate::annotations::{AnnotationId, AnnotationStore, FileIdentity};

/// Hash data extracted from an XC sidecar JSON file.
/// Mirrors `xc_lib::cache::SidecarHashes` but defined locally to avoid
/// pulling xc-lib (which depends on reqwest) into the WASM frontend.
#[derive(Clone, Debug, Default)]
pub struct SidecarHashes {
    pub blake3: Option<String>,
    pub sha256: Option<String>,
    pub file_size: Option<u64>,
    pub spot_hash_b3: Option<String>,
    pub content_hash: Option<String>,
    pub data_offset: Option<u64>,
    pub data_size: Option<u64>,
}

impl SidecarHashes {
    pub fn is_empty(&self) -> bool {
        self.blake3.is_none() && self.sha256.is_none()
            && self.file_size.is_none() && self.spot_hash_b3.is_none()
    }
}

/// Overall verification result against reference hashes (XC sidecar or .batm).
#[derive(Clone, Debug, Default, PartialEq)]
pub enum VerifyOutcome {
    #[default]
    /// No verification attempted yet (hashes still computing or no reference available).
    Pending,
    /// Primary hash matched reference.
    Match,
    /// Primary hash failed, but content_hash matched (header-only change).
    ContentMatch,
    /// All verification failed.
    Mismatch,
}

/// Per-file settings that persist when switching between files.
/// Files in the same sequence group share settings.
#[derive(Clone, Debug)]
pub struct FileSettings {
    pub gain_mode: GainMode,
    pub gain_db: f64,
    /// Stashed gain for the other HFR state (swapped on HFR toggle).
    pub gain_db_stash: f64,
    pub notch_enabled: bool,
    pub notch_bands: Vec<crate::dsp::notch::NoiseBand>,
    pub notch_profile_name: String,
    pub notch_harmonic_suppression: f64,
    pub noise_reduce_enabled: bool,
    pub noise_reduce_strength: f64,
    pub noise_reduce_floor: Option<crate::dsp::spectral_sub::NoiseFloor>,
}

impl Default for FileSettings {
    fn default() -> Self {
        Self {
            gain_mode: GainMode::Off,
            gain_db: 0.0,
            gain_db_stash: 0.0,
            notch_enabled: false,
            notch_bands: Vec::new(),
            notch_profile_name: String::new(),
            notch_harmonic_suppression: 0.0,
            noise_reduce_enabled: false,
            noise_reduce_strength: 0.6,
            noise_reduce_floor: None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct LoadedFile {
    pub name: String,
    pub audio: AudioData,
    pub spectrogram: SpectrogramData,
    pub preview: Option<PreviewImage>,
    /// Higher-resolution overview image computed after full spectrogram is ready.
    /// Falls back to `preview` when not yet available.
    pub overview_image: Option<PreviewImage>,
    pub xc_metadata: Option<Vec<(String, String)>>,
    /// Hash data from XC sidecar (for verification against computed identity).
    pub xc_hashes: Option<SidecarHashes>,
    pub is_recording: bool,  // true = unsaved recording (show indicator on web)
    /// Transient listening file — auto-removed when listening stops, converted to
    /// a recording file when the user hits record.
    pub is_live_listen: bool,
    /// Per-file gain and noise filter settings.
    pub settings: FileSettings,
    /// Insertion order (index at time of push).
    pub add_order: usize,
    /// File.lastModified timestamp from the File API (ms since epoch), if available.
    pub last_modified_ms: Option<f64>,
    /// Multi-layered file identity for annotation matching. Computed progressively after load.
    pub identity: Option<FileIdentity>,
    /// File handle for on-demand range reading (Layer 3/4 hash computation).
    pub file_handle: Option<crate::audio::streaming_source::FileHandle>,
    /// Cached peak level (dBFS) of first 30s. None = not yet computed (e.g. streaming still loading).
    pub cached_peak_db: Option<f64>,
    /// Cached peak level (dBFS) of entire file. None = not yet computed.
    pub cached_full_peak_db: Option<f64>,
    /// Read-only mode: annotations are ephemeral, no auto-save to central store or sidecar.
    pub read_only: bool,
    /// A file-adjacent .batm sidecar existed when this file was loaded.
    /// When true, auto-save updates the sidecar alongside the central store.
    pub had_sidecar: bool,
    /// Overall verification result against reference hashes.
    pub verify_outcome: VerifyOutcome,
    /// True after user clicks "Calculate all hashes" — enables indicators on all hash rows.
    pub all_hashes_verified: bool,
    /// WAV cue-point markers parsed from the file (read-only display).
    pub wav_markers: Vec<crate::types::WavMarker>,
    /// Loading entry ID while this file is still loading (for inline progress display).
    /// Set when the file is pushed to `files`, cleared by `loading_done`.
    pub loading_id: Option<u64>,
}

impl LoadedFile {
    /// Get the recording start time as milliseconds since Unix epoch, if available.
    ///
    /// Sources (in priority order):
    /// 1. GUANO "Timestamp" field (ISO 8601) — actual recording start
    /// 2. `last_modified_ms` from the File API — file modification time as fallback,
    ///    adjusted backwards by the file duration to approximate recording start
    pub fn recording_start_epoch_ms(&self) -> Option<f64> {
        self.recording_start_info().map(|(ms, _)| ms)
    }

    /// Get the recording start time and its source description.
    ///
    /// Returns `(epoch_ms, source_label)` where `source_label` describes the
    /// origin: "GUANO Timestamp" or "File modified date (approx.)".
    pub fn recording_start_info(&self) -> Option<(f64, &'static str)> {
        // Try GUANO Timestamp first
        if let Some(ref guano) = self.audio.metadata.guano {
            if let Some((_, ts)) = guano.fields.iter().find(|(k, _)| k == "Timestamp") {
                if let Some(epoch) = parse_iso8601_to_epoch_ms(ts) {
                    return Some((epoch, "GUANO Timestamp"));
                }
            }
        }
        // Fallback: file last-modified minus duration ≈ recording start
        self.last_modified_ms
            .map(|lm| (lm - self.audio.duration_secs * 1000.0, "File modified date (approx.)"))
    }
}

/// Parse a subset of ISO 8601 timestamps to epoch milliseconds.
/// Handles formats like "2023-07-15T22:30:45", "2023-07-15T22:30:45Z",
/// "2023-07-15T22:30:45.123+02:00", "2023-07-15T22:30:45-05:00".
fn parse_iso8601_to_epoch_ms(s: &str) -> Option<f64> {
    // Use js_sys::Date.parse() which handles ISO 8601 natively
    let ms = js_sys::Date::parse(s);
    if ms.is_nan() { None } else { Some(ms) }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Selection {
    pub time_start: f64,
    pub time_end: f64,
    /// None means no frequency constraint (time-only selection).
    pub freq_low: Option<f64>,
    /// None means no frequency constraint (time-only selection).
    pub freq_high: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PlaybackMode {
    Normal,
    Heterodyne,
    TimeExpansion,
    PitchShift,
    PhaseVocoder,
    ZeroCrossing,
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum ExportFormat {
    #[default]
    Wav,
    Mp4,
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum VideoResolution {
    #[default]
    Hd720,
    Hd1080,
    MatchCanvas,
}

impl VideoResolution {
    pub fn dimensions(self, canvas_w: u32, canvas_h: u32) -> (u32, u32) {
        match self {
            Self::Hd720 => (1280, 720),
            Self::Hd1080 => (1920, 1080),
            Self::MatchCanvas => (canvas_w, canvas_h),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Hd720 => "720p",
            Self::Hd1080 => "1080p",
            Self::MatchCanvas => "Match canvas",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum VideoCodec {
    #[default]
    H264,
    Av1,
}

impl VideoCodec {
    pub fn label(self) -> &'static str {
        match self {
            Self::H264 => "H.264",
            Self::Av1 => "AV1",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum AudioCodecOption {
    /// Automatically pick the best available codec (AAC preferred, then Opus).
    #[default]
    Auto,
    /// Force AAC audio.
    Aac,
    /// Force Opus audio.
    Opus,
    /// No audio track in the exported video.
    NoAudio,
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum VideoViewMode {
    /// Keep the spectrogram static, only move the playhead line.
    #[default]
    StaticPlayhead,
    /// Scroll the spectrogram so the playhead stays near the left quarter.
    ScrollingView,
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum SpectrogramDisplay {
    #[default]
    FlowOptical,
    PhaseCoherence,
    FlowCentroid,
    FlowGradient,
    Phase,
}

impl SpectrogramDisplay {
    pub fn flow_algo(self) -> FlowAlgo {
        match self {
            Self::FlowOptical => FlowAlgo::Optical,
            Self::PhaseCoherence => FlowAlgo::PhaseCoherence,
            Self::FlowCentroid => FlowAlgo::Centroid,
            Self::FlowGradient => FlowAlgo::Gradient,
            Self::Phase => FlowAlgo::Phase,
        }
    }
}

// FlowColorScheme is defined in oversample-core and re-exported here for backward compatibility.
pub use oversample_core::types::FlowColorScheme;

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum RightSidebarTab {
    #[default]
    Metadata,
    Selection,
    Psd,
    Analysis,
    Harmonics,
    Notch,
    Pulses,
    DebugLog,
}

impl RightSidebarTab {
    pub fn label(self) -> &'static str {
        match self {
            Self::Metadata => "Info",
            Self::Selection => "Annotations",
            Self::Psd => "Power spectrum",
            Self::Analysis => "Analysis",
            Self::Harmonics => "Harmonics (beta)",
            Self::Notch => "Noise Filter",
            Self::Pulses => "Pulses",
            Self::DebugLog => "Debug Log",
        }
    }

    pub const ALL: &'static [RightSidebarTab] = &[
        Self::Metadata,
        Self::Selection,
        Self::Psd,
        Self::Analysis,
        Self::Harmonics,
        Self::Notch,
        Self::Pulses,
        Self::DebugLog,
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum FilterQuality {
    #[default]
    Fast,
    Spectral,
}

// ── New enums ────────────────────────────────────────────────────────────────

/// Bandpass filter mode: Auto (from FF), Off, or On (manual).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum BandpassMode {
    #[default]
    Auto,
    Off,
    On,
}

/// Whether the bandpass frequency range follows the Focus or is set independently.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum BandpassRange {
    #[default]
    FollowFocus,
    Custom,
}

/// Which spectrogram overlay handle is being dragged / hovered.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SpectrogramHandle {
    FfUpper,       // FF upper boundary
    FfLower,       // FF lower boundary
    FfMiddle,      // FF midpoint (transpose whole range)
    HetCenter,     // HET center freq
    HetBandUpper,  // HET upper band edge
    HetBandLower,  // HET lower band edge
}

/// How the Play button initiates playback.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum PlayStartMode {
    #[default]
    Auto,      // automatically choose: Selected > FromHere > All
    All,       // play from start of file
    FromHere,  // play from current scroll position
    Selected,  // play selection (falls back to All if no selection)
}

impl PlayStartMode {
    /// Whether this mode uses "from-here" scrolling (negative scroll allowed, "here" marker shown).
    pub fn uses_from_here(&self) -> bool {
        matches!(self, PlayStartMode::FromHere | PlayStartMode::Auto)
    }
}

/// What happens when the Record button is pressed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RecordMode {
    ToFile,      // save to filesystem (Tauri only)
    ToMemory,    // keep in browser memory
    ListenOnly,  // grey out record, user can only listen
}

/// Active interaction tool for the main spectrogram canvas.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum CanvasTool {
    #[default]
    Hand,      // drag to pan
    Selection, // drag to select
}

/// Position of a resize handle on an annotation bounding box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ResizeHandlePosition {
    TopLeft, Top, TopRight,
    Left, Right,
    BottomLeft, Bottom, BottomRight,
}

/// What the overview strip shows.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum OverviewView {
    Spectrogram,
    #[default]
    Waveform,
}

/// What the main panel displays.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum MainView {
    #[default]
    Spectrogram,
    XformedSpec,
    Waveform,
    ZcChart,
    Flow,
    Chromagram,
}

impl MainView {
    pub fn label(self) -> &'static str {
        match self {
            Self::Spectrogram => "Spectrogram",
            Self::XformedSpec => "Transformed Spec",
            Self::Waveform => "Waveform",
            Self::ZcChart => "ZC Chart",
            Self::Flow => "Flow",
            Self::Chromagram => "Chromagram",
        }
    }

    pub fn short_label(self) -> &'static str {
        match self {
            Self::Spectrogram => "Spec",
            Self::XformedSpec => "Xform S",
            Self::Waveform => "Wave",
            Self::ZcChart => "ZC",
            Self::Flow => "Flow",
            Self::Chromagram => "Chroma",
        }
    }

    /// Whether this view mode uses the spectrogram renderer.
    pub fn is_spectrogram(self) -> bool {
        matches!(self, Self::Spectrogram | Self::XformedSpec | Self::Flow | Self::Chromagram)
    }

    pub const ALL: &'static [MainView] = &[
        Self::Spectrogram,
        Self::XformedSpec,
        Self::Waveform,
        Self::ZcChart,
        Self::Flow,
        Self::Chromagram,
    ];
}

// ── FFT mode ─────────────────────────────────────────────────────────────────

/// FFT window mode for spectrogram computation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FftMode {
    /// Fixed FFT size at all LOD levels (128–8192).
    Single(usize),
    /// Adaptive S: [1024, 1024, 512, 512, 256, 128]
    AdaptiveS,
    /// Adaptive M: [1024, 1024, 1024, 512, 512, 256]
    AdaptiveM,
    /// Adaptive L: [2048, 2048, 2048, 1024, 512, 512]
    AdaptiveL,
}

impl FftMode {
    /// Per-LOD FFT sizes for each adaptive mode. Index = LOD level (0–5).
    const ADAPTIVE_S: [usize; 6] = [1024, 1024, 512, 512, 256, 128];
    const ADAPTIVE_M: [usize; 6] = [1024, 1024, 1024, 512, 512, 256];
    const ADAPTIVE_L: [usize; 6] = [2048, 2048, 2048, 1024, 512, 512];

    /// The actual FFT size to use for a given LOD level (0–5).
    pub fn fft_for_lod(&self, lod: u8) -> usize {
        let idx = (lod as usize).min(5);
        match self {
            FftMode::Single(sz) => *sz,
            FftMode::AdaptiveS => Self::ADAPTIVE_S[idx],
            FftMode::AdaptiveM => Self::ADAPTIVE_M[idx],
            FftMode::AdaptiveL => Self::ADAPTIVE_L[idx],
        }
    }

    /// The maximum FFT size this mode will ever produce (across all LODs).
    /// Determines the output tile height: `max_fft() / 2 + 1` bins.
    pub fn max_fft_size(&self) -> usize {
        match self {
            FftMode::Single(sz) => *sz,
            FftMode::AdaptiveS => 1024,
            FftMode::AdaptiveM => 1024,
            FftMode::AdaptiveL => 2048,
        }
    }
}

/// Display filter mode: controls how each processing stage affects the spectrogram.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum DisplayFilterMode {
    /// Stage is disabled for display.
    #[default]
    Off,
    /// Automatic/smart defaults for display.
    Auto,
    /// Use same settings as playback.
    Same,
    /// Custom display-only settings (NR strength, Gain brightness).
    Custom,
}

impl DisplayFilterMode {
    pub const ALL: [DisplayFilterMode; 4] = [
        DisplayFilterMode::Off,
        DisplayFilterMode::Auto,
        DisplayFilterMode::Same,
        DisplayFilterMode::Custom,
    ];

    pub fn short_label(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Auto => "aut",
            Self::Same => "sam",
            Self::Custom => "cst",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Auto => "Auto",
            Self::Same => "Same",
            Self::Custom => "Custom",
        }
    }
}

/// Auto-gain strategy.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum GainMode {
    /// No gain adjustment at all.
    #[default]
    Off,
    /// Manual dB boost only (from slider).
    Manual,
    /// Peak normalization: scan first N seconds, boost so peak ≈ −3 dBFS.
    /// Manual slider adds on top.
    AutoPeak,
    /// AGC (Automatic Gain Control): smooth per-sample leveler that targets
    /// −3 dBFS with attack/release envelope following, noise gate, and limiter.
    /// Manual slider adds on top.
    Adaptive,
}

impl GainMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Manual => "Manual",
            Self::AutoPeak => "Peak",
            Self::Adaptive => "AGC",
        }
    }

    pub fn is_auto(self) -> bool {
        matches!(self, Self::AutoPeak | Self::Adaptive)
    }
}

/// Where the peak is measured for AutoPeak gain mode.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum PeakSource {
    /// Raw audio, first 30 seconds (or full file if shorter).
    #[default]
    First30s,
    /// Raw audio, entire file.
    FullWave,
    /// Raw audio, current selection range only.
    Selection,
    /// Post-DSP peak (after bandpass/HFR/NR chain), computed on demand.
    Processed,
}

impl PeakSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::First30s => "30s",
            Self::FullWave => "Full",
            Self::Selection => "Sel",
            Self::Processed => "DSP",
        }
    }
}

/// Which floating layer panel is currently open (only one at a time).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LayerPanel {
    HfrMode,
    Tool,
    FreqRange,
    MainView,
    PlayMode,
    RecordMode,
    Channel,
    Gain,
    SelectionCombo,
    ListenMode,
}

/// A navigation history entry (for overview back/forward buttons).
#[derive(Clone, Copy, Debug)]
pub struct NavEntry {
    pub scroll_offset: f64,
    pub zoom_level: f64,
}

/// A snapshot of a file's annotation set for undo/redo.
#[derive(Clone, Debug)]
pub struct UndoEntry {
    pub file_idx: usize,
    pub snapshot: Option<crate::annotations::AnnotationSet>,
}

/// Undo/redo stack for annotation operations.
#[derive(Clone, Debug, Default)]
pub struct UndoStack {
    pub undo: Vec<UndoEntry>,
    pub redo: Vec<UndoEntry>,
}

impl UndoStack {
    const MAX_SIZE: usize = 100;

    pub fn push_undo(&mut self, entry: UndoEntry) {
        self.undo.push(entry);
        if self.undo.len() > Self::MAX_SIZE {
            self.undo.remove(0);
        }
        self.redo.clear();
    }
}

/// A time-position bookmark created during or after playback.
#[derive(Clone, Copy, Debug)]
pub struct Bookmark {
    pub time: f64,
}


#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum ChromaColormap {
    Warm,
    #[default]
    PitchClass,
    Solid,
    Octave,
    Flow,
}

impl ChromaColormap {
    pub fn label(self) -> &'static str {
        match self {
            Self::PitchClass => "Pitch Class",
            Self::Warm => "Warm",
            Self::Solid => "Solid",
            Self::Octave => "Octave",
            Self::Flow => "Flow",
        }
    }

    pub const ALL: &'static [ChromaColormap] = &[
        Self::PitchClass,
        Self::Warm,
        Self::Solid,
        Self::Octave,
        Self::Flow,
    ];
}

/// Frequency range preset for the chromagram view.
///
/// Each preset defines which octaves to display. Octave numbering follows
/// scientific pitch notation extended upward: octave 0 starts at C0 (16.35 Hz),
/// octave 10 at C10 (~16.7 kHz), octave 13 at C13 (~134 kHz), etc.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum ChromaRange {
    /// C0–B9 (16 Hz – 16.7 kHz) — standard musical range.
    #[default]
    Musical,
    /// C0–B11 (16 Hz – 33.5 kHz) — audible + near-ultrasound.
    Audible,
    /// C0–B13 (16 Hz – 134 kHz) — includes bat echolocation range.
    Extended,
    /// C5–B14 (523 Hz – 268 kHz) — focused on ultrasound.
    Ultrasound,
    /// All octaves from C0 to the highest representable.
    Full,
}

impl ChromaRange {
    /// (min_octave, num_octaves) — which octave indices to include.
    pub fn octave_params(self) -> (usize, usize) {
        match self {
            Self::Musical    => (0, 10),   // oct 0–9
            Self::Audible    => (0, 12),   // oct 0–11
            Self::Extended   => (0, 14),   // oct 0–13
            Self::Ultrasound => (5, 10),   // oct 5–14
            Self::Full       => (0, 16),   // oct 0–15
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Musical    => "Musical (0\u{2013}17 kHz)",
            Self::Audible    => "Audible (0\u{2013}34 kHz)",
            Self::Extended   => "Extended (0\u{2013}134 kHz)",
            Self::Ultrasound => "Ultrasound (0.5\u{2013}268 kHz)",
            Self::Full       => "Full",
        }
    }

    pub const ALL: &'static [ChromaRange] = &[
        Self::Musical,
        Self::Audible,
        Self::Extended,
        Self::Ultrasound,
        Self::Full,
    ];
}

/// Style for frequency shield/flag color bars on the spectrogram edge.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum ShieldStyle {
    /// Single solid color per band based on the changing digit.
    #[default]
    Solid,
    /// Three-band resistor color encoding (heraldic bend shield).
    Resistor,
    /// No shields — frequency color bars are hidden.
    Off,
}

impl ShieldStyle {
    pub const ALL: [ShieldStyle; 3] = [Self::Solid, Self::Resistor, Self::Off];

    pub fn label(self) -> &'static str {
        match self {
            Self::Solid => "Solid",
            Self::Resistor => "Resistor bands",
            Self::Off => "Off",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Solid => "solid",
            Self::Resistor => "resistor",
            Self::Off => "off",
        }
    }

    pub fn from_key(s: &str) -> Self {
        match s {
            "resistor" => Self::Resistor,
            "off" => Self::Off,
            _ => Self::Solid,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum FileSortMode {
    #[default]
    GroupedAdded,
    AddOrder,
    ByName,
    ByDate,
    ByMetadataDate,
    Grouped,
    ByDateGrouped,
}

impl FileSortMode {
    pub const ALL: &[FileSortMode] = &[
        Self::GroupedAdded,
        Self::AddOrder,
        Self::ByName,
        Self::ByDate,
        Self::ByMetadataDate,
        Self::Grouped,
        Self::ByDateGrouped,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::GroupedAdded => "Grouped + added",
            Self::AddOrder => "Added",
            Self::ByName => "Name",
            Self::ByDate => "Date",
            Self::ByMetadataDate => "Meta date",
            Self::Grouped => "Grouped",
            Self::ByDateGrouped => "Date, grouped",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum LeftSidebarTab {
    #[default]
    Files,
    Project,
    Settings,
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum StatusLevel {
    #[default]
    Error,
    Info,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MicMode {
    Auto,
    Browser,
    Cpal,
    RawUsb,
}

/// Playback mode for live listening (like PlaybackMode but without TimeExpansion).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum ListenMode {
    #[default]
    Heterodyne,
    PitchShift,
    PhaseVocoder,
    ZeroCrossing,
    Normal,
    ReadyMic,
}

/// Microphone acquisition strategy.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MicStrategy {
    Ask,
    Selected,
    Browser,
    None,
}

/// Which backend is handling mic audio.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MicBackend {
    Browser,
    Cpal,
    RawUsb,
}

/// State of mic acquisition lifecycle.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum MicAcquisitionState {
    #[default]
    Idle,
    AwaitingChoice,
    Acquiring,
    Ready,
    Failed,
}

/// Pending mic action (what to do once mic is acquired).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MicPendingAction {
    Listen,
    Record,
}

/// Whether a recording is ready to begin.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum RecordReadyState {
    #[default]
    None,
    AwaitingConfirmation,
    Confirmed,
}

/// Mono or stereo channel mode for mic recording.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum ChannelMode {
    #[default]
    Mono,
    Stereo,
}

/// Information about the selected mic device.
#[derive(Clone, Debug)]
pub struct MicDeviceInfo {
    pub name: String,
    pub connection_type: String,
    pub supported_rates: Vec<u32>,
    pub supported_bit_depths: Vec<u16>,
    pub max_channels: u16,
}

/// GPS location fix for embedding in recording GUANO metadata.
#[derive(Clone, Debug)]
pub struct GpsLocation {
    pub latitude: f64,
    pub longitude: f64,
    pub elevation: Option<f64>,
    pub accuracy: Option<f64>,
}

// ── Loading progress ─────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq)]
pub enum LoadingStage {
    Decoding,
    Preview,
    Spectrogram(u16), // 0–100 %
    Finalizing,
    Streaming,
}

#[derive(Clone, Debug)]
pub struct LoadingEntry {
    pub id: u64,
    pub name: String,
    pub stage: LoadingStage,
}

// ── AppState ─────────────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
pub struct AppState {
    pub files: RwSignal<Vec<LoadedFile>>,
    pub current_file_index: RwSignal<Option<usize>>,
    pub file_sort_mode: RwSignal<FileSortMode>,
    pub show_file_previews: RwSignal<bool>,
    pub selection: RwSignal<Option<Selection>>,
    pub last_selection: RwSignal<Option<Selection>>,
    pub playback_mode: RwSignal<PlaybackMode>,
    pub het_frequency: RwSignal<f64>,
    pub te_factor: RwSignal<f64>,
    pub zoom_level: RwSignal<f64>,
    pub scroll_offset: RwSignal<f64>,
    pub is_playing: RwSignal<bool>,
    pub playhead_time: RwSignal<f64>,
    pub active_playback_selection: RwSignal<Option<Selection>>,
    pub loading_files: RwSignal<Vec<LoadingEntry>>,
    pub loading_next_id: RwSignal<u64>,
    pub ps_factor: RwSignal<f64>,
    pub pv_factor: RwSignal<f64>,
    pub pv_hq: RwSignal<bool>,
    pub zc_factor: RwSignal<f64>,
    pub het_interacting: RwSignal<bool>,
    pub is_dragging: RwSignal<bool>,
    pub spectrogram_display: RwSignal<SpectrogramDisplay>,
    pub flow_enabled: RwSignal<bool>,
    pub right_sidebar_tab: RwSignal<RightSidebarTab>,
    pub right_sidebar_collapsed: RwSignal<bool>,
    pub right_sidebar_width: RwSignal<f64>,
    pub right_sidebar_dropdown_open: RwSignal<bool>,
    pub flow_intensity_gate: RwSignal<f32>,
    pub flow_gate: RwSignal<f32>,
    pub flow_opacity: RwSignal<f32>,
    pub flow_shift_gain: RwSignal<f32>,
    pub flow_color_gamma: RwSignal<f32>,
    pub flow_color_scheme: RwSignal<FlowColorScheme>,
    pub min_display_freq: RwSignal<Option<f64>>,
    pub max_display_freq: RwSignal<Option<f64>>,
    pub mouse_freq: RwSignal<Option<f64>>,
    pub mouse_canvas_x: RwSignal<f64>,
    pub mouse_in_label_area: RwSignal<bool>,
    pub mouse_in_time_axis: RwSignal<bool>,
    pub label_hover_opacity: RwSignal<f64>,
    pub follow_cursor: RwSignal<bool>,
    pub follow_suspended: RwSignal<bool>,
    pub follow_visible_since: RwSignal<Option<f64>>,
    pub pre_play_scroll: RwSignal<f64>,
    pub user_panned_during_playback: RwSignal<bool>,
    // Filter EQ (driven by bandpass_mode effect)
    pub filter_enabled: RwSignal<bool>,
    pub filter_band_mode: RwSignal<u8>,
    pub filter_freq_low: RwSignal<f64>,
    pub filter_freq_high: RwSignal<f64>,
    pub filter_db_below: RwSignal<f64>,
    pub filter_db_selected: RwSignal<f64>,
    pub filter_db_harmonics: RwSignal<f64>,
    pub filter_db_above: RwSignal<f64>,
    pub filter_hovering_band: RwSignal<Option<u8>>,
    pub filter_quality: RwSignal<FilterQuality>,
    pub het_cutoff: RwSignal<f64>,
    pub sidebar_collapsed: RwSignal<bool>,
    pub sidebar_width: RwSignal<f64>,
    // Gain
    pub gain_db: RwSignal<f64>,
    /// Stashed gain_db for the other HFR state (swapped on HFR toggle).
    pub gain_db_stash: RwSignal<f64>,
    pub auto_gain: RwSignal<bool>,
    pub gain_mode: RwSignal<GainMode>,
    /// Remembers last auto-gain mode so toggle restores it (default: Adaptive).
    pub gain_mode_last_auto: RwSignal<GainMode>,
    /// Where to measure peak for AutoPeak gain mode.
    pub peak_source: RwSignal<PeakSource>,
    /// Cache for recently computed selection peak values.
    pub selection_peak_cache: RwSignal<crate::audio::peak::PeakCache>,
    /// Whether a peak scan is currently in progress (for UI indicator).
    pub peak_scanning: RwSignal<bool>,
    // Waveform view gain (visual only, independent of audio gain)
    pub wave_view_gain_db: RwSignal<f64>,
    pub wave_view_auto_gain: RwSignal<bool>,

    // Channel
    pub channel_view: RwSignal<ChannelView>,

    // ── New signals ──────────────────────────────────────────────────────────

    // Tool
    pub canvas_tool: RwSignal<CanvasTool>,

    // HFR (High Frequency Range) mode
    pub hfr_enabled: RwSignal<bool>,

    // Bandpass
    pub bandpass_mode: RwSignal<BandpassMode>,
    pub bandpass_range: RwSignal<BandpassRange>,

    // Overview
    pub overview_view: RwSignal<OverviewView>,

    // Navigation history (for back/forward buttons in overview)
    pub nav_history: RwSignal<Vec<NavEntry>>,
    pub nav_index: RwSignal<usize>,

    // Bookmarks
    pub bookmarks: RwSignal<Vec<Bookmark>>,
    pub show_bookmark_popup: RwSignal<bool>,

    // Play start mode (All / FromHere / Selected)
    pub play_start_mode: RwSignal<PlayStartMode>,

    // Record mode (ToFile / ToMemory / ListenOnly)
    pub record_mode: RwSignal<RecordMode>,

    // Play-from-here time (updated by Spectrogram on scroll/zoom change)
    pub play_from_here_time: RwSignal<f64>,

    // Tile system: incrementing this triggers a spectrogram redraw
    pub tile_ready_signal: RwSignal<u32>,

    /// Generation counter for background preload. Incremented when file/LOD changes
    /// to cancel stale preload jobs.
    pub bg_preload_gen: RwSignal<u32>,

    // Spectrogram display settings (applied at render time, no tile regen needed)
    /// dB floor (default -80.0). Values below this map to black.
    pub spect_floor_db: RwSignal<f32>,
    /// dB range (default 80.0). floor + range = ceiling.
    pub spect_range_db: RwSignal<f32>,
    /// Gamma curve (default 1.0 = linear). <1 = brighter darks, >1 = more contrast.
    pub spect_gamma: RwSignal<f32>,
    /// Additive dB gain offset (default 0.0).
    pub spect_gain_db: RwSignal<f32>,
    /// Show tile debug overlay (borders, LOD labels) on the spectrogram canvas.
    pub debug_tiles: RwSignal<bool>,
    /// FFT window mode for spectrogram computation.
    /// Single size or multi-resolution (different sizes per frequency band).
    pub spect_fft_mode: RwSignal<FftMode>,

    /// Enable reassignment spectrogram (sharper time-frequency localization).
    pub reassign_enabled: RwSignal<bool>,

    // Which floating layer panel is currently open
    pub layer_panel_open: RwSignal<Option<LayerPanel>>,

    // Actual pixel width of the main spectrogram canvas (written by Spectrogram, read by Overview)
    pub spectrogram_canvas_width: RwSignal<f64>,

    // Main panel view mode
    pub main_view: RwSignal<MainView>,

    // Spectrogram drag handles (FF + HET)
    pub spec_drag_handle: RwSignal<Option<SpectrogramHandle>>,
    pub spec_hover_handle: RwSignal<Option<SpectrogramHandle>>,

    // FF frequency range (0.0 = no FF active)
    pub ff_freq_lo: RwSignal<f64>,
    pub ff_freq_hi: RwSignal<f64>,

    // Per-parameter auto flags (true = computed from FF)
    pub het_freq_auto: RwSignal<bool>,
    pub het_cutoff_auto: RwSignal<bool>,
    pub te_factor_auto: RwSignal<bool>,
    pub ps_factor_auto: RwSignal<bool>,
    pub pv_factor_auto: RwSignal<bool>,

    /// Output frequency range to highlight on spectrogram (set by hover in HFR panel).
    pub output_freq_highlight: RwSignal<Option<(f64, f64)>>,

    // Microphone (independent listen + record)
    pub mic_listening: RwSignal<bool>,
    pub mic_recording: RwSignal<bool>,
    pub mic_sample_rate: RwSignal<u32>,
    pub mic_samples_recorded: RwSignal<usize>,
    pub mic_bits_per_sample: RwSignal<u16>,
    pub mic_max_sample_rate: RwSignal<u32>, // 0 = auto (device default)
    /// Maximum seconds of listen buffer to capture on long-press record.
    pub mic_preroll_buffer_secs: RwSignal<u32>,
    pub mic_mode: RwSignal<MicMode>,
    pub mic_supported_rates: RwSignal<Vec<u32>>, // actual rates from cpal device query
    /// File index of the currently-recording live file (None if not recording).
    /// Used to update the live file in-place during recording and finalization.
    pub mic_live_file_idx: RwSignal<Option<usize>>,
    /// Generation counter for the live processing loop.  Incremented each time
    /// `spawn_live_processing_loop` is called.  Older loops exit when they see
    /// they've been superseded, preventing duplicate-loop races (e.g. listen →
    /// record with no files open used to spawn two loops on the same file_index).
    pub mic_processing_gen: RwSignal<u32>,
    /// Number of pre-roll samples captured from the listen buffer when the user
    /// long-pressed record.  Zero = no pre-roll.  Used to write a WAV cue marker.
    pub mic_preroll_samples: RwSignal<usize>,
    /// Wall-clock time (Date.now()) when the long-press gesture started.
    /// Used to compensate for audio accumulated during the gesture hold period.
    pub mic_gesture_start_ms: RwSignal<Option<f64>>,
    /// Wall-clock time (Date.now()) when recording started, for timer display.
    pub mic_recording_start_time: RwSignal<Option<f64>>,
    /// Wrapping counter incremented by setInterval(100ms) while recording.
    pub mic_timer_tick: RwSignal<u32>,
    /// Current mic device name (populated on open or query).
    pub mic_device_name: RwSignal<Option<String>>,
    /// Connection type: "USB", "Internal", "Bluetooth", etc.
    pub mic_connection_type: RwSignal<Option<String>>,
    /// Whether GPS location embedding is enabled (privacy toggle, persisted).
    pub gps_location_enabled: RwSignal<bool>,
    /// GPS location acquired at recording start (cleared after finalization).
    pub recording_location: RwSignal<Option<GpsLocation>>,
    /// WiFi SSIDs where location embedding is suppressed (home networks, persisted).
    pub home_wifi_ssids: RwSignal<Vec<String>>,
    /// Whether to include phone model in recording metadata (privacy toggle, persisted, default true).
    pub device_model_enabled: RwSignal<bool>,
    /// Cached device manufacturer (e.g. "samsung"), fetched once on first recording. Android only.
    pub cached_device_make: RwSignal<Option<String>>,
    /// Cached device model (e.g. "SM-A556E"), fetched once on first recording. Android only.
    pub cached_device_model: RwSignal<Option<String>>,
    /// USB mic manufacturer name (from USB descriptors), if available.
    pub mic_manufacturer: RwSignal<Option<String>>,
    /// Whether a USB audio device is currently connected.
    pub mic_usb_connected: RwSignal<bool>,
    /// What Auto mode resolved to (Cpal or RawUsb). Ignored when mode is not Auto.
    pub mic_effective_mode: RwSignal<MicMode>,
    /// Target scroll offset during recording. The rAF animation loop interpolates
    /// scroll_offset toward this value for smooth waterfall scrolling.
    pub mic_recording_target_scroll: RwSignal<f64>,
    /// Rightmost spectrogram column with actual data during recording.
    /// Used to clip the canvas so partial tiles don't show black padding.
    pub mic_live_data_cols: RwSignal<usize>,
    /// True when a USB audio device is detected but lacks permission.
    /// Used to change Record/Listen button labels to "Allow USB mic".
    pub mic_needs_permission: RwSignal<bool>,
    /// User's preferred device name for mic input. None = use system default.
    pub mic_selected_device: RwSignal<Option<String>>,
    /// Whether the mic chooser modal dialog is visible.
    pub show_mic_chooser: RwSignal<bool>,
    /// Whether the privacy settings modal dialog is visible.
    pub show_privacy_settings: RwSignal<bool>,
    /// Whether the about dialog is visible.
    pub show_about: RwSignal<bool>,
    /// Peak audio level from mic (0.0..1.0).
    pub mic_peak_level: RwSignal<f32>,
    /// Mic acquisition strategy (Ask, Selected, Browser, None).
    pub mic_strategy: RwSignal<MicStrategy>,
    /// Which backend is handling mic audio.
    pub mic_backend: RwSignal<Option<MicBackend>>,
    /// State of mic acquisition lifecycle.
    pub mic_acquisition_state: RwSignal<MicAcquisitionState>,
    /// Pending mic action (Listen or Record).
    pub mic_pending_action: RwSignal<Option<MicPendingAction>>,
    /// Whether a recording is ready to begin.
    pub record_ready_state: RwSignal<RecordReadyState>,
    /// Whether the mic permission dialog has been shown.
    pub mic_permission_dialog_shown: RwSignal<bool>,
    /// Maximum bit depth for mic recording (0 = auto).
    pub mic_max_bit_depth: RwSignal<u16>,
    /// Mono or stereo channel mode for mic recording.
    pub mic_channel_mode: RwSignal<ChannelMode>,
    /// Information about the selected mic device.
    pub mic_device_info: RwSignal<Option<MicDeviceInfo>>,

    // Listen mode settings (independent from HFR file playback)
    pub listen_mode: RwSignal<ListenMode>,
    pub listen_het_frequency: RwSignal<f64>,
    pub listen_het_cutoff: RwSignal<f64>,
    /// Number of context chunks for PS/PV overlap-save buffering (2/4/8/16).
    pub listen_context_chunks: RwSignal<u32>,
    pub listen_bandpass_enabled: RwSignal<bool>,
    pub listen_bandpass_lo: RwSignal<f64>,
    pub listen_bandpass_hi: RwSignal<f64>,

    // Transient status message (e.g. permission errors)
    pub status_message: RwSignal<Option<String>>,
    pub status_level: RwSignal<StatusLevel>,

    // Debug log entries: (timestamp_ms, level, message)
    pub debug_log_entries: RwSignal<Vec<(f64, String, String)>>,

    // Platform detection
    pub is_mobile: RwSignal<bool>,
    pub is_tauri: bool,

    /// True when the browser viewport is pinch-zoomed in (visualViewport.scale > 1).
    /// Used to show a zoom-out button and disable custom pinch handlers.
    pub viewport_zoomed: RwSignal<bool>,
    /// Visual viewport position/size for placing the zoom-out button in the
    /// visible area when pinch-zoomed. (offset_top, offset_left, vp_width, scale)
    pub visual_viewport_rect: RwSignal<(f64, f64, f64, f64)>,

    // XC browser
    pub xc_browser_open: RwSignal<bool>,

    // (hfr_saved_* signals removed — now in FocusStack)

    // Axis drag (left axis frequency range selection)
    pub axis_drag_start_freq: RwSignal<Option<f64>>,
    pub axis_drag_current_freq: RwSignal<Option<f64>>,

    // Cursor time at mouse position (for bottom bar feedback)
    pub cursor_time: RwSignal<Option<f64>>,

    // Left sidebar settings page
    pub left_sidebar_tab: RwSignal<LeftSidebarTab>,

    // User colormap preference (when not overridden by HFR/flow)
    pub colormap_preference: RwSignal<Colormap>,
    // Chromagram colormap mode
    pub chroma_colormap: RwSignal<ChromaColormap>,
    // Chromagram display: gain boost in dB (0 = no boost, positive = amplify)
    pub chroma_gain: RwSignal<f32>,
    // Chromagram display: gamma curve (1.0 = linear)
    pub chroma_gamma: RwSignal<f32>,
    // Chromagram frequency range preset
    pub chroma_range: RwSignal<ChromaRange>,
    // Colormap preference used when HFR mode is active
    pub hfr_colormap_preference: RwSignal<Colormap>,
    // When false, the Range button is hidden at full range
    pub always_show_view_range: RwSignal<bool>,

    // Notch noise filtering
    pub notch_enabled: RwSignal<bool>,
    pub notch_bands: RwSignal<Vec<crate::dsp::notch::NoiseBand>>,
    pub notch_detecting: RwSignal<bool>,
    pub notch_profile_name: RwSignal<String>,
    pub notch_hovering_band: RwSignal<Option<usize>>,
    /// Harmonic suppression strength (0.0–1.0). Attenuates 2x and 3x harmonics of noise.
    pub notch_harmonic_suppression: RwSignal<f64>,

    // Spectral subtraction noise reduction
    pub noise_reduce_enabled: RwSignal<bool>,
    pub noise_reduce_strength: RwSignal<f64>,
    pub noise_reduce_floor: RwSignal<Option<crate::dsp::spectral_sub::NoiseFloor>>,
    pub noise_reduce_learning: RwSignal<bool>,

    // Pulse detection
    pub detected_pulses: RwSignal<Vec<crate::dsp::pulse_detect::DetectedPulse>>,
    pub pulse_overlay_enabled: RwSignal<bool>,
    pub selected_pulse_index: RwSignal<Option<usize>>,
    pub pulse_detecting: RwSignal<bool>,

    // File identity hashing
    /// Whether a full hash computation (Layer 3/4) is currently running.
    pub hash_computing: RwSignal<bool>,
    /// Generation counter for cancelling in-progress hash computations.
    pub hash_generation: RwSignal<u32>,

    // Annotations
    pub annotation_store: RwSignal<AnnotationStore>,
    pub annotations_dirty: RwSignal<bool>,
    pub selected_annotation_ids: RwSignal<Vec<AnnotationId>>,
    /// Anchor for shift-click range selection in annotation tree.
    pub last_clicked_annotation_id: RwSignal<Option<AnnotationId>>,
    /// When true, clicking an annotation pushes its frequency focus override.
    pub annotation_auto_focus: RwSignal<bool>,
    /// When true, export uses each region's own freq bounds for DSP; when false, uses global HFR.
    pub export_use_region_focus: RwSignal<bool>,
    /// Id of annotation currently being dragged in the sidebar tree.
    pub dragging_annotation_id: RwSignal<Option<AnnotationId>>,
    /// Drop target: (target_id, position) where position is "before", "after", or "inside" (for groups).
    pub drop_target: RwSignal<Option<(AnnotationId, String)>>,
    /// Undo/redo stack for annotation operations.
    pub undo_stack: RwSignal<UndoStack>,
    /// Active annotation resize drag: (annotation_id, handle position).
    pub annotation_drag_handle: RwSignal<Option<(AnnotationId, ResizeHandlePosition)>>,
    /// Hovered annotation resize handle (for cursor + highlight).
    pub annotation_hover_handle: RwSignal<Option<(AnnotationId, ResizeHandlePosition)>>,
    /// Snapshot of original bounds before resize drag: (time_start, time_end, freq_low, freq_high).
    pub annotation_drag_original: RwSignal<Option<(f64, f64, Option<f64>, Option<f64>)>>,
    /// Whether the annotation label editing panel is active in the selection combo button.
    pub annotation_editing: RwSignal<bool>,
    /// True when editing a just-created annotation (Escape = cancel/delete).
    pub annotation_is_new_edit: RwSignal<bool>,

    // Project
    /// Whether the Projects beta feature is enabled (persisted to localStorage).
    pub projects_enabled: RwSignal<bool>,
    /// Currently loaded .batproj project (None = no project open).
    pub current_project: RwSignal<Option<crate::project::BatProject>>,
    /// Whether the project has unsaved changes.
    pub project_dirty: RwSignal<bool>,
    /// Save status for UI feedback: "", "Saving...", "Saved"
    pub project_save_status: RwSignal<&'static str>,

    // Timeline
    /// Multi-selected file indices for timeline creation (separate from current_file_index).
    pub selected_file_indices: RwSignal<Vec<usize>>,
    /// Active timeline view (when Some, spectrogram/waveform render in timeline mode).
    pub active_timeline: RwSignal<Option<crate::timeline::TimelineView>>,
    /// Currently selected multitrack track label (None = primary/default).
    pub active_timeline_track: RwSignal<Option<String>>,

    // Display-affecting checkboxes (spectrogram intensity settings)
    pub display_auto_gain: RwSignal<bool>,
    pub display_eq: RwSignal<bool>,
    pub display_noise_filter: RwSignal<bool>,
    /// When true, spectrogram tiles are computed from DSP-transformed audio
    /// (same transform as playback mode: pitch shift, heterodyne, etc.)
    pub display_transform: RwSignal<bool>,
    // ZC saved display settings (restored when entering ZC; defaults: eq=true, noise=true)
    pub zc_saved_display_auto_gain: RwSignal<bool>,
    pub zc_saved_display_eq: RwSignal<bool>,
    pub zc_saved_display_noise_filter: RwSignal<bool>,
    // Normal saved display settings (restored when leaving ZC; defaults: all false)
    pub normal_saved_display_auto_gain: RwSignal<bool>,
    pub normal_saved_display_eq: RwSignal<bool>,
    pub normal_saved_display_noise_filter: RwSignal<bool>,

    // Independent gain signals for Xformed Spec view
    pub xform_spect_gain_db: RwSignal<f32>,
    pub xform_spect_floor_db: RwSignal<f32>,
    pub xform_spect_range_db: RwSignal<f32>,
    pub xform_spect_gamma: RwSignal<f32>,

    // Display DSP filter panel (per-stage control of spectrogram processing)
    pub display_filter_enabled: RwSignal<bool>,
    pub display_filter_eq: RwSignal<DisplayFilterMode>,
    pub display_filter_notch: RwSignal<DisplayFilterMode>,
    pub display_filter_nr: RwSignal<DisplayFilterMode>,
    pub display_filter_transform: RwSignal<DisplayFilterMode>,
    pub display_filter_gain: RwSignal<DisplayFilterMode>,
    /// Extra dB boost applied to spectrogram display from Auto/Same gain modes.
    pub display_gain_boost: RwSignal<f32>,
    // Decimation (downsample after DSP transform)
    pub display_filter_decimate: RwSignal<DisplayFilterMode>,
    /// Target decimation sample rate in Hz (used for Custom mode; Auto computes from transform).
    pub display_decimate_rate: RwSignal<u32>,
    /// Effective decimation target rate resolved from display_filter_decimate mode (0 = no decimation).
    pub display_decimate_effective: RwSignal<u32>,
    /// Browser's default audio output sample rate (detected from AudioContext, typically 44100 or 48000).
    pub browser_sample_rate: RwSignal<u32>,
    // Custom NR settings (display-only)
    pub display_nr_strength: RwSignal<f64>,
    // Auto-learned noise floor for display (computed from first ~500ms of file)
    pub display_auto_noise_floor: RwSignal<Option<crate::dsp::spectral_sub::NoiseFloor>>,

    // PSD (Power Spectral Density) panel
    pub psd_nfft: RwSignal<usize>,
    pub psd_apply_eq: RwSignal<bool>,
    pub psd_apply_notch: RwSignal<bool>,
    pub psd_apply_nr: RwSignal<bool>,
    /// Temporary frequency overlays from PSD hover: Vec<(freq_hz, label, color_css)>.
    pub psd_hover_freqs: RwSignal<Vec<(f64, String, String)>>,

    // Bat Book
    pub bat_book_open: RwSignal<bool>,
    /// Auto or Manual(region). Drives `bat_book_region` via an Effect.
    pub bat_book_mode: RwSignal<crate::bat_book::types::BatBookMode>,
    /// Effective region — set by the auto-resolve Effect or manual selection.
    /// Downstream code (manifest Memo, ref panel, etc.) reads this.
    pub bat_book_region: RwSignal<crate::bat_book::types::BatBookRegion>,
    /// Result of auto-resolution (None when in Manual mode).
    pub bat_book_auto_resolved: RwSignal<Option<crate::bat_book::types::AutoResolved>>,
    /// User's starred/favourite bat book regions.
    pub bat_book_favourites: RwSignal<Vec<crate::bat_book::types::BatBookRegion>>,
    /// Currently selected bat book entry IDs (supports multi-select via shift-click).
    pub bat_book_selected_ids: RwSignal<Vec<String>>,
    pub bat_book_ref_open: RwSignal<bool>,
    // (bat_book_saved_* signals removed — now in FocusStack)
    /// Last-clicked bat book entry ID, used for shift-click range selection.
    pub bat_book_last_clicked_id: RwSignal<Option<String>>,
    /// When true, selecting bat book entries pushes their frequency focus override.
    pub bat_book_auto_focus: RwSignal<bool>,

    // Timeline display: show wall-clock time instead of file-relative time
    pub show_clock_time: RwSignal<bool>,

    /// Frequency shield/flag color bar style (persisted to localStorage).
    pub shield_style: RwSignal<ShieldStyle>,

    /// Whether the analysis/status bar is visible (persisted to localStorage).
    pub show_status_bar: RwSignal<bool>,

    // Layered frequency focus stack
    pub focus_stack: RwSignal<crate::focus_stack::FocusStack>,

    // Clean view: hide all overlays while holding backtick
    pub clean_view: RwSignal<bool>,

    // Export UI
    /// Whether the export section is expanded/collapsed.
    pub export_section_open: RwSignal<bool>,
    /// Selected export format: WAV or MP4.
    pub export_format: RwSignal<ExportFormat>,
    /// Video export progress (0.0 to 1.0), None = not exporting.
    pub video_export_progress: RwSignal<Option<f64>>,
    /// Video export status message.
    pub video_export_status: RwSignal<Option<String>>,
    /// Set to true to request cancellation of an in-progress video export.
    pub video_export_cancel: RwSignal<bool>,
    /// Selected video resolution preset.
    pub video_resolution: RwSignal<VideoResolution>,
    /// Selected video codec.
    pub video_codec: RwSignal<VideoCodec>,
    /// Selected audio codec for video export.
    pub video_audio_codec: RwSignal<AudioCodecOption>,
    /// Video view mode: static playhead vs scrolling.
    pub video_view_mode: RwSignal<VideoViewMode>,
}

fn detect_tauri() -> bool {
    let Some(window) = web_sys::window() else { return false };
    js_sys::Reflect::get(&window, &wasm_bindgen::JsValue::from_str("__TAURI_INTERNALS__"))
        .map(|v| !v.is_undefined())
        .unwrap_or(false)
}

/// Returns true if the user-agent string indicates a mobile device.
/// This is a one-time check (UA doesn't change during the session).
fn detect_mobile_ua() -> bool {
    let Some(window) = web_sys::window() else { return false };
    if let Ok(ua) = window.navigator().user_agent() {
        let ua_lower = ua.to_lowercase();
        if ua_lower.contains("android") || ua_lower.contains("iphone") || ua_lower.contains("ipad") || ua_lower.contains("mobile") {
            return true;
        }
    }
    false
}

/// Returns true if the viewport width is below the mobile breakpoint.
pub fn is_mobile_viewport() -> bool {
    let Some(window) = web_sys::window() else { return false };
    if let Ok(w) = window.inner_width() {
        if let Some(w) = w.as_f64() {
            return w < 768.0;
        }
    }
    false
}

fn detect_mobile() -> bool {
    detect_mobile_ua() || is_mobile_viewport()
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

impl AppState {
    pub fn new() -> Self {
        let s = Self {
            files: RwSignal::new(Vec::new()),
            current_file_index: RwSignal::new(None),
            file_sort_mode: RwSignal::new(FileSortMode::AddOrder),
            show_file_previews: RwSignal::new(false),
            selection: RwSignal::new(None),
            last_selection: RwSignal::new(None),
            playback_mode: RwSignal::new(PlaybackMode::Normal),
            het_frequency: RwSignal::new(45_000.0),
            te_factor: RwSignal::new(10.0),
            zoom_level: RwSignal::new(1.0),
            scroll_offset: RwSignal::new(0.0),
            is_playing: RwSignal::new(false),
            playhead_time: RwSignal::new(0.0),
            active_playback_selection: RwSignal::new(None),
            loading_files: RwSignal::new(Vec::new()),
            loading_next_id: RwSignal::new(0),
            ps_factor: RwSignal::new(10.0),
            pv_factor: RwSignal::new(10.0),
            pv_hq: RwSignal::new(true),
            zc_factor: RwSignal::new(8.0),
            het_interacting: RwSignal::new(false),
            is_dragging: RwSignal::new(false),
            spectrogram_display: RwSignal::new(SpectrogramDisplay::FlowOptical),
            flow_enabled: RwSignal::new(false),
            right_sidebar_tab: RwSignal::new(RightSidebarTab::Metadata),
            right_sidebar_collapsed: RwSignal::new(true),
            right_sidebar_width: RwSignal::new(220.0),
            right_sidebar_dropdown_open: RwSignal::new(false),
            flow_intensity_gate: RwSignal::new(0.5),
            flow_gate: RwSignal::new(0.75),
            flow_opacity: RwSignal::new(0.75),
            flow_shift_gain: RwSignal::new(3.0),
            flow_color_gamma: RwSignal::new(1.0),
            flow_color_scheme: RwSignal::new(FlowColorScheme::default()),
            min_display_freq: RwSignal::new(None),
            max_display_freq: RwSignal::new(None),
            mouse_freq: RwSignal::new(None),
            mouse_canvas_x: RwSignal::new(0.0),
            mouse_in_label_area: RwSignal::new(false),
            mouse_in_time_axis: RwSignal::new(false),
            label_hover_opacity: RwSignal::new(0.0),
            follow_cursor: RwSignal::new(true),
            follow_suspended: RwSignal::new(false),
            follow_visible_since: RwSignal::new(None),
            pre_play_scroll: RwSignal::new(0.0),
            user_panned_during_playback: RwSignal::new(false),
            filter_enabled: RwSignal::new(false),
            filter_band_mode: RwSignal::new(3),
            filter_freq_low: RwSignal::new(20_000.0),
            filter_freq_high: RwSignal::new(60_000.0),
            filter_db_below: RwSignal::new(-40.0),
            filter_db_selected: RwSignal::new(0.0),
            filter_db_harmonics: RwSignal::new(-30.0),
            filter_db_above: RwSignal::new(-40.0),
            filter_hovering_band: RwSignal::new(None),
            filter_quality: RwSignal::new(FilterQuality::Spectral),
            het_cutoff: RwSignal::new(15_000.0),
            sidebar_collapsed: RwSignal::new(false),
            sidebar_width: RwSignal::new(220.0),
            gain_db: RwSignal::new(0.0),
            gain_db_stash: RwSignal::new(0.0),
            auto_gain: RwSignal::new(false),
            gain_mode: RwSignal::new(GainMode::Off),
            gain_mode_last_auto: RwSignal::new(GainMode::AutoPeak),
            peak_source: RwSignal::new(PeakSource::First30s),
            selection_peak_cache: RwSignal::new(crate::audio::peak::PeakCache::default()),
            peak_scanning: RwSignal::new(false),
            wave_view_gain_db: RwSignal::new(0.0),
            wave_view_auto_gain: RwSignal::new(false),

            channel_view: RwSignal::new(ChannelView::Stereo),

            // New
            canvas_tool: RwSignal::new(CanvasTool::Hand),
            hfr_enabled: RwSignal::new(false),
            bandpass_mode: RwSignal::new(BandpassMode::Auto),
            bandpass_range: RwSignal::new(BandpassRange::FollowFocus),
            overview_view: RwSignal::new(OverviewView::Waveform),
            nav_history: RwSignal::new(Vec::new()),
            nav_index: RwSignal::new(0),
            bookmarks: RwSignal::new(Vec::new()),
            show_bookmark_popup: RwSignal::new(false),
            play_start_mode: RwSignal::new(PlayStartMode::Auto),
            record_mode: RwSignal::new(if detect_tauri() { RecordMode::ToFile } else { RecordMode::ToMemory }),
            play_from_here_time: RwSignal::new(0.0),
            tile_ready_signal: RwSignal::new(0),
            bg_preload_gen: RwSignal::new(0),
            spect_floor_db: RwSignal::new(-120.0),
            spect_range_db: RwSignal::new(120.0),
            spect_gamma: RwSignal::new(1.0),
            spect_gain_db: RwSignal::new(0.0),
            debug_tiles: RwSignal::new(false),
            spect_fft_mode: RwSignal::new(FftMode::AdaptiveM),
            reassign_enabled: RwSignal::new(false),
            layer_panel_open: RwSignal::new(None),
            spectrogram_canvas_width: RwSignal::new(1000.0),
            main_view: RwSignal::new(MainView::Spectrogram),
            spec_drag_handle: RwSignal::new(None),
            spec_hover_handle: RwSignal::new(None),
            ff_freq_lo: RwSignal::new(0.0),
            ff_freq_hi: RwSignal::new(0.0),
            het_freq_auto: RwSignal::new(true),
            het_cutoff_auto: RwSignal::new(true),
            te_factor_auto: RwSignal::new(true),
            ps_factor_auto: RwSignal::new(true),
            pv_factor_auto: RwSignal::new(true),
            output_freq_highlight: RwSignal::new(None),
            mic_listening: RwSignal::new(false),
            mic_recording: RwSignal::new(false),
            mic_sample_rate: RwSignal::new(0),
            mic_samples_recorded: RwSignal::new(0),
            mic_bits_per_sample: RwSignal::new(16),
            mic_max_sample_rate: RwSignal::new(0),
            mic_preroll_buffer_secs: RwSignal::new(10),
            mic_mode: RwSignal::new(if detect_tauri() { MicMode::Auto } else { MicMode::Browser }),
            mic_supported_rates: RwSignal::new(Vec::new()),
            mic_live_file_idx: RwSignal::new(None),
            mic_processing_gen: RwSignal::new(0),
            mic_preroll_samples: RwSignal::new(0),
            mic_gesture_start_ms: RwSignal::new(None),
            mic_recording_start_time: RwSignal::new(None),
            mic_timer_tick: RwSignal::new(0),
            mic_device_name: RwSignal::new(None),
            mic_connection_type: RwSignal::new(None),
            gps_location_enabled: RwSignal::new({
                web_sys::window()
                    .and_then(|w| w.local_storage().ok().flatten())
                    .and_then(|ls| ls.get_item("oversample_gps_enabled").ok().flatten())
                    .map(|v| v == "true")
                    .unwrap_or(false)
            }),
            recording_location: RwSignal::new(None),
            home_wifi_ssids: RwSignal::new({
                web_sys::window()
                    .and_then(|w| w.local_storage().ok().flatten())
                    .and_then(|ls| ls.get_item("oversample_home_wifi").ok().flatten())
                    .map(|v| {
                        v.split('\n')
                            .filter(|s| !s.is_empty())
                            .map(|s| s.to_string())
                            .collect()
                    })
                    .unwrap_or_default()
            }),
            device_model_enabled: RwSignal::new({
                web_sys::window()
                    .and_then(|w| w.local_storage().ok().flatten())
                    .and_then(|ls| ls.get_item("oversample_device_model").ok().flatten())
                    .map(|v| v != "false")
                    .unwrap_or(true) // default on
            }),
            cached_device_make: RwSignal::new(None),
            cached_device_model: RwSignal::new(None),
            mic_manufacturer: RwSignal::new(None),
            mic_usb_connected: RwSignal::new(false),
            mic_effective_mode: RwSignal::new(if detect_tauri() { MicMode::Cpal } else { MicMode::Browser }),
            mic_recording_target_scroll: RwSignal::new(0.0),
            mic_live_data_cols: RwSignal::new(0),
            mic_needs_permission: RwSignal::new(false),
            mic_selected_device: RwSignal::new(None),
            show_mic_chooser: RwSignal::new(false),
            show_privacy_settings: RwSignal::new(false),
            show_about: RwSignal::new(false),
            mic_peak_level: RwSignal::new(0.0),
            mic_strategy: RwSignal::new(if detect_tauri() { MicStrategy::Ask } else { MicStrategy::Browser }),
            mic_backend: RwSignal::new(None),
            mic_acquisition_state: RwSignal::new(MicAcquisitionState::Idle),
            mic_pending_action: RwSignal::new(None),
            record_ready_state: RwSignal::new(RecordReadyState::None),
            mic_permission_dialog_shown: RwSignal::new(false),
            mic_max_bit_depth: RwSignal::new(0),
            mic_channel_mode: RwSignal::new(ChannelMode::Mono),
            mic_device_info: RwSignal::new(None),
            listen_mode: RwSignal::new(ListenMode::default()),
            listen_het_frequency: RwSignal::new(45_000.0),
            listen_het_cutoff: RwSignal::new(15_000.0),
            listen_context_chunks: RwSignal::new(4),
            listen_bandpass_enabled: RwSignal::new(false),
            listen_bandpass_lo: RwSignal::new(18_000.0),
            listen_bandpass_hi: RwSignal::new(96_000.0),
            status_message: RwSignal::new(None),
            status_level: RwSignal::new(StatusLevel::Error),
            debug_log_entries: RwSignal::new(Vec::new()),
            is_mobile: RwSignal::new(detect_mobile()),
            is_tauri: detect_tauri(),
            viewport_zoomed: RwSignal::new(false),
            visual_viewport_rect: RwSignal::new((0.0, 0.0, 0.0, 1.0)),
            xc_browser_open: RwSignal::new(false),
            axis_drag_start_freq: RwSignal::new(None),
            axis_drag_current_freq: RwSignal::new(None),
            cursor_time: RwSignal::new(None),
            left_sidebar_tab: RwSignal::new(LeftSidebarTab::default()),
            colormap_preference: RwSignal::new(Colormap::Viridis),
            chroma_colormap: RwSignal::new(ChromaColormap::PitchClass),
            chroma_gain: RwSignal::new(0.0),
            chroma_gamma: RwSignal::new(1.0),
            chroma_range: RwSignal::new(ChromaRange::Musical),
            hfr_colormap_preference: RwSignal::new(Colormap::Inferno),
            always_show_view_range: RwSignal::new(false),

            notch_enabled: RwSignal::new(false),
            notch_bands: RwSignal::new(Vec::new()),
            notch_detecting: RwSignal::new(false),
            notch_profile_name: RwSignal::new(String::new()),
            notch_hovering_band: RwSignal::new(None),
            notch_harmonic_suppression: RwSignal::new(0.0),

            noise_reduce_enabled: RwSignal::new(false),
            noise_reduce_strength: RwSignal::new(0.6),
            noise_reduce_floor: RwSignal::new(None),
            noise_reduce_learning: RwSignal::new(false),

            detected_pulses: RwSignal::new(Vec::new()),
            pulse_overlay_enabled: RwSignal::new(true),
            selected_pulse_index: RwSignal::new(None),
            pulse_detecting: RwSignal::new(false),

            hash_computing: RwSignal::new(false),
            hash_generation: RwSignal::new(0),

            annotation_store: RwSignal::new(AnnotationStore::default()),
            annotations_dirty: RwSignal::new(false),
            selected_annotation_ids: RwSignal::new(Vec::new()),
            last_clicked_annotation_id: RwSignal::new(None),
            annotation_auto_focus: RwSignal::new(false),
            export_use_region_focus: RwSignal::new(true),
            dragging_annotation_id: RwSignal::new(None),
            drop_target: RwSignal::new(None),
            undo_stack: RwSignal::new(UndoStack::default()),
            annotation_drag_handle: RwSignal::new(None),
            annotation_hover_handle: RwSignal::new(None),
            annotation_drag_original: RwSignal::new(None),
            annotation_editing: RwSignal::new(false),
            annotation_is_new_edit: RwSignal::new(false),

            projects_enabled: RwSignal::new({
                web_sys::window()
                    .and_then(|w: web_sys::Window| w.local_storage().ok().flatten())
                    .and_then(|ls: web_sys::Storage| ls.get_item("oversample_projects_enabled").ok().flatten())
                    .map(|v| v == "true")
                    .unwrap_or(false)
            }),
            current_project: RwSignal::new(None),
            project_dirty: RwSignal::new(false),
            project_save_status: RwSignal::new(""),

            selected_file_indices: RwSignal::new(Vec::new()),
            active_timeline: RwSignal::new(None),
            active_timeline_track: RwSignal::new(None),

            display_auto_gain: RwSignal::new(false),
            display_eq: RwSignal::new(false),
            display_noise_filter: RwSignal::new(false),
            display_transform: RwSignal::new(false),

            zc_saved_display_auto_gain: RwSignal::new(false),
            zc_saved_display_eq: RwSignal::new(true),
            zc_saved_display_noise_filter: RwSignal::new(true),
            normal_saved_display_auto_gain: RwSignal::new(false),
            normal_saved_display_eq: RwSignal::new(false),
            normal_saved_display_noise_filter: RwSignal::new(false),

            xform_spect_gain_db: RwSignal::new(0.0),
            xform_spect_floor_db: RwSignal::new(-120.0),
            xform_spect_range_db: RwSignal::new(120.0),
            xform_spect_gamma: RwSignal::new(1.0),

            display_filter_enabled: RwSignal::new(false),
            display_filter_eq: RwSignal::new(DisplayFilterMode::Off),
            display_filter_notch: RwSignal::new(DisplayFilterMode::Off),
            display_filter_nr: RwSignal::new(DisplayFilterMode::Auto),
            display_filter_transform: RwSignal::new(DisplayFilterMode::Off),
            display_filter_gain: RwSignal::new(DisplayFilterMode::Auto),
            display_filter_decimate: RwSignal::new(DisplayFilterMode::Auto),
            display_decimate_rate: RwSignal::new(48000),
            display_decimate_effective: RwSignal::new(0),
            browser_sample_rate: RwSignal::new(0),
            display_gain_boost: RwSignal::new(0.0),
            display_nr_strength: RwSignal::new(0.8),
            display_auto_noise_floor: RwSignal::new(None),

            psd_nfft: RwSignal::new(1024),
            psd_apply_eq: RwSignal::new(false),
            psd_apply_notch: RwSignal::new(false),
            psd_apply_nr: RwSignal::new(false),
            psd_hover_freqs: RwSignal::new(Vec::new()),

            bat_book_open: RwSignal::new(false),
            bat_book_mode: RwSignal::new({
                use crate::bat_book::types::{BatBookMode, BatBookRegion};
                let ls = web_sys::window()
                    .and_then(|w: web_sys::Window| w.local_storage().ok().flatten());
                // Try new-format key first
                let new_key = ls.as_ref()
                    .and_then(|s| s.get_item("oversample_bat_book_mode").ok().flatten());
                match new_key {
                    Some(k) => BatBookMode::from_storage_key(&k),
                    None => {
                        // Migration: check legacy key
                        let legacy = ls.as_ref()
                            .and_then(|s| s.get_item("oversample_bat_book_region").ok().flatten());
                        match legacy {
                            Some(k) => BatBookRegion::from_storage_key(&k)
                                .map(BatBookMode::Manual)
                                .unwrap_or(BatBookMode::Auto),
                            None => BatBookMode::Auto, // brand new user
                        }
                    }
                }
            }),
            bat_book_region: RwSignal::new(crate::bat_book::types::BatBookRegion::Global),
            bat_book_auto_resolved: RwSignal::new(None),
            bat_book_favourites: RwSignal::new({
                web_sys::window()
                    .and_then(|w: web_sys::Window| w.local_storage().ok().flatten())
                    .and_then(|ls: web_sys::Storage| ls.get_item("oversample_bat_book_favourites").ok().flatten())
                    .map(|v| {
                        v.split(',')
                            .filter_map(|k| crate::bat_book::types::BatBookRegion::from_storage_key(k.trim()))
                            .collect()
                    })
                    .unwrap_or_default()
            }),
            bat_book_selected_ids: RwSignal::new(Vec::new()),
            bat_book_ref_open: RwSignal::new(false),
            bat_book_last_clicked_id: RwSignal::new(None),
            bat_book_auto_focus: RwSignal::new(true),
            show_clock_time: RwSignal::new(false),
            shield_style: RwSignal::new({
                web_sys::window()
                    .and_then(|w| w.local_storage().ok().flatten())
                    .and_then(|ls| ls.get_item("oversample_shield_style").ok().flatten())
                    .map(|v| ShieldStyle::from_key(&v))
                    .unwrap_or_default()
            }),
            show_status_bar: RwSignal::new({
                web_sys::window()
                    .and_then(|w| w.local_storage().ok().flatten())
                    .and_then(|ls| ls.get_item("oversample_show_status_bar").ok().flatten())
                    .map(|v| v == "true")
                    .unwrap_or(false)
            }),
            focus_stack: RwSignal::new(crate::focus_stack::FocusStack::new()),
            clean_view: RwSignal::new(false),

            // Export UI
            export_section_open: RwSignal::new(false),
            export_format: RwSignal::new(ExportFormat::default()),
            video_export_progress: RwSignal::new(None),
            video_export_status: RwSignal::new(None),
            video_export_cancel: RwSignal::new(false),
            video_resolution: RwSignal::new(VideoResolution::default()),
            video_codec: RwSignal::new(VideoCodec::default()),
            video_audio_codec: RwSignal::new(AudioCodecOption::default()),
            video_view_mode: RwSignal::new(VideoViewMode::default()),
        };

        // On mobile, start with sidebar collapsed
        if s.is_mobile.get_untracked() {
            s.sidebar_collapsed.set(true);
        }

        s
    }

    /// Returns the single selected annotation ID, or None if zero or multiple are selected.
    pub fn selected_annotation_id(&self) -> Option<AnnotationId> {
        let ids = self.selected_annotation_ids.get();
        if ids.len() == 1 { Some(ids[0].clone()) } else { None }
    }

    pub fn current_file(&self) -> Option<LoadedFile> {
        let files = self.files.get();
        let idx = self.current_file_index.get()?;
        files.get(idx).cloned()
    }

    /// Push current scroll/zoom onto the navigation history stack.
    pub fn push_nav(&self) {
        let entry = NavEntry {
            scroll_offset: self.scroll_offset.get_untracked(),
            zoom_level: self.zoom_level.get_untracked(),
        };
        let idx = self.nav_index.get_untracked();
        self.nav_history.update(|hist| {
            hist.truncate(idx + 1);
            if hist.last().map(|e: &NavEntry| (e.scroll_offset - entry.scroll_offset).abs() < 0.05).unwrap_or(false) {
                return;
            }
            hist.push(entry);
            if hist.len() > 100 {
                hist.remove(0);
            }
        });
        let new_len = self.nav_history.get_untracked().len();
        self.nav_index.set(new_len.saturating_sub(1));
    }

    /// Snapshot the current file's annotation set onto the undo stack.
    /// Call this BEFORE making any annotation mutation.
    pub fn snapshot_annotations(&self) {
        let idx = match self.current_file_index.get_untracked() {
            Some(i) => i,
            None => return,
        };
        let store = self.annotation_store.get_untracked();
        let snapshot = store.sets.get(idx).cloned().flatten();
        self.undo_stack.update(|stack| {
            stack.push_undo(UndoEntry { file_idx: idx, snapshot });
        });
    }

    /// Undo the last annotation operation.
    pub fn undo_annotations(&self) {
        let entry = {
            let mut popped = None;
            self.undo_stack.update(|stack| {
                popped = stack.undo.pop();
            });
            match popped {
                Some(e) => e,
                None => return,
            }
        };

        // Save current state to redo before restoring
        let store = self.annotation_store.get_untracked();
        let current = store.sets.get(entry.file_idx).cloned().flatten();
        self.undo_stack.update(|stack| {
            stack.redo.push(UndoEntry { file_idx: entry.file_idx, snapshot: current });
        });

        // Restore the snapshot
        self.annotation_store.update(|store| {
            store.ensure_len(entry.file_idx + 1);
            store.sets[entry.file_idx] = entry.snapshot;
        });
        self.annotations_dirty.set(true);
    }

    /// Redo the last undone annotation operation.
    pub fn redo_annotations(&self) {
        let entry = {
            let mut popped = None;
            self.undo_stack.update(|stack| {
                popped = stack.redo.pop();
            });
            match popped {
                Some(e) => e,
                None => return,
            }
        };

        // Save current state to undo before restoring
        let store = self.annotation_store.get_untracked();
        let current = store.sets.get(entry.file_idx).cloned().flatten();
        self.undo_stack.update(|stack| {
            stack.undo.push(UndoEntry { file_idx: entry.file_idx, snapshot: current });
        });

        // Restore the snapshot
        self.annotation_store.update(|store| {
            store.ensure_len(entry.file_idx + 1);
            store.sets[entry.file_idx] = entry.snapshot;
        });
        self.annotations_dirty.set(true);
    }

    /// Whether there's something to undo.
    pub fn can_undo(&self) -> bool {
        !self.undo_stack.get().undo.is_empty()
    }

    /// Whether there's something to redo.
    pub fn can_redo(&self) -> bool {
        !self.undo_stack.get().redo.is_empty()
    }

    pub fn show_info_toast(&self, msg: impl Into<String>) {
        self.status_level.set(StatusLevel::Info);
        self.status_message.set(Some(msg.into()));
    }

    pub fn show_error_toast(&self, msg: impl Into<String>) {
        self.status_level.set(StatusLevel::Error);
        self.status_message.set(Some(msg.into()));
    }

    /// Start tracking a loading file. Returns a unique ID for updates.
    pub fn loading_start(&self, name: &str) -> u64 {
        let id = self.loading_next_id.get_untracked();
        self.loading_next_id.set(id + 1);
        self.loading_files.update(|v| {
            v.push(LoadingEntry {
                id,
                name: name.to_string(),
                stage: LoadingStage::Decoding,
            });
        });
        id
    }

    /// Update the stage for a loading file by ID.
    pub fn loading_update(&self, id: u64, stage: LoadingStage) {
        self.loading_files.update(|v| {
            if let Some(entry) = v.iter_mut().find(|e| e.id == id) {
                entry.stage = stage;
            }
        });
    }

    /// Remove a loading entry (finished or failed) and clear the loading_id on the file.
    pub fn loading_done(&self, id: u64) {
        self.loading_files.update(|v| v.retain(|e| e.id != id));
        self.files.update(|files| {
            if let Some(f) = files.iter_mut().find(|f| f.loading_id == Some(id)) {
                f.loading_id = None;
            }
        });
    }

    pub fn log_debug(&self, level: &str, msg: impl Into<String>) {
        let timestamp = js_sys::Date::now();
        let msg_str = msg.into();
        log::info!("[{}] {}", level, &msg_str);
        self.debug_log_entries.update(|entries| {
            entries.push((timestamp, level.to_string(), msg_str));
            if entries.len() > 500 {
                entries.drain(0..entries.len() - 500);
            }
        });
    }

    /// Temporarily suspend follow-cursor when the user scrolls or pans.
    /// Re-engagement happens automatically once the playhead is on-screen
    /// and 200ms have passed since the last scroll action.
    pub fn suspend_follow(&self) {
        if self.is_playing.get_untracked() {
            self.user_panned_during_playback.set(true);
        }
        if self.follow_cursor.get_untracked() && self.is_playing.get_untracked() {
            self.follow_suspended.set(true);
            self.follow_visible_since.set(Some(js_sys::Date::now()));
        }
    }

    pub fn compute_auto_gain(&self) -> f64 {
        let files = self.files.get();
        let idx = self.current_file_index.get();
        self.compute_auto_gain_inner(&files, idx)
    }

    /// Untracked version for use inside render Effects that already subscribe
    /// to `files` and `current_file_index`. Avoids redundant Vec clone + subscription.
    pub fn compute_auto_gain_untracked(&self) -> f64 {
        self.files.with_untracked(|files| {
            let idx = self.current_file_index.get_untracked();
            self.compute_auto_gain_inner(files, idx)
        })
    }

    fn compute_auto_gain_inner(&self, files: &[LoadedFile], idx: Option<usize>) -> f64 {
        let Some(file_index) = idx else { return 0.0 };
        let Some(file) = files.get(file_index) else { return 0.0 };

        let peak_db = match self.peak_source.get() {
            PeakSource::First30s => file.cached_peak_db,
            PeakSource::FullWave => {
                // Fall back to 30s peak while full scan is in progress.
                // If playing, prefer the 30s peak to avoid mid-play gain jumps
                // when the full scan completes.
                if self.is_playing.get_untracked() && file.cached_full_peak_db.is_none() {
                    file.cached_peak_db
                } else {
                    file.cached_full_peak_db.or(file.cached_peak_db)
                }
            }
            PeakSource::Selection => {
                self.lookup_selection_peak(file_index, file).or(file.cached_peak_db)
            }
            PeakSource::Processed => {
                // Post-DSP peak: for now fall back to raw peak.
                // Full implementation requires running the DSP chain on a sample window.
                file.cached_peak_db
            }
        };
        let Some(peak_db) = peak_db else { return 0.0 };
        // Cap at +60 dB to avoid extreme amplification of very quiet recordings
        (-3.0 - peak_db).min(60.0)
    }

    /// Look up cached selection peak, or trigger an async scan if not cached.
    /// Returns None if no selection or not yet computed.
    /// Does not start new scans while audio is playing to avoid mid-play gain jumps.
    fn lookup_selection_peak(&self, file_index: usize, file: &LoadedFile) -> Option<f64> {
        let sel = self.selection.get()?;
        let sr = file.audio.sample_rate as f64;
        let start_sample = (sel.time_start * sr) as u64;
        let end_sample = (sel.time_end * sr) as u64;
        if end_sample <= start_sample { return None; }

        let key = (file_index, start_sample, end_sample);

        // Check cache (reactive read so we re-run when cache updates)
        if let Some(&peak_db) = self.selection_peak_cache.get().get(&key) {
            return peak_db;
        }

        // Don't start new scans while playing — avoid mid-play gain jumps
        if self.is_playing.get_untracked() {
            return None;
        }

        // Not cached — kick off async scan
        crate::audio::peak::start_selection_peak_scan(
            *self, file_index, start_sample, end_sample,
        );

        // Return None for now; will re-run when cache is updated
        None
    }

    // ── Focus Stack helpers ─────────────────────────────────────────────

    /// Called by drag handles, axis drag, input fields.
    /// Updates the focus stack and syncs output signals immediately.
    pub fn set_ff_range(&self, lo: f64, hi: f64) {
        use crate::focus_stack::FocusRange;
        self.focus_stack.update(|s| {
            s.set_user_range(FocusRange::new(lo, hi));
        });
        self.sync_focus_outputs();
    }

    /// Set only the lower FF bound (for drag handles).
    pub fn set_ff_lo(&self, lo: f64) {
        let hi = self.ff_freq_hi.get_untracked();
        self.set_ff_range(lo, hi);
    }

    /// Set only the upper FF bound (for drag handles).
    pub fn set_ff_hi(&self, hi: f64) {
        let lo = self.ff_freq_lo.get_untracked();
        self.set_ff_range(lo, hi);
    }

    /// Push a bat book FF override. Enables HFR if not already on.
    pub fn push_bat_book_ff(&self, lo: f64, hi: f64) {
        use crate::focus_stack::{FocusRange, FocusSource};
        self.focus_stack.update(|s| {
            s.push_override(FocusSource::BatBook, FocusRange::new(lo, hi));
            if !s.hfr_enabled() {
                s.set_hfr_enabled(true);
            }
        });
        // Ensure playback mode is not Normal when HFR is on
        if self.playback_mode.get_untracked() == PlaybackMode::Normal {
            let saved = self.focus_stack.get_untracked().saved_playback_mode();
            self.playback_mode.set(saved.unwrap_or(PlaybackMode::PitchShift));
        }
        if self.bandpass_mode.get_untracked() == BandpassMode::Off {
            let saved = self.focus_stack.get_untracked().saved_bandpass_mode();
            self.bandpass_mode.set(saved.unwrap_or(BandpassMode::Auto));
        }
        self.sync_focus_outputs();
    }

    /// Pop the bat book FF override. Restores previous state if not adopted.
    pub fn pop_bat_book_ff(&self) {
        use crate::focus_stack::{FocusRange, FocusSource};
        let mut restore: Option<FocusRange> = None;
        self.focus_stack.update(|s| {
            restore = s.pop_override(FocusSource::BatBook);
        });
        if let Some(range) = restore {
            if !range.is_active() {
                // No active focus to restore — turn off HFR
                self.focus_stack.update(|s| s.set_hfr_enabled(false));
                self.playback_mode.set(PlaybackMode::Normal);
                self.bandpass_mode.set(BandpassMode::Off);
            }
        }
        // If adopted (restore is None): user range is correct, HFR stays as-is
        self.sync_focus_outputs();
    }

    /// Push an annotation FF override. Only for annotations with freq bounds.
    pub fn push_annotation_ff(&self, lo: f64, hi: f64) {
        use crate::focus_stack::{FocusRange, FocusSource};
        self.focus_stack.update(|s| {
            s.push_override(FocusSource::Annotation, FocusRange::new(lo, hi));
            if !s.hfr_enabled() {
                s.set_hfr_enabled(true);
            }
        });
        if self.playback_mode.get_untracked() == PlaybackMode::Normal {
            let saved = self.focus_stack.get_untracked().saved_playback_mode();
            self.playback_mode.set(saved.unwrap_or(PlaybackMode::PitchShift));
        }
        if self.bandpass_mode.get_untracked() == BandpassMode::Off {
            let saved = self.focus_stack.get_untracked().saved_bandpass_mode();
            self.bandpass_mode.set(saved.unwrap_or(BandpassMode::Auto));
        }
        self.sync_focus_outputs();
    }

    /// Pop the annotation FF override.
    pub fn pop_annotation_ff(&self) {
        use crate::focus_stack::{FocusRange, FocusSource};
        let mut restore: Option<FocusRange> = None;
        self.focus_stack.update(|s| {
            restore = s.pop_override(FocusSource::Annotation);
        });
        if let Some(range) = restore {
            if !range.is_active() && !self.focus_stack.get_untracked().has_override(FocusSource::BatBook) {
                self.focus_stack.update(|s| s.set_hfr_enabled(false));
                self.playback_mode.set(PlaybackMode::Normal);
                self.bandpass_mode.set(BandpassMode::Off);
            }
        }
        self.sync_focus_outputs();
    }

    /// Frequency bounds implied by the current annotation selection, if any.
    pub fn selected_annotation_focus_range(&self) -> Option<(f64, f64)> {
        let idx = self.current_file_index.get_untracked()?;
        let ids = self.selected_annotation_ids.get_untracked();
        if ids.is_empty() {
            return None;
        }

        let store = self.annotation_store.get_untracked();
        let set = store.sets.get(idx)?.as_ref()?;

        let mut freq_lo = f64::MAX;
        let mut freq_hi = f64::MIN;
        let mut found = false;

        for ann in &set.annotations {
            if !ids.contains(&ann.id) {
                continue;
            }

            let range = match &ann.kind {
                AnnotationKind::Region(region) => match (region.freq_low, region.freq_high) {
                    (Some(lo), Some(hi)) => Some((lo.min(hi), lo.max(hi))),
                    _ => None,
                },
                AnnotationKind::Measurement(measurement) => Some((
                    measurement.start_freq.min(measurement.end_freq),
                    measurement.start_freq.max(measurement.end_freq),
                )),
                _ => None,
            };

            if let Some((lo, hi)) = range {
                freq_lo = freq_lo.min(lo);
                freq_hi = freq_hi.max(hi);
                found = true;
            }
        }

        if found && freq_hi - freq_lo > 100.0 {
            Some((freq_lo, freq_hi))
        } else {
            None
        }
    }

    /// Keep the annotation focus override in sync with the current selection.
    pub fn sync_annotation_auto_focus(&self) {
        if !self.annotation_auto_focus.get_untracked() {
            self.pop_annotation_ff();
            return;
        }

        if let Some((lo, hi)) = self.selected_annotation_focus_range() {
            self.push_annotation_ff(lo, hi);
        } else {
            self.pop_annotation_ff();
        }
    }

    /// Toggle HFR on/off. Saves/restores playback mode, bandpass, and gain.
    pub fn toggle_hfr(&self) {
        // Swap gain_db between HFR-on and HFR-off so we don't blast eardrums
        let current_gain = self.gain_db.get_untracked();
        let stashed_gain = self.gain_db_stash.get_untracked();
        self.gain_db.set(stashed_gain);
        self.gain_db_stash.set(current_gain);

        let stack = self.focus_stack.get_untracked();
        if stack.hfr_enabled() {
            // Turning off: save current mode
            let current_mode = self.playback_mode.get_untracked();
            let current_bp = self.bandpass_mode.get_untracked();
            self.focus_stack.update(|s| {
                s.set_saved_playback_mode(Some(current_mode));
                s.set_saved_bandpass_mode(Some(current_bp));
                s.set_hfr_enabled(false);
            });
            self.bandpass_mode.set(BandpassMode::Off);
            self.playback_mode.set(PlaybackMode::Normal);
        } else {
            // Turning on
            self.focus_stack.update(|s| {
                s.set_hfr_enabled(true);
            });
            let stack = self.focus_stack.get_untracked();
            match stack.saved_playback_mode() {
                Some(mode) => self.playback_mode.set(mode),
                None => {
                    if self.playback_mode.get_untracked() == PlaybackMode::Normal {
                        self.playback_mode.set(PlaybackMode::PitchShift);
                    }
                }
            }
            self.bandpass_mode.set(
                stack.saved_bandpass_mode().unwrap_or(BandpassMode::Auto),
            );
        }
        self.sync_focus_outputs();
    }

    /// Sync the focus stack's effective range to the output signals
    /// (ff_freq_lo, ff_freq_hi, hfr_enabled).
    fn sync_focus_outputs(&self) {
        let stack = self.focus_stack.get_untracked();
        let eff = stack.effective_range();
        let hfr = stack.hfr_enabled();
        if self.ff_freq_lo.get_untracked() != eff.lo {
            self.ff_freq_lo.set(eff.lo);
        }
        if self.ff_freq_hi.get_untracked() != eff.hi {
            self.ff_freq_hi.set(eff.hi);
        }
        if self.hfr_enabled.get_untracked() != hfr {
            self.hfr_enabled.set(hfr);
        }
    }
}
