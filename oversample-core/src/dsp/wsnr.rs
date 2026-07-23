use realfft::RealFftPlanner;

/// wSNR grade thresholds (from xeno-canto wSNR v38J).
/// Note: the plugin uses 49.5/34.5/19.5 but the article says 50/35/20/5.
/// We use the plugin's half-dB-offset thresholds for compatibility.
#[derive(Clone, Debug, PartialEq)]
pub enum WsnrGrade {
    A,
    B,
    C,
    D,
    E,
}

impl WsnrGrade {
    pub fn from_snr(db: f64) -> Self {
        if db > 49.5 {
            WsnrGrade::A
        } else if db > 34.5 {
            WsnrGrade::B
        } else if db > 19.5 {
            WsnrGrade::C
        } else if db > 4.5 {
            WsnrGrade::D
        } else {
            WsnrGrade::E
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            WsnrGrade::A => "A",
            WsnrGrade::B => "B",
            WsnrGrade::C => "C",
            WsnrGrade::D => "D",
            WsnrGrade::E => "E",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FadeInfo {
    pub fade_in_secs: f64,
    pub fade_out_secs: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WsnrResult {
    pub snr_db: f64,
    pub grade: WsnrGrade,
    pub signal_db: f64,
    pub noise_db: f64,
    pub fade: FadeInfo,
    pub is_clipped: bool,
    pub clipping_samples: usize,
    pub is_ultrasonic: bool,
    pub dense_soundscape: bool,
    pub dense_pct: f64,
    pub has_silent_gaps: bool,
    pub warnings: Vec<String>,
}

/// Main wSNR analysis entry point.
pub fn analyze_wsnr(samples: &[f32], sample_rate: u32) -> WsnrResult {
    let duration = samples.len() as f64 / sample_rate as f64;
    let mut warnings = Vec::new();

    // Guard: too short
    if duration < 0.6 {
        warnings.push("Too short recording (<0.6s)".into());
        return WsnrResult {
            snr_db: 0.0,
            grade: WsnrGrade::E,
            signal_db: f64::NEG_INFINITY,
            noise_db: 0.0,
            fade: FadeInfo {
                fade_in_secs: 0.0,
                fade_out_secs: 0.0,
            },
            is_clipped: false,
            clipping_samples: 0,
            is_ultrasonic: false,
            dense_soundscape: false,
            dense_pct: 0.0,
            has_silent_gaps: false,
            warnings,
        };
    }

    // Guard: too low sample rate
    if sample_rate < 30000 {
        warnings.push(format!(
            "Sample rate too low for wSNR ({} Hz, need >= 30kHz)",
            sample_rate
        ));
        return WsnrResult {
            snr_db: 0.0,
            grade: WsnrGrade::E,
            signal_db: f64::NEG_INFINITY,
            noise_db: 0.0,
            fade: FadeInfo {
                fade_in_secs: 0.0,
                fade_out_secs: 0.0,
            },
            is_clipped: false,
            clipping_samples: 0,
            is_ultrasonic: false,
            dense_soundscape: false,
            dense_pct: 0.0,
            has_silent_gaps: false,
            warnings,
        };
    }

    // Step 1: Clipping detection
    let (is_clipped, clipping_samples, peak_amplitude) = detect_clipping(samples);
    if is_clipped {
        warnings.push(format!(
            "Recording clipped ({} samples at {:.1} dB)",
            clipping_samples,
            linear_to_db(peak_amplitude)
        ));
    }

    // Step 2: Ultrasonic content detection
    let (is_ultrasonic, ultrasonic_msg) = detect_ultrasonic(samples, sample_rate);
    if let Some(msg) = ultrasonic_msg {
        warnings.push(msg);
    }

    // Step 3: Guard silent files
    let peak = samples.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
    if peak < 1e-10 {
        warnings.push("File appears silent".into());
        return WsnrResult {
            snr_db: 0.0,
            grade: WsnrGrade::E,
            signal_db: f64::NEG_INFINITY,
            noise_db: 0.0,
            fade: FadeInfo {
                fade_in_secs: 0.0,
                fade_out_secs: 0.0,
            },
            is_clipped,
            clipping_samples,
            is_ultrasonic,
            dense_soundscape: false,
            dense_pct: 0.0,
            has_silent_gaps: false,
            warnings,
        };
    }

    // Step 4: HP filter at 100Hz for fade/transient detection (matches plugin)
    let hp_samples = highpass_simple(samples, 100.0, sample_rate);

    // Step 5: Transient detection
    let start_transient = detect_start_transient(&hp_samples, sample_rate);
    let end_transient = detect_end_transient(&hp_samples, sample_rate);

    // Step 6: Fade detection
    let (fade_in_secs, fade_out_secs) = detect_fades(&hp_samples, sample_rate);

    // Use the larger of fade vs transient exclusion (as the plugin does)
    let start_exclude = if start_transient {
        fade_in_secs.max(0.2)
    } else {
        fade_in_secs
    };
    let end_exclude = if end_transient {
        fade_out_secs.max(0.7)
    } else {
        fade_out_secs
    };

    if start_transient && start_exclude <= 0.2 {
        warnings.push("Start transient removed (0.2s)".into());
    }
    if end_transient && end_exclude <= 0.7 {
        warnings.push("End transient removed (0.7s)".into());
    }
    if fade_in_secs > 0.0 {
        warnings.push(format!(
            "Fade-in detected and excluded ({:.1}s)",
            start_exclude
        ));
    }
    if fade_out_secs > 0.0 {
        warnings.push(format!(
            "Fade-out detected and excluded ({:.1}s)",
            end_exclude
        ));
    }

    // Step 7: Extract analysis region
    let start_sample = (start_exclude * sample_rate as f64) as usize;
    let end_sample = samples
        .len()
        .saturating_sub((end_exclude * sample_rate as f64) as usize);

    if end_sample <= start_sample + (sample_rate as usize / 10) {
        warnings.push("Usable region too short after fade/transient removal".into());
        return WsnrResult {
            snr_db: 0.0,
            grade: WsnrGrade::E,
            signal_db: f64::NEG_INFINITY,
            noise_db: 0.0,
            fade: FadeInfo {
                fade_in_secs,
                fade_out_secs,
            },
            is_clipped,
            clipping_samples,
            is_ultrasonic,
            dense_soundscape: false,
            dense_pct: 0.0,
            has_silent_gaps: false,
            warnings,
        };
    }

    let analysis_region = &samples[start_sample..end_sample];

    // Step 8: ITU-R 468 noise measurement
    let itu_filtered = apply_weighting(analysis_region, sample_rate, itu_r_468_gain, sample_rate);
    // Use ~5ms windows (matching the plugin's 240 samples @ 48kHz) and exclude silent gaps
    let noise_window = ((sample_rate / 200) as usize).clamp(40, 2400);
    let (noise_db, has_silent_gaps) = noise_floor_db(&itu_filtered, noise_window);

    // Step 9: ISO 226 @ 80 phon signal measurement
    let iso_filtered = apply_weighting(
        analysis_region,
        sample_rate,
        iso_226_80phon_gain,
        sample_rate,
    );
    let signal_db = peak_amplitude_db(&iso_filtered);

    // Step 10: Dense soundscape detection
    let (dense_soundscape, dense_pct) = detect_dense_soundscape(&itu_filtered, noise_db);
    if dense_soundscape {
        warnings.push(format!("(Beta) Dense soundscape detected ({:.1}% near noise floor) \u{2014} measurement may be less accurate", dense_pct));
    }

    // Step 11: Compute SNR and grade
    let snr_db = signal_db - noise_db;
    let grade = WsnrGrade::from_snr(snr_db);

    if snr_db < 20.0 && !dense_soundscape {
        warnings.push("Low SNR detected \u{2014} reliability may be reduced below 20 dB".into());
    }
    if has_silent_gaps {
        warnings.push(
            "Silent gaps detected \u{2014} noise floor estimated from active audio only; \
             SNR may be overestimated in files with very low ITU-band noise"
                .into(),
        );
    }
    if is_ultrasonic && grade == WsnrGrade::A {
        warnings.push(
            "A grade reflects low audible-band noise (expected for ultrasonic equipment); \
             does not assess bat call quality"
                .into(),
        );
    }

    WsnrResult {
        snr_db,
        grade,
        signal_db,
        noise_db,
        fade: FadeInfo {
            fade_in_secs,
            fade_out_secs,
        },
        is_clipped,
        clipping_samples,
        is_ultrasonic,
        dense_soundscape,
        dense_pct,
        has_silent_gaps,
        warnings,
    }
}

// ---------------------------------------------------------------------------
// Clipping detection
// ---------------------------------------------------------------------------

fn detect_clipping(samples: &[f32]) -> (bool, usize, f32) {
    let mut peak: f32 = 0.0;
    let mut clip_count = 0usize;
    for &s in samples {
        let a = s.abs();
        if a > peak {
            peak = a;
        }
        if a > 0.999 {
            clip_count += 1;
        }
    }
    let is_clipped = peak > 0.9999 && clip_count > 4;
    (is_clipped, clip_count, peak)
}

// ---------------------------------------------------------------------------
// Ultrasonic content detection
// ---------------------------------------------------------------------------

fn detect_ultrasonic(samples: &[f32], sample_rate: u32) -> (bool, Option<String>) {
    if sample_rate <= 48000 {
        return (false, None);
    }

    let fft_size = 4096;
    let bin_20k = (20000.0 / (sample_rate as f64 / fft_size as f64)) as usize;
    let num_bins = fft_size / 2 + 1;

    if bin_20k >= num_bins {
        return (false, None);
    }

    // Analyze a few windows from the middle of the file
    let mut planner = RealFftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(fft_size);

    let window: Vec<f32> = (0..fft_size)
        .map(|i| {
            0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / (fft_size - 1) as f32).cos())
        })
        .collect();

    let mid = samples.len() / 2;
    let mut total_below = 0.0f64;
    let mut total_above = 0.0f64;
    let mut windows_analyzed = 0;

    for offset in [0, fft_size, fft_size * 2] {
        let start = mid.saturating_sub(fft_size * 2) + offset;
        if start + fft_size > samples.len() {
            continue;
        }

        let mut frame: Vec<f32> = samples[start..start + fft_size]
            .iter()
            .zip(window.iter())
            .map(|(&s, &w)| s * w)
            .collect();

        let mut spectrum = fft.make_output_vec();
        if fft.process(&mut frame, &mut spectrum).is_ok() {
            for (i, c) in spectrum.iter().enumerate() {
                let energy = (c.re * c.re + c.im * c.im) as f64;
                if i < bin_20k {
                    total_below += energy;
                } else {
                    total_above += energy;
                }
            }
            windows_analyzed += 1;
        }
    }

    if windows_analyzed == 0 {
        return (false, None);
    }

    let total = total_below + total_above;
    if total < 1e-20 {
        return (false, None);
    }

    let ratio = total_above / total;
    if ratio > 0.5 {
        (true, Some(format!(
            "Ultrasonic content ({}kHz SR, {:.0}% energy above 20kHz). wSNR reflects audible-band quality only.",
            sample_rate / 1000, ratio * 100.0
        )))
    } else if ratio > 0.001 && sample_rate >= 256000 {
        // High sample rate with some detectable ultrasonic energy — likely a bat detector
        // recording where the target calls didn't happen to land in the analysis windows.
        (false, Some(format!(
            "Possible ultrasonic content ({}kHz SR, {:.0}% energy above 20kHz) \u{2014} wSNR reflects audible-band quality only.",
            sample_rate / 1000, ratio * 100.0
        )))
    } else {
        (false, None)
    }
}

// ---------------------------------------------------------------------------
// Transient detection (matches plugin: compare peak in edge vs reference region)
// ---------------------------------------------------------------------------

fn detect_start_transient(samples: &[f32], sample_rate: u32) -> bool {
    let s02 = (0.2 * sample_rate as f64) as usize;
    let s12 = (1.2 * sample_rate as f64) as usize;
    if s12 >= samples.len() {
        return false;
    }
    let peak_edge = peak_in_range(samples, 0, s02);
    let peak_ref = peak_in_range(samples, s02, s12);
    peak_ref > 1e-10 && peak_edge > peak_ref * 2.0
}

fn detect_end_transient(samples: &[f32], sample_rate: u32) -> bool {
    let len = samples.len();
    let s07 = (0.7 * sample_rate as f64) as usize;
    let s17 = (1.7 * sample_rate as f64) as usize;
    if s17 >= len {
        return false;
    }
    let peak_edge = peak_in_range(samples, len - s07, len);
    let peak_ref = peak_in_range(samples, len - s17, len - s07);
    peak_ref > 1e-10 && peak_edge > peak_ref * 2.0
}

fn peak_in_range(samples: &[f32], start: usize, end: usize) -> f32 {
    let end = end.min(samples.len());
    let start = start.min(end);
    samples[start..end]
        .iter()
        .map(|s| s.abs())
        .fold(0.0f32, f32::max)
}

// ---------------------------------------------------------------------------
// Fade detection (ported from hittatoning2 in wSNR_v38J.ny)
//
// The plugin compares RMS in progressively deeper slices from each end to
// the RMS of the middle region.  A slice whose RMS is more than 1 dB below
// the middle is considered part of a fade.  The function walks outward
// through time checkpoints and records the furthest one still below threshold.
// ---------------------------------------------------------------------------

fn detect_fades(hp_samples: &[f32], sample_rate: u32) -> (f64, f64) {
    let duration = hp_samples.len() as f64 / sample_rate as f64;

    // Get middle RMS
    let mid_rms_db = {
        let (mid_start, mid_end) = if duration > 30.0 {
            (10.0, duration - 10.0)
        } else if duration > 15.0 {
            (3.0, duration - 3.0)
        } else if duration > 8.0 {
            (2.0, duration - 2.0)
        } else if duration > 5.0 {
            (1.0, duration - 1.0)
        } else if duration > 2.0 {
            (0.5, duration - 0.5)
        } else {
            (0.3, duration - 0.3)
        };
        let s = (mid_start * sample_rate as f64) as usize;
        let e = (mid_end * sample_rate as f64) as usize;
        rms_of_range(hp_samples, s, e)
    };

    if mid_rms_db < -100.0 {
        return (0.0, 0.0);
    }

    // Time checkpoints from the plugin (hittatoning2), from edge inward
    // The plugin checks each independently and keeps advancing if still below threshold
    let checkpoints: &[(f64, f64)] = &[
        // (time_from_edge, min_duration_required)
        (0.10, 0.0),
        (0.20, 0.0),
        (0.30, 2.0),
        (0.40, 2.0),
        (0.50, 2.0),
        (0.75, 5.0),
        (1.00, 5.0),
        (1.25, 8.0),
        (1.50, 8.0),
        (2.00, 8.0),
        (2.50, 8.0),
        (3.00, 15.0),
        (4.00, 15.0),
        (5.00, 30.0),
        (6.00, 30.0),
        (7.00, 30.0),
        (8.00, 30.0),
        (9.00, 30.0),
        (10.00, 30.0),
    ];

    let sense_level = -1.0; // matches hittatoning2

    // Check if the very first 0-0.1s slice is more than 10dB below middle
    // If not, there's no fade (matches the plugin's initial guard)
    let first_slice_rms = rms_of_range(hp_samples, 0, (0.1 * sample_rate as f64) as usize);

    let fade_in = if first_slice_rms - mid_rms_db > -10.0 {
        0.0
    } else {
        let mut fade = 0.2; // default if first slice is quiet
        for &(t, min_dur) in &checkpoints[2..] {
            if duration < min_dur {
                break;
            }
            let prev_t = t - checkpoints
                .iter()
                .find(|&&(ct, _)| ct == t)
                .map(|_| {
                    // Get the slice: from prev checkpoint to this one
                    // But for simplicity, just measure from (t - slice_width) to t
                    0.0
                })
                .unwrap_or(0.0);
            let _ = prev_t; // not needed with the simpler approach below
                            // Measure RMS in a slice around this checkpoint from the start
            let slice_end = (t * sample_rate as f64) as usize;
            let slice_start = if t >= 0.1 {
                ((t - 0.1) * sample_rate as f64) as usize
            } else {
                0
            };
            if slice_end >= hp_samples.len() {
                break;
            }
            let slice_rms = rms_of_range(hp_samples, slice_start, slice_end);
            if slice_rms - mid_rms_db < sense_level {
                fade = t;
            }
        }
        fade
    };

    // Same for fade-out (from end)
    let last_slice_rms = rms_of_range(
        hp_samples,
        hp_samples
            .len()
            .saturating_sub((0.1 * sample_rate as f64) as usize),
        hp_samples.len(),
    );

    let fade_out = if last_slice_rms - mid_rms_db > -10.0 {
        0.0
    } else {
        let mut fade = 0.2;
        for &(t, min_dur) in &checkpoints[2..] {
            if duration < min_dur {
                break;
            }
            let from_end = hp_samples
                .len()
                .saturating_sub((t * sample_rate as f64) as usize);
            let slice_to = hp_samples
                .len()
                .saturating_sub(((t - 0.1) * sample_rate as f64) as usize);
            if from_end >= slice_to || from_end >= hp_samples.len() {
                break;
            }
            let slice_rms = rms_of_range(hp_samples, from_end, slice_to);
            if slice_rms - mid_rms_db < sense_level {
                fade = t;
            }
        }
        fade
    };

    (fade_in, fade_out)
}

/// Compute RMS of a range in dB.
fn rms_of_range(samples: &[f32], start: usize, end: usize) -> f64 {
    let end = end.min(samples.len());
    let start = start.min(end);
    let count = end - start;
    if count == 0 {
        return -120.0;
    }
    let sum_sq: f64 = samples[start..end]
        .iter()
        .map(|&s| (s as f64) * (s as f64))
        .sum();
    let rms = (sum_sq / count as f64).sqrt();
    linear_to_db(rms as f32)
}

// ---------------------------------------------------------------------------
// Simple high-pass filter (single-pole IIR) for fade/transient detection
// ---------------------------------------------------------------------------

fn highpass_simple(samples: &[f32], cutoff_hz: f64, sample_rate: u32) -> Vec<f32> {
    if samples.is_empty() {
        return Vec::new();
    }
    let rc = 1.0 / (2.0 * std::f64::consts::PI * cutoff_hz);
    let dt = 1.0 / sample_rate as f64;
    let alpha = (rc / (rc + dt)) as f32;

    let mut output = Vec::with_capacity(samples.len());
    let mut prev_in = samples[0];
    let mut prev_out = 0.0f32;
    output.push(0.0);

    for &sample in &samples[1..] {
        let out = alpha * (prev_out + sample - prev_in);
        output.push(out);
        prev_in = sample;
        prev_out = out;
    }
    output
}

// ---------------------------------------------------------------------------
// Frequency-domain weighting filter (overlap-add FFT)
// Adapted from filters.rs apply_eq_filter pattern
// ---------------------------------------------------------------------------

fn apply_weighting(
    samples: &[f32],
    sample_rate: u32,
    gain_fn: fn(f64, u32) -> f32,
    sr: u32,
) -> Vec<f32> {
    if samples.is_empty() {
        return Vec::new();
    }

    let fft_size = 4096;
    let hop_size = fft_size / 2;
    let len = samples.len();

    let window: Vec<f32> = (0..fft_size)
        .map(|i| {
            0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / (fft_size - 1) as f32).cos())
        })
        .collect();

