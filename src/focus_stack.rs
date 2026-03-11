use crate::state::{BandpassMode, PlaybackMode};

/// Identifies the source of a frequency focus override.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FocusSource {
    /// The user's own preference (drag handles, axis drag, input fields).
    /// This is the base layer -- always present, never popped.
    User,
    /// Bat book selection override.
    BatBook,
    /// Annotation selection override.
    Annotation,
}

impl FocusSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::User => "User",
            Self::BatBook => "Bat Book",
            Self::Annotation => "Annotation",
        }
    }
}

/// A snapshot of the frequency focus range.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FocusRange {
    pub lo: f64,
    pub hi: f64,
}

impl FocusRange {
    pub fn new(lo: f64, hi: f64) -> Self {
        Self { lo, hi }
    }

    pub fn inactive() -> Self {
        Self { lo: 0.0, hi: 0.0 }
    }

    pub fn is_active(&self) -> bool {
        self.hi > self.lo
    }
}

/// A single layer in the focus stack.
#[derive(Clone, Debug)]
pub struct FocusLayer {
    pub source: FocusSource,
    pub range: FocusRange,
    /// True when the user has manually modified this override's range.
    /// When adopted, the override becomes the user's preference and
    /// no restore happens on pop.
    pub adopted: bool,
}

/// Debug snapshot of a single layer, returned by `debug_layers()`.
pub struct DebugLayer {
    pub source: FocusSource,
    pub range: FocusRange,
    pub adopted: bool,
    /// True if this layer is currently determining the effective range.
    pub is_effective: bool,
}

/// Layered frequency focus state. Stored in a single `RwSignal<FocusStack>`.
///
/// The stack has a permanent "user" base layer and zero or more override layers
/// (BatBook, Annotation) on top. The effective range is the topmost layer when
/// HFR is enabled, or inactive (0/0) when HFR is off.
///
/// When the user manually modifies the FF range while an override is active,
/// the override is marked "adopted" — it won't restore the previous range
/// when popped.
#[derive(Clone, Debug)]
pub struct FocusStack {
    /// The user's base preference (always exists).
    user_range: FocusRange,
    /// Override layers. Later entries have higher priority.
    /// Invariant: no duplicate FocusSources.
    overrides: Vec<FocusLayer>,
    /// Whether HFR is enabled (orthogonal to the layer stack).
    hfr_enabled: bool,
    /// Saved playback mode for HFR restore.
    hfr_saved_playback_mode: Option<PlaybackMode>,
    /// Saved bandpass mode for HFR restore.
    hfr_saved_bandpass_mode: Option<BandpassMode>,
}

impl FocusStack {
    pub fn new() -> Self {
        Self {
            user_range: FocusRange::inactive(),
            overrides: Vec::new(),
            hfr_enabled: false,
            hfr_saved_playback_mode: None,
            hfr_saved_bandpass_mode: None,
        }
    }

    // ── Queries ──────────────────────────────────────────────────────────

    /// The effective FF range: highest-priority override, or user_range.
    /// Returns inactive (0/0) when HFR is off.
    pub fn effective_range(&self) -> FocusRange {
        if !self.hfr_enabled {
            return FocusRange::inactive();
        }
        self.effective_range_ignoring_hfr()
    }

    /// The range that *would* be effective if HFR were on.
    /// Useful for knowing what will be restored when HFR is re-enabled.
    pub fn effective_range_ignoring_hfr(&self) -> FocusRange {
        self.overrides
            .last()
            .map(|l| l.range)
            .unwrap_or(self.user_range)
    }

    /// The user's own range (what would be active if all overrides were removed).
    pub fn user_range(&self) -> FocusRange {
        self.user_range
    }

    pub fn hfr_enabled(&self) -> bool {
        self.hfr_enabled
    }

    /// Is a particular source currently active as an override?
    pub fn has_override(&self, source: FocusSource) -> bool {
        self.overrides.iter().any(|l| l.source == source)
    }

    /// Is the topmost override (if any) adopted by the user?
    pub fn is_top_adopted(&self) -> bool {
        self.overrides.last().map_or(false, |l| l.adopted)
    }

    /// Is a specific override adopted?
    pub fn is_adopted(&self, source: FocusSource) -> bool {
        self.overrides
            .iter()
            .find(|l| l.source == source)
            .map_or(false, |l| l.adopted)
    }

    /// Get the topmost active source.
    pub fn active_source(&self) -> FocusSource {
        self.overrides
            .last()
            .map(|l| l.source)
            .unwrap_or(FocusSource::User)
    }

    /// For debug display: return all layers from bottom to top.
    pub fn debug_layers(&self) -> Vec<DebugLayer> {
        let effective_source = self.active_source();
        let mut out = vec![DebugLayer {
            source: FocusSource::User,
            range: self.user_range,
            adopted: false,
            is_effective: self.overrides.is_empty(),
        }];
        for layer in &self.overrides {
            out.push(DebugLayer {
                source: layer.source,
                range: layer.range,
                adopted: layer.adopted,
                is_effective: layer.source == effective_source,
            });
        }
        out
    }

    pub fn saved_playback_mode(&self) -> Option<PlaybackMode> {
        self.hfr_saved_playback_mode
    }

    pub fn saved_bandpass_mode(&self) -> Option<BandpassMode> {
        self.hfr_saved_bandpass_mode
    }

    // ── Mutations ────────────────────────────────────────────────────────

    /// User sets FF directly (drag handle, axis drag, input field).
    /// If an override is active, marks it as "adopted" (user took ownership).
    pub fn set_user_range(&mut self, range: FocusRange) {
        self.user_range = range;
        if let Some(top) = self.overrides.last_mut() {
            top.range = range; // keep override in sync with what user sees
            top.adopted = true;
        }
    }

    /// Push an override layer (bat book, annotation).
    /// If this source already exists, updates its range and resets adopted.
    pub fn push_override(&mut self, source: FocusSource, range: FocusRange) {
        if let Some(existing) = self.overrides.iter_mut().find(|l| l.source == source) {
            existing.range = range;
            existing.adopted = false;
        } else {
            self.overrides.push(FocusLayer {
                source,
                range,
                adopted: false,
            });
        }
    }

    /// Pop an override layer.
    /// Returns `Some(range_to_restore_to)` if the override was NOT adopted.
    /// Returns `None` if adopted (user already owns the range, no restore needed).
    pub fn pop_override(&mut self, source: FocusSource) -> Option<FocusRange> {
        let idx = self.overrides.iter().position(|l| l.source == source)?;
        let layer = self.overrides.remove(idx);
        if layer.adopted {
            // User modified the override — it's already their preference.
            None
        } else {
            // Restore to whatever the next layer down says.
            Some(self.effective_range_ignoring_hfr())
        }
    }

    /// Set HFR enabled/disabled.
    pub fn set_hfr_enabled(&mut self, enabled: bool) {
        self.hfr_enabled = enabled;
    }

    pub fn set_saved_playback_mode(&mut self, mode: Option<PlaybackMode>) {
        self.hfr_saved_playback_mode = mode;
    }

    pub fn set_saved_bandpass_mode(&mut self, mode: Option<BandpassMode>) {
        self.hfr_saved_bandpass_mode = mode;
    }
}
