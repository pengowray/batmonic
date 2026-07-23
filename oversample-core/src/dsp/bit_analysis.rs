/// Per-bit statistics for a single bit position.
#[derive(Clone, Debug, PartialEq)]
pub struct BitStat {
    pub count: usize,
    pub first_index: Option<usize>,
    pub last_index: Option<usize>,
}

/// Caution flags for a single bit.
#[derive(Clone, Debug, PartialEq)]
pub enum BitCaution {
    /// Bit is only set during the first or last second (fade artifact)
    OnlyInFade,
    /// Bit is always 1 in every sample
    Always1,
    /// Very low usage count for a bit that should be populated
    VeryLowUsage,
    /// Sign bit usage is far from 50% among non-silent samples
    SignBitSkewed,
}

/// Value space coverage analysis for integer audio (16-bit and 24-bit).
#[derive(Clone, Debug, PartialEq)]
pub struct ValueCoverage {
    /// Number of distinct sample values observed
    pub unique_count: u32,
    /// Total possible values for this bit depth (2^bits_per_sample)
    pub value_space: u32,
    /// Percentage of value space used (0..100)
    pub coverage_pct: f64,
    /// log2(unique_count) — equivalent bit depth based on distinct values
    pub resolution_bits: f64,
}

/// Full bit-depth analysis result.
#[derive(Clone, Debug, PartialEq)]
pub struct BitAnalysis {
    pub bits_per_sample: u16,
    pub is_float: bool,
    pub total_samples: usize,
    pub duration_secs: f64,
    pub bit_stats: Vec<BitStat>,
    pub bit_cautions: Vec<Vec<BitCaution>>,
    pub effective_bits: u16,
    /// Fractional effective bit depth estimated via Shannon entropy sum
    pub effective_bits_f64: f64,
    pub summary: String,
    pub warnings: Vec<String>,
    /// Per-bit counts for positive samples only (sign bit = 0)
    pub positive_counts: Vec<usize>,
    /// Per-bit counts for negative samples only (sign bit = 1)
    pub negative_counts: Vec<usize>,
    pub positive_total: usize,
    pub negative_total: usize,
    /// Samples that are exactly zero (silence)
    pub zero_total: usize,
    /// Adjacent non-overlapping bit pair analysis: pair_counts[p] = [count_00, count_01, count_10, count_11]
    pub pair_counts: Vec<[usize; 4]>,
    /// For integer files: number of MSBs (below sign bit) that are always 0 in positive
    /// samples and always 1 in negative samples (sign-extension headroom). 0 for floats.
    pub headroom_bits: u16,
    /// Estimated noise floor in dBFS: minimum RMS of 512-sample windows above -80 dBFS.
    /// -120.0 if no active windows found.
    pub noise_floor_db: f64,
    /// Value space coverage (only for 16-bit and 24-bit integer files)
    pub value_coverage: Option<ValueCoverage>,
}

/// Bit label for display in the grid.
pub fn bit_label(bit_index: usize, bits_per_sample: u16, is_float: bool) -> String {
    let bit_pos = bits_per_sample as usize - 1 - bit_index;
    if is_float && bits_per_sample == 32 {
        match bit_pos {
            31 => "S".into(),
            23..=30 => format!("E{}", bit_pos - 23),
            _ => format!("M{}", bit_pos),
        }
    } else if bit_pos == bits_per_sample as usize - 1 {
        "S".into()
    } else {
        format!("{}", bit_pos)
    }
}