    // Build per-bin gain table
    let num_bins = fft_size / 2 + 1;
    let freq_per_bin = sample_rate as f64 / fft_size as f64;
    let gains: Vec<f32> = (0..num_bins)
        .map(|i| {
            let freq = i as f64 * freq_per_bin;
            gain_fn(freq, sr)
        })
        .collect();

    let mut planner = RealFftPlanner::<f32>::new();
    let fft_fwd = planner.plan_fft_forward(fft_size);
    let fft_inv = planner.plan_fft_inverse(fft_size);

    let mut output = vec![0.0f32; len];
    let mut window_sum = vec![0.0f32; len];

    let mut pos = 0;
    while pos < len {
        let mut frame = vec![0.0f32; fft_size];
        for (i, &w) in window.iter().enumerate() {
            if pos + i < len {
                frame[i] = samples[pos + i] * w;
            }
        }

        let mut spectrum = fft_fwd.make_output_vec();
        fft_fwd
            .process(&mut frame, &mut spectrum)
            .expect("FFT forward failed");

        for (bin, gain) in gains.iter().enumerate() {
            if bin < spectrum.len() {
                spectrum[bin] *= *gain;
            }
        }

        let mut time_out = fft_inv.make_output_vec();
        fft_inv
            .process(&mut spectrum, &mut time_out)
            .expect("FFT inverse failed");

        let norm = 1.0 / fft_size as f32;

        for i in 0..fft_size {
            if pos + i < len {
                output[pos + i] += time_out[i] * norm * window[i];
                window_sum[pos + i] += window[i] * window[i];
            }
        }

        pos += hop_size;
    }

