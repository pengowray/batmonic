//! Characterization tests for audio decoding via
//! [`oversample_core::audio::loader::load_audio`].
//!
//! Part of the **Phase-0 refactor safety net** (1.0 foundation work). These pin
//! the *current* decode output so later refactors — notably unifying the native
//! Tauri decoder (`src-tauri/src/audio_decode.rs`) onto this shared one — cannot
//! silently change sample rate, channel mixing, or sample data.
//!
//! Two layers:
//!  - `corpus_decodes_with_invariants` — every fixture decodes and satisfies
//!    structural invariants (broad regression net).
//!  - `golden_snapshot_stable` — a deterministic subset is hashed and compared
//!    against a committed snapshot (catches subtle numeric drift).
//!
//! The golden snapshot is regenerable: delete
//! `tests/golden/decode_corpus.snapshot` or run with `UPDATE_GOLDEN=1`.
//!
//! Fixtures come from the `bat-demo-sounds` repo, expected as a sibling of the
//! batchi checkout at `../batchi-extras/bat-demo-sounds`. If it is not present,
//! the tests SKIP (printing a notice) rather than fail.

use oversample_core::audio::loader::load_audio;
use std::path::{Path, PathBuf};

/// Workspace root (one level above this crate).
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// An override for the bat-demo-sounds *repo root*, so its location isn't
/// hardcoded. Resolution order:
///   1. the `BAT_DEMO_SOUNDS_DIR` environment variable, then
///   2. the same key in a `.env` file at the workspace root.
/// Relative paths are resolved against the workspace root.
fn corpus_root_override() -> Option<PathBuf> {
    fn clean(v: &str) -> Option<String> {
        let v = v.trim().trim_matches('"').trim_matches('\'').trim();
        (!v.is_empty()).then(|| v.to_string())
    }
    let raw = std::env::var("BAT_DEMO_SOUNDS_DIR")
        .ok()
        .and_then(|v| clean(&v))
        .or_else(|| {
            let env_file = workspace_root().join(".env");
            let contents = std::fs::read_to_string(env_file).ok()?;
            contents.lines().find_map(|line| {
                let line = line.trim();
                if line.starts_with('#') {
                    return None;
                }
                let rest = line
                    .strip_prefix("BAT_DEMO_SOUNDS_DIR")?
                    .trim_start()
                    .strip_prefix('=')?;
                clean(rest)
            })
        })?;
    let p = PathBuf::from(&raw);
    Some(if p.is_absolute() {
        p
    } else {
        workspace_root().join(p)
    })
}

/// The bat-demo-sounds `sounds/` directory. Uses the override above when set,
/// otherwise the default sibling checkout `../batchi-extras/bat-demo-sounds`.
fn corpus_dir() -> PathBuf {
    corpus_root_override()
        .unwrap_or_else(|| workspace_root().join("../batchi-extras/bat-demo-sounds"))
        .join("sounds")
}

