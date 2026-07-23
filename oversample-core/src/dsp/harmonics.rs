use crate::audio::source::ChannelView;
use crate::dsp::fft::{hann_window, plan_fft_forward};
use crate::types::{AudioData, SpectrogramColumn, SpectrogramData};
use realfft::num_complex::Complex;
use std::f32::consts::PI;

type Complex32 = Complex<f32>;

#[derive(Clone)]
pub struct HarmonicsAnalysis {
    // --- Phase Coherence ---
    /// Per-bin average phase coherence [0,1], indexed by FFT bin.
    pub phase_coherence: Vec<f32>,
    /// Mean phase coherence across all active bins.
    pub phase_coherence_mean: f32,
    /// Ratio of coherence at harmonic bins vs. mean (>1 = harmonics more coherent).
    pub harmonic_coherence_ratio: f32,

    // --- Harmonic Decay Profile ---
    /// Detected fundamental frequency in Hz (None if not found).
    pub fundamental_freq: Option<f32>,
    /// Amplitudes at 1f, 2f, 3f, ... normalised so A1=1.0.
    pub harmonic_amplitudes: Vec<f32>,
    /// Best-fit power-law exponent α where A_n ≈ 1/n^α.
    pub decay_exponent: f32,
    /// True if each successive harmonic is strictly weaker.
    pub decay_is_monotonic: bool,
    /// Harmonic indices (0-based) where amplitude anomalously exceeds the prior harmonic.
    pub decay_anomaly_indices: Vec<usize>,

    // --- Spectral Flux ---
    /// Half-wave-rectified onset flux, one value per spectrogram frame transition.
    pub flux_per_frame: Vec<f32>,
    pub flux_mean: f32,
    pub flux_peak: f32,
    /// Number of frames with significant flux immediately before an onset (pre-ringing).
    pub preringing_count: usize,
    /// [0,1]: fraction of active transitions where peak bin is stuck (staircasing).
    pub staircasing_score: f32,

    // --- Summary ---
    pub artifact_indicators: Vec<String>,
}

impl PartialEq for HarmonicsAnalysis {
    fn eq(&self, other: &Self) -> bool {
        // Cheap comparison — if the same file produced both, these will be identical.
        self.phase_coherence_mean == other.phase_coherence_mean
            && self.fundamental_freq == other.fundamental_freq
            && self.flux_per_frame.len() == other.flux_per_frame.len()
            && self.phase_coherence.len() == other.phase_coherence.len()
    }
}

// ---------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------

