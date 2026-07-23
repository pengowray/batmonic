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

#[derive(Clone)]
struct TimelineSourceSegment {
    source: Arc<dyn AudioSource>,
    timeline_start_sample: u64,
    duration_samples: u64,
}

/// Virtual source that stitches multiple files onto one timeline-local sample axis.
/// Gaps between segments read as silence.
pub struct TimelineAudioSource {
    segments: Vec<TimelineSourceSegment>,
    sample_rate: u32,
    channels: u32,
    total_samples: u64,
}

impl std::fmt::Debug for TimelineAudioSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TimelineAudioSource")
            .field("segments", &self.segments.len())
            .field("sample_rate", &self.sample_rate)
            .field("channels", &self.channels)
            .field("total_samples", &self.total_samples)
            .finish()
    }
}

impl TimelineAudioSource {
    pub fn new(segments: Vec<(Arc<dyn AudioSource>, f64, f64)>, sample_rate: u32) -> Self {
        let mut mapped_segments = Vec::with_capacity(segments.len());
        let mut channels = 1u32;
        let mut total_samples = 0u64;

        for (source, timeline_offset_secs, duration_secs) in segments {
            let timeline_start_sample =
                (timeline_offset_secs * sample_rate as f64).round().max(0.0) as u64;
            let duration_samples = (duration_secs * sample_rate as f64).round().max(0.0) as u64;
            channels = channels.max(source.channel_count());
            total_samples =
                total_samples.max(timeline_start_sample.saturating_add(duration_samples));
            mapped_segments.push(TimelineSourceSegment {
                source,
                timeline_start_sample,
                duration_samples,
            });
        }

        Self {
            segments: mapped_segments,
            sample_rate,
            channels,
            total_samples,
        }
    }
}

impl AudioSource for TimelineAudioSource {
    fn total_samples(&self) -> u64 {
        self.total_samples
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn channel_count(&self) -> u32 {
        self.channels
    }

    fn read_samples(&self, channel: ChannelView, start: u64, buf: &mut [f32]) -> usize {
        if start >= self.total_samples || buf.is_empty() {
            return 0;
        }

        let n = buf.len().min((self.total_samples - start) as usize);
        buf[..n].fill(0.0);
        let request_end = start + n as u64;

        // Reverse iteration preserves TimelineView::file_at_time semantics: if
        // segments overlap, the earlier timeline segment wins.
        for seg in self.segments.iter().rev() {
            let seg_start = seg.timeline_start_sample;
            let seg_end = seg_start.saturating_add(seg.duration_samples);
            let overlap_start = start.max(seg_start);
            let overlap_end = request_end.min(seg_end);
            if overlap_end <= overlap_start {
                continue;
            }

            let dst_start = (overlap_start - start) as usize;
            let len = (overlap_end - overlap_start) as usize;
            let local_start = overlap_start - seg_start;
            let _ =
                seg.source
                    .read_samples(channel, local_start, &mut buf[dst_start..dst_start + len]);
        }

        n
    }

    fn is_fully_loaded(&self) -> bool {
        self.segments.iter().all(|seg| seg.source.is_fully_loaded())
    }

    fn as_contiguous(&self) -> Option<&[f32]> {
        None
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

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
    fn read_samples(&self, channel: ChannelView, start: u64, buf: &mut [f32]) -> usize;

    /// Convenience: read a region and return a Vec.
    fn read_region(&self, channel: ChannelView, start: u64, len: usize) -> Vec<f32> {
        let mut buf = vec![0.0f32; len];
        let n = self.read_samples(channel, start, &mut buf);
        buf.truncate(n);
        buf
    }

    /// Whether all samples are available in memory (small file / legacy mode).
    fn is_fully_loaded(&self) -> bool;

    /// Whether `total_samples()` is still an estimate (`true`) rather than the
    /// exact decoded length (`false`). Streaming MP3/OGG return `true` until
    /// decode reaches real EOF (their length starts as a bitrate guess);
    /// in-memory / fully-decoded sources are always exact.
    fn length_is_estimated(&self) -> bool {
        false
    }

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
        // Bound the read in u64 so a `start` past the buffer (or, on wasm32,
        // past usize::MAX) saturates to 0 instead of truncating into a
        // valid-looking offset. Only the final, provably in-range index is cast.
        let frames = self.frame_count() as u64;
        let avail = frames.saturating_sub(start);
        if ch >= channels {
            // Invalid channel index — return silence
            for s in buf.iter_mut() {
                *s = 0.0;
            }
            return (buf.len() as u64).min(avail) as usize;
        }
        let n = (buf.len() as u64).min(avail) as usize;
        if n == 0 {
            return 0;
        }
        let start = start as usize; // lossless: start < frames <= usize::MAX
        for i in 0..n {
            buf[i] = raw[(start + i) * channels + ch];
        }
        n
    }

    /// Read from the mono-mixed buffer.
    fn read_mono(&self, start: u64, buf: &mut [f32]) -> usize {
        // Bound the read in u64 (see `extract_channel`): a `start` at/after the
        // end saturates to 0 rather than wrapping on a 32-bit `usize`.
        let len = self.samples.len() as u64;
        let avail = len.saturating_sub(start);
        let n = (buf.len() as u64).min(avail) as usize;
        if n == 0 {
            return 0;
        }
        let start = start as usize; // lossless: start < len <= usize::MAX
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

    fn read_samples(&self, channel: ChannelView, start: u64, buf: &mut [f32]) -> usize {
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
                    let n = buf
                        .len()
                        .min(self.frame_count().saturating_sub(start as usize));
                    for s in buf[..n].iter_mut() {
                        *s = 0.0;
                    }
                    return n;
                }
                let raw = self.raw_samples.as_ref().unwrap();
                let start = start as usize;
                let channels = self.channels as usize;
                let frames = self.frame_count();
                let avail = frames.saturating_sub(start);
                let n = buf.len().min(avail);
                for (i, sample) in buf.iter_mut().enumerate().take(n) {
                    let base = (start + i) * channels;
                    *sample = raw[base] - raw[base + 1];
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