    for i in 0..len {
        if window_sum[i] > 1e-6 {
            output[i] /= window_sum[i];
        }
    }

    output
}

// ---------------------------------------------------------------------------
// ITU-R 468 weighting curve (noise measurement)
//
// From the plugin: hp@6500Hz + lowpass2@9700Hz + lowpass4@9700Hz + gain correction.
// We approximate this as a bandpass 6500-9700Hz with appropriate rolloff slopes
// plus the sample-rate-dependent gain correction.
// ---------------------------------------------------------------------------

fn itu_r_468_gain(freq: f64, sample_rate: u32) -> f32 {
    // High-pass at 6500Hz: simple 1st-order rolloff below 6500
    let hp_gain = if freq < 1.0 {
        0.0
    } else {
        let ratio = freq / 6500.0;
        // 1st order HP: ratio / sqrt(1 + ratio^2)
        ratio / (1.0 + ratio * ratio).sqrt()
    };

    // Low-pass at 9700Hz: the plugin uses lowpass2 + lowpass4 (total ~6th order ≈ -36dB/oct)
    let lp_gain = if freq < 1.0 {
        1.0
    } else {
        let ratio = freq / 9700.0;
        // ~6th order LP approximation: 1 / (1 + ratio^2)^3
        1.0 / (1.0 + ratio * ratio).powf(3.0)
    };

    // Gain correction from the plugin (varies by sample rate)
    let gain_correction_db = if sample_rate == 48000 {
        17.0 + 2.6
    } else if sample_rate <= 44100 {
        17.0 + 2.9
    } else if sample_rate == 96000 {
        17.0 + 1.1
    } else {
        17.0
    };
    let gain_correction = 10.0_f64.powf(gain_correction_db / 20.0);

    (hp_gain * lp_gain * gain_correction) as f32
}

