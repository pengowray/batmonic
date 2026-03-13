//! Power Spectral Density estimation using Welch's method.
//!
//! Computes averaged periodograms over overlapping Hann-windowed segments,
//! with peak detection and bandwidth analysis (-6 dB, -10 dB).

use leptos::prelude::GetUntracked;
use realfft::RealFftPlanner;
use std::cell::RefCell;
use std::collections::HashMap;

// ── Thread-local caches ─────────────────────────────────────────────────────

thread_local! {
    static PSD_FFT_PLANNER: RefCell<RealFftPlanner<f32>> = RefCell::new(RealFftPlanner::new());
    static PSD_HANN_CACHE: RefCell<HashMap<usize, Vec<f32>>> = RefCell::new(HashMap::new());
}

fn hann_window(size: usize) -> Vec<f32> {
    PSD_HANN_CACHE.with(|cache| {
        cache
            .borrow_mut()
            .entry(size)
            .or_insert_with(|| {
                (0..size)
                    .map(|i| {
                        0.5 * (1.0
                            - (2.0 * std::f32::consts::PI * i as f32 / (size - 1) as f32).cos())
                    })
                    .collect()
            })
            .clone()
    })
}

/// Hann window power correction factor: sum of squared window values.
fn hann_power_sum(size: usize) -> f64 {
    let w = hann_window(size);
    w.iter().map(|&v| (v as f64) * (v as f64)).sum()
}

// ── Result types ────────────────────────────────────────────────────────────

/// Result of a PSD computation.
#[derive(Clone, Debug)]
pub struct PsdResult {
    /// Power spectral density in dB per bin (length = nfft/2 + 1).
    /// Bin 0 = DC, bin N = Nyquist.
    pub power_db: Vec<f64>,
    /// Frequency resolution in Hz per bin.
    pub freq_resolution: f64,
    /// Sample rate of the source audio.
    pub sample_rate: u32,
    /// NFFT size used.
    pub nfft: usize,
    /// Number of frames averaged.
    pub frame_count: usize,
    /// All detected peaks, sorted by power (strongest first).
    pub peaks: Vec<PsdPeak>,
}

/// Peak frequency and bandwidth analysis from a PSD.
#[derive(Clone, Debug)]
pub struct PsdPeak {
    /// Peak frequency in Hz.
    pub freq_hz: f64,
    /// Peak power in dB.
    pub power_db: f64,
    /// Bin index of the peak.
    pub bin_index: usize,
    /// -6 dB bandwidth: (low_hz, high_hz). None if the peak doesn't drop 6 dB.
    pub bw_6db: Option<(f64, f64)>,
    /// -10 dB bandwidth: (low_hz, high_hz). None if the peak doesn't drop 10 dB.
    pub bw_10db: Option<(f64, f64)>,
}

// ── Computation ─────────────────────────────────────────────────────────────

/// Compute PSD using Welch's method (synchronous).
///
/// - `samples`: mono f32 audio
/// - `sample_rate`: Hz
/// - `nfft`: FFT size (e.g. 256, 512, 1024, 2048, 4096)
///
/// Uses 50% overlap and Hann window.
pub fn compute_psd(samples: &[f32], sample_rate: u32, nfft: usize) -> PsdResult {
    let n_bins = nfft / 2 + 1;
    let hop = nfft / 2;
    let window = hann_window(nfft);
    let power_norm = hann_power_sum(nfft) * sample_rate as f64;

    let fft = PSD_FFT_PLANNER.with(|p| p.borrow_mut().plan_fft_forward(nfft));
    let mut input = fft.make_input_vec();
    let mut spectrum = fft.make_output_vec();

    let mut accum = vec![0.0f64; n_bins];
    let mut frame_count = 0usize;

    let mut pos = 0usize;
    while pos + nfft <= samples.len() {
        let frame = &samples[pos..pos + nfft];

        // Apply window
        for (inp, (&s, &w)) in input.iter_mut().zip(frame.iter().zip(window.iter())) {
            *inp = s * w;
        }

        // FFT
        fft.process(&mut input, &mut spectrum).expect("FFT failed");

        // Accumulate |X[k]|²
        for (acc, c) in accum.iter_mut().zip(spectrum.iter()) {
            *acc += (c.re as f64) * (c.re as f64) + (c.im as f64) * (c.im as f64);
        }

        frame_count += 1;
        pos += hop;
    }

    // Average and normalize to PSD, convert to dB
    let power_db: Vec<f64> = if frame_count > 0 {
        accum
            .iter()
            .enumerate()
            .map(|(i, &sum)| {
                let mut psd = sum / (frame_count as f64 * power_norm);
                // Double non-DC, non-Nyquist bins (one-sided spectrum)
                if i > 0 && i < n_bins - 1 {
                    psd *= 2.0;
                }
                if psd > 0.0 {
                    10.0 * psd.log10()
                } else {
                    -200.0
                }
            })
            .collect()
    } else {
        vec![-200.0; n_bins]
    };

    let freq_resolution = sample_rate as f64 / nfft as f64;
    let peaks = find_peaks(&power_db, freq_resolution);

    PsdResult {
        power_db,
        freq_resolution,
        sample_rate,
        nfft,
        frame_count,
        peaks,
    }
}