/// Compute the harmonics analysis summary for sidebar display.
/// Does not return 2-D per-frame coherence (call `compute_coherence_frames` for the heatmap).
pub fn analyze_harmonics(audio: &AudioData, spectrogram: &SpectrogramData) -> HarmonicsAnalysis {
    let fft_size = derive_fft_size(audio.sample_rate, spectrogram.freq_resolution);
    let hop_size = derive_hop_size(audio.sample_rate, spectrogram.time_resolution);

    // Spectral flux (cheap — reuses existing SpectrogramData).
    let flux_per_frame = compute_spectral_flux_frames(&spectrogram.columns);
    let flux_mean = mean_f32(&flux_per_frame);
    let flux_peak = flux_per_frame.iter().copied().fold(0.0f32, f32::max);
    let preringing_count = count_preringing(&flux_per_frame, flux_peak);
    let staircasing_score =
        compute_staircasing_score(&spectrogram.columns, &flux_per_frame, flux_peak);

    // Harmonic decay (cheap — uses existing SpectrogramData).
    let avg_spectrum = compute_avg_spectrum(&spectrogram.columns);
    let fundamental_bin = detect_fundamental_hps(&avg_spectrum);
    let fundamental_freq = fundamental_bin.map(|b| b as f32 * spectrogram.freq_resolution as f32);
    let (harmonic_amplitudes, decay_exponent, decay_is_monotonic, decay_anomaly_indices) =
        if let Some(f_bin) = fundamental_bin {
            compute_harmonic_decay(
                &avg_spectrum,
                f_bin,
                spectrogram.max_freq,
                spectrogram.freq_resolution,
            )
        } else {
            (vec![], 1.0, true, vec![])
        };

    // Phase coherence (requires a new STFT pass to keep complex output).
    let (phase_coherence, _) = if audio.source.total_samples() as usize >= fft_size {
        let frames = compute_complex_stft(audio, fft_size, hop_size);
        compute_phase_coherence_summary(&frames, fft_size, hop_size)
    } else {
        let n = fft_size / 2 + 1;
        (vec![0.5f32; n], Vec::new())
    };

    let n_active = phase_coherence.iter().filter(|&&c| c > 0.0).count().max(1);
    let phase_coherence_mean = phase_coherence.iter().copied().sum::<f32>() / n_active as f32;
    let harmonic_coherence_ratio =
        compute_harmonic_coherence_ratio(&phase_coherence, fundamental_bin);

    // Build human-readable artifact indicators.
    let mut indicators = Vec::new();
    if phase_coherence_mean < 0.35 {
        indicators.push(
            "Low mean phase coherence — significant phase drift suggests heavy processing"
                .to_string(),
        );
    } else if phase_coherence_mean < 0.55 {
        indicators.push("Moderate phase drift — possible processing artifacts".to_string());
    }
    if harmonic_coherence_ratio < 0.7 && fundamental_freq.is_some() {
        indicators.push(
            "Harmonic bins show reduced phase coherence vs. noise floor — suggests synthetic harmonics"
                .to_string(),
        );
    }
    for &i in &decay_anomaly_indices {
        indicators.push(format!(
            "H{} ({:.1} kHz) amplitude anomaly — violates natural power-law decay",
            i + 1,
            fundamental_freq.unwrap_or(0.0) * (i + 1) as f32 / 1000.0
        ));
    }
    if preringing_count > 0 {
        indicators.push(format!(
            "{} frame(s) of pre-ringing detected — windowing / STFT artifact",
            preringing_count
        ));
    }
    if staircasing_score > 0.5 {
        indicators.push(format!(
            "High staircasing score ({:.2}) — frequency sweeps show step quantisation",
            staircasing_score
        ));
    }
    if indicators.is_empty() {
        indicators.push("No significant artifacts detected".to_string());
    }

    HarmonicsAnalysis {
        phase_coherence,
        phase_coherence_mean,
        harmonic_coherence_ratio,
        fundamental_freq,
        harmonic_amplitudes,
        decay_exponent,
        decay_is_monotonic,
        decay_anomaly_indices,
        flux_per_frame,
        flux_mean,
        flux_peak,
        preringing_count,
        staircasing_score,
        artifact_indicators: indicators,
    }
}