// ---------------------------------------------------------------------------
// ISO 226 @ 80 phon weighting curve (signal measurement)
//
// From the plugin:
//   hp@220Hz (x2) + lp@15500Hz (if SR>31kHz, 8th order x2)
//   + eq-band 80Hz +4dB Q2.5
//   + eq-band 800Hz +3dB Q2
//   + eq-band 1600Hz -8dB Q1
//   + eq-band 3000Hz +8dB Q2.2
//   + eq-band 8900Hz -14dB Q1.4
//   + eq-band 19000Hz +5 or +12dB Q0.7 (depending on SR)
// ---------------------------------------------------------------------------

fn iso_226_80phon_gain(freq: f64, sample_rate: u32) -> f32 {
    if freq < 0.1 {
        return 0.0;
    }

    // Double high-pass at 220Hz (two cascaded 1st-order = 2nd order, -12dB/oct)
    let hp_ratio = freq / 220.0;
    let hp_gain = {
        let single = hp_ratio / (1.0 + hp_ratio * hp_ratio).sqrt();
        single * single // two cascaded
    };

    // Low-pass at 15500Hz (only if SR > 31kHz, which is always true for us since we guard SR >= 30kHz)
    // The plugin uses lowpass8 x2 = ~16th order
    let lp_gain = if sample_rate > 31000 {
        let lp_ratio = freq / 15500.0;
        1.0 / (1.0 + lp_ratio.powi(2)).powf(8.0)
    } else {
        1.0
    };

    // Parametric EQ bands
    let eq_gain = parametric_eq(freq, 80.0, 4.0, 2.5)
        * parametric_eq(freq, 800.0, 3.0, 2.0)
        * parametric_eq(freq, 1600.0, -8.0, 1.0)
        * parametric_eq(freq, 3000.0, 8.0, 2.2)
        * parametric_eq(freq, 8900.0, -14.0, 1.4);

    // 19kHz band depends on sample rate
    let eq_19k = if sample_rate <= 48000 {
        parametric_eq(freq, 19000.0, 5.0, 0.7)
    } else {
        parametric_eq(freq, 19000.0, 12.0, 0.7)
    };

    (hp_gain * lp_gain * eq_gain * eq_19k) as f32
}