/// Analyze bit usage across all samples.
pub fn analyze_bits(
    samples: &[f32],
    bits_per_sample: u16,
    is_float: bool,
    duration_secs: f64,
) -> BitAnalysis {
    let n_bits = bits_per_sample as usize;
    let total = samples.len();
    if total == 0 {
        return BitAnalysis {
            bits_per_sample,
            is_float,
            total_samples: 0,
            duration_secs,
            bit_stats: Vec::new(),
            bit_cautions: Vec::new(),
            effective_bits: 0,
            effective_bits_f64: 0.0,
            summary: "No samples".into(),
            warnings: vec!["File contains no audio samples".into()],
            positive_counts: Vec::new(),
            negative_counts: Vec::new(),
            positive_total: 0,
            negative_total: 0,
            zero_total: 0,
            pair_counts: Vec::new(),
            headroom_bits: 0,
            noise_floor_db: -120.0,
            value_coverage: None,
        };
    }
    let n_pairs = n_bits / 2;

    let mut counts = vec![0usize; n_bits];
    let mut first = vec![None::<usize>; n_bits];
    let mut last = vec![None::<usize>; n_bits];
    let mut pos_counts = vec![0usize; n_bits];
    let mut neg_counts = vec![0usize; n_bits];
    let mut pos_total = 0usize;
    let mut neg_total = 0usize;
    let mut zero_total = 0usize;
    let mut pair_counts = vec![[0usize; 4]; n_pairs];

    let mut unique_seen = if !is_float && (bits_per_sample == 16 || bits_per_sample == 24) {
        vec![0u8; 1usize << (bits_per_sample - 3)]
    } else {
        Vec::new()
    };

    if is_float && bits_per_sample == 32 {
        analyze_float_bits(
            samples,
            &mut counts,
            &mut first,
            &mut last,
            &mut pos_counts,
            &mut neg_counts,
            &mut pos_total,
            &mut neg_total,
            &mut zero_total,
            &mut pair_counts,
        );
    } else {
        analyze_int_bits(
            samples,
            bits_per_sample,
            &mut counts,
            &mut first,
            &mut last,
            &mut pos_counts,
            &mut neg_counts,
            &mut pos_total,
            &mut neg_total,
            &mut zero_total,
            &mut pair_counts,
            &mut unique_seen,
        );
    }

    let bit_stats: Vec<BitStat> = (0..n_bits)
        .map(|i| BitStat {
            count: counts[i],
            first_index: first[i],
            last_index: last[i],
        })
        .collect();

    let effective_bits = detect_effective_bits(&bit_stats, bits_per_sample, is_float);
    let effective_bits_f64 = estimate_fractional_bits(&bit_stats, bits_per_sample, is_float, total);
    let headroom_bits = detect_headroom(
        &pos_counts,
        &neg_counts,
        pos_total,
        neg_total,
        bits_per_sample,
        is_float,
    );

    let bit_cautions = compute_cautions(
        &bit_stats,
        bits_per_sample,
        is_float,
        total,
        duration_secs,
        pos_total,
        neg_total,
    );

    let mut warnings = Vec::new();
    if total < 1000 {
        warnings.push("Very low sample count — analysis may be unreliable".into());
    }

    let summary = make_summary(bits_per_sample, is_float, effective_bits, total);

    // Noise floor: minimum RMS of 512-sample non-overlapping windows above -80 dBFS
    let noise_floor_db = {
        const WINDOW: usize = 512;
        const GAP_THRESHOLD: f64 = 1e-4; // -80 dBFS
        let ws = WINDOW.min(samples.len());
        let mut min_rms: Option<f64> = None;
        let mut pos = 0;
        while pos + ws <= samples.len() {
            let sum_sq: f64 = samples[pos..pos + ws]
                .iter()
                .map(|&s| (s as f64) * (s as f64))
                .sum();
            let rms = (sum_sq / ws as f64).sqrt();
            if rms > GAP_THRESHOLD {
                match min_rms {
                    None => min_rms = Some(rms),
                    Some(prev) if rms < prev => min_rms = Some(rms),
                    _ => {}
                }
            }
            pos += ws;
        }
        match min_rms {
            Some(rms) if rms > 0.0 => 20.0 * rms.log10(),
            _ => -120.0,
        }
    };

    let value_coverage = if !unique_seen.is_empty() {
        let unique_count: u32 = unique_seen.iter().map(|&b| b.count_ones()).sum();
        let value_space = 1u32 << bits_per_sample;
        let coverage_pct = unique_count as f64 / value_space as f64 * 100.0;
        let resolution_bits = if unique_count > 1 {
            (unique_count as f64).log2()
        } else {
            0.0
        };
        Some(ValueCoverage {
            unique_count,
            value_space,
            coverage_pct,
            resolution_bits,
        })
    } else {
        None
    };

    BitAnalysis {
        bits_per_sample,
        is_float,
        total_samples: total,
        duration_secs,
        bit_stats,
        bit_cautions,
        effective_bits,
        effective_bits_f64,
        summary,
        warnings,
        positive_counts: pos_counts,
        negative_counts: neg_counts,
        positive_total: pos_total,
        negative_total: neg_total,
        zero_total,
        pair_counts,
        headroom_bits,
        noise_floor_db,
        value_coverage,
    }
}

