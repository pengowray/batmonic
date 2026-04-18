use crate::canvas::colors::magnitude_to_greyscale;
use crate::types::PreRendered;
use crate::types::{AudioData, PreviewImage, SpectrogramColumn, SpectrogramData};
use realfft::RealFftPlanner;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

thread_local! {
    static FFT_PLANNER: RefCell<RealFftPlanner<f32>> = RefCell::new(RealFftPlanner::new());
    static HANN_CACHE: RefCell<HashMap<usize, Vec<f32>>> = RefCell::new(HashMap::new());
    static THANN_CACHE: RefCell<HashMap<usize, Vec<f32>>> = RefCell::new(HashMap::new());
    static DHANN_CACHE: RefCell<HashMap<usize, Vec<f32>>> = RefCell::new(HashMap::new());
}

fn hann_window(size: usize) -> Vec<f32> {
    HANN_CACHE.with(|cache| {
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

/// Time-ramped Hann window: `(m - center) * h[m]`.
/// Used for time reassignment (measures displacement from frame center).
fn t_hann_window(size: usize) -> Vec<f32> {
    THANN_CACHE.with(|cache| {
        cache
            .borrow_mut()
            .entry(size)
            .or_insert_with(|| {
                let h = hann_window(size);
                let center = (size - 1) as f32 / 2.0;
                (0..size).map(|i| (i as f32 - center) * h[i]).collect()
            })
            .clone()
    })
}

/// Derivative of the Hann window: `h'[m] = -π/(N-1) * sin(2πm/(N-1))`.
/// Used for frequency reassignment.
fn dh_window(size: usize) -> Vec<f32> {
    DHANN_CACHE.with(|cache| {
        cache
            .borrow_mut()
            .entry(size)
            .or_insert_with(|| {
                let n_minus_1 = (size - 1) as f32;
                (0..size)
                    .map(|i| {
                        -std::f32::consts::PI / n_minus_1
                            * (2.0 * std::f32::consts::PI * i as f32 / n_minus_1).sin()
                    })
                    .collect()
            })
            .clone()
    })
}

/// Compute a reassigned spectrogram tile from raw audio samples.
///
/// Performs 3 FFTs per frame (standard Hann, time-ramped, derivative-windowed),
/// then accumulates |X|² at corrected (time, frequency) positions using
/// nearest-neighbor assignment.
///
/// Returns a `PreRendered` with `db_data` in the same format as normal tiles
/// (row 0 = highest freq, absolute dB values).
///
/// `samples` must cover at least `col_count * hop_size + fft_size` samples.
pub fn compute_reassigned_tile(
    samples: &[f32],
    col_count: usize,
    fft_size: usize,
    hop_size: usize,
    threshold_db: f32,
) -> PreRendered {
    let n_bins = fft_size / 2 + 1;

    if samples.len() < fft_size || col_count == 0 {
        return PreRendered {
            width: 0, height: 0,
            pixels: Vec::new(), db_data: Vec::new(), flow_shifts: Vec::new(),
        };
    }

    let fft = FFT_PLANNER.with(|p| p.borrow_mut().plan_fft_forward(fft_size));
    let win_h = hann_window(fft_size);
    let win_th = t_hann_window(fft_size);
    let win_dh = dh_window(fft_size);

    // Reusable FFT buffers
    let mut input = fft.make_input_vec();
    let mut spec_h = fft.make_output_vec();
    let mut spec_th = fft.make_output_vec();
    let mut spec_dh = fft.make_output_vec();

    // Accumulation grid (f64 for precision)
    let grid_size = col_count * n_bins;
    let mut accum = vec![0.0f64; grid_size];

    let threshold_power = 10.0f32.powf(threshold_db / 10.0); // power threshold
    let two_pi = 2.0 * std::f64::consts::PI;
    let fft_over_two_pi = fft_size as f64 / two_pi;

    let total_cols = if samples.len() >= fft_size {
        (samples.len() - fft_size) / hop_size + 1
    } else {
        0
    };
    let actual_cols = col_count.min(total_cols);

    for col_i in 0..actual_cols {
        let pos = col_i * hop_size;
        if pos + fft_size > samples.len() {
            break;
        }

        let frame = &samples[pos..pos + fft_size];

        // FFT with standard Hann window
        for (inp, (&s, &w)) in input.iter_mut().zip(frame.iter().zip(win_h.iter())) {
            *inp = s * w;
        }
        fft.process(&mut input, &mut spec_h).expect("FFT failed");

        // FFT with time-ramped Hann window
        for (inp, (&s, &w)) in input.iter_mut().zip(frame.iter().zip(win_th.iter())) {
            *inp = s * w;
        }
        fft.process(&mut input, &mut spec_th).expect("FFT failed");

        // FFT with derivative Hann window
        for (inp, (&s, &w)) in input.iter_mut().zip(frame.iter().zip(win_dh.iter())) {
            *inp = s * w;
        }
        fft.process(&mut input, &mut spec_dh).expect("FFT failed");

        for k in 0..n_bins {
            let xh = spec_h[k];
            let power = xh.norm_sqr();

            if power < threshold_power {
                continue;
            }

            // Complex division: X_th / X_h and X_dh / X_h
            use realfft::num_complex::Complex;
            let xh64 = Complex::<f64>::new(xh.re as f64, xh.im as f64);
            let xth64 = Complex::<f64>::new(spec_th[k].re as f64, spec_th[k].im as f64);
            let xdh64 = Complex::<f64>::new(spec_dh[k].re as f64, spec_dh[k].im as f64);

            // Corrected time: t_hat = n - Re(X_th / X_h)
            let t_hat = col_i as f64 - (xth64 / xh64).re;

            // Corrected frequency bin: f_hat = k + (N / 2π) * Im(X_dh / X_h)
            let f_hat = k as f64 + fft_over_two_pi * (xdh64 / xh64).im;

            // Clamp and round to nearest grid point
            let t_idx = t_hat.round().clamp(0.0, (col_count - 1) as f64) as usize;
            let f_idx = f_hat.round().clamp(0.0, (n_bins - 1) as f64) as usize;

            accum[f_idx * col_count + t_idx] += power as f64;
        }
    }

    // Convert accumulation grid to dB, flipped vertically (row 0 = highest freq)
    let width = col_count as u32;
    let height = n_bins as u32;
    let mut db_data = vec![f32::NEG_INFINITY; grid_size];

    for bin in 0..n_bins {
        let y = n_bins - 1 - bin; // flip: row 0 = highest freq
        for col in 0..col_count {
            let val = accum[bin * col_count + col];
            if val > 0.0 {
                db_data[y * col_count + col] = (10.0 * val.log10()) as f32;
            }
        }
    }

    PreRendered {
        width,
        height,
        pixels: Vec::new(),
        db_data,
        flow_shifts: Vec::new(),
    }
}

/// Compute a spectrogram from audio data using a Short-Time Fourier Transform (STFT).
///
/// Uses a Hann window for spectral leakage reduction.
pub fn compute_spectrogram(
    audio: &AudioData,
    fft_size: usize,
    hop_size: usize,
) -> SpectrogramData {
    let fft = FFT_PLANNER.with(|p| p.borrow_mut().plan_fft_forward(fft_size));

    // Use the in-memory mono-mixed samples directly — allocating a fresh Vec
    // via read_region would duplicate up to a gigabyte for long audiobooks
    // that already fit in audio.samples (and OOM the WASM heap). For streaming
    // sources audio.samples is the head (~30s), which is all we need anyway.
    let samples: &[f32] = &audio.samples;

    let mut columns = Vec::new();

    let window = hann_window(fft_size);

    // Pre-allocate FFT buffers once and reuse across frames
    let mut input = fft.make_input_vec();
    let mut spectrum = fft.make_output_vec();

    let mut pos = 0;
    while pos + fft_size <= samples.len() {
        // Fill input in-place (no allocation per frame)
        for (inp, (&s, &w)) in input
            .iter_mut()
            .zip(samples[pos..pos + fft_size].iter().zip(window.iter()))
        {
            *inp = s * w;
        }

        fft.process(&mut input, &mut spectrum).expect("FFT failed");

        let magnitudes: Vec<f32> = spectrum.iter().map(|c| c.norm()).collect();

        let time_offset = pos as f64 / audio.sample_rate as f64;
        columns.push(SpectrogramColumn {
            magnitudes,
            time_offset,
        });

        pos += hop_size;
    }

    let freq_resolution = audio.sample_rate as f64 / fft_size as f64;
    let time_resolution = hop_size as f64 / audio.sample_rate as f64;
    let max_freq = audio.sample_rate as f64 / 2.0;

    let total_columns = columns.len();
    SpectrogramData {
        columns: Arc::new(columns),
        total_columns,
        freq_resolution,
        time_resolution,
        max_freq,
        sample_rate: audio.sample_rate,
    }
}

/// Compute a partial spectrogram — only columns `col_start .. col_start + col_count`.
///
/// Identical FFT parameters to `compute_spectrogram`.  Used for chunked async
/// computation so the browser stays responsive between chunks.
pub fn compute_spectrogram_partial(
    audio: &AudioData,
    fft_size: usize,
    hop_size: usize,
    col_start: usize,
    col_count: usize,
) -> Vec<SpectrogramColumn> {
    // Use audio.samples directly — same rationale as compute_spectrogram:
    // avoid allocating a duplicate Vec for multi-hour files.
    compute_stft_columns(&audio.samples, audio.sample_rate, fft_size, hop_size, col_start, col_count)
}

/// Compute STFT columns directly from a sample slice.
///
/// Like `compute_spectrogram_partial` but works on a raw `&[f32]` slice instead
/// of `&AudioData`, avoiding the need to wrap live recording buffers in `Arc<Vec<f32>>`.
pub fn compute_stft_columns(
    samples: &[f32],
    sample_rate: u32,
    fft_size: usize,
    hop_size: usize,
    col_start: usize,
    col_count: usize,
) -> Vec<SpectrogramColumn> {
    if samples.len() < fft_size || col_count == 0 {
        return vec![];
    }

    let fft = FFT_PLANNER.with(|p| p.borrow_mut().plan_fft_forward(fft_size));
    let window = hann_window(fft_size);
    let mut input = fft.make_input_vec();
    let mut spectrum = fft.make_output_vec();

    let total_cols = (samples.len() - fft_size) / hop_size + 1;
    let col_end = (col_start + col_count).min(total_cols);

    let mut columns = Vec::with_capacity(col_end.saturating_sub(col_start));
    for col_i in col_start..col_end {
        let pos = col_i * hop_size;
        if pos + fft_size > samples.len() {
            break;
        }
        for (inp, (&s, &w)) in input
            .iter_mut()
            .zip(samples[pos..pos + fft_size].iter().zip(window.iter()))
        {
            *inp = s * w;
        }
        fft.process(&mut input, &mut spectrum).expect("FFT failed");
        let magnitudes: Vec<f32> = spectrum.iter().map(|c| c.norm()).collect();
        let time_offset = pos as f64 / sample_rate as f64;
        columns.push(SpectrogramColumn { magnitudes, time_offset });
    }
    columns
}

/// Compute a fast low-resolution preview spectrogram as an RGBA pixel buffer.
/// Uses FFT=256 with a dynamic hop to produce roughly `target_width` columns.
pub fn compute_preview(audio: &AudioData, target_width: u32, target_height: u32) -> PreviewImage {
    // For streaming files, only the head samples are in memory — don't try to
    // read the entire multi-GB file. audio.samples contains the loaded portion.
    let total = if audio.source.is_fully_loaded() {
        audio.source.total_samples() as usize
    } else {
        audio.samples.len()
    };
    if total < 256 {
        // Too short for even one FFT frame
        return PreviewImage {
            width: 1,
            height: 1,
            pixels: Arc::new(vec![0, 0, 0, 255]),
        };
    }

    let fft_size = 256;
    let hop = (total / target_width as usize).max(fft_size);
    let spec = compute_spectrogram(audio, fft_size, hop);

    if spec.columns.is_empty() {
        return PreviewImage {
            width: 1,
            height: 1,
            pixels: Arc::new(vec![0, 0, 0, 255]),
        };
    }

    let src_w = spec.columns.len();
    let src_h = spec.columns[0].magnitudes.len();
    let out_w = (src_w as u32).min(target_width);
    let out_h = (src_h as u32).min(target_height);

    // Find global max magnitude for normalization
    let max_mag = spec
        .columns
        .iter()
        .flat_map(|c| c.magnitudes.iter())
        .copied()
        .fold(0.0f32, f32::max);

    let mut pixels = vec![0u8; (out_w * out_h * 4) as usize];

    for x in 0..out_w {
        let src_col = (x as usize * src_w) / out_w as usize;
        let col = &spec.columns[src_col.min(src_w - 1)];
        for y in 0..out_h {
            // Map output row to source bin (row 0 = highest freq)
            let src_bin = src_h - 1 - ((y as usize * src_h) / out_h as usize).min(src_h - 1);
            let mag = col.magnitudes[src_bin];
            let grey = magnitude_to_greyscale(mag, max_mag);
            let idx = (y * out_w + x) as usize * 4;
            pixels[idx] = grey;
            pixels[idx + 1] = grey;
            pixels[idx + 2] = grey;
            pixels[idx + 3] = 255;
        }
    }

    PreviewImage {
        width: out_w,
        height: out_h,
        pixels: Arc::new(pixels),
    }
}

/// Compute a higher-resolution overview image by downsampling existing SpectrogramData.
/// Produces a ~1024×256 greyscale RGBA image (same format as PreviewImage).
pub fn compute_overview_from_spectrogram(data: &SpectrogramData) -> Option<PreviewImage> {
    if data.columns.is_empty() {
        return None;
    }

    let src_w = data.columns.len();
    let src_h = data.columns[0].magnitudes.len();
    if src_h == 0 { return None; }

    let out_w = (src_w as u32).min(1024);
    let out_h = (src_h as u32).min(256);

    let max_mag = data.columns.iter()
        .flat_map(|c| c.magnitudes.iter())
        .copied()
        .fold(0.0f32, f32::max);
    if max_mag <= 0.0 { return None; }

    let mut pixels = vec![0u8; (out_w * out_h * 4) as usize];

    for x in 0..out_w {
        let src_col = (x as usize * src_w) / out_w as usize;
        let col = &data.columns[src_col.min(src_w - 1)];
        let col_len = col.magnitudes.len();
        if col_len == 0 { continue; }
        for y in 0..out_h {
            let src_bin = src_h - 1 - ((y as usize * src_h) / out_h as usize).min(src_h - 1);
            let mag = col.magnitudes[src_bin.min(col_len - 1)];
            let grey = magnitude_to_greyscale(mag, max_mag);
            let idx = (y * out_w + x) as usize * 4;
            pixels[idx] = grey;
            pixels[idx + 1] = grey;
            pixels[idx + 2] = grey;
            pixels[idx + 3] = 255;
        }
    }

    Some(PreviewImage {
        width: out_w,
        height: out_h,
        pixels: Arc::new(pixels),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::source::InMemorySource;
    use crate::types::{AudioData, FileMetadata};

    fn test_audio(samples: Vec<f32>, sample_rate: u32) -> AudioData {
        let samples = Arc::new(samples);
        let source = Arc::new(InMemorySource {
            samples: samples.clone(),
            raw_samples: None,
            sample_rate,
            channels: 1,
        });
        AudioData {
            duration_secs: samples.len() as f64 / sample_rate as f64,
            samples,
            source,
            sample_rate,
            channels: 1,
            metadata: FileMetadata {
                file_size: 0,
                format: "test",
                bits_per_sample: 32,
                is_float: true,
                guano: None,
                data_offset: None,
                data_size: None,
            },
        }
    }

    #[test]
    fn test_spectrogram_basic() {
        let sample_rate = 44100u32;
        let freq = 1000.0f64;
        let num_samples = 4096;

        let samples: Vec<f32> = (0..num_samples)
            .map(|i| {
                let t = i as f64 / sample_rate as f64;
                (2.0 * std::f64::consts::PI * freq * t).sin() as f32
            })
            .collect();

        let audio = test_audio(samples, sample_rate);

        let result = compute_spectrogram(&audio, 1024, 512);
        assert!(!result.columns.is_empty());
        assert_eq!(result.sample_rate, sample_rate);

        // The peak bin should be near 1000 Hz
        let col = &result.columns[1]; // skip first column (edge effects)
        let peak_bin = col
            .magnitudes
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .unwrap()
            .0;
        let peak_freq = peak_bin as f64 * result.freq_resolution;
        let error = (peak_freq - freq).abs();
        assert!(
            error < result.freq_resolution * 2.0,
            "Peak at {peak_freq} Hz, expected ~{freq} Hz"
        );
    }
}