/// Async version that yields to the browser every `yield_interval` frames.
pub async fn compute_psd_async(
    samples: &[f32],
    sample_rate: u32,
    nfft: usize,
    generation: u32,
    gen_signal: leptos::prelude::RwSignal<u32>,
) -> Option<PsdResult> {
    use wasm_bindgen::prelude::*;

    let n_bins = nfft / 2 + 1;
    let hop = nfft / 2;
    let window = hann_window(nfft);
    let power_norm = hann_power_sum(nfft) * sample_rate as f64;

    let fft = PSD_FFT_PLANNER.with(|p| p.borrow_mut().plan_fft_forward(nfft));
    let mut input = fft.make_input_vec();
    let mut spectrum = fft.make_output_vec();

    let mut accum = vec![0.0f64; n_bins];
    let mut frame_count = 0usize;

    let yield_interval = 64;
    let mut pos = 0usize;
    while pos + nfft <= samples.len() {
        let frame = &samples[pos..pos + nfft];

        for (inp, (&s, &w)) in input.iter_mut().zip(frame.iter().zip(window.iter())) {
            *inp = s * w;
        }
        fft.process(&mut input, &mut spectrum).expect("FFT failed");

        for (acc, c) in accum.iter_mut().zip(spectrum.iter()) {
            *acc += (c.re as f64) * (c.re as f64) + (c.im as f64) * (c.im as f64);
        }

        frame_count += 1;
        pos += hop;

        if frame_count % yield_interval == 0 {
            // Yield to browser
            let promise = js_sys::Promise::new(&mut |resolve, _| {
                let win = web_sys::window().unwrap();
                let cb = Closure::once_into_js(move || {
                    let _ = resolve.call0(&JsValue::NULL);
                });
                let _ = win.set_timeout_with_callback_and_timeout_and_arguments_0(
                    cb.unchecked_ref(),
                    0,
                );
            });
            let _ = wasm_bindgen_futures::JsFuture::from(promise).await;

            // Check cancellation
            if gen_signal.get_untracked() != generation {
                return None;
            }
        }
    }

    let power_db: Vec<f64> = if frame_count > 0 {
        accum
            .iter()
            .enumerate()
            .map(|(i, &sum)| {
                let mut psd = sum / (frame_count as f64 * power_norm);
                if i > 0 && i < n_bins - 1 {
                    psd *= 2.0;
                }
                if psd > 0.0 {
                    10.0 * psd.log10()
                } else {
                    -200.0
                }
            })
            .collect()
    } else {
        vec![-200.0; n_bins]
    };

    let freq_resolution = sample_rate as f64 / nfft as f64;
    let peaks = find_peaks(&power_db, freq_resolution);

    Some(PsdResult {
        power_db,
        freq_resolution,
        sample_rate,
        nfft,
        frame_count,
        peaks,
    })
}

// ── Peak detection ──────────────────────────────────────────────────────────

/// Maximum number of peaks to return.
const MAX_PEAKS: usize = 8;

/// Minimum prominence (dB) a local maximum must have relative to the valleys
/// on either side to be counted as a peak.
const MIN_PROMINENCE_DB: f64 = 3.0;