fn analyze_int_bits(
    samples: &[f32],
    bits_per_sample: u16,
    counts: &mut [usize],
    first: &mut [Option<usize>],
    last: &mut [Option<usize>],
    pos_counts: &mut [usize],
    neg_counts: &mut [usize],
    pos_total: &mut usize,
    neg_total: &mut usize,
    zero_total: &mut usize,
    pair_counts: &mut [[usize; 4]],
    unique_seen: &mut [u8],
) {
    let n_bits = bits_per_sample as usize;
    let max_val = (1u32 << (bits_per_sample - 1)) as f64;
    let mask_bits = bits_per_sample as u32;
    let n_pairs = n_bits / 2;

    for (idx, &s) in samples.iter().enumerate() {
        let int_val = (s as f64 * max_val).round() as i32;
        let bits = int_val as u32;
        if !unique_seen.is_empty() {
            let unsigned_idx = (int_val + max_val as i32) as usize;
            unique_seen[unsigned_idx >> 3] |= 1 << (unsigned_idx & 7);
        }
        let is_negative = bits & (1 << (mask_bits - 1)) != 0;
        let is_zero = int_val == 0;
        if is_zero {
            *zero_total += 1;
        } else if is_negative {
            *neg_total += 1;
        } else {
            *pos_total += 1;
        }
        for b in 0..n_bits {
            let bit_pos = mask_bits as usize - 1 - b;
            if bits & (1 << bit_pos) != 0 {
                counts[b] += 1;
                if is_negative {
                    neg_counts[b] += 1;
                } else {
                    pos_counts[b] += 1;
                }
                if first[b].is_none() {
                    first[b] = Some(idx);
                }
                last[b] = Some(idx);
            }
        }
        for (p, pair_count) in pair_counts.iter_mut().enumerate().take(n_pairs) {
            let b0 = 2 * p;
            let b1 = 2 * p + 1;
            let bit_pos0 = mask_bits as usize - 1 - b0;
            let bit_pos1 = mask_bits as usize - 1 - b1;
            let v0 = ((bits >> bit_pos0) & 1) as usize;
            let v1 = ((bits >> bit_pos1) & 1) as usize;
            let combo = v0 * 2 + v1; // 00=0, 01=1, 10=2, 11=3
            pair_count[combo] += 1;
        }
    }
}

fn analyze_float_bits(
    samples: &[f32],
    counts: &mut [usize],
    first: &mut [Option<usize>],
    last: &mut [Option<usize>],
    pos_counts: &mut [usize],
    neg_counts: &mut [usize],
    pos_total: &mut usize,
    neg_total: &mut usize,
    zero_total: &mut usize,
    pair_counts: &mut [[usize; 4]],
) {
    let n_pairs = 16; // 32 bits / 2
    for (idx, &s) in samples.iter().enumerate() {
        let bits = s.to_bits();
        let is_negative = bits & (1 << 31) != 0;
        // Treat both +0.0 and -0.0 as zero
        let is_zero = s == 0.0;
        if is_zero {
            *zero_total += 1;
        } else if is_negative {
            *neg_total += 1;
        } else {
            *pos_total += 1;
        }
        for b in 0..32usize {
            let bit_pos = 31 - b;
            if bits & (1 << bit_pos) != 0 {
                counts[b] += 1;
                if is_negative {
                    neg_counts[b] += 1;
                } else {
                    pos_counts[b] += 1;
                }
                if first[b].is_none() {
                    first[b] = Some(idx);
                }
                last[b] = Some(idx);
            }
        }
        for (p, pair_count) in pair_counts.iter_mut().enumerate().take(n_pairs) {
            let b0 = 2 * p;
            let b1 = 2 * p + 1;
            let bit_pos0 = 31 - b0;
            let bit_pos1 = 31 - b1;
            let v0 = ((bits >> bit_pos0) & 1) as usize;
            let v1 = ((bits >> bit_pos1) & 1) as usize;
            let combo = v0 * 2 + v1;
            pair_count[combo] += 1;
        }
    }
}

fn entropy_bit(p: f64) -> f64 {
    if p <= 0.0 || p >= 1.0 {
        0.0
    } else {
        -(p * p.log2() + (1.0 - p) * (1.0 - p).log2())
    }
}

