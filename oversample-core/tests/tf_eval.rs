// SPDX-License-Identifier: GPL-3.0-only OR MIT OR Apache-2.0
//! Time–frequency evaluation harness.
//!
//! Objective, ground-truth metrics that compare how faithfully different
//! spectral methods (STFT vs the Resonate algorithm) render *synthetic* signals
//! whose true instantaneous frequency, amplitude, and timing are known by
//! construction. The point is to turn "is the image better?" into numbers we can
//! A/B offline, so resonator changes (per-bin alpha, focus-weighted density,
//! FFT-steered placement) get measured instead of eyeballed on the phone.
//!
//! It directly answers the "rises a little vs a little more" question: the
//! `dprime` metric renders many noisy realizations of two nearly-identical
//! signals and reports how separable they are *relative to* within-class jitter.
//!
//! ## Running
//!
//! Sanity tests (fast, run by default, guard against metric regressions):
//!   cargo test -p oversample-core --test tf_eval
//!
//! Human-readable comparison report (slower; prints a table):
//!   cargo test -p oversample-core --test tf_eval report -- --ignored --nocapture
//!
//! ## Reading the numbers
//! - `ridge_rms_hz`   — how far the brightest ridge sits from the true f(t). Lower better.
//! - `freq_spread_hz` — RMS bandwidth of a pure tone's energy. Lower = sharper in frequency.
//! - `time_spread_ms` — RMS temporal spread of a compact burst. Lower = sharper in time.
//! - `dprime`         — separability of two close signals vs noise jitter. Higher better.
//!
//! The headline tradeoff this surfaces: STFT forces ONE global time/frequency
//! compromise via `fft_size` (short window → sharp in time, blurry in frequency;
//! long window → the reverse). Resonators set frequency resolution per-bin via
//! `bandwidth_hz`, decoupled from the column rate — which is the lever the
//! roadmap is about to start exploiting.

use oversample_core::canvas::colors::magnitude_to_greyscale;
use oversample_core::dsp::fft::compute_stft_columns;
use oversample_core::dsp::resonators::{
    compute_resonator_columns, warmup_samples, ResonatorAlphaMode, ResonatorLayout,
};
use oversample_core::types::SpectrogramColumn;
use resonators::{alpha_from_tau, ResonatorBank, ResonatorConfig};
use std::path::Path;

// Ultrasonic sample rate representative of bat-recording gear (Nyquist 96 kHz),
// so test frequencies sit in the band the app actually analyzes and resonator
// Q = f/bw is realistic.
const SR: u32 = 192_000;
const NARROW_BW: f32 = 60.0; // narrowest resonator bandwidth used anywhere here
const TAU: f64 = std::f64::consts::TAU;

// ---------------------------------------------------------------------------
// Synthetic signals with known ground truth
// ---------------------------------------------------------------------------

struct Signal {
    samples: Vec<f32>,
    sr: u32,
    dur_s: f64,
}

fn n_for(dur_s: f64, sr: u32) -> usize {
    (dur_s * sr as f64).round() as usize
}

/// Constant-frequency tone — ground-truth IF is `freq` everywhere.
fn tone(sr: u32, freq: f64, dur_s: f64, amp: f32) -> Signal {
    let n = n_for(dur_s, sr);
    let w = TAU * freq / sr as f64;
    let samples = (0..n)
        .map(|i| (amp as f64 * (w * i as f64).sin()) as f32)
        .collect();
    Signal { samples, sr, dur_s }
}

/// Linear chirp from `f0` to `f1` over `dur_s` — ground-truth IF is
/// `f0 + (f1-f0) * t/dur` (instantaneous frequency = d(phase)/dt / 2π).
fn linear_chirp(sr: u32, f0: f64, f1: f64, dur_s: f64, amp: f32) -> Signal {
    let n = n_for(dur_s, sr);
    let k = (f1 - f0) / dur_s; // Hz/s
    let samples = (0..n)
        .map(|i| {
            let t = i as f64 / sr as f64;
            let phase = TAU * (f0 * t + 0.5 * k * t * t);
            (amp as f64 * phase.sin()) as f32
        })
        .collect();
    Signal { samples, sr, dur_s }
}

/// Gaussian-windowed tone burst centered at `t0_s` with envelope std `sigma_s`
/// — a temporally compact event for time-localization tests.
fn gauss_burst(sr: u32, carrier: f64, t0_s: f64, sigma_s: f64, dur_s: f64, amp: f32) -> Signal {
    let n = n_for(dur_s, sr);
    let w = TAU * carrier / sr as f64;
    let samples = (0..n)
        .map(|i| {
            let t = i as f64 / sr as f64;
            let env = (-0.5 * ((t - t0_s) / sigma_s).powi(2)).exp();
            (amp as f64 * env * (w * i as f64).sin()) as f32
        })
        .collect();
    Signal { samples, sr, dur_s }
}

