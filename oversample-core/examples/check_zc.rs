//! Parse a `.zc` file and dump summary stats. Useful for spot-checking
//! the parser against real recordings.
//!
//! `cargo run -p oversample-core --release --example check_zc -- <path>`

use oversample_core::audio::zc;

fn main() {
    let path = std::env::args().nth(1).expect("usage: check_zc <path>");
    let bytes = std::fs::read(&path).expect("read file");

    if !zc::is_zc(&bytes) {
        eprintln!("Not a recognised Anabat .zc file");
        std::process::exit(1);
    }

    let data = zc::parse_zc(&bytes).expect("parse");
    let md = &data.metadata;
    println!("File: {} ({} bytes)", path, bytes.len());
    println!("  file_type: {}", md.file_type);
    println!(
        "  res1: {}, divratio: {}, vres: {}",
        md.res1, md.divratio, md.vres
    );
    println!("  tape: {:?}", md.tape);
    println!("  date: {:?}", md.date);
    println!("  location: {:?}", md.location);
    println!("  species: {:?}", md.species);
    println!("  note1: {:?}", md.note1);
    if let Some(ts) = md.timestamp {
        println!(
            "  timestamp: {:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:06}",
            ts.year, ts.month, ts.day, ts.hour, ts.minute, ts.second, ts.microseconds_total
        );
    }
    if !md.id_code.is_empty() {
        println!("  id_code: {:?}", md.id_code);
    }
    if !md.gps.is_empty() {
        println!("  gps: {:?}", md.gps);
    }
    if !md.guano.is_empty() {
        println!("  guano:");
        for (k, v) in &md.guano {
            println!("    {} = {}", k, v);
        }
    }

    println!();
    let on = data.on_dot_count();
    let off = data.off_mask.iter().filter(|&&b| b).count();
    println!(
        "Dots: total = {}, ON = {}, OFF = {}",
        data.times_s.len(),
        on,
        off
    );
    println!("Duration: {:.3} s", data.duration_secs());

    let valid_freqs: Vec<f64> = data
        .freqs_hz
        .iter()
        .zip(&data.off_mask)
        .filter_map(|(&f, &off)| (!off && f > 0.0).then_some(f))
        .collect();
    if !valid_freqs.is_empty() {
        let min = valid_freqs.iter().cloned().fold(f64::INFINITY, f64::min);
        let max = valid_freqs
            .iter()
            .cloned()
            .fold(f64::NEG_INFINITY, f64::max);
        let sum: f64 = valid_freqs.iter().sum();
        let mean = sum / valid_freqs.len() as f64;
        println!(
            "Frequency: min = {:.0} Hz, max = {:.0} Hz, mean = {:.0} Hz  (n={})",
            min,
            max,
            mean,
            valid_freqs.len()
        );
    }

    // First and last few dots for sanity-check.
    println!();
    println!("First 10 ON dots (time s, freq Hz):");
    let mut shown = 0;
    for (i, (&t, &f)) in data.times_s.iter().zip(&data.freqs_hz).enumerate() {
        if data.off_mask[i] {
            continue;
        }
        println!("  [{:5}] {:.6}s  {:.0} Hz", i, t, f);
        shown += 1;
        if shown >= 10 {
            break;
        }
    }

    // Synthesised waveform stats.
    println!();
    let synth = zc::synthesise_waveform(&data, 384_000);
    let peak = synth.iter().map(|s| s.abs()).fold(0.0_f32, f32::max);
    let rms = if !synth.is_empty() {
        (synth.iter().map(|&s| (s as f64).powi(2)).sum::<f64>() / synth.len() as f64).sqrt() as f32
    } else {
        0.0
    };
    let nonzero = synth.iter().filter(|&&s| s.abs() > 0.001).count();
    println!("Synthesised waveform at 384 kHz: {} samples ({:.3}s, peak {:.3}, RMS {:.3}, {}/{} non-silent samples = {:.1}%)",
        synth.len(),
        synth.len() as f64 / 384_000.0,
        peak, rms, nonzero, synth.len(),
        if synth.is_empty() { 0.0 } else { nonzero as f64 * 100.0 / synth.len() as f64 });
}