/// Parametric EQ bell curve: returns linear gain at the given frequency.
/// gain_db is the boost/cut at center frequency, Q controls bandwidth.
fn parametric_eq(freq: f64, center: f64, gain_db: f64, q: f64) -> f64 {
    let gain_linear = 10.0_f64.powf(gain_db / 20.0);
    if (freq - center).abs() < 0.001 {
        return gain_linear;
    }
    // Standard parametric EQ frequency response
    // H(f) = 1 + (G - 1) / (1 + ((f/fc - fc/f) * Q)^2)
    // where G = linear gain at center
    let ratio = freq / center;
    let x = (ratio - 1.0 / ratio) * q;
    let shape = 1.0 / (1.0 + x * x);
    1.0 + (gain_linear - 1.0) * shape
}

// ---------------------------------------------------------------------------
// Gap-aware noise floor measurement
//
// Uses non-overlapping windows (~5ms, matching the plugin's 240 samples @ 48kHz).
// Windows with RMS below GAP_THRESHOLD (−80 dBFS) are treated as silent gaps
// and excluded from the noise floor estimate, matching the plugin's intent of
// measuring background noise during "active" audio rather than digital silence
// between calls.
// ---------------------------------------------------------------------------

fn noise_floor_db(samples: &[f32], window_size: usize) -> (f64, bool) {
    if samples.is_empty() || window_size == 0 {
        return (-120.0, false);
    }
    let ws = window_size.min(samples.len());

    // Windows below this RMS (−80 dBFS linear) are considered silent gaps
    const GAP_THRESHOLD: f64 = 1e-4;

    let mut min_active_rms: Option<f64> = None;
    let mut min_all_rms: f64 = f64::MAX;
    let mut total_windows = 0usize;
    let mut active_windows = 0usize;

    let mut pos = 0;
    while pos + ws <= samples.len() {
        let sum_sq: f64 = samples[pos..pos + ws]
            .iter()
            .map(|&s| (s as f64) * (s as f64))
            .sum();
        let rms = (sum_sq / ws as f64).sqrt();

        total_windows += 1;
        if rms < min_all_rms {
            min_all_rms = rms;
        }
        if rms > GAP_THRESHOLD {
            active_windows += 1;
            match min_active_rms {
                None => min_active_rms = Some(rms),
                Some(prev) if rms < prev => min_active_rms = Some(rms),
                _ => {}
            }
        }

        pos += ws;
    }

    let has_silent_gaps = active_windows < total_windows;

    match min_active_rms {
        Some(rms) => (linear_to_db(rms as f32), has_silent_gaps),
        None => {
            // All windows are silent — no content in this frequency band at all
            // (e.g. ultrasonic recording with nothing in audible ITU band).
            // Fall back to absolute minimum; has_silent_gaps is false because
            // there are no "gaps between calls", just uniform silence in this band.
            let fallback = if min_all_rms < f64::MAX {
                min_all_rms
            } else {
                0.0
            };
            (linear_to_db(fallback as f32), false)
        }
    }
}