/// Add reproducible white-ish noise (deterministic LCG, seedable) for the
/// discriminability metric — each realization is a slightly different draw.
fn with_noise(sig: &Signal, amp: f32, seed: u64) -> Signal {
    let mut state = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(1);
    let samples = sig
        .samples
        .iter()
        .map(|&s| {
            // xorshift64* → uniform in [-1,1)
            state ^= state >> 12;
            state ^= state << 25;
            state ^= state >> 27;
            let u = (state.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 / (1u64 << 53) as f64;
            s + amp * (2.0 * u - 1.0) as f32
        })
        .collect();
    Signal {
        samples,
        sr: sig.sr,
        dur_s: sig.dur_s,
    }
}

// ---------------------------------------------------------------------------
// Render adapters → a common Spectro (calls the REAL app compute paths)
// ---------------------------------------------------------------------------

struct Spectro {
    cols: Vec<Vec<f32>>, // [col][bin] magnitude
    col_time: Vec<f64>,  // effective center time of each column, seconds
    n_bins: usize,
    fft_size: usize,
    sr: u32,
}

impl Spectro {
    fn bin_hz(&self, bin: usize) -> f64 {
        bin as f64 * self.sr as f64 / self.fft_size as f64
    }
    fn freq_to_bin(&self, f: f64) -> f64 {
        f * self.fft_size as f64 / self.sr as f64
    }
}

fn from_columns(
    cols_in: Vec<SpectrogramColumn>,
    fft_size: usize,
    sr: u32,
    time_shift_s: f64,
) -> Spectro {
    let n_bins = fft_size / 2 + 1;
    let mut cols = Vec::with_capacity(cols_in.len());
    let mut col_time = Vec::with_capacity(cols_in.len());
    for c in cols_in {
        col_time.push(c.time_offset + time_shift_s);
        cols.push(c.magnitudes);
    }
    Spectro {
        cols,
        col_time,
        n_bins,
        fft_size,
        sr,
    }
}

/// STFT render. `time_offset` from the compute path is the window *start*, so we
/// shift by +fft/2 samples to place each column at its window center.
fn render_stft(sig: &Signal, fft_size: usize, hop: usize) -> Spectro {
    let total = if sig.samples.len() >= fft_size {
        (sig.samples.len() - fft_size) / hop + 1
    } else {
        0
    };
    let cols = compute_stft_columns(&sig.samples, sig.sr, fft_size, hop, 0, total);
    from_columns(
        cols,
        fft_size,
        sig.sr,
        fft_size as f64 / 2.0 / sig.sr as f64,
    )
}

/// Resonator render. `fft_size` here only sets the output bin grid (rows) and
/// brightness scale; frequency resolution comes from `bw`. The column's
/// `time_offset` is end-of-hop ("now"); we leave it uncompensated, so the EMA's
/// group delay (~τ = 1/(2π·bw)) shows up honestly as a small time lag.
fn render_reso(
    sig: &Signal,
    fft_size: usize,
    hop: usize,
    bw: f32,
    layout: ResonatorLayout,
) -> Spectro {
    let total = sig.samples.len() / hop;
    let cols = compute_resonator_columns(
        &sig.samples,
        sig.sr,
        fft_size,
        hop,
        0,
        total,
        bw,
        ResonatorAlphaMode::ConstBandwidth,
        50.0,
        layout,
        None,
    );
    from_columns(cols, fft_size, sig.sr, 0.0)
}

// ---------------------------------------------------------------------------
// Metrics
// ---------------------------------------------------------------------------

/// Brightest bin within `search`, refined with parabolic peak interpolation so
/// we measure the method's ridge accuracy rather than bin quantization.
fn ridge_hz(col: &[f32], sr: u32, fft_size: usize, search: std::ops::Range<usize>) -> Option<f64> {
    let lo = search.start.max(1);
    let hi = search.end.min(col.len().saturating_sub(1));
    if lo >= hi {
        return None;
    }
    let mut k = lo;
    let mut best = col[lo];
    for i in lo..hi {
        if col[i] > best {
            best = col[i];
            k = i;
        }
    }
    if best <= 0.0 {
        return None;
    }
    let m0 = col[k - 1] as f64;
    let m1 = col[k] as f64;
    let m2 = col[k + 1] as f64;
    let denom = m0 - 2.0 * m1 + m2;
    let delta = if denom.abs() > 1e-12 {
        (0.5 * (m0 - m2) / denom).clamp(-0.5, 0.5)
    } else {
        0.0
    };
    Some((k as f64 + delta) * sr as f64 / fft_size as f64)
}

/// RMS error (Hz) between the brightest ridge and the true instantaneous
/// frequency, over columns whose center time falls in `[t_lo, t_hi]`.
fn ridge_rms_hz<F: Fn(f64) -> f64>(s: &Spectro, t_lo: f64, t_hi: f64, true_if: F) -> f64 {
    let mut sse = 0.0;
    let mut n = 0u32;
    for (ci, &t) in s.col_time.iter().enumerate() {
        if t < t_lo || t > t_hi {
            continue;
        }
        if let Some(f) = ridge_hz(&s.cols[ci], s.sr, s.fft_size, 1..s.n_bins) {
            let e = f - true_if(t);
            sse += e * e;
            n += 1;
        }
    }
    if n == 0 {
        f64::NAN
    } else {
        (sse / n as f64).sqrt()
    }
}

/// −3 dB (half-power) bandwidth (Hz) of the ridge, averaged over the eval
/// window. This is the textbook frequency-resolution measure: the *width of the
/// bright line* the eye reads, found by walking down each side of the peak to
/// half its power. Unlike `freq_spread_hz` (RMS) it ignores far-out tails, so it
/// is fair to single-pole resonators whose `1/Δf` skirts would otherwise swamp
/// an RMS integral. Lower ⇒ sharper line. (For a single-pole EMA this ≈ the
/// resonator `bandwidth_hz`; for a Hann STFT ≈ 1.44 · sr/fft_size.)
fn freq_bw3db_hz(s: &Spectro, t_lo: f64, t_hi: f64) -> f64 {
    let mut acc = 0.0;
    let mut n = 0u32;
    for (ci, &t) in s.col_time.iter().enumerate() {
        if t < t_lo || t > t_hi {
            continue;
        }
        let col = &s.cols[ci];
        // Peak bin (the ridge) over the full band.
        let mut k = 1usize;
        let mut best = col[1];
        for i in 1..s.n_bins - 1 {
            if col[i] > best {
                best = col[i];
                k = i;
            }
        }
        if best <= 0.0 {
            continue;
        }
        let half = (best as f64).powi(2) * 0.5;
        // Walk right to the half-power crossing, linearly interpolated in power.
        let right = {
            let mut j = k;
            while j + 1 < s.n_bins && (col[j + 1] as f64).powi(2) > half {
                j += 1;
            }
            if j + 1 < s.n_bins {
                let p0 = (col[j] as f64).powi(2);
                let p1 = (col[j + 1] as f64).powi(2);
                let frac = if (p0 - p1).abs() > 1e-20 {
                    ((p0 - half) / (p0 - p1)).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                s.bin_hz(j) + frac * (s.bin_hz(j + 1) - s.bin_hz(j))
            } else {
                s.bin_hz(j)
            }
        };
        let left = {
            let mut j = k;
            while j > 0 && (col[j - 1] as f64).powi(2) > half {
                j -= 1;
            }
            if j > 0 {
                let p0 = (col[j] as f64).powi(2);
                let p1 = (col[j - 1] as f64).powi(2);
                let frac = if (p0 - p1).abs() > 1e-20 {
                    ((p0 - half) / (p0 - p1)).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                s.bin_hz(j) - frac * (s.bin_hz(j) - s.bin_hz(j - 1))
            } else {
                s.bin_hz(j)
            }
        };
        acc += (right - left).max(0.0);
        n += 1;
    }
    if n == 0 {
        f64::NAN
    } else {
        acc / n as f64
    }
}

/// Power-weighted RMS bandwidth (Hz) of a column's spectrum, averaged over the
/// eval window. Unlike the −3 dB width this DOES include the skirts, so it
/// doubles as a *leakage* indicator: resonators have heavier single-pole tails
/// than a Hann window and score worse here even when their line is sharper.
fn freq_spread_hz(s: &Spectro, t_lo: f64, t_hi: f64) -> f64 {
    let mut acc = 0.0;
    let mut n = 0u32;
    for (ci, &t) in s.col_time.iter().enumerate() {
        if t < t_lo || t > t_hi {
            continue;
        }
        let col = &s.cols[ci];
        let mut sw = 0.0;
        let mut sfw = 0.0;
        for (k, &m) in col.iter().enumerate() {
            let p = (m as f64).powi(2);
            sw += p;
            sfw += p * s.bin_hz(k);
        }
        if sw <= 0.0 {
            continue;
        }
        let mean = sfw / sw;
        let mut sv = 0.0;
        for (k, &m) in col.iter().enumerate() {
            sv += (m as f64).powi(2) * (s.bin_hz(k) - mean).powi(2);
        }
        acc += (sv / sw).sqrt();
        n += 1;
    }
    if n == 0 {
        f64::NAN
    } else {
        acc / n as f64
    }
}

/// RMS temporal spread (ms) of band-limited energy around its centroid. For a
/// compact burst this is "how smeared the event is in time."
fn time_spread_ms(s: &Spectro, band_lo_hz: f64, band_hi_hz: f64) -> f64 {
    let blo = s.freq_to_bin(band_lo_hz).floor().max(0.0) as usize;
    let bhi = (s.freq_to_bin(band_hi_hz).ceil() as usize).min(s.n_bins);
    let e: Vec<f64> = s
        .cols
        .iter()
        .map(|col| (blo..bhi).map(|k| (col[k] as f64).powi(2)).sum())
        .collect();
    let tot: f64 = e.iter().sum();
    if tot <= 0.0 {
        return f64::NAN;
    }
    let centroid: f64 = e
        .iter()
        .zip(&s.col_time)
        .map(|(&ee, &t)| ee * t)
        .sum::<f64>()
        / tot;
    let var: f64 = e
        .iter()
        .zip(&s.col_time)
        .map(|(&ee, &t)| ee * (t - centroid).powi(2))
        .sum::<f64>()
        / tot;
    var.sqrt() * 1000.0
}

fn mean_std(v: &[f64]) -> (f64, f64) {
    if v.is_empty() {
        return (f64::NAN, f64::NAN);
    }
    let m = v.iter().sum::<f64>() / v.len() as f64;
    let var = v.iter().map(|x| (x - m).powi(2)).sum::<f64>() / v.len() as f64;
    (m, var.sqrt())
}

/// Flatten a spectro into a per-image vector normalized to unit peak, so image
/// distances aren't dominated by absolute brightness differences.
fn flatten_norm(s: &Spectro) -> Vec<f32> {
    let mut v: Vec<f32> = s.cols.iter().flatten().copied().collect();
    let max = v.iter().copied().fold(0.0f32, f32::max);
    if max > 0.0 {
        for x in &mut v {
            *x /= max;
        }
    }
    v
}

fn l2(a: &[f32], b: &[f32]) -> f64 {
    a.iter()
        .zip(b)
        .map(|(x, y)| ((*x - *y) as f64).powi(2))
        .sum::<f64>()
        .sqrt()
}

/// Discriminability d′ between two image sets: (mean between-class distance −
/// mean within-class distance) / pooled std. Higher ⇒ the method separates the
/// two signals more cleanly relative to the noise jitter within each class.
fn dprime(a: &[Vec<f32>], b: &[Vec<f32>]) -> f64 {
    let mut within = Vec::new();
    for i in 0..a.len() {
        for j in i + 1..a.len() {
            within.push(l2(&a[i], &a[j]));
        }
    }
    for i in 0..b.len() {
        for j in i + 1..b.len() {
            within.push(l2(&b[i], &b[j]));
        }
    }
    let mut between = Vec::new();
    for x in a {
        for y in b {
            between.push(l2(x, y));
        }
    }
    let (mw, sw) = mean_std(&within);
    let (mb, sb) = mean_std(&between);
    let pooled = ((sw * sw + sb * sb) / 2.0).sqrt();
    if pooled <= 0.0 {
        f64::NAN
    } else {
        (mb - mw) / pooled
    }
}

// ---------------------------------------------------------------------------
// Step-1 experiment: per-bin alpha schedules (built directly, bypassing the
// shipped global-bandwidth path, so we can measure the win BEFORE any churn)
// ---------------------------------------------------------------------------

/// Sum of pure tones (normalized) — for showing several lines at once.
fn multitone(sr: u32, freqs: &[f64], dur_s: f64, amp: f32) -> Signal {
    let n = n_for(dur_s, sr);
    let samples = (0..n)
        .map(|i| {
            let t = i as f64 / sr as f64;
            let s: f64 = freqs.iter().map(|&f| (TAU * f * t).sin()).sum();
            (amp as f64 * s / freqs.len() as f64) as f32
        })
        .collect();
    Signal { samples, sr, dur_s }
}

#[derive(Clone, Copy)]
enum AlphaSched {
    /// The shipped behavior: one global bandwidth (Hz) for every bin. Q = f/bw
    /// rises with frequency, so high-freq bins are needlessly slow.
    ConstBw(f32),
    /// Constant Q = f/bw: narrow (sharp in frequency) at low freq, wide (fast in
    /// time) at high freq — the natural per-bin tradeoff for FM bat calls.
    ConstQ(f32),
}

impl AlphaSched {
    fn label(&self) -> String {
        match self {
            AlphaSched::ConstBw(bw) => format!("ConstBw {bw:.0}Hz"),
            AlphaSched::ConstQ(q) => format!("ConstQ {q:.0}"),
        }
    }
}

/// Render resonators with an explicit per-bin alpha schedule, bypassing the
/// shipped global-bandwidth `compute_resonator_columns`. Linear output grid of
/// `fft_size/2+1` rows over 0..Nyquist (one resonator per row); magnitudes
/// scaled by `fft_size*0.5` to match app brightness. The point: prove a per-bin
/// tradeoff helps in the harness before wiring it into `build_reso_setup`.
fn render_reso_sched(sig: &Signal, fft_size: usize, hop: usize, sched: AlphaSched) -> Spectro {
    let sr = sig.sr as f32;
    let nyq = sr * 0.5;
    let n_bins = fft_size / 2 + 1;
    let denom = (n_bins - 1).max(1) as f32;
    let configs: Vec<ResonatorConfig> = (0..n_bins)
        .map(|k| {
            let f = (k as f32 * nyq / denom).max(0.01);
            let bw = match sched {
                AlphaSched::ConstBw(bw) => bw,
                AlphaSched::ConstQ(q) => (f / q).clamp(1.0, nyq * 0.99),
            };
            let tau = 1.0 / (std::f32::consts::TAU * bw);
            ResonatorConfig::new(f, alpha_from_tau(tau, sr), 1.0)
        })
        .collect();
    let mut bank = ResonatorBank::new(&configs, sr);
    let mag_scale = fft_size as f32 * 0.5;
    let total = sig.samples.len() / hop;
    let mut cols = Vec::with_capacity(total);
    let mut col_time = Vec::with_capacity(total);
    let mut pos = 0usize;
    for frame in 0..total {
        let next = pos + hop;
        if next > sig.samples.len() {
            break;
        }
        bank.process_samples(&sig.samples[pos..next]);
        pos = next;
        cols.push((0..n_bins).map(|k| bank.magnitude(k) * mag_scale).collect());
        col_time.push((frame + 1) as f64 * hop as f64 / sig.sr as f64);
    }
    Spectro {
        cols,
        col_time,
        n_bins,
        fft_size,
        sr: sig.sr,
    }
}

// ---------------------------------------------------------------------------
// Step-4 hybrid: FFT-steered combinations of spectrograms. All of these are
// per-pixel math on a shared linear grid — NO time-varying filter coefficients
// (so no Resonate "memory effect" risk) and NO renderer changes. The FFT is the
// cheap "where/what is the energy" map that steers the resonator detail.
// ---------------------------------------------------------------------------

/// Sum two signals (truncated to the shorter) — e.g. a sustained tone + a click.
fn mix(a: &Signal, b: &Signal) -> Signal {
    let n = a.samples.len().min(b.samples.len());
    let samples = (0..n).map(|i| a.samples[i] + b.samples[i]).collect();
    Signal {
        samples,
        sr: a.sr,
        dur_s: a.dur_s,
    }
}

/// For each target time, the index of the nearest column in `s` (both col_time
/// arrays are monotonically increasing, so a single forward sweep suffices).
fn nearest_cols(s: &Spectro, t_targets: &[f64]) -> Vec<usize> {
    let mut out = Vec::with_capacity(t_targets.len());
    let mut j = 0usize;
    for &t in t_targets {
        while j + 1 < s.col_time.len() && s.col_time[j + 1] <= t {
            j += 1;
        }
        let nearer_next =
            j + 1 < s.col_time.len() && (s.col_time[j + 1] - t).abs() < (s.col_time[j] - t).abs();
        out.push(if nearer_next { j + 1 } else { j });
    }
    out
}

/// Sample spectro `b` on `a`'s (time, frequency) grid — nearest column by time,
/// nearest bin by frequency. Lets us combine spectrograms computed at different
/// fft_size (e.g. a coarse-time gate FFT against a fine resonator bank).
fn resample_to(a: &Spectro, b: &Spectro) -> Vec<Vec<f32>> {
    let cmap = nearest_cols(b, &a.col_time);
    (0..a.cols.len())
        .map(|i| {
            let bc = &b.cols[cmap[i]];
            (0..a.n_bins)
                .map(|bin| {
                    let bb = (a.bin_hz(bin) * b.fft_size as f64 / b.sr as f64).round() as usize;
                    bc[bb.min(b.n_bins - 1)]
                })
                .collect()
        })
        .collect()
}

/// Element-wise fuse `b` into `a`'s grid via `op(a_val, b_val)`.
fn fuse(a: &Spectro, b: &Spectro, op: impl Fn(f32, f32) -> f32) -> Spectro {
    let bres = resample_to(a, b);
    let cols: Vec<Vec<f32>> = a
        .cols
        .iter()
        .zip(&bres)
        .map(|(ac, bc)| ac.iter().zip(bc).map(|(&x, &y)| op(x, y)).collect())
        .collect();
    Spectro {
        cols,
        col_time: a.col_time.clone(),
        n_bins: a.n_bins,
        fft_size: a.fft_size,
        sr: a.sr,
    }
}

/// Geometric mean — high only where BOTH agree, so it keeps the resonator's
/// sharp ridge (FFT also has energy there) while pulling down its leakage skirts
/// (FFT is dark there). Parameter-free.
fn geomean(x: f32, y: f32) -> f32 {
    (x.max(0.0) * y.max(0.0)).sqrt()
}

/// Per-pixel "tonalness" in [0,1] from an FFT spectro: a bin steady over time is
/// tonal (→1, favour the sharp/slow bank); a bin that jumps is transient (→0,
/// favour the fast bank). `k` scales sensitivity to change.
fn tonalness(f: &Spectro, k: f64) -> Vec<Vec<f32>> {
    let fmax = f
        .cols
        .iter()
        .flatten()
        .copied()
        .fold(0.0f32, f32::max)
        .max(1e-9) as f64;
    (0..f.cols.len())
        .map(|ci| {
            let prev = ci.saturating_sub(1);
            (0..f.n_bins)
                .map(|b| {
                    let d = (f.cols[ci][b] - f.cols[prev][b]).abs() as f64 / fmax;
                    (1.0 / (1.0 + k * d)) as f32
                })
                .collect()
        })
        .collect()
}

/// Adaptive hybrid: a short-FFT tonalness weight blends a SHARP bank (good
/// frequency precision) with a FAST bank (good time precision) per pixel — one
/// image with sharp lines on sustained tones AND crisp edges on transients,
/// which neither single bank can do alone. The gate FFT is short (good time
/// resolution) so it localizes transients tightly; its weights are upsampled to
/// the bank's fine bin grid.
fn adaptive_blend(
    sig: &Signal,
    fft_bank: usize,
    hop: usize,
    sharp: AlphaSched,
    fast: AlphaSched,
    k: f64,
) -> Spectro {
    let s = render_reso_sched(sig, fft_bank, hop, sharp);
    let fa = render_reso_sched(sig, fft_bank, hop, fast);
    let fft_img = render_stft(sig, 256, hop); // short window ⇒ tight transient detection
    let wgrid = Spectro {
        cols: tonalness(&fft_img, k),
        col_time: fft_img.col_time.clone(),
        n_bins: fft_img.n_bins,
        fft_size: fft_img.fft_size,
        sr: fft_img.sr,
    };
    let w = resample_to(&s, &wgrid);
    let faw = resample_to(&s, &fa);
    let cols: Vec<Vec<f32>> = (0..s.cols.len())
        .map(|i| {
            (0..s.n_bins)
                .map(|b| w[i][b] * s.cols[i][b] + (1.0 - w[i][b]) * faw[i][b])
                .collect()
        })
        .collect();
    Spectro {
        cols,
        col_time: s.col_time.clone(),
        n_bins: s.n_bins,
        fft_size: s.fft_size,
        sr: s.sr,
    }
}

/// Like `time_spread_ms` but isolates TRANSIENTS: subtract each bin's temporal
/// minimum (the steady baseline, e.g. a sustained tone's tail) before measuring
/// spread, so a click's time localization isn't diluted by a co-present tone.
fn time_spread_transient_ms(s: &Spectro, band_lo_hz: f64, band_hi_hz: f64) -> f64 {
    let blo = s.freq_to_bin(band_lo_hz).floor().max(0.0) as usize;
    let bhi = (s.freq_to_bin(band_hi_hz).ceil() as usize).min(s.n_bins);
    let mut base = vec![f32::INFINITY; s.n_bins];
    for col in &s.cols {
        for b in blo..bhi {
            base[b] = base[b].min(col[b]);
        }
    }
    let e: Vec<f64> = s
        .cols
        .iter()
        .map(|col| {
            (blo..bhi)
                .map(|b| ((col[b] - base[b]).max(0.0) as f64).powi(2))
                .sum()
        })
        .collect();
    let tot: f64 = e.iter().sum();
    if tot <= 0.0 {
        return f64::NAN;
    }
    let centroid: f64 = e
        .iter()
        .zip(&s.col_time)
        .map(|(&ee, &t)| ee * t)
        .sum::<f64>()
        / tot;
    let var: f64 = e
        .iter()
        .zip(&s.col_time)
        .map(|(&ee, &t)| ee * (t - centroid).powi(2))
        .sum::<f64>()
        / tot;
    var.sqrt() * 1000.0
}

// ---------------------------------------------------------------------------
// Image export (faithful grayscale via the app's dB colormap; 24-bit BMP so it
// opens natively with no extra crate). Renders are for eyeballing the structure
// the metrics summarize — top = high freq.
// ---------------------------------------------------------------------------

fn spectro_to_canvas(s: &Spectro, w: usize, h: usize, fmax: f64) -> Vec<u8> {
    let max_mag = s.cols.iter().flatten().copied().fold(0.0f32, f32::max);
    let n_cols = s.cols.len().max(1);
    let mut rgb = vec![0u8; w * h * 3];
    for py in 0..h {
        let freq = (1.0 - py as f64 / (h - 1).max(1) as f64) * fmax; // top = high freq
        let bin = ((freq * s.fft_size as f64 / s.sr as f64).round() as usize).min(s.n_bins - 1);
        for px in 0..w {
            let col = (px * n_cols / w).min(n_cols - 1);
            let g = magnitude_to_greyscale(s.cols[col][bin], max_mag);
            let idx = (py * w + px) * 3;
            rgb[idx] = g;
            rgb[idx + 1] = g;
            rgb[idx + 2] = g;
        }
    }
    rgb
}

/// Write a 24-bit uncompressed BMP. `rgb` is row-major, top-to-bottom; BMP is
/// stored bottom-up with BGR pixels and rows padded to 4 bytes.
fn write_bmp(path: &Path, w: usize, h: usize, rgb: &[u8]) -> std::io::Result<()> {
    let row_pad = (4 - (w * 3) % 4) % 4;
    let pixel_bytes = (w * 3 + row_pad) * h;
    let file_size = 54 + pixel_bytes;
    let mut buf = Vec::with_capacity(file_size);
    buf.extend_from_slice(b"BM");
    buf.extend_from_slice(&(file_size as u32).to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes());
    buf.extend_from_slice(&54u32.to_le_bytes());
    buf.extend_from_slice(&40u32.to_le_bytes());
    buf.extend_from_slice(&(w as i32).to_le_bytes());
    buf.extend_from_slice(&(h as i32).to_le_bytes()); // positive ⇒ bottom-up
    buf.extend_from_slice(&1u16.to_le_bytes());
    buf.extend_from_slice(&24u16.to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes());
    buf.extend_from_slice(&(pixel_bytes as u32).to_le_bytes());
    buf.extend_from_slice(&2835i32.to_le_bytes());
    buf.extend_from_slice(&2835i32.to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes());
    for y in (0..h).rev() {
        let row = &rgb[y * w * 3..(y + 1) * w * 3];
        for x in 0..w {
            buf.push(row[x * 3 + 2]); // B
            buf.push(row[x * 3 + 1]); // G
            buf.push(row[x * 3]); // R
        }
        buf.extend(std::iter::repeat(0u8).take(row_pad));
    }
    std::fs::write(path, &buf)
}

fn slug(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Method configs under comparison
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum Method {
    Stft { fft: usize },
    Reso { fft: usize, bw: f32 },
}

impl Method {
    fn label(&self) -> String {
        match self {
            Method::Stft { fft } => format!("STFT fft={fft}"),
            Method::Reso { fft, bw } => format!("Reso  bw={bw:.0}Hz (grid {fft})"),
        }
    }
    fn render(&self, sig: &Signal, hop: usize) -> Spectro {
        match *self {
            Method::Stft { fft } => render_stft(sig, fft, hop),
            Method::Reso { fft, bw } => render_reso(sig, fft, hop, bw, ResonatorLayout::Linear),
        }
    }
}

const HOP: usize = 128; // fixed across methods → shared time axis (≈2.67 ms @ 48k)

fn methods() -> Vec<Method> {
    vec![
        Method::Stft { fft: 512 },  // sharp in time, blurry in frequency
        Method::Stft { fft: 2048 }, // sharp in frequency, blurry in time
        Method::Reso {
            fft: 2048,
            bw: NARROW_BW,
        }, // sharp in freq, slow to track
        Method::Reso {
            fft: 2048,
            bw: 400.0,
        }, // faster tracking, blurrier
    ]
}

/// Eval window that skips both methods' startup/edge transients (the longest
/// STFT window and the resonator EMA warm-up).
fn eval_window(dur_s: f64) -> (f64, f64) {
    let stft_warm = 2048.0 / 2.0 / SR as f64;
    let reso_warm = warmup_samples(SR, NARROW_BW) as f64 / SR as f64;
    let lo = stft_warm.max(reso_warm) + 0.005;
    (lo, dur_s - 0.01)
}

// ---------------------------------------------------------------------------
// Report (ignored by default — prints a comparison table)
// ---------------------------------------------------------------------------

#[test]
#[ignore = "human-readable report; run with `report -- --ignored --nocapture`"]
fn report() {
    let dur = 0.30;
    let (t_lo, t_hi) = eval_window(dur);
    let ms = methods();

    println!("\n=== TF-eval: STFT vs Resonators ===");
    println!(
        "sr={SR} Hz, hop={HOP} ({:.2} ms), dur={dur}s, eval window [{t_lo:.3}, {t_hi:.3}]s\n",
        HOP as f64 / SR as f64 * 1000.0
    );

    // 1. Pure tone @ 40 kHz → frequency localization.
    //   bw_3db = line sharpness (resonators should win); rms_bw = includes the
    //   skirts, so it doubles as a leakage indicator (resonators lose: heavier
    //   single-pole tails than a Hann window).
    let t = tone(SR, 40_000.0, dur, 0.5);
    println!("[1] Pure tone 40 kHz  — ridge accuracy, line sharpness, leakage");
    println!(
        "    {:<28} {:>12} {:>11} {:>11}",
        "method", "ridge_rms_hz", "bw_3db_hz", "rms_bw_hz"
    );
    for m in &ms {
        let s = m.render(&t, HOP);
        let rms = ridge_rms_hz(&s, t_lo, t_hi, |_| 40_000.0);
        let bw3 = freq_bw3db_hz(&s, t_lo, t_hi);
        let leak = freq_spread_hz(&s, t_lo, t_hi);
        println!(
            "    {:<28} {:>12.1} {:>11.0} {:>11.0}",
            m.label(),
            rms,
            bw3,
            leak
        );
    }

    // 2. Linear chirp 20→90 kHz → ridge tracking + line thickness (stays under
    //    the 96 kHz Nyquist). bw_3db here is the instantaneous line thickness,
    //    so it shows the long FFT window smearing a fast sweep.
    let (cf0, cf1) = (20_000.0, 90_000.0);
    let c = linear_chirp(SR, cf0, cf1, dur, 0.5);
    let k = (cf1 - cf0) / dur;
    println!(
        "\n[2] Linear chirp 20→90 kHz — tracking & line thickness ({:.0} Hz/ms)",
        k / 1000.0
    );
    println!(
        "    {:<28} {:>12} {:>11}",
        "method", "ridge_rms_hz", "bw_3db_hz"
    );
    for m in &ms {
        let s = m.render(&c, HOP);
        let rms = ridge_rms_hz(&s, t_lo, t_hi, |t| cf0 + k * t);
        let bw3 = freq_bw3db_hz(&s, t_lo, t_hi);
        println!("    {:<28} {:>12.1} {:>11.0}", m.label(), rms, bw3);
    }

    // 3. Gaussian burst @ 50 kHz, σ=2 ms → time localization. The power-weighted
    //    spread of a σ amplitude-Gaussian is σ/√2, so the ideal floor ≈ 1.41 ms;
    //    a method adds its own window smear in quadrature on top.
    let b = gauss_burst(SR, 50_000.0, dur / 2.0, 0.002, dur, 0.6);
    println!("\n[3] Gaussian burst 50 kHz σ=2 ms — temporal sharpness (ideal floor ≈ 1.41 ms)");
    println!("    {:<28} {:>14}", "method", "time_spread_ms");
    for m in &ms {
        let s = m.render(&b, HOP);
        let ts = time_spread_ms(&s, 40_000.0, 60_000.0);
        println!("    {:<28} {:>14.2}", m.label(), ts);
    }

    // 4. Discriminability: chirp that rises a little vs a little more. Higher
    //    noise + a smaller Δ keep d′ out of the trivially-separable regime so
    //    the metric actually ranks the methods on a hard call.
    let dur2 = 0.20;
    let a_gen =
        |seed: u64| with_noise(&linear_chirp(SR, 20_000.0, 60_000.0, dur2, 0.5), 0.12, seed);
    let b_gen =
        |seed: u64| with_noise(&linear_chirp(SR, 20_000.0, 62_000.0, dur2, 0.5), 0.12, seed);
    const N: u64 = 10;
    println!("\n[4] Discriminability d′ — chirp 20→60 kHz vs 20→62 kHz (noise σ=0.12, {N} draws)");
    println!("    {:<28} {:>10}", "method", "dprime");
    for m in &ms {
        let a: Vec<Vec<f32>> = (0..N)
            .map(|s| flatten_norm(&m.render(&a_gen(s), HOP)))
            .collect();
        let b: Vec<Vec<f32>> = (0..N)
            .map(|s| flatten_norm(&m.render(&b_gen(s + 1000), HOP)))
            .collect();
        println!("    {:<28} {:>10.2}", m.label(), dprime(&a, &b));
    }

    // 5. STEP-1 EXPERIMENT — per-bin alpha schedule on a FINE output grid
    //    (fft=8192 ⇒ 23 Hz bins, so the grid no longer caps resonator Q).
    //    ConstBw is the shipped flat schedule; ConstQ narrows low-freq bins
    //    (sharper lines) and widens high-freq bins (faster time tracking).
    let fine = 8192;
    let scheds = [
        AlphaSched::ConstBw(60.0),
        AlphaSched::ConstQ(150.0),
        AlphaSched::ConstQ(400.0),
    ];
    let probe_freqs = [8_000.0, 40_000.0, 80_000.0];
    println!("\n[5] Per-bin alpha (fine grid fft={fine}) — line sharpness bw_3db_hz by tone freq");
    println!(
        "    {:<18} {:>9} {:>9} {:>9}",
        "schedule", "8kHz", "40kHz", "80kHz"
    );
    for sched in scheds {
        let cells: Vec<f64> = probe_freqs
            .iter()
            .map(|&f| {
                freq_bw3db_hz(
                    &render_reso_sched(&tone(SR, f, dur, 0.5), fine, HOP, sched),
                    t_lo,
                    t_hi,
                )
            })
            .collect();
        println!(
            "    {:<18} {:>9.0} {:>9.0} {:>9.0}",
            sched.label(),
            cells[0],
            cells[1],
            cells[2]
        );
    }
    println!("\n    time_spread_ms on an 80 kHz σ=1 ms burst (lower = faster high-freq tracking):");
    let hb = gauss_burst(SR, 80_000.0, dur / 2.0, 0.001, dur, 0.6);
    for sched in scheds {
        let ts = time_spread_ms(
            &render_reso_sched(&hb, fine, HOP, sched),
            70_000.0,
            90_000.0,
        );
        println!("    {:<18} {:>9.2}", sched.label(), ts);
    }

    // 6. HYBRID leakage cleanup — geometric-mean / FFT-gated fuse of the sharp
    //    (narrow-bw) resonator with a clean short FFT: keep the resonator's
    //    crisp swept line but suppress its heavy single-pole tails where the FFT
    //    is dark. On the 20→90 kHz chirp (reso tracks crisply but leaks; a short
    //    FFT is clean but blurry).
    let fcg = 4096;
    let chirp6 = linear_chirp(SR, 20_000.0, 90_000.0, dur, 0.5);
    let r6 = render_reso_sched(&chirp6, fcg, HOP, AlphaSched::ConstBw(60.0));
    let f6 = render_stft(&chirp6, 512, HOP);
    let fmax6 = f6
        .cols
        .iter()
        .flatten()
        .copied()
        .fold(0.0f32, f32::max)
        .max(1e-9);
    let gate_thresh = 0.03 * fmax6;
    let fused_gm = fuse(&r6, &f6, geomean);
    let fused_gate = fuse(&r6, &f6, move |r, fv| {
        r * (fv / gate_thresh).clamp(0.0, 1.0)
    });
    println!("\n[6] Hybrid leakage cleanup (20→90 kHz chirp, bank grid {fcg})");
    println!(
        "    {:<24} {:>11} {:>11}",
        "method", "bw_3db_hz", "rms_bw_hz"
    );
    for (name, s) in [
        ("FFT-512 (clean/blurry)", &f6),
        ("Reso bw60 (crisp/leaky)", &r6),
        ("Fused geomean", &fused_gm),
        ("Fused FFT-gated", &fused_gate),
    ] {
        println!(
            "    {:<24} {:>11.0} {:>11.0}",
            name,
            freq_bw3db_hz(s, t_lo, t_hi),
            freq_spread_hz(s, t_lo, t_hi)
        );
    }

    // 7. HYBRID adaptive blend — mixed signal: a sustained 40 kHz tone PLUS a
    //    50 kHz click at mid-time. The sharp bank nails the tone but smears the
    //    click; the fast bank nails the click but blurs the tone; the FFT-
    //    tonalness blend should get BOTH at once.
    let tone7 = tone(SR, 40_000.0, dur, 0.5);
    let click7 = gauss_burst(SR, 50_000.0, dur / 2.0, 0.0005, dur, 0.9);
    let mixed7 = mix(&tone7, &click7);
    let sharp7 = AlphaSched::ConstBw(40.0);
    let fast7 = AlphaSched::ConstBw(1000.0);
    let r_sharp7 = render_reso_sched(&mixed7, fcg, HOP, sharp7);
    let r_fast7 = render_reso_sched(&mixed7, fcg, HOP, fast7);
    let r_blend7 = adaptive_blend(&mixed7, fcg, HOP, sharp7, fast7, 80.0);
    let tone_hi = (dur / 2.0 - 0.01).max(t_lo + 0.01); // tone-only window (before the click)
    println!("\n[7] Hybrid adaptive blend (40 kHz tone + 50 kHz click, bank grid {fcg})");
    println!(
        "    {:<20} {:>15} {:>18}",
        "method", "tone bw_3db_hz", "click t_spread_ms"
    );
    for (name, s) in [
        ("Sharp bank bw40", &r_sharp7),
        ("Fast bank bw1000", &r_fast7),
        ("Adaptive blend", &r_blend7),
    ] {
        let tone_bw = freq_bw3db_hz(s, t_lo, tone_hi);
        let click_ts = time_spread_transient_ms(s, 47_000.0, 53_000.0);
        println!("    {:<20} {:>15.0} {:>18.2}", name, tone_bw, click_ts);
    }

    // Optional image export for eyeballing — set TF_EVAL_OUT=<dir>.
    if let Ok(dir) = std::env::var("TF_EVAL_OUT") {
        let rdir = format!("{dir}/renders");
        let _ = std::fs::create_dir_all(&rdir);
        let (cw, ch) = (520usize, 360usize);
        let fmax = SR as f64 / 2.0;
        let signals: [(&str, Signal); 3] = [
            ("tone40k", tone(SR, 40_000.0, dur, 0.5)),
            (
                "chirp20-90k",
                linear_chirp(SR, 20_000.0, 90_000.0, dur, 0.5),
            ),
            (
                "burst50k",
                gauss_burst(SR, 50_000.0, dur / 2.0, 0.002, dur, 0.6),
            ),
        ];
        for (name, sig) in &signals {
            for m in &ms {
                let rgb = spectro_to_canvas(&m.render(sig, HOP), cw, ch, fmax);
                let _ = write_bmp(
                    Path::new(&format!("{rdir}/{name}__{}.bmp", slug(&m.label()))),
                    cw,
                    ch,
                    &rgb,
                );
            }
        }
        // Step-1 visual: three tones (8/40/80 kHz) under flat vs constant-Q.
        let mt = multitone(SR, &[8_000.0, 40_000.0, 80_000.0], dur, 0.6);
        for sched in scheds {
            let rgb = spectro_to_canvas(&render_reso_sched(&mt, fine, HOP, sched), cw, ch, fmax);
            let _ = write_bmp(
                Path::new(&format!("{rdir}/multitone__{}.bmp", slug(&sched.label()))),
                cw,
                ch,
                &rgb,
            );
        }
        // Step-4 hybrid visuals: chirp leakage cleanup + mixed-signal blend.
        for (name, s) in [
            ("hybrid_chirp__reso_bw60", &r6),
            ("hybrid_chirp__fused_gate", &fused_gate),
            ("hybrid_mixed__sharp", &r_sharp7),
            ("hybrid_mixed__fast", &r_fast7),
            ("hybrid_mixed__blend", &r_blend7),
        ] {
            let rgb = spectro_to_canvas(s, cw, ch, fmax);
            let _ = write_bmp(Path::new(&format!("{rdir}/{name}.bmp")), cw, ch, &rgb);
        }
        println!("\nWrote renders to {rdir}");
    }
    println!();
}

// ---------------------------------------------------------------------------
// Sanity tests (run by default — guard the metrics against regressions)
// ---------------------------------------------------------------------------

#[test]
fn tone_localizes_in_frequency() {
    let dur = 0.18;
    let (lo, hi) = eval_window(dur);
    let t = tone(SR, 40_000.0, dur, 0.5);
    // A long STFT window and a narrow resonator bank should both put the ridge
    // within ~150 Hz of the true 40 kHz tone.
    let stft = render_stft(&t, 2048, HOP);
    let reso = render_reso(&t, 2048, HOP, NARROW_BW, ResonatorLayout::Linear);
    let stft_rms = ridge_rms_hz(&stft, lo, hi, |_| 40_000.0);
    let reso_rms = ridge_rms_hz(&reso, lo, hi, |_| 40_000.0);
    assert!(
        stft_rms < 150.0,
        "STFT tone ridge RMS too high: {stft_rms:.1} Hz"
    );
    assert!(
        reso_rms < 150.0,
        "Reso tone ridge RMS too high: {reso_rms:.1} Hz"
    );
}

#[test]
fn chirp_ridge_is_tracked() {
    let dur = 0.2;
    let (lo, hi) = eval_window(dur);
    // Moderate sweep, comfortably under the 96 kHz Nyquist.
    let f0 = 30_000.0;
    let f1 = 60_000.0;
    let k = (f1 - f0) / dur;
    let c = linear_chirp(SR, f0, f1, dur, 0.5);
    // At least one method should track the sweep to within ~1.5 kHz RMS.
    let best = methods()
        .iter()
        .map(|m| ridge_rms_hz(&m.render(&c, HOP), lo, hi, |t| f0 + k * t))
        .filter(|x| x.is_finite())
        .fold(f64::INFINITY, f64::min);
    assert!(
        best < 1500.0,
        "no method tracked the chirp (best RMS {best:.1} Hz)"
    );
}

#[test]
fn stft_window_tradeoff_holds() {
    // The textbook STFT tradeoff must show up: the long window is sharper in
    // frequency (smaller freq_spread on a tone) while the short window is
    // sharper in time (smaller time_spread on a burst). If this inverts, the
    // harness is mis-wired.
    let dur = 0.18;
    let (lo, hi) = eval_window(dur);

    let t = tone(SR, 40_000.0, dur, 0.5);
    let short_f = freq_bw3db_hz(&render_stft(&t, 512, HOP), lo, hi);
    let long_f = freq_bw3db_hz(&render_stft(&t, 2048, HOP), lo, hi);
    assert!(
        long_f < short_f,
        "long window not sharper in frequency: long={long_f:.0} short={short_f:.0}"
    );

    let b = gauss_burst(SR, 50_000.0, dur / 2.0, 0.0015, dur, 0.6);
    let short_t = time_spread_ms(&render_stft(&b, 512, HOP), 40_000.0, 60_000.0);
    let long_t = time_spread_ms(&render_stft(&b, 2048, HOP), 40_000.0, 60_000.0);
    assert!(
        short_t < long_t,
        "short window not sharper in time: short={short_t:.2} long={long_t:.2}"
    );
}

#[test]
fn metrics_are_finite() {
    let dur = 0.12;
    let (lo, hi) = eval_window(dur);
    let t = tone(SR, 40_000.0, dur, 0.5);
    let b = gauss_burst(SR, 40_000.0, dur / 2.0, 0.002, dur, 0.6);
    for m in methods() {
        assert!(ridge_rms_hz(&m.render(&t, HOP), lo, hi, |_| 40_000.0).is_finite());
        assert!(freq_bw3db_hz(&m.render(&t, HOP), lo, hi).is_finite());
        assert!(freq_spread_hz(&m.render(&t, HOP), lo, hi).is_finite());
        assert!(time_spread_ms(&m.render(&b, HOP), 30_000.0, 50_000.0).is_finite());
    }
}

#[test]
fn hybrid_functions_run() {
    // Guards the step-4 hybrid wiring (cross-grid fuse + adaptive blend) and
    // metric finiteness — quality is judged in the report, not asserted here.
    let dur = 0.12;
    let fcg = 2048;
    let tone = tone(SR, 40_000.0, dur, 0.5);
    let click = gauss_burst(SR, 50_000.0, dur / 2.0, 0.0005, dur, 0.9);
    let mixed = mix(&tone, &click);

    let r = render_reso_sched(&mixed, fcg, HOP, AlphaSched::ConstBw(60.0));
    let f = render_stft(&mixed, 512, HOP); // different grid ⇒ exercises resample
    let fused = fuse(&r, &f, geomean);
    assert_eq!(fused.n_bins, r.n_bins);
    assert_eq!(fused.cols.len(), r.cols.len());

    let blend = adaptive_blend(
        &mixed,
        fcg,
        HOP,
        AlphaSched::ConstBw(40.0),
        AlphaSched::ConstBw(1000.0),
        80.0,
    );
    assert_eq!(blend.n_bins, r.n_bins);
    let (lo, _) = eval_window(dur);
    let tone_hi = (dur / 2.0 - 0.01).max(lo + 0.01);
    assert!(freq_bw3db_hz(&blend, lo, tone_hi).is_finite());
    assert!(time_spread_transient_ms(&blend, 47_000.0, 53_000.0).is_finite());
}
