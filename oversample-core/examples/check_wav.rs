//! Quick CLI to run the new LSB-autocorrelation and pipistrelle-signature
//! detectors against a WAV file. Useful for spot-checking detection on real
//! recordings without spinning up the full app.
//!
//! Run: `cargo run -p oversample-core --example check_wav -- <path/to/file.wav>`

use hound::WavReader;
use oversample_core::dsp::{lsb_autocorr, pipistrelle};

fn main() {
    let path = std::env::args().nth(1).expect("usage: check_wav <path>");
    let mut reader = WavReader::open(&path).expect("open wav");
    let spec = reader.spec();
    println!(
        "File: {}\n  channels={} sample_rate={} bits={} format={:?}",
        path, spec.channels, spec.sample_rate, spec.bits_per_sample, spec.sample_format
    );

    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Int => {
            let scale = (1u32 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.unwrap() as f32 / scale)
                .collect()
        }
        hound::SampleFormat::Float => reader.samples::<f32>().map(|s| s.unwrap()).collect(),
    };

    // Downmix to mono if needed
    let mono: Vec<f32> = if spec.channels > 1 {
        let ch = spec.channels as usize;
        samples
            .chunks(ch)
            .map(|c| c.iter().sum::<f32>() / ch as f32)
            .collect()
    } else {
        samples
    };
    println!(
        "  {} mono samples ({:.2} s)\n",
        mono.len(),
        mono.len() as f64 / spec.sample_rate as f64
    );

    let is_float = matches!(spec.sample_format, hound::SampleFormat::Float);

    println!("=== LSB autocorrelation ===");
    let lsb = lsb_autocorr::analyze_lsb_autocorr(&mono, spec.bits_per_sample, is_float);
    println!("  Verdict: {:?}", lsb.verdict);
    println!(
        "  Quietest window: idx={}  stdev={:.2}  nonzero_frac={:.3}",
        lsb.quietest_window_idx, lsb.quietest_window_stdev, lsb.quiet_lsb_nonzero_frac
    );
    println!(
        "  chi2={:.1}  lag1_acf={:+.4}  lag256_acf={:+.4}",
        lsb.quiet_lsb_chi2, lsb.quiet_lsb_lag1_acf, lsb.quiet_lsb_lag256_acf
    );
    println!("  GCD nonzero: {}", lsb.gcd_nonzero);
    println!("  {}\n", lsb.explanation);

    println!("=== Pipistrelle firmware signature ===");
    let pip = pipistrelle::detect(&mono, spec.sample_rate, spec.bits_per_sample, is_float);
    println!("  Verdict: {:?}", pip.verdict);
    println!("  Best dBcut: {:?}", pip.best_db_cut);
    println!(
        "  Best normalized residual: {:.4} ({:.2}%)",
        pip.best_normalized_residual,
        pip.best_normalized_residual * 100.0
    );
    println!("  Best in-range fraction: {:.3}", pip.best_in_range_frac);
    println!(
        "  Windows used: {}  samples analyzed: {}",
        pip.windows_used, pip.samples_analyzed
    );
    println!("  Per preset:");
    for s in &pip.per_preset {
        println!(
            "    dBcut={:>2}  residual={:.4}  in_range={:.3}",
            s.db_cut, s.normalized_residual, s.in_range_frac
        );
    }
    println!("  {}", pip.explanation);
}