// ---------------------------------------------------------------------------
// Peak amplitude in dB (with -3dB correction as per the plugin)
// ---------------------------------------------------------------------------

fn peak_amplitude_db(samples: &[f32]) -> f64 {
    let peak = samples.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
    linear_to_db(peak) - 3.0
}

// ---------------------------------------------------------------------------
// Dense soundscape detection
// ---------------------------------------------------------------------------

fn detect_dense_soundscape(itu_filtered: &[f32], noise_db: f64) -> (bool, f64) {
    if itu_filtered.is_empty() {
        return (false, 0.0);
    }

    // Use RMS windows of 40 samples (matching the plugin)
    let ws = 40usize.min(itu_filtered.len());
    let mut count_near_floor = 0usize;
    let mut total_windows = 0usize;
    let threshold_db = noise_db + 3.0;

    let mut sum_sq: f64 = itu_filtered[..ws]
        .iter()
        .map(|&s| (s as f64) * (s as f64))
        .sum();

    let check = |sum: f64| -> bool {
        let rms = (sum.max(0.0) / ws as f64).sqrt();
        linear_to_db(rms as f32) < threshold_db
    };

    if check(sum_sq) {
        count_near_floor += 1;
    }
    total_windows += 1;

    // Step by window_size to avoid overlapping windows (faster, good enough for detection)
    let step = ws;
    let mut pos = ws;
    while pos + ws <= itu_filtered.len() {
        sum_sq = itu_filtered[pos..pos + ws]
            .iter()
            .map(|&s| (s as f64) * (s as f64))
            .sum();
        if check(sum_sq) {
            count_near_floor += 1;
        }
        total_windows += 1;
        pos += step;
    }

    let pct = if total_windows > 0 {
        count_near_floor as f64 / total_windows as f64 * 100.0
    } else {
        0.0
    };

    let is_dense = count_near_floor < 30 || pct < 1.0;
    (is_dense, pct)
}