/// Estimate effective bit depth as a fractional value using Shannon entropy.
/// Each bit's contribution = entropy of its usage probability.
/// For float: uses sign bit + mantissa bits (skipping exponent bits).
/// For integer: sums entropy across all N bits.
fn estimate_fractional_bits(
    stats: &[BitStat],
    bits_per_sample: u16,
    is_float: bool,
    total: usize,
) -> f64 {
    if total == 0 {
        return 0.0;
    }
    let total_f = total as f64;
    let result = if is_float && bits_per_sample == 32 {
        // Sign bit (index 0) + mantissa bits (indices 9..32)
        // Exponent bits (1..9) skipped — their usage patterns reflect value range, not precision
        let sign_h = entropy_bit(stats[0].count as f64 / total_f);
        let mantissa_h: f64 = (9..32)
            .map(|i| entropy_bit(stats[i].count as f64 / total_f))
            .sum();
        sign_h + mantissa_h
    } else {
        stats[..bits_per_sample as usize]
            .iter()
            .map(|s| entropy_bit(s.count as f64 / total_f))
            .sum()
    };
    result.min(bits_per_sample as f64)
}

fn detect_effective_bits(stats: &[BitStat], bits_per_sample: u16, is_float: bool) -> u16 {
    if is_float {
        // For float, count how many trailing mantissa bits are unused
        // Mantissa bits are at indices 9..32 (bit_index 9 = M22, bit_index 31 = M0)
        let mantissa_start = 9; // first mantissa bit index
        let mut unused_trailing = 0u16;
        for b in (mantissa_start..32).rev() {
            if stats[b].count == 0 {
                unused_trailing += 1;
            } else {
                break;
            }
        }
        let mantissa_bits_used = 23 - unused_trailing;
        // Approximate equivalent integer bits: 1 (sign) + mantissa_bits_used
        // This is a rough heuristic
        (1 + mantissa_bits_used).min(bits_per_sample)
    } else {
        // For integer, count unused LSBs
        let n = bits_per_sample as usize;
        let mut unused_lsb = 0u16;
        for b in (0..n).rev() {
            // b goes from 0 (MSB) to n-1 (LSB); we scan from LSB upward
            if stats[b].count == 0 {
                unused_lsb += 1;
            } else {
                break;
            }
        }
        bits_per_sample - unused_lsb
    }
}

fn detect_headroom(
    pos_counts: &[usize],
    neg_counts: &[usize],
    pos_total: usize,
    neg_total: usize,
    bits_per_sample: u16,
    is_float: bool,
) -> u16 {
    if is_float || (pos_total == 0 && neg_total == 0) {
        return 0;
    }
    let n = bits_per_sample as usize;
    let mut headroom = 0u16;
    for idx in 1..n {
        // bit is headroom if always 0 in positive and always 1 in negative
        let pos_ok = pos_total == 0 || pos_counts[idx] == 0;
        let neg_ok = neg_total == 0 || neg_counts[idx] == neg_total;
        if pos_ok && neg_ok {
            headroom += 1;
        } else {
            break;
        }
    }
    headroom
}

fn compute_cautions(
    stats: &[BitStat],
    bits_per_sample: u16,
    is_float: bool,
    total: usize,
    duration_secs: f64,
    pos_total: usize,
    neg_total: usize,
) -> Vec<Vec<BitCaution>> {
    let n = bits_per_sample as usize;
    let sample_rate_approx = if duration_secs > 0.0 {
        total as f64 / duration_secs
    } else {
        0.0
    };
    let one_sec_samples = sample_rate_approx as usize;
    let track_fades = duration_secs >= 3.0 && one_sec_samples > 0;

    // Sign bit skew: among non-silent samples, is the negative fraction far from 50%?
    let non_silent = pos_total + neg_total;
    let sign_bit_skewed = non_silent >= 5_000 && {
        let neg_frac = neg_total as f64 / non_silent as f64;
        (neg_frac - 0.5).abs() > 0.15
    };

    (0..n)
        .map(|b| {
            let stat = &stats[b];
            let mut cautions = Vec::new();

            if stat.count == 0 {
                return cautions;
            }

            // Sign bit skew (only applies to bit index 0)
            if b == 0 && sign_bit_skewed {
                cautions.push(BitCaution::SignBitSkewed);
            }

            // Always 1
            if stat.count == total {
                cautions.push(BitCaution::Always1);
            }

            // Only used in fade (first/last second)
            if track_fades {
                if let (Some(fi), Some(li)) = (stat.first_index, stat.last_index) {
                    let in_first_sec = fi < one_sec_samples;
                    let in_last_sec = li >= total.saturating_sub(one_sec_samples);
                    let only_edges = in_first_sec
                        && in_last_sec
                        && li.saturating_sub(fi) < 2 * one_sec_samples
                        && stat.count < one_sec_samples * 2;
                    if only_edges && stat.count < total / 2 {
                        cautions.push(BitCaution::OnlyInFade);
                    }
                }
            }

            // Very low usage for bits that should be populated
            if !is_float {
                let is_sign = b == 0;
                let threshold = if is_sign { total / 1000 } else { total / 10000 };
                if stat.count > 0 && stat.count <= threshold.max(1) && total > 1000 {
                    cautions.push(BitCaution::VeryLowUsage);
                }
            }

            cautions
        })
        .collect()
}