/// Compute per-frame, per-bin phase coherence for the heatmap visualisation.
/// Returns shape `[num_frame_transitions][num_bins]`, values in [0, 1].
pub fn compute_coherence_frames(audio: &AudioData, spectrogram: &SpectrogramData) -> Vec<Vec<f32>> {
    let fft_size = derive_fft_size(audio.sample_rate, spectrogram.freq_resolution);
    let hop_size = derive_hop_size(audio.sample_rate, spectrogram.time_resolution);
    if (audio.source.total_samples() as usize) < fft_size {
        return Vec::new();
    }
    let frames = compute_complex_stft(audio, fft_size, hop_size);
    let (_, coherence_frames) = compute_phase_coherence_summary(&frames, fft_size, hop_size);
    coherence_frames
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn derive_fft_size(sample_rate: u32, freq_resolution: f64) -> usize {
    let n = (sample_rate as f64 / freq_resolution).round() as usize;
    n.max(64)
}

fn derive_hop_size(sample_rate: u32, time_resolution: f64) -> usize {
    let h = (time_resolution * sample_rate as f64).round() as usize;
    h.max(1)
}

fn compute_complex_stft(
    audio: &AudioData,
    fft_size: usize,
    hop_size: usize,
) -> Vec<Vec<Complex32>> {
    let total = audio.source.total_samples() as usize;
    let samples = audio.source.read_region(ChannelView::MonoMix, 0, total);
    let fft = plan_fft_forward(fft_size);
    let window = hann_window(fft_size);
    let mut frames = Vec::new();
    let mut input = fft.make_input_vec();
    let mut spectrum = fft.make_output_vec();
    let mut pos = 0;
    while pos + fft_size <= samples.len() {
        for (inp, (&s, &w)) in input
            .iter_mut()
            .zip(samples[pos..pos + fft_size].iter().zip(window.iter()))
        {
            *inp = s * w;
        }
        fft.process(&mut input, &mut spectrum).expect("FFT failed");
        frames.push(spectrum.to_vec());
        pos += hop_size;
    }
    frames
}

/// Returns `(per_bin_coherence, per_frame_coherence_map)`.
///
/// `per_bin_coherence`: one [0,1] value per bin — low RMS deviation = high coherence.
/// `per_frame_coherence_map`: shape [n_frames-1][n_bins].
fn compute_phase_coherence_summary(
    frames: &[Vec<Complex32>],
    fft_size: usize,
    hop_size: usize,
) -> (Vec<f32>, Vec<Vec<f32>>) {
    let n_bins = frames.first().map(|f| f.len()).unwrap_or(fft_size / 2 + 1);

    if frames.len() < 2 {
        return (vec![0.5f32; n_bins], Vec::new());
    }

    let mut sum_dev_sq = vec![0.0f64; n_bins];
    let mut count_dev = vec![0usize; n_bins];
    let mut coherence_frames: Vec<Vec<f32>> = Vec::with_capacity(frames.len() - 1);

    for t in 0..frames.len() - 1 {
        let mut frame_coh = vec![0.0f32; n_bins];
        for k in 0..n_bins {
            let mag_prev = frames[t][k].norm();
            let mag_next = frames[t + 1][k].norm();
            // Gate: skip very quiet bins — phase is undefined there.
            if mag_prev < 1e-8 || mag_next < 1e-8 {
                frame_coh[k] = 0.0;
                continue;
            }
            // Expected phase advance at bin k per hop.
            let expected = 2.0 * PI * k as f32 * hop_size as f32 / fft_size as f32;
            // Actual phase advance: arg(X[t+1] · conj(X[t])).
            let cross = frames[t + 1][k] * frames[t][k].conj();
            let actual = cross.arg();
            let deviation = wrap_to_pi(actual - expected);
            // Immediate frame coherence: 1 at zero deviation, 0 at π.
            frame_coh[k] = 1.0 / (1.0 + deviation.abs() / PI);
            sum_dev_sq[k] += (deviation * deviation) as f64;
            count_dev[k] += 1;
        }
        coherence_frames.push(frame_coh);
    }

    // Per-bin coherence from RMS phase deviation over all frame pairs.
    let per_bin: Vec<f32> = (0..n_bins)
        .map(|k| {
            let n = count_dev[k];
            if n == 0 {
                return 0.0;
            }
            let rms_dev = (sum_dev_sq[k] / n as f64).sqrt() as f32;
            (1.0 - (rms_dev / PI).min(1.0)).max(0.0)
        })
        .collect();

    (per_bin, coherence_frames)
}

fn wrap_to_pi(x: f32) -> f32 {
    let tau = 2.0 * PI;
    let x = x % tau;
    if x > PI {
        x - tau
    } else if x < -PI {
        x + tau
    } else {
        x
    }
}

/// Average magnitude spectrum across all spectrogram frames.
fn compute_avg_spectrum(columns: &[SpectrogramColumn]) -> Vec<f32> {
    if columns.is_empty() {
        return Vec::new();
    }
    let n = columns[0].magnitudes.len();
    let mut avg = vec![0.0f32; n];
    for col in columns {
        for (a, &m) in avg.iter_mut().zip(col.magnitudes.iter()) {
            *a += m;
        }
    }
    let count = columns.len() as f32;
    avg.iter_mut().for_each(|v| *v /= count);
    avg
}

/// Harmonic Product Spectrum fundamental frequency detector.
/// Returns the FFT bin of the detected fundamental (None if detection fails).
fn detect_fundamental_hps(avg_spectrum: &[f32]) -> Option<usize> {
    let n = avg_spectrum.len();
    if n < 8 {
        return None;
    }
    // HPS using 4 harmonics; limit by 4× downsampling.
    let hps_len = n / 4;
    if hps_len < 2 {
        return None;
    }
    let mut hps = vec![0.0f32; hps_len];
    for k in 1..hps_len {
        let k2 = (k * 2).min(n - 1);
        let k3 = (k * 3).min(n - 1);
        let k4 = (k * 4).min(n - 1);
        hps[k] = avg_spectrum[k] * avg_spectrum[k2] * avg_spectrum[k3] * avg_spectrum[k4];
    }
    // Skip first 1 % of bins to avoid DC / subharmonic artefacts.
    let min_bin = (hps_len / 100).max(1);
    for val in hps.iter_mut().take(min_bin) {
        *val = 0.0;
    }
    let (peak_k, peak_v) = hps
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())?;
    if peak_k == 0 || *peak_v <= 0.0 {
        None
    } else {
        Some(peak_k)
    }
}