// ---------------------------------------------------------------------------
// Utility
// ---------------------------------------------------------------------------

fn linear_to_db(value: f32) -> f64 {
    if value.abs() < 1e-20 {
        -120.0
    } else {
        20.0 * (value.abs() as f64).log10()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grade_thresholds_match_xc_v38j() {
        // Thresholds are >49.5 / >34.5 / >19.5 / >4.5 (strict greater-than).
        assert_eq!(WsnrGrade::from_snr(60.0), WsnrGrade::A);
        assert_eq!(WsnrGrade::from_snr(49.51), WsnrGrade::A);
        assert_eq!(WsnrGrade::from_snr(49.5), WsnrGrade::B); // boundary is exclusive
        assert_eq!(WsnrGrade::from_snr(40.0), WsnrGrade::B);
        assert_eq!(WsnrGrade::from_snr(25.0), WsnrGrade::C);
        assert_eq!(WsnrGrade::from_snr(10.0), WsnrGrade::D);
        assert_eq!(WsnrGrade::from_snr(0.0), WsnrGrade::E);
        assert_eq!(WsnrGrade::from_snr(-10.0), WsnrGrade::E);
    }

    #[test]
    fn grade_labels_round_trip() {
        for (db, expected) in [
            (60.0, "A"),
            (40.0, "B"),
            (25.0, "C"),
            (10.0, "D"),
            (0.0, "E"),
        ] {
            assert_eq!(WsnrGrade::from_snr(db).label(), expected);
        }
    }

    #[test]
    fn analyze_wsnr_too_short_recording_warns() {
        // <0.6s of audio should land in the "too short" branch.
        let samples = vec![0.0f32; 1000]; // ~0.023s at 44.1kHz
        let result = analyze_wsnr(&samples, 44_100);
        assert!(result.warnings.iter().any(|w| w.contains("Too short")));
    }
}