/// Find all significant local maxima in the PSD, sorted by power (strongest first).
fn find_peaks(power_db: &[f64], freq_resolution: f64) -> Vec<PsdPeak> {
    if power_db.len() < 3 {
        return Vec::new();
    }

    let n = power_db.len();

    // Find local maxima (bins where power[i] > both neighbours), skip DC (bin 0)
    let mut candidates: Vec<(usize, f64)> = Vec::new();
    for i in 2..n - 1 {
        if power_db[i] > power_db[i - 1] && power_db[i] >= power_db[i + 1] {
            candidates.push((i, power_db[i]));
        }
    }

    if candidates.is_empty() {
        // Fallback: global max
        let start_bin = 1;
        let (peak_bin, &peak_power) = power_db[start_bin..]
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(i, v)| (i + start_bin, v))
            .unwrap_or((0, &-200.0));
        candidates.push((peak_bin, peak_power));
    }

    // Filter by prominence: for each candidate, walk left and right to find
    // the lowest valley before reaching a higher peak.
    let mut peaks: Vec<(usize, f64, f64)> = Vec::new(); // (bin, power, prominence)
    for &(bin, power) in &candidates {
        // Walk left to find minimum before a higher peak
        let mut left_min = power;
        for j in (1..bin).rev() {
            left_min = left_min.min(power_db[j]);
            if power_db[j] > power {
                break;
            }
        }
        // Walk right
        let mut right_min = power;
        for j in (bin + 1)..n {
            right_min = right_min.min(power_db[j]);
            if power_db[j] > power {
                break;
            }
        }
        let prominence = power - left_min.max(right_min);
        if prominence >= MIN_PROMINENCE_DB {
            peaks.push((bin, power, prominence));
        }
    }

    // If no peaks passed prominence filter, use the single strongest candidate
    if peaks.is_empty() {
        if let Some(&(bin, power)) = candidates.iter().max_by(|a, b| a.1.partial_cmp(&b.1).unwrap()) {
            peaks.push((bin, power, 0.0));
        }
    }

    // Sort by power descending, take top MAX_PEAKS
    peaks.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    peaks.truncate(MAX_PEAKS);

    peaks
        .iter()
        .map(|&(bin, power, _)| {
            let freq_hz = bin as f64 * freq_resolution;
            let bw_6db = find_bandwidth(power_db, bin, power, 6.0, freq_resolution);
            let bw_10db = find_bandwidth(power_db, bin, power, 10.0, freq_resolution);
            PsdPeak {
                freq_hz,
                power_db: power,
                bin_index: bin,
                bw_6db,
                bw_10db,
            }
        })
        .collect()
}

/// Find bandwidth at `drop_db` below peak using linear interpolation.
fn find_bandwidth(
    power_db: &[f64],
    peak_bin: usize,
    peak_power: f64,
    drop_db: f64,
    freq_resolution: f64,
) -> Option<(f64, f64)> {
    let threshold = peak_power - drop_db;

    // Walk left from peak
    let low_freq = {
        let mut low_bin = None;
        for i in (1..peak_bin).rev() {
            if power_db[i] < threshold {
                // Interpolate between bin i and i+1
                let frac = if (power_db[i + 1] - power_db[i]).abs() > 1e-12 {
                    (threshold - power_db[i]) / (power_db[i + 1] - power_db[i])
                } else {
                    0.5
                };
                low_bin = Some((i as f64 + frac) * freq_resolution);
                break;
            }
        }
        low_bin
    };

    // Walk right from peak
    let high_freq = {
        let mut high_bin = None;
        for i in (peak_bin + 1)..power_db.len() {
            if power_db[i] < threshold {
                // Interpolate between bin i-1 and i
                let frac = if (power_db[i - 1] - power_db[i]).abs() > 1e-12 {
                    (threshold - power_db[i - 1]) / (power_db[i] - power_db[i - 1])
                } else {
                    0.5
                };
                high_bin = Some(((i - 1) as f64 + frac) * freq_resolution);
                break;
            }
        }
        high_bin
    };

    match (low_freq, high_freq) {
        (Some(lo), Some(hi)) => Some((lo, hi)),
        _ => None,
    }
}
