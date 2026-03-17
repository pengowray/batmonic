//! AudioSource trait and implementations for sample access abstraction.
//!
//! This module provides the foundation for streaming large audio files.
//! Instead of requiring all samples in memory as `Arc<Vec<f32>>`, consumers
//! can use the `AudioSource` trait to read samples on demand.
//!
//! Currently only `InMemorySource` is implemented (wrapping the existing
//! `Arc<Vec<f32>>`). Future phases will add `StreamingWavSource` for files
//! that are too large to fit in WASM memory.

use std::sync::Arc;

/// Default analysis window in seconds.
/// Whole-file analysis operations (auto-gain, wSNR, bit analysis) should
/// default to scanning only this many seconds from the start of the file,
/// unless the user explicitly requests full-file analysis.
pub const DEFAULT_ANALYSIS_WINDOW_SECS: f64 = 30.0;

/// 64-bit sample position for large file support.
/// On wasm32, `usize` is 32 bits and cannot address more than ~4 billion
/// samples. Use `SamplePos` for all global sample position arithmetic.
pub type SamplePos = u64;

/// Channel selection for multi-channel files.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum ChannelView {
    /// Stereo playback (L+R to separate speakers), mono-mix for display.
    #[default]
    Stereo,
    /// Mono downmix of all channels (single channel for both display and playback).
    MonoMix,
    /// A specific channel by index (0 = first/left, 1 = second/right, etc.).
    Channel(u32),
    /// Left minus Right difference (stereo only).
    Difference,
}

/// Trait abstracting sample access for audio data.
///
/// All consumers should eventually target this instead of `Arc<Vec<f32>>`.
/// During the migration period, `AudioData` carries both `samples` (legacy)
/// and `source` (new) fields.
pub trait AudioSource: Send + Sync {
    /// Total number of per-channel sample frames.
    fn total_samples(&self) -> u64;

    /// Sample rate in Hz.
    fn sample_rate(&self) -> u32;

    /// Number of channels in the original file.
    fn channel_count(&self) -> u32;

    /// Duration in seconds.
    fn duration_secs(&self) -> f64 {
        self.total_samples() as f64 / self.sample_rate() as f64
    }

    /// Read decoded f32 samples for the given channel view into `buf`.
    ///
    /// `start` is a sample-frame index (not a byte offset).
    /// Returns the number of samples actually written to `buf` (may be less
    /// than `buf.len()` if the region extends past the end of the file).
    fn read_samples(
        &self,
        channel: ChannelView,
        start: u64,
        buf: &mut [f32],
    ) -> usize;

    /// Convenience: read a region and return a Vec.
    fn read_region(&self, channel: ChannelView, start: u64, len: usize) -> Vec<f32> {
        let mut buf = vec![0.0f32; len];
        let n = self.read_samples(channel, start, &mut buf);
        buf.truncate(n);
        buf
    }

    /// Whether all samples are available in memory (small file / legacy mode).
    fn is_fully_loaded(&self) -> bool;

    /// For backward compatibility: get a direct reference to in-memory mono samples.
    /// Returns `None` for streaming sources.
    fn as_contiguous(&self) -> Option<&[f32]>;

    /// Downcast support for accessing implementation-specific methods (e.g. prefetch).
    fn as_any(&self) -> &dyn std::any::Any;
}

/// In-memory audio source wrapping `Arc<Vec<f32>>`.
///
/// This is the zero-cost migration path: the existing mono-mixed sample
/// buffer is wrapped and exposed through the `AudioSource` trait. All
/// existing code can continue using `audio.samples` directly during the
/// transition period.
pub struct InMemorySource {
    /// Mono-mixed samples (current format, always populated).
    pub samples: Arc<Vec<f32>>,
    /// Original interleaved samples (for multi-channel access).
    /// `None` for mono files (where `samples` already contains the single channel).
    pub raw_samples: Option<Arc<Vec<f32>>>,
    /// Sample rate in Hz.
    pub sample_rate: u32,
    /// Original channel count before mono mixing.
    pub channels: u32,
}

impl std::fmt::Debug for InMemorySource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InMemorySource")
            .field("len", &self.samples.len())
            .field("sample_rate", &self.sample_rate)
            .field("channels", &self.channels)
            .field("has_raw", &self.raw_samples.is_some())
            .finish()
    }
}

impl InMemorySource {
    /// Get the number of per-channel frames.
    fn frame_count(&self) -> usize {
        self.samples.len()
    }

    /// Extract a single channel from raw interleaved samples.
    fn extract_channel(&self, ch: u32, start: u64, buf: &mut [f32]) -> usize {
        let raw = match &self.raw_samples {
            Some(r) => r,
            None => {
                // Mono file — Channel(0) is the same as MonoMix
                return self.read_mono(start, buf);
            }
        };
        let ch = ch as usize;
        let channels = self.channels as usize;
        if ch >= channels {
            // Invalid channel index — return silence
            for s in buf.iter_mut() { *s = 0.0; }
            return buf.len().min(self.frame_count().saturating_sub(start as usize));
        }
        let start = start as usize;
        let frames = self.frame_count();
        let avail = frames.saturating_sub(start);
        let n = buf.len().min(avail);
        for i in 0..n {
            buf[i] = raw[(start + i) * channels + ch];
        }
        n
    }

    /// Read from the mono-mixed buffer.
    fn read_mono(&self, start: u64, buf: &mut [f32]) -> usize {
        let start = start as usize;
        let avail = self.samples.len().saturating_sub(start);
        let n = buf.len().min(avail);
        buf[..n].copy_from_slice(&self.samples[start..start + n]);
        n
    }
}

impl AudioSource for InMemorySource {
    fn total_samples(&self) -> u64 {
        self.samples.len() as u64
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn channel_count(&self) -> u32 {
        self.channels
    }

    fn is_fully_loaded(&self) -> bool {
        true
    }

    fn read_samples(
        &self,
        channel: ChannelView,
        start: u64,
        buf: &mut [f32],
    ) -> usize {
        match channel {
            ChannelView::Stereo | ChannelView::MonoMix => self.read_mono(start, buf),
            ChannelView::Channel(ch) => {
                if self.channels == 1 {
                    self.read_mono(start, buf)
                } else {
                    self.extract_channel(ch, start, buf)
                }
            }
            ChannelView::Difference => {
                if self.channels < 2 || self.raw_samples.is_none() {
                    // Mono: difference is silence
                    let n = buf.len().min(self.frame_count().saturating_sub(start as usize));
                    for s in buf[..n].iter_mut() { *s = 0.0; }
                    return n;
                }
                let raw = self.raw_samples.as_ref().unwrap();
                let start = start as usize;
                let channels = self.channels as usize;
                let frames = self.frame_count();
                let avail = frames.saturating_sub(start);
                let n = buf.len().min(avail);
                for i in 0..n {
                    let base = (start + i) * channels;
                    buf[i] = raw[base] - raw[base + 1];
                }
                n
            }
        }
    }

    fn as_contiguous(&self) -> Option<&[f32]> {
        Some(&self.samples)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ChannelView {
    /// Short label for UI display.
    pub fn label(&self) -> &'static str {
        match self {
            ChannelView::Stereo => "Stereo",
            ChannelView::MonoMix => "L+R",
            ChannelView::Channel(0) => "L",
            ChannelView::Channel(1) => "R",
            ChannelView::Channel(n) => {
                // For channels beyond stereo, we can't return a static str easily.
                // This is a pragmatic compromise.
                match n {
                    2 => "Ch3",
                    3 => "Ch4",
                    _ => "Ch?",
                }
            }
            ChannelView::Difference => "L-R",
        }
    }
}