/// Extract normalised harmonic amplitudes and fit a power-law decay exponent.
fn compute_harmonic_decay(
    avg_spectrum: &[f32],
    fundamental_bin: usize,
    max_freq: f64,
    freq_resolution: f64,
) -> (Vec<f32>, f32, bool, Vec<usize>) {
    let n = avg_spectrum.len();
    let mut amplitudes = Vec::new();

    for h in 1..=8usize {
        let bin = fundamental_bin * h;
        if bin >= n {
            break;
        }
        let freq = bin as f64 * freq_resolution;
        if freq > max_freq {
            break;
        }
        // Peak within ±1 bin window for robustness.
        let lo = bin.saturating_sub(1);
        let hi = (bin + 1).min(n - 1);
        let amp = avg_spectrum[lo..=hi].iter().copied().fold(0.0f32, f32::max);
        amplitudes.push(amp);
    }

    if amplitudes.is_empty() {
        return (vec![], 1.0, true, vec![]);
    }

    let a1 = amplitudes[0].max(1e-10);
    let normalised: Vec<f32> = amplitudes.iter().map(|&a| a / a1).collect();

    // Least-squares fit of log(A_n / A1) = -α * log(n).
    let decay_exponent = if normalised.len() >= 2 {
        let mut num = 0.0f32;
        let mut den = 0.0f32;
        for (i, &amp) in normalised.iter().enumerate().skip(1) {
            let log_n = ((i + 1) as f32).ln();
            let log_a = amp.max(1e-10).ln();
            num += -log_a * log_n;
            den += log_n * log_n;
        }
        if den > 0.0 {
            num / den
        } else {
            1.0
        }
    } else {
        1.0
    };

    // Detect monotonicity anomalies.
    let mut decay_is_monotonic = true;
    let mut anomaly_indices = Vec::new();
    for i in 1..normalised.len() {
        // Flag if this harmonic has ≥ 95 % of the previous one's energy.
        if normalised[i] >= normalised[i - 1] * 0.95 {
            decay_is_monotonic = false;
            anomaly_indices.push(i);
        }
    }

    (
        normalised,
        decay_exponent,
        decay_is_monotonic,
        anomaly_indices,
    )
}

/// Coherence ratio: mean coherence at harmonic bins divided by overall mean.
fn compute_harmonic_coherence_ratio(
    phase_coherence: &[f32],
    fundamental_bin: Option<usize>,
) -> f32 {
    let Some(f_bin) = fundamental_bin else {
        return 0.5;
    };
    if phase_coherence.is_empty() {
        return 0.5;
    }
    let mean_all = phase_coherence.iter().copied().sum::<f32>() / phase_coherence.len() as f32;
    if mean_all < 1e-6 {
        return 0.5;
    }
    let n = phase_coherence.len();
    let mut harmonic_sum = 0.0f32;
    let mut harmonic_count = 0usize;
    for h in 1..=5usize {
        let bin = f_bin * h;
        if bin >= n {
            break;
        }
        harmonic_sum += phase_coherence[bin];
        harmonic_count += 1;
    }
    if harmonic_count == 0 {
        return 0.5;
    }
    let harmonic_mean = harmonic_sum / harmonic_count as f32;
    (harmonic_mean / mean_all).clamp(0.0, 2.0)
}

/// Half-wave-rectified onset spectral flux per frame transition.
fn compute_spectral_flux_frames(columns: &[SpectrogramColumn]) -> Vec<f32> {
    if columns.len() < 2 {
        return Vec::new();
    }
    let mut flux = Vec::with_capacity(columns.len() - 1);
    for t in 1..columns.len() {
        let prev = &columns[t - 1].magnitudes;
        let curr = &columns[t].magnitudes;
        let f: f32 = prev
            .iter()
            .zip(curr.iter())
            .map(|(&p, &c)| {
                let diff = c - p;
                if diff > 0.0 {
                    diff * diff
                } else {
                    0.0
                }
            })
            .sum::<f32>()
            .sqrt();
        flux.push(f);
    }
    flux
}