fn make_summary(
    bits_per_sample: u16,
    is_float: bool,
    effective_bits: u16,
    total_samples: usize,
) -> String {
    if total_samples == 0 {
        return "No samples".into();
    }

    if is_float {
        if effective_bits < 24 {
            format!("~{}-bit precision in 32-bit float", effective_bits)
        } else {
            "32-bit float".into()
        }
    } else if effective_bits < bits_per_sample {
        format!(
            "{}-bit recording stored as {}-bit",
            effective_bits, bits_per_sample
        )
    } else {
        format!("{}-bit", bits_per_sample)
    }
}

/// Determine if a bit at the given index is "expected to be used" (shown red if unused).
pub fn is_expected_used(
    bit_index: usize,
    bits_per_sample: u16,
    is_float: bool,
    effective_bits: u16,
) -> bool {
    if is_float && bits_per_sample == 32 {
        match bit_index {
            0 => true,     // sign bit should be used (unless all positive)
            1..=8 => true, // exponent bits should generally be used
            _ => {
                // Mantissa: only upper bits expected
                let mantissa_idx = bit_index - 9; // 0 = M22, 22 = M0
                let expected_mantissa = if effective_bits > 1 {
                    (effective_bits - 1) as usize
                } else {
                    0
                };
                mantissa_idx < expected_mantissa
            }
        }
    } else {
        // Integer: bits from sign down to effective_bits boundary are expected
        // bit_index 0 = MSB (sign), bit_index n-1 = LSB
        let bit_pos = bits_per_sample as usize - 1 - bit_index;
        bit_pos >= (bits_per_sample - effective_bits) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn float32_labels() {
        // bit_index 0 maps to the MSB. For float32: 1 sign + 8 exponent + 23 mantissa.
        assert_eq!(bit_label(0, 32, true), "S");
        assert_eq!(bit_label(1, 32, true), "E7");
        assert_eq!(bit_label(8, 32, true), "E0");
        assert_eq!(bit_label(9, 32, true), "M22");
        assert_eq!(bit_label(31, 32, true), "M0");
    }

    #[test]
    fn int16_labels() {
        // 16-bit signed: MSB is sign, the rest are the bit position number.
        assert_eq!(bit_label(0, 16, false), "S");
        assert_eq!(bit_label(1, 16, false), "14");
        assert_eq!(bit_label(15, 16, false), "0");
    }

    #[test]
    fn int24_labels() {
        assert_eq!(bit_label(0, 24, false), "S");
        assert_eq!(bit_label(23, 24, false), "0");
    }

    #[test]
    fn analyze_bits_empty_input() {
        let result = analyze_bits(&[], 16, false, 0.0);
        assert_eq!(result.total_samples, 0);
        assert_eq!(result.effective_bits, 0);
        assert_eq!(result.noise_floor_db, -120.0);
        assert!(result.warnings.iter().any(|w| w.contains("no audio")));
    }

    #[test]
    fn analyze_bits_silent_input_has_low_noise_floor() {
        // 1000 samples of perfect silence — noise floor should hit the floor sentinel.
        let samples = vec![0.0f32; 1000];
        let result = analyze_bits(&samples, 16, false, 0.0);
        assert_eq!(result.total_samples, 1000);
        // Pure silence: all RMS windows below -80 dBFS gap, no valid floor measured.
        assert_eq!(result.noise_floor_db, -120.0);
    }
}
