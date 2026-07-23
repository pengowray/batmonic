//! Characterizes the MP3 length-estimate divergence behind audit lows #8/#12.
//!
//! To avoid reading a multi-hundred-MB file just to learn its duration, the
//! streaming loader (`src/components/file_sidebar/streaming_load.rs`) probes only
//! the first 64 KB and calls `parse_mp3_header(first_64kb, full_file_size)` to get
//! an up-front length. For an MP3 **without** a Xing/Info header, the true frame
//! count is not recoverable from a 64 KB prefix — symphonia estimates it from the
//! bytes it can see (a proportional under-estimate) or, for very short prefixes,
//! the loader falls back to a 128 kbps bitrate guess (an over-estimate for higher
//! bitrates). Either way the estimate is wrong, and it drives the file's
//! `duration_secs`, spectrogram `total_columns`, and playback range until the
//! background decode reconciles it to the true count (the frontend fix). An MP3
//! **with** a Xing/Info header carries the exact count in its first frame, so the
//! same 64 KB parse is already correct.
//!
//! This test pins that loader behavior (host-testable). The frontend reconcile +
//! playback clamp that consume the corrected count touch Leptos signals / Web
//! Audio and are verified by compilation + reasoning, not here.
//!
//! Fixtures: ffmpeg/libmp3lame, 1 kHz tone, 48 kHz mono, CBR 320 kbps — one
//! written with `-write_xing 0` (header-less), one with the default Info tag.
//! They live in `tests/mp3_estimate/` (NOT `tests/fixtures/`) so they don't
//! perturb the decode-corpus golden snapshot.

use oversample_core::audio::loader::{load_audio, parse_mp3_header};
use std::path::{Path, PathBuf};

/// The streaming loader probes only the first 64 KB before estimating length.
const STREAMING_HEADER_BYTES: usize = 65536;
/// One MPEG-1 Layer III frame = 1152 samples; the Xing path is exact to within one.
const MP3_FRAME: u64 = 1152;

fn fixture(name: &str) -> Option<Vec<u8>> {
    let p: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/mp3_estimate")
        .join(name);
    std::fs::read(p).ok()
}

fn real_frames(bytes: &[u8]) -> u64 {
    load_audio(bytes).expect("decode").samples.len() as u64
}

/// `estimated_total_frames` from parsing only the first `n` bytes, but passing
/// the TRUE file size — exactly what the streaming loader does.
fn prefix_estimate(bytes: &[u8], n: usize) -> u64 {
    let head = &bytes[..n.min(bytes.len())];
    parse_mp3_header(head, bytes.len() as u64)
        .expect("parse header")
        .estimated_total_frames
}

#[test]
fn headerless_mp3_streaming_estimate_diverges_from_true_length() {
    let Some(bytes) = fixture("tone_48k_mono_noxing.mp3") else {
        eprintln!("SKIP: tone_48k_mono_noxing.mp3 missing");
        return;
    };
    // Must be well past the probe window, or there is no truncation to exercise.
    assert!(
        bytes.len() > STREAMING_HEADER_BYTES * 2,
        "fixture too small ({} B) to exercise the 64 KB truncation",
        bytes.len(),
    );

    let real = real_frames(&bytes);

    // Parsing the WHOLE file is accurate — the format itself is fine; only the
    // truncated prefix parse is the problem.
    let full = prefix_estimate(&bytes, bytes.len());
    assert_eq!(
        full, real,
        "full-file parse should match the decoded length"
    );

    // Parsing only the 64 KB the streaming loader sees is materially wrong.
    let est = prefix_estimate(&bytes, STREAMING_HEADER_BYTES);
    let off = (est as f64 - real as f64).abs() / real as f64;
    assert!(
        off > 0.25,
        "streaming 64 KB estimate should diverge >25% from the true length; \
         est={est}, real={real}, off={off:.2}",
    );
}

#[test]
fn xing_tagged_mp3_streaming_estimate_is_accurate() {
    let Some(bytes) = fixture("tone_48k_mono_xing.mp3") else {
        eprintln!("SKIP: tone_48k_mono_xing.mp3 missing");
        return;
    };
    let real = real_frames(&bytes);
    // The Xing/Info header in the first frame carries the exact count, so even
    // the 64 KB prefix parse is right (within one MP3 frame).
    let est = prefix_estimate(&bytes, STREAMING_HEADER_BYTES);
    let diff = (est as i64 - real as i64).unsigned_abs();
    assert!(
        diff <= MP3_FRAME,
        "Xing-tagged MP3 should parse its exact length from the header; \
         est={est}, real={real}, diff={diff}",
    );
}