/// Count frames where flux is non-trivial but a much larger onset follows shortly after.
fn count_preringing(flux: &[f32], flux_peak: f32) -> usize {
    if flux.is_empty() || flux_peak < 1e-10 {
        return 0;
    }
    let onset_threshold = flux_peak * 0.4;
    let preflux_threshold = flux_peak * 0.12;
    let look_ahead = 5usize;
    let mut count = 0usize;
    for t in 0..flux.len() {
        if flux[t] < preflux_threshold || flux[t] >= onset_threshold {
            continue;
        }
        let window_end = (t + 1 + look_ahead).min(flux.len());
        let has_onset_after = flux[t + 1..window_end].iter().any(|&f| f > onset_threshold);
        if has_onset_after {
            count += 1;
        }
    }
    count
}

/// Staircasing score: fraction of active transitions where peak bin does not move.
fn compute_staircasing_score(columns: &[SpectrogramColumn], flux: &[f32], flux_peak: f32) -> f32 {
    if columns.len() < 2 || flux_peak < 1e-10 {
        return 0.0;
    }
    let flux_threshold = flux_peak * 0.1;
    let mut stuck = 0usize;
    let mut total_active = 0usize;

    for t in 1..columns.len() {
        let f = if t - 1 < flux.len() { flux[t - 1] } else { 0.0 };
        if f < flux_threshold {
            continue;
        }
        total_active += 1;
        let prev_peak = peak_bin(&columns[t - 1].magnitudes);
        let curr_peak = peak_bin(&columns[t].magnitudes);
        if prev_peak == curr_peak {
            stuck += 1;
        }
    }

    if total_active == 0 {
        0.0
    } else {
        stuck as f32 / total_active as f32
    }
}

fn peak_bin(magnitudes: &[f32]) -> usize {
    magnitudes
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
        .map(|(k, _)| k)
        .unwrap_or(0)
}

fn mean_f32(v: &[f32]) -> f32 {
    if v.is_empty() {
        0.0
    } else {
        v.iter().sum::<f32>() / v.len() as f32
    }
}

// ---------------------------------------------------------------------------
// Tile-based phase coherence rendering
// ---------------------------------------------------------------------------

/// Compute phase coherence tile data for a range of audio samples.
///
/// Returns a `PreRendered` with absolute dB values and phase deviation shifts
/// for deferred compositing at render time.
///
/// `samples`: raw audio covering at least `(col_count + 1) * hop_size + fft_size` samples.
/// `col_count`: number of output columns (typically 256 / TILE_COLS).
/// `fft_size`, `hop_size`: STFT parameters matching the main spectrogram.
pub fn compute_tile_phase_data(
    samples: &[f32],
    col_count: usize,
    fft_size: usize,
    hop_size: usize,
) -> crate::types::PreRendered {
    use crate::canvas::colors::magnitude_to_db;

    // Compute complex STFT frames. We need col_count + 1 frames to produce
    // col_count phase-deviation columns (deviation between consecutive frames).
    let n_frames_needed = col_count + 1;
    let fft = plan_fft_forward(fft_size);
    let window = hann_window(fft_size);
    let n_bins = fft_size / 2 + 1;

    let mut frames: Vec<Vec<Complex32>> = Vec::with_capacity(n_frames_needed);
    let mut input = fft.make_input_vec();
    let mut spectrum = fft.make_output_vec();

    for f_idx in 0..n_frames_needed {
        let pos = f_idx * hop_size;
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
        frames.push(spectrum.to_vec());
    }

    let actual_cols = if frames.len() >= 2 {
        frames.len() - 1
    } else {
        0
    };
    let width = actual_cols.max(1) as u32;
    let height = n_bins as u32;
    let total = (width as usize) * (height as usize);
    let mut db_data = vec![f32::NEG_INFINITY; total];
    let mut flow_shifts = vec![0.0f32; total];

    for col in 0..actual_cols {
        let frame_prev = &frames[col];
        let frame_curr = &frames[col + 1];

        for k in 0..n_bins {
            let mag = frame_curr[k].norm();
            let mag_prev = frame_prev[k].norm();

            let row = n_bins - 1 - k;
            let idx = row * width as usize + col;

            // Store magnitude as absolute dB
            db_data[idx] = magnitude_to_db(mag);

            // Gate: skip very quiet bins — phase is undefined there
            if mag < 1e-8 || mag_prev < 1e-8 {
                flow_shifts[idx] = 0.0; // neutral
            } else {
                // Expected phase advance at bin k per hop
                let expected = 2.0 * PI * k as f32 * hop_size as f32 / fft_size as f32;
                // Actual phase advance: arg(X[t+1] · conj(X[t]))
                let cross = frame_curr[k] * frame_prev[k].conj();
                let actual = cross.arg();
                let deviation = wrap_to_pi(actual - expected);

                // Normalize deviation from [-PI, PI] to [-1.0, 1.0]
                flow_shifts[idx] = deviation / PI;
            }
        }
    }

    crate::types::PreRendered {
        width,
        height,
        pixels: Vec::new(),
        db_data,
        flow_shifts,
    }
}