/// Small synthetic fixtures (ffmpeg-generated) covering formats the demo corpus
/// lacks: FLAC, OGG (Vorbis), and M4A (AAC). Committed under `tests/fixtures`.
fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// FNV-1a over the raw little-endian bytes of the decoded samples.
fn fnv1a(samples: &[f32]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for s in samples {
        for b in s.to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    h
}

fn is_audio(p: &Path) -> bool {
    matches!(
        p.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref(),
        Some("wav" | "mp3" | "w4v" | "flac" | "ogg" | "m4a")
    )
}

fn read_audio_dir(dir: &Path) -> Vec<PathBuf> {
    match std::fs::read_dir(dir) {
        Ok(rd) => rd
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| is_audio(p))
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// All decodable fixtures (demo corpus + synthetic format fixtures), sorted by
/// path for determinism.
fn audio_files() -> Vec<PathBuf> {
    let mut v = read_audio_dir(&corpus_dir());
    v.extend(read_audio_dir(&fixtures_dir()));
    v.sort();
    v
}

#[test]
#[ignore = "exhaustive 312-fixture sweep (~6 min in debug); run with `--ignored` \
            or the nightly safety-net job. The fast golden-snapshot test is the routine net."]
fn corpus_decodes_with_invariants() {
    let files = audio_files();
    if files.is_empty() {
        eprintln!(
            "SKIP corpus_decodes_with_invariants: no fixtures at {:?} \
             (bat-demo-sounds repo not present at ../batchi-extras?)",
            corpus_dir()
        );
        return;
    }

    let mut failures: Vec<String> = Vec::new();
    let mut by_format: std::collections::BTreeMap<&'static str, usize> = Default::default();

    for path in &files {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                failures.push(format!("{name}: read error: {e}"));
                continue;
            }
        };
        match load_audio(&bytes) {
            Ok(a) => {
                *by_format.entry(a.metadata.format).or_insert(0) += 1;
                if a.sample_rate < 4_000 || a.sample_rate > 2_000_000 {
                    failures.push(format!("{name}: implausible sample_rate {}", a.sample_rate));
                }
                if a.channels == 0 || a.channels > 8 {
                    failures.push(format!("{name}: implausible channels {}", a.channels));
                }
                if a.samples.is_empty() {
                    failures.push(format!("{name}: decoded to zero samples"));
                }
                if a.samples.iter().any(|s| !s.is_finite()) {
                    failures.push(format!("{name}: non-finite sample(s)"));
                }
                // duration_secs is defined as mono_samples / sample_rate
                let expect = a.samples.len() as f64 / a.sample_rate.max(1) as f64;
                if (expect - a.duration_secs).abs() > 1e-6 * expect.max(1.0) {
                    failures.push(format!(
                        "{name}: duration_secs {} != samples/rate {expect}",
                        a.duration_secs
                    ));
                }
            }
            Err(e) => failures.push(format!("{name}: decode error: {e}")),
        }
    }

    eprintln!(
        "decoded {} fixtures across formats: {:?}",
        files.len(),
        by_format
    );
    assert!(
        failures.is_empty(),
        "decode invariant failures ({}):\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn golden_snapshot_stable() {
    let files = audio_files();
    if files.is_empty() {
        eprintln!("SKIP golden_snapshot_stable: no fixtures (bat-demo-sounds repo not present at ../batchi-extras?)");
        return;
    }

    // Deterministic subset: every non-wav fixture (rarer formats) plus the
    // first 12 wavs alphabetically. Stable as long as the submodule is.
    let mut wav_taken = 0;
    let mut lines: Vec<String> = Vec::new();
    for p in &files {
        let ext = p
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if ext == "wav" {
            if wav_taken >= 12 {
                continue;
            }
            wav_taken += 1;
        }
        let bytes = std::fs::read(p).unwrap();
        let a = load_audio(&bytes).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
        lines.push(format!(
            "{}|{}|{}|{}|{:016x}",
            p.file_name().unwrap().to_string_lossy(),
            a.sample_rate,
            a.channels,
            a.samples.len(),
            fnv1a(&a.samples)
        ));
    }
    lines.sort();
    let actual = lines.join("\n") + "\n";

    let snap = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/decode_corpus.snapshot");
    let regen = std::env::var_os("UPDATE_GOLDEN").is_some();
    if regen || !snap.exists() {
        std::fs::create_dir_all(snap.parent().unwrap()).unwrap();
        std::fs::write(&snap, &actual).unwrap();
        eprintln!(
            "WROTE golden snapshot ({} fixtures) -> {:?}",
            lines.len(),
            snap
        );
        return;
    }

    let expected = std::fs::read_to_string(&snap).unwrap();
    assert_eq!(
        actual, expected,
        "decode output drifted from golden snapshot.\n\
         If this change is intentional, regenerate with UPDATE_GOLDEN=1."
    );
}