/// Compute instantaneous phase angle tile data.
///
/// Unlike `compute_tile_phase_data` which stores inter-frame phase *deviation*,
/// this stores the raw phase angle of each bin in each frame, normalized to
/// [-1.0, 1.0] (from [-PI, PI]). Hue is mapped from the phase angle.
pub fn compute_tile_phase_angle_data(
    samples: &[f32],
    col_count: usize,
    fft_size: usize,
    hop_size: usize,
) -> crate::types::PreRendered {
    use crate::canvas::colors::magnitude_to_db;

    let fft = plan_fft_forward(fft_size);
    let window = hann_window(fft_size);
    let n_bins = fft_size / 2 + 1;

    let mut input = fft.make_input_vec();
    let mut spectrum = fft.make_output_vec();

    let mut actual_cols = 0usize;
    let width = col_count.max(1) as u32;
    let height = n_bins as u32;
    let total = (width as usize) * (height as usize);
    let mut db_data = vec![f32::NEG_INFINITY; total];
    let mut flow_shifts = vec![0.0f32; total];

    for col in 0..col_count {
        let pos = col * hop_size;
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

        for (k, spec_bin) in spectrum.iter().enumerate().take(n_bins) {
            let mag = spec_bin.norm();
            let row = n_bins - 1 - k;
            let idx = row * width as usize + col;

            db_data[idx] = magnitude_to_db(mag);

            // Store instantaneous phase normalized to [-1, 1]
            if mag < 1e-8 {
                flow_shifts[idx] = 0.0;
            } else {
                flow_shifts[idx] = spec_bin.arg() / PI;
            }
        }
        actual_cols += 1;
    }

    let final_width = actual_cols.max(1) as u32;
    // If we computed fewer columns than expected, truncate
    if final_width < width {
        let fw = final_width as usize;
        let fh = height as usize;
        let mut new_db = vec![f32::NEG_INFINITY; fw * fh];
        let mut new_shifts = vec![0.0f32; fw * fh];
        for row in 0..fh {
            for col in 0..fw {
                new_db[row * fw + col] = db_data[row * (width as usize) + col];
                new_shifts[row * fw + col] = flow_shifts[row * (width as usize) + col];
            }
        }
        db_data = new_db;
        flow_shifts = new_shifts;
    }

    crate::types::PreRendered {
        width: final_width,
        height,
        pixels: Vec::new(),
        db_data,
        flow_shifts,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn col(mags: Vec<f32>) -> SpectrogramColumn {
        SpectrogramColumn {
            magnitudes: mags,
            time_offset: 0.0,
        }
    }

    #[test]
    fn wrap_to_pi_clamps_into_canonical_range() {
        assert!((wrap_to_pi(0.0) - 0.0).abs() < 1e-6);
        assert!((wrap_to_pi(PI - 0.01) - (PI - 0.01)).abs() < 1e-5);
        // 3π/2 → -π/2 after wrap.
        assert!((wrap_to_pi(1.5 * PI) - (-0.5 * PI)).abs() < 1e-5);
        // -3π/2 → π/2.
        assert!((wrap_to_pi(-1.5 * PI) - 0.5 * PI).abs() < 1e-5);
    }

    #[test]
    fn mean_f32_handles_empty_and_nonempty() {
        assert_eq!(mean_f32(&[]), 0.0);
        assert!((mean_f32(&[1.0, 2.0, 3.0, 4.0]) - 2.5).abs() < 1e-6);
    }

    #[test]
    fn peak_bin_returns_index_of_max() {
        assert_eq!(peak_bin(&[0.1, 0.5, 0.3, 0.9, 0.2]), 3);
        assert_eq!(peak_bin(&[0.0]), 0);
        assert_eq!(peak_bin(&[]), 0);
    }

    #[test]
    fn compute_avg_spectrum_empty_returns_empty() {
        assert!(compute_avg_spectrum(&[]).is_empty());
    }

    #[test]
    fn compute_avg_spectrum_averages_per_bin() {
        let cols = vec![col(vec![1.0, 2.0, 3.0]), col(vec![3.0, 4.0, 5.0])];
        let avg = compute_avg_spectrum(&cols);
        assert_eq!(avg, vec![2.0, 3.0, 4.0]);
    }

    #[test]
    fn derive_fft_size_clamps_to_minimum_64() {
        // Pathological inputs (huge resolution / weird rate) → still ≥ 64.
        assert_eq!(derive_fft_size(44_100, 100_000.0), 64);
        // Sensible case: 44.1k / 10.77Hz ≈ 4096 bins.
        let n = derive_fft_size(44_100, 44_100.0 / 4096.0);
        assert_eq!(n, 4096);
    }

    #[test]
    fn derive_hop_size_clamps_to_minimum_1() {
        assert_eq!(derive_hop_size(44_100, 0.0), 1);
        // 256 samples at 44.1k ≈ 5.8 ms.
        let h = derive_hop_size(44_100, 256.0 / 44_100.0);
        assert_eq!(h, 256);
    }

    #[test]
    fn compute_spectral_flux_frames_returns_empty_for_short_inputs() {
        assert!(compute_spectral_flux_frames(&[]).is_empty());
        assert!(compute_spectral_flux_frames(&[col(vec![1.0])]).is_empty());
    }

    #[test]
    fn compute_spectral_flux_frames_half_wave_rectifies() {
        // Three columns: rising then falling.
        // t=0->1: bin 0 rises 0.0->2.0 (positive diff² = 4).
        // t=1->2: bin 0 falls 2.0->1.0 (negative diff → ignored).
        let cols = vec![col(vec![0.0]), col(vec![2.0]), col(vec![1.0])];
        let flux = compute_spectral_flux_frames(&cols);
        assert_eq!(flux.len(), 2);
        assert!((flux[0] - 2.0).abs() < 1e-5); // sqrt(4)
        assert_eq!(flux[1], 0.0);
    }

    #[test]
    fn detect_fundamental_hps_finds_fundamental_in_synthetic_spectrum() {
        // Build a spectrum where bin 50 and its harmonics (100, 150, 200) are large.
        let mut spec = vec![0.0f32; 1024];
        for &bin in &[50, 100, 150, 200] {
            spec[bin] = 1.0;
        }
        let detected = detect_fundamental_hps(&spec);
        assert_eq!(detected, Some(50));
    }

    #[test]
    fn detect_fundamental_hps_returns_none_for_silent_spectrum() {
        let spec = vec![0.0f32; 1024];
        assert_eq!(detect_fundamental_hps(&spec), None);
    }

    #[test]
    fn count_preringing_returns_zero_for_empty_flux() {
        assert_eq!(count_preringing(&[], 1.0), 0);
        assert_eq!(count_preringing(&[0.5, 0.6, 0.7], 0.0), 0);
    }

    #[test]
    fn count_preringing_counts_low_flux_before_onset() {
        // peak=1.0, onset_threshold=0.4, preflux=[0.12, 0.4).
        // A frame in [0.12, 0.4) followed within 5 frames by a >0.4 spike counts.
        let peak = 1.0;
        let flux = vec![0.0, 0.2, 0.25, 1.0, 0.0, 0.0];
        // Frames 1 and 2 are in the preflux band, followed by frame 3's onset.
        assert_eq!(count_preringing(&flux, peak), 2);
    }
}
