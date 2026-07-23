/// Convert a magnitude to absolute dB: `20 * log10(mag)`.
///
/// Tiles store absolute dB so that all LOD levels produce identical values
/// for the same audio sample.  A reference level (`20 * log10(max_mag)`) is
/// applied at render time as a negative gain offset.
///
/// Returns `f32::NEG_INFINITY` for zero/negative magnitudes.
#[inline]
pub fn magnitude_to_db(mag: f32) -> f32 {
    if mag <= 0.0 {
        return f32::NEG_INFINITY;
    }
    20.0 * mag.log10()
}

/// Map a spectrogram magnitude to a greyscale pixel value (0-255).
/// Uses log scale (dB) for perceptual brightness.
pub fn magnitude_to_greyscale(mag: f32, max_mag: f32) -> u8 {
    let abs_db = magnitude_to_db(mag);
    let ref_db = if max_mag > 0.0 {
        20.0 * max_mag.log10()
    } else {
        0.0
    };
    db_to_greyscale(abs_db, -80.0, 80.0, 1.0, -ref_db)
}

/// Convert a dB value to a greyscale pixel value (0-255) using display settings.
#[inline]
pub fn db_to_greyscale(db: f32, floor_db: f32, range_db: f32, gamma: f32, gain_db: f32) -> u8 {
    if !db.is_finite() {
        return 0;
    }
    let adjusted = db + gain_db;
    let normalized = ((adjusted - floor_db) / range_db).clamp(0.0, 1.0);
    if gamma == 1.0 {
        (normalized * 255.0) as u8
    } else {
        (normalized.powf(gamma) * 255.0) as u8
    }
}

/// Standard resistor color code bands (0=black, 1=brown, ..., 9=white).
pub const RESISTOR_BANDS: [[u8; 3]; 10] = [
    [40, 40, 40],    // 0 - black (lightened for visibility)
    [139, 69, 19],   // 1 - brown
    [255, 0, 0],     // 2 - red
    [255, 165, 0],   // 3 - orange
    [255, 255, 0],   // 4 - yellow
    [0, 128, 0],     // 5 - green
    [0, 0, 255],     // 6 - blue
    [148, 0, 211],   // 7 - violet
    [128, 128, 128], // 8 - grey
    [255, 255, 255], // 9 - white
];

/// Resistor color band color for frequency markers (single digit based on 10 kHz).
pub fn freq_marker_color(freq_hz: f64) -> [u8; 3] {
    let digit = (freq_hz / 10_000.0).round() as u32 % 10;
    RESISTOR_BANDS[digit as usize]
}

/// Solid shield color: picks the resistor band color for the digit that
/// changes at the given scale. `division_hz` is the bar interval (e.g. 10000
/// for major bars, 1000 for minor 1 kHz bars).
pub fn freq_shield_color(freq_hz: f64, division_hz: f64) -> [u8; 3] {
    let digit = (freq_hz / division_hz).round() as u32 % 10;
    RESISTOR_BANDS[digit as usize]
}

/// Three-band resistor color encoding for a frequency in Hz.
/// Returns [first_digit, second_digit, multiplier] colors.
/// E.g. 40 kHz = 40×1k → [yellow(4), black(0), orange(×1k=10^3)]
pub fn freq_resistor_bands(freq_hz: f64) -> [[u8; 3]; 3] {
    let hz = freq_hz.round() as u64;
    if hz == 0 {
        return [RESISTOR_BANDS[0]; 3];
    }
    let mut val = hz;
    let mut multiplier_exp: u32 = 0;
    while val >= 100 {
        val /= 10;
        multiplier_exp += 1;
    }
    // val is now 1–99
    let d1 = (val / 10) as usize; // first significant digit
    let d2 = (val % 10) as usize; // second significant digit
    let mult = multiplier_exp as usize; // multiplier = 10^mult
    [
        RESISTOR_BANDS[d1 % 10],
        RESISTOR_BANDS[d2 % 10],
        RESISTOR_BANDS[mult % 10],
    ]
}

/// Hermite smoothstep: smooth transition from 0 to 1 between edge0 and edge1.
fn smoothstep(x: f32, edge0: f32, edge1: f32) -> f32 {
    if edge1 <= edge0 {
        return if x >= edge0 { 1.0 } else { 0.0 };
    }
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Map a greyscale base value and a frequency-shift amount to an RGB triple.
/// `shift` > 0 → energy moving upward in frequency → red tint.
/// `shift` < 0 → energy moving downward in frequency → blue tint.
///
/// Two smooth gates control when color appears:
/// - `intensity_gate` (0.0–1.0): how bright must a pixel be to show color
/// - `flow_gate` (0.0–1.0): how large must the shift be to show color
/// - `opacity` (0.0–1.0): overall color strength multiplier
pub fn flow_rgb(
    grey: u8,
    shift: f32,
    intensity_gate: f32,
    flow_gate: f32,
    opacity: f32,
    shift_gain: f32,
    color_gamma: f32,
) -> [u8; 3] {
    let g_norm = grey as f32 / 255.0;

    // Smooth intensity gate: ramp from gate*0.6 to gate*1.4
    let ig_lo = intensity_gate * 0.6;
    let ig_hi = (intensity_gate * 1.4).min(1.0);
    let intensity_factor = smoothstep(g_norm, ig_lo, ig_hi);

    // Smooth flow gate: ramp based on |shift|
    let abs_shift = shift.abs();
    let mg_lo = flow_gate * 0.3;
    let mg_hi = (flow_gate * 2.0).max(0.05);
    let flow_factor = smoothstep(abs_shift, mg_lo, mg_hi);

    let effective = intensity_factor * flow_factor * opacity;
    if effective < 0.001 {
        return [grey, grey, grey];
    }

    // Shift magnitude determines color intensity; effective controls blend with grey
    let s_raw = (shift * shift_gain).clamp(-1.0, 1.0);
    // Apply color gamma to magnitude, preserve sign
    let s = if color_gamma == 1.0 {
        s_raw
    } else {
        s_raw.abs().powf(color_gamma).copysign(s_raw)
    };
    let g = grey as f32;
    let (r, gv, b) = if s > 0.0 {
        // Upward shift → red
        let r = (g + s * (255.0 - g)).min(255.0);
        let gb = (g * (1.0 - 0.5 * s)).max(0.0);
        (r, gb, gb)
    } else {
        // Downward shift → blue
        let a = -s;
        let b = (g + a * (255.0 - g)).min(255.0);
        let rg = (g * (1.0 - 0.5 * a)).max(0.0);
        (rg, rg, b)
    };
    // Blend between grey and colored pixel using effective as alpha
    [
        (g + effective * (r - g)).clamp(0.0, 255.0) as u8,
        (g + effective * (gv - g)).clamp(0.0, 255.0) as u8,
        (g + effective * (b - g)).clamp(0.0, 255.0) as u8,
    ]
}

// ── Selectable flow color schemes ────────────────────────────────────────────

use crate::types::FlowColorScheme;

/// Dispatch to the selected flow color scheme.
/// Phase and PhaseCoherence algorithms ignore this and use their own color functions.
pub fn flow_rgb_scheme(
    grey: u8,
    shift: f32,
    intensity_gate: f32,
    flow_gate: f32,
    opacity: f32,
    shift_gain: f32,
    color_gamma: f32,
    scheme: FlowColorScheme,
) -> [u8; 3] {
    match scheme {
        FlowColorScheme::RedBlue => flow_rgb(
            grey,
            shift,
            intensity_gate,
            flow_gate,
            opacity,
            shift_gain,
            color_gamma,
        ),
        _ => flow_rgb_diverging(
            grey,
            shift,
            intensity_gate,
            flow_gate,
            opacity,
            shift_gain,
            color_gamma,
            scheme,
        ),
    }
}

/// Generic diverging colormap: interpolates between two endpoint colors through
/// a neutral midpoint, with brightness from greyscale and direction from shift.
fn flow_rgb_diverging(
    grey: u8,
    shift: f32,
    intensity_gate: f32,
    flow_gate: f32,
    opacity: f32,
    shift_gain: f32,
    color_gamma: f32,
    scheme: FlowColorScheme,
) -> [u8; 3] {
    let g_norm = grey as f32 / 255.0;

    // Smooth intensity gate
    let ig_lo = intensity_gate * 0.6;
    let ig_hi = (intensity_gate * 1.4).min(1.0);
    let intensity_factor = smoothstep(g_norm, ig_lo, ig_hi);

    // Smooth flow gate
    let abs_shift = shift.abs();
    let mg_lo = flow_gate * 0.3;
    let mg_hi = (flow_gate * 2.0).max(0.05);
    let flow_factor = smoothstep(abs_shift, mg_lo, mg_hi);

    let effective = intensity_factor * flow_factor * opacity;
    if effective < 0.001 {
        return [grey, grey, grey];
    }

    // Shift magnitude determines color intensity; effective controls blend with grey
    let s_raw = (shift * shift_gain).clamp(-1.0, 1.0);
    let s = if color_gamma == 1.0 {
        s_raw
    } else {
        s_raw.abs().powf(color_gamma).copysign(s_raw)
    };

    // Endpoint colors for each scheme: (neg_r, neg_g, neg_b), (pos_r, pos_g, pos_b)
    let (neg, pos) = match scheme {
        FlowColorScheme::CoolWarm => ([59.0, 76.0, 192.0], [180.0, 4.0, 38.0]),
        FlowColorScheme::TealOrange => ([0.0, 128.0, 128.0], [210.0, 105.0, 30.0]),
        FlowColorScheme::PurpleGreen => ([128.0, 0.0, 128.0], [0.0, 160.0, 0.0]),
        FlowColorScheme::Spectral => ([48.0, 18.0, 220.0], [220.0, 18.0, 48.0]),
        _ => ([0.0, 0.0, 255.0], [255.0, 0.0, 0.0]), // fallback
    };

    let g = grey as f32;
    let t = s.abs();
    // Interpolate: grey base toward endpoint color based on shift magnitude
    let endpoint = if s >= 0.0 { pos } else { neg };
    let r = g + t * (endpoint[0] - g);
    let gv = g + t * (endpoint[1] - g);
    let b = g + t * (endpoint[2] - g);
    // Blend between grey and colored pixel using effective as alpha
    [
        (g + effective * (r - g)).clamp(0.0, 255.0) as u8,
        (g + effective * (gv - g)).clamp(0.0, 255.0) as u8,
        (g + effective * (b - g)).clamp(0.0, 255.0) as u8,
    ]
}

/// Map a greyscale base value and a phase deviation to an RGB triple.
/// `deviation` in [-1.0, 1.0]: 0 = coherent (no phase drift), ±1 = max deviation.
///
/// Coherent pixels → bright blue-white. Forward deviation → red. Backward → blue.
/// `intensity_gate` gates quiet pixels. `opacity` blends between grey and color.
/// `shift_gain` amplifies the deviation before coloring.
pub fn coherence_rgb(
    grey: u8,
    deviation: f32,
    intensity_gate: f32,
    flow_gate: f32,
    opacity: f32,
    shift_gain: f32,
    color_gamma: f32,
) -> [u8; 3] {
    let g_norm = grey as f32 / 255.0;

    // Smooth intensity gate
    let ig_lo = intensity_gate * 0.6;
    let ig_hi = (intensity_gate * 1.4).min(1.0);
    let intensity_factor = smoothstep(g_norm, ig_lo, ig_hi);

    // Smooth flow gate on |deviation| (same pattern as flow_rgb)
    let abs_dev_raw = deviation.abs();
    let mg_lo = flow_gate * 0.3;
    let mg_hi = (flow_gate * 2.0).max(0.05);
    let flow_factor = smoothstep(abs_dev_raw, mg_lo, mg_hi);

    let effective = intensity_factor * flow_factor * opacity;
    if effective < 0.001 {
        return [grey, grey, grey];
    }

    // Scale deviation by shift_gain (default 3.0), apply color gamma
    let dev_raw = (deviation * shift_gain).clamp(-1.0, 1.0);
    let dev = if color_gamma == 1.0 {
        dev_raw
    } else {
        dev_raw.abs().powf(color_gamma).copysign(dev_raw)
    };
    let abs_dev = dev.abs();

    // Gamma-adjusted brightness from greyscale
    let bright = g_norm.powf(0.75) * 255.0;

    // Coherent (|dev| small): bright blue-white (#c8e8ff at full brightness)
    let coh_r = 0.78;
    let coh_g = 0.91;
    let coh_b = 1.0;

    let (r, g, b) = if abs_dev < 0.3 {
        // Mostly coherent: blue-white
        (bright * coh_r, bright * coh_g, bright * coh_b)
    } else if dev > 0.0 {
        // Forward deviation → red
        let strength = ((abs_dev - 0.1) / 0.9).clamp(0.0, 1.0);
        let r = bright * (coh_r + strength * (1.0 - coh_r));
        let g = bright * coh_g * (1.0 - strength * 0.7);
        let b = bright * coh_b * (1.0 - strength * 0.85);
        (r, g, b)
    } else {
        // Backward deviation → deep blue
        let strength = ((abs_dev - 0.1) / 0.9).clamp(0.0, 1.0);
        let r = bright * coh_r * (1.0 - strength * 0.85);
        let g = bright * coh_g * (1.0 - strength * 0.5);
        let b = bright * coh_b;
        (r, g, b)
    };

    // Blend with greyscale by effective opacity
    let grey_f = grey as f32;
    let final_r = (grey_f + effective * (r - grey_f)).clamp(0.0, 255.0) as u8;
    let final_g = (grey_f + effective * (g - grey_f)).clamp(0.0, 255.0) as u8;
    let final_b = (grey_f + effective * (b - grey_f)).clamp(0.0, 255.0) as u8;
    [final_r, final_g, final_b]
}

// ── Oklch perceptually uniform phase colormap ────────────────────────────────

/// Convert Oklch (L, C, h) to linear sRGB (r, g, b).
/// L: lightness [0, 1], C: chroma [0, ~0.37], h: hue in radians.
fn oklch_to_linear_srgb(l: f32, c: f32, h: f32) -> (f32, f32, f32) {
    // Oklch → Oklab
    let a = c * h.cos();
    let b_ab = c * h.sin();
    // Oklab → LMS (inverse of cube-root transform)
    let l_ = l + 0.396_337_78 * a + 0.215_803_76 * b_ab;
    let m_ = l - 0.105_561_346 * a - 0.063_854_17 * b_ab;
    let s_ = l - 0.089_484_18 * a - 1.291_485_5 * b_ab;
    let l3 = l_ * l_ * l_;
    let m3 = m_ * m_ * m_;
    let s3 = s_ * s_ * s_;
    // LMS → linear sRGB
    let r = 4.076_741_7 * l3 - 3.307_711_6 * m3 + 0.230_969_94 * s3;
    let g = -1.268_438 * l3 + 2.609_757_4 * m3 - 0.341_319_38 * s3;
    let b = -0.0041960863 * l3 - 0.703_418_6 * m3 + 1.707_614_7 * s3;
    (r, g, b)
}

/// Linear sRGB component to sRGB with gamma encoding.
#[inline]
fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.0031308 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// Pre-computed 256-entry Oklch phase hue colormap.
/// Index maps linearly to hue angle [0, 2*PI). Fixed L=0.75, C=0.12 for
/// sRGB gamut safety and good visibility. Perceptually uniform: all hues
/// have equal perceived brightness, unlike HSV.
static OKLCH_PHASE_LUT: std::sync::LazyLock<[[u8; 3]; 256]> = std::sync::LazyLock::new(|| {
    let mut lut = [[0u8; 3]; 256];
    for (i, entry) in lut.iter_mut().enumerate() {
        let hue = i as f32 * std::f32::consts::TAU / 256.0;
        let (r, g, b) = oklch_to_linear_srgb(0.75, 0.12, hue);
        *entry = [
            (linear_to_srgb(r.clamp(0.0, 1.0)) * 255.0) as u8,
            (linear_to_srgb(g.clamp(0.0, 1.0)) * 255.0) as u8,
            (linear_to_srgb(b.clamp(0.0, 1.0)) * 255.0) as u8,
        ];
    }
    lut
});

/// Map instantaneous phase angle and magnitude to an RGB triple using
/// an Oklch perceptually uniform circular colormap.
///
/// `phase` in [-1.0, 1.0] (normalized from [-PI, PI]).
/// `grey` is the magnitude-derived brightness (0-255).
/// Hue = phase angle via Oklch LUT. Brightness scaled by magnitude.
pub fn phase_rgb(grey: u8, phase: f32, intensity_gate: f32) -> [u8; 3] {
    let g_norm = grey as f32 / 255.0;

    // Intensity gate (same pattern as flow_rgb)
    let ig_lo = intensity_gate * 0.6;
    let ig_hi = (intensity_gate * 1.4).min(1.0);
    let intensity_factor = smoothstep(g_norm, ig_lo, ig_hi);

    if intensity_factor < 0.001 {
        return [0, 0, 0];
    }

    // Map normalized phase [-1, 1] to LUT index [0, 255]
    let idx = (((phase + 1.0) * 0.5) * 255.0).clamp(0.0, 255.0) as u8;
    let [base_r, base_g, base_b] = OKLCH_PHASE_LUT[idx as usize];

    // Scale by magnitude-derived brightness
    let brightness = g_norm;
    [
        (base_r as f32 * brightness).clamp(0.0, 255.0) as u8,
        (base_g as f32 * brightness).clamp(0.0, 255.0) as u8,
        (base_b as f32 * brightness).clamp(0.0, 255.0) as u8,
    ]
}

/// Label for a frequency marker (number only, e.g. "40").
pub fn freq_marker_label(freq_hz: f64) -> String {
    format!("{}", (freq_hz / 1000.0).round() as u32)
}

/// Apply viridis colormap to a greyscale pixel value (0=dark purple, 255=yellow).
#[inline]
pub fn greyscale_to_viridis(grey: u8) -> [u8; 3] {
    VIRIDIS_LUT[grey as usize]
}

/// Apply inferno colormap to a greyscale pixel value (0=black, 255=pale yellow).
#[inline]
pub fn greyscale_to_inferno(grey: u8) -> [u8; 3] {
    INFERNO_LUT[grey as usize]
}

/// Apply magma colormap to a greyscale pixel value (0=black, 255=pale yellow-white).
#[inline]
pub fn greyscale_to_magma(grey: u8) -> [u8; 3] {
    MAGMA_LUT[grey as usize]
}

/// Apply plasma colormap to a greyscale pixel value (0=dark blue, 255=yellow).
#[inline]
pub fn greyscale_to_plasma(grey: u8) -> [u8; 3] {
    PLASMA_LUT[grey as usize]
}

/// Apply cividis colormap to a greyscale pixel value (0=dark blue, 255=yellow).
#[inline]
pub fn greyscale_to_cividis(grey: u8) -> [u8; 3] {
    CIVIDIS_LUT[grey as usize]
}

/// Apply turbo colormap to a greyscale pixel value (0=dark blue, 255=dark red).
#[inline]
pub fn greyscale_to_turbo(grey: u8) -> [u8; 3] {
    TURBO_LUT[grey as usize]
}

/// Standard matplotlib viridis perceptual colormap — 256 RGB entries.
pub const VIRIDIS_LUT: [[u8; 3]; 256] = [
    [68, 1, 84],
    [68, 2, 86],
    [69, 4, 87],
    [69, 5, 89],
    [70, 7, 90],
    [70, 8, 92],
    [70, 10, 93],
    [70, 11, 94],
    [71, 13, 96],
    [71, 14, 97],
    [71, 16, 99],
    [71, 17, 100],
    [71, 19, 101],
    [72, 20, 103],
    [72, 22, 104],
    [72, 23, 105],
    [72, 24, 106],
    [72, 26, 108],
    [72, 27, 109],
    [72, 28, 110],
    [72, 29, 111],
    [72, 31, 112],
    [72, 32, 113],
    [72, 33, 115],
    [72, 35, 116],
    [72, 36, 117],
    [72, 37, 118],
    [72, 38, 119],
    [72, 40, 120],
    [72, 41, 121],
    [71, 42, 122],
    [71, 44, 122],
    [71, 45, 123],
    [71, 46, 124],
    [71, 47, 125],
    [70, 48, 126],
    [70, 50, 126],
    [70, 51, 127],
    [69, 52, 128],
    [69, 53, 129],
    [69, 55, 129],
    [68, 56, 130],
    [68, 57, 131],
    [68, 58, 131],
    [67, 60, 132],
    [67, 61, 132],
    [66, 62, 133],
    [66, 63, 133],
    [66, 64, 134],
    [65, 66, 134],
    [65, 67, 135],
    [64, 68, 135],
    [64, 69, 136],
    [63, 71, 136],
    [63, 72, 137],
    [62, 73, 137],
    [62, 74, 137],
    [62, 76, 138],
    [61, 77, 138],
    [61, 78, 138],
    [60, 79, 139],
    [60, 80, 139],
    [59, 82, 139],
    [59, 83, 140],
    [58, 84, 140],
    [58, 85, 140],
    [57, 86, 141],
    [57, 88, 141],
    [56, 89, 141],
    [56, 90, 141],
    [55, 91, 142],
    [55, 92, 142],
    [54, 93, 142],
    [54, 94, 142],
    [53, 96, 142],
    [53, 97, 142],
    [52, 98, 143],
    [52, 99, 143],
    [51, 100, 143],
    [51, 101, 143],
    [50, 103, 143],
    [50, 104, 143],
    [49, 105, 143],
    [49, 106, 143],
    [49, 107, 143],
    [48, 108, 143],
    [48, 109, 143],
    [47, 110, 143],
    [47, 111, 143],
    [46, 112, 143],
    [46, 113, 143],
    [46, 114, 143],
    [45, 115, 143],
    [45, 116, 142],
    [44, 117, 142],
    [44, 118, 142],
    [44, 119, 142],
    [43, 120, 142],
    [43, 121, 142],
    [42, 122, 142],
    [42, 123, 141],
    [42, 124, 141],
    [41, 125, 141],
    [41, 126, 141],
    [41, 127, 141],
    [40, 128, 140],
    [40, 129, 140],
    [40, 130, 140],
    [39, 131, 140],
    [39, 132, 139],
    [39, 133, 139],
    [38, 134, 139],
    [38, 135, 138],
    [38, 136, 138],
    [37, 137, 138],
    [37, 138, 137],
    [37, 139, 137],
    [36, 140, 137],
    [36, 141, 136],
    [36, 142, 136],
    [35, 143, 135],
    [35, 144, 135],
    [35, 145, 134],
    [35, 146, 134],
    [34, 147, 133],
    [34, 148, 133],
    [34, 149, 132],
    [33, 150, 132],
    [33, 151, 131],
    [33, 152, 131],
    [33, 153, 130],
    [33, 154, 130],
    [33, 155, 129],
    [33, 156, 128],
    [33, 157, 128],
    [33, 158, 127],
    [33, 159, 126],
    [34, 160, 126],
    [34, 161, 125],
    [34, 162, 124],
    [35, 163, 124],
    [35, 164, 123],
    [35, 165, 122],
    [36, 166, 122],
    [36, 167, 121],
    [37, 168, 120],
    [37, 169, 119],
    [38, 170, 119],
    [39, 171, 118],
    [39, 172, 117],
    [40, 173, 116],
    [41, 174, 115],
    [42, 175, 115],
    [42, 176, 114],
    [43, 177, 113],
    [44, 178, 112],
    [46, 179, 111],
    [47, 180, 110],
    [48, 181, 109],
    [49, 182, 108],
    [50, 183, 107],
    [52, 184, 106],
    [53, 185, 105],
    [55, 186, 104],
    [56, 187, 103],
    [58, 188, 101],
    [59, 189, 100],
    [61, 190, 99],
    [63, 191, 98],
    [64, 192, 97],
    [66, 193, 95],
    [68, 194, 94],
    [70, 195, 93],
    [72, 196, 91],
    [74, 197, 90],
    [76, 198, 89],
    [78, 199, 87],
    [80, 200, 86],
    [82, 201, 84],
    [84, 202, 83],
    [86, 203, 81],
    [89, 204, 80],
    [91, 205, 78],
    [93, 206, 76],
    [95, 207, 75],
    [98, 208, 73],
    [100, 209, 71],
    [103, 210, 69],
    [105, 211, 68],
    [108, 212, 66],
    [110, 213, 64],
    [113, 214, 62],
    [116, 215, 60],
    [118, 215, 58],
    [121, 216, 56],
    [124, 217, 54],
    [127, 218, 52],
    [129, 219, 50],
    [132, 220, 48],
    [135, 221, 46],
    [138, 222, 44],
    [141, 222, 42],
    [144, 223, 40],
    [147, 224, 38],
    [150, 225, 36],
    [153, 225, 33],
    [156, 226, 31],
    [159, 227, 29],
    [162, 228, 27],
    [165, 228, 25],
    [168, 229, 23],
    [172, 230, 21],
    [175, 230, 19],
    [178, 231, 17],
    [181, 232, 16],
    [185, 232, 14],
    [188, 233, 12],
    [191, 234, 11],
    [195, 234, 10],
    [198, 235, 9],
    [201, 236, 8],
    [205, 236, 8],
    [208, 237, 8],
    [211, 237, 8],
    [215, 238, 9],
    [218, 238, 10],
    [221, 239, 11],
    [225, 239, 13],
    [228, 240, 15],
    [231, 240, 17],
    [235, 241, 19],
    [238, 241, 22],
    [241, 242, 25],
    [244, 242, 28],
    [247, 243, 31],
    [250, 243, 35],
    [253, 244, 38],
    [253, 244, 42],
    [253, 245, 45],
    [253, 245, 49],
    [253, 246, 53],
    [253, 246, 57],
    [254, 247, 61],
    [254, 247, 65],
    [254, 248, 69],
    [254, 248, 73],
    [254, 249, 78],
    [254, 249, 82],
    [254, 250, 87],
    [254, 250, 91],
    [254, 251, 96],
    [254, 251, 101],
    [255, 252, 105],
    [255, 252, 110],
    [255, 253, 115],
    [255, 253, 120],
];

/// Standard matplotlib inferno perceptual colormap — 256 RGB entries.
pub const INFERNO_LUT: [[u8; 3]; 256] = [
    [0, 0, 3],
    [0, 0, 4],
    [0, 0, 6],
    [1, 0, 7],
    [1, 1, 9],
    [1, 1, 11],
    [2, 1, 14],
    [2, 2, 16],
    [3, 2, 18],
    [4, 3, 20],
    [4, 3, 22],
    [5, 4, 24],
    [6, 4, 27],
    [7, 5, 29],
    [8, 6, 31],
    [9, 6, 33],
    [10, 7, 35],
    [11, 7, 38],
    [13, 8, 40],
    [14, 8, 42],
    [15, 9, 45],
    [16, 9, 47],
    [18, 10, 50],
    [19, 10, 52],
    [20, 11, 54],
    [22, 11, 57],
    [23, 11, 59],
    [25, 11, 62],
    [26, 11, 64],
    [28, 12, 67],
    [29, 12, 69],
    [31, 12, 71],
    [32, 12, 74],
    [34, 11, 76],
    [36, 11, 78],
    [38, 11, 80],
    [39, 11, 82],
    [41, 11, 84],
    [43, 10, 86],
    [45, 10, 88],
    [46, 10, 90],
    [48, 10, 92],
    [50, 9, 93],
    [52, 9, 95],
    [53, 9, 96],
    [55, 9, 97],
    [57, 9, 98],
    [59, 9, 100],
    [60, 9, 101],
    [62, 9, 102],
    [64, 9, 102],
    [65, 9, 103],
    [67, 10, 104],
    [69, 10, 105],
    [70, 10, 105],
    [72, 11, 106],
    [74, 11, 106],
    [75, 12, 107],
    [77, 12, 107],
    [79, 13, 108],
    [80, 13, 108],
    [82, 14, 108],
    [83, 14, 109],
    [85, 15, 109],
    [87, 15, 109],
    [88, 16, 109],
    [90, 17, 109],
    [91, 17, 110],
    [93, 18, 110],
    [95, 18, 110],
    [96, 19, 110],
    [98, 20, 110],
    [99, 20, 110],
    [101, 21, 110],
    [102, 21, 110],
    [104, 22, 110],
    [106, 23, 110],
    [107, 23, 110],
    [109, 24, 110],
    [110, 24, 110],
    [112, 25, 110],
    [114, 25, 109],
    [115, 26, 109],
    [117, 27, 109],
    [118, 27, 109],
    [120, 28, 109],
    [122, 28, 109],
    [123, 29, 108],
    [125, 29, 108],
    [126, 30, 108],
    [128, 31, 107],
    [129, 31, 107],
    [131, 32, 107],
    [133, 32, 106],
    [134, 33, 106],
    [136, 33, 106],
    [137, 34, 105],
    [139, 34, 105],
    [141, 35, 105],
    [142, 36, 104],
    [144, 36, 104],
    [145, 37, 103],
    [147, 37, 103],
    [149, 38, 102],
    [150, 38, 102],
    [152, 39, 101],
    [153, 40, 100],
    [155, 40, 100],
    [156, 41, 99],
    [158, 41, 99],
    [160, 42, 98],
    [161, 43, 97],
    [163, 43, 97],
    [164, 44, 96],
    [166, 44, 95],
    [167, 45, 95],
    [169, 46, 94],
    [171, 46, 93],
    [172, 47, 92],
    [174, 48, 91],
    [175, 49, 91],
    [177, 49, 90],
    [178, 50, 89],
    [180, 51, 88],
    [181, 51, 87],
    [183, 52, 86],
    [184, 53, 86],
    [186, 54, 85],
    [187, 55, 84],
    [189, 55, 83],
    [190, 56, 82],
    [191, 57, 81],
    [193, 58, 80],
    [194, 59, 79],
    [196, 60, 78],
    [197, 61, 77],
    [199, 62, 76],
    [200, 62, 75],
    [201, 63, 74],
    [203, 64, 73],
    [204, 65, 72],
    [205, 66, 71],
    [207, 68, 70],
    [208, 69, 68],
    [209, 70, 67],
    [210, 71, 66],
    [212, 72, 65],
    [213, 73, 64],
    [214, 74, 63],
    [215, 75, 62],
    [217, 77, 61],
    [218, 78, 59],
    [219, 79, 58],
    [220, 80, 57],
    [221, 82, 56],
    [222, 83, 55],
    [223, 84, 54],
    [224, 86, 52],
    [226, 87, 51],
    [227, 88, 50],
    [228, 90, 49],
    [229, 91, 48],
    [230, 92, 46],
    [230, 94, 45],
    [231, 95, 44],
    [232, 97, 43],
    [233, 98, 42],
    [234, 100, 40],
    [235, 101, 39],
    [236, 103, 38],
    [237, 104, 37],
    [237, 106, 35],
    [238, 108, 34],
    [239, 109, 33],
    [240, 111, 31],
    [240, 112, 30],
    [241, 114, 29],
    [242, 116, 28],
    [242, 117, 26],
    [243, 119, 25],
    [243, 121, 24],
    [244, 122, 22],
    [245, 124, 21],
    [245, 126, 20],
    [246, 128, 18],
    [246, 129, 17],
    [247, 131, 16],
    [247, 133, 14],
    [248, 135, 13],
    [248, 136, 12],
    [248, 138, 11],
    [249, 140, 9],
    [249, 142, 8],
    [249, 144, 8],
    [250, 145, 7],
    [250, 147, 6],
    [250, 149, 6],
    [250, 151, 6],
    [251, 153, 6],
    [251, 155, 6],
    [251, 157, 6],
    [251, 158, 7],
    [251, 160, 7],
    [251, 162, 8],
    [251, 164, 10],
    [251, 166, 11],
    [251, 168, 13],
    [251, 170, 14],
    [251, 172, 16],
    [251, 174, 18],
    [251, 176, 20],
    [251, 177, 22],
    [251, 179, 24],
    [251, 181, 26],
    [251, 183, 28],
    [251, 185, 30],
    [250, 187, 33],
    [250, 189, 35],
    [250, 191, 37],
    [250, 193, 40],
    [249, 195, 42],
    [249, 197, 44],
    [249, 199, 47],
    [248, 201, 49],
    [248, 203, 52],
    [248, 205, 55],
    [247, 207, 58],
    [247, 209, 60],
    [246, 211, 63],
    [246, 213, 66],
    [245, 215, 69],
    [245, 217, 72],
    [244, 219, 75],
    [244, 220, 79],
    [243, 222, 82],
    [243, 224, 86],
    [243, 226, 89],
    [242, 228, 93],
    [242, 230, 96],
    [241, 232, 100],
    [241, 233, 104],
    [241, 235, 108],
    [241, 237, 112],
    [241, 238, 116],
    [241, 240, 121],
    [241, 242, 125],
    [242, 243, 129],
    [242, 244, 133],
    [243, 246, 137],
    [244, 247, 141],
    [245, 248, 145],
    [246, 250, 149],
    [247, 251, 153],
    [249, 252, 157],
    [250, 253, 160],
    [252, 254, 164],
];

/// Standard matplotlib magma perceptual colormap — 256 RGB entries.
pub const MAGMA_LUT: [[u8; 3]; 256] = [
    [0, 0, 3],
    [0, 0, 4],
    [0, 0, 6],
    [1, 0, 7],
    [1, 1, 9],
    [1, 1, 11],
    [2, 2, 13],
    [2, 2, 15],
    [3, 3, 17],
    [4, 3, 19],
    [4, 4, 21],
    [5, 4, 23],
    [6, 5, 25],
    [7, 5, 27],
    [8, 6, 29],
    [9, 7, 31],
    [10, 7, 34],
    [11, 8, 36],
    [12, 9, 38],
    [13, 10, 40],
    [14, 10, 42],
    [15, 11, 44],
    [16, 12, 47],
    [17, 12, 49],
    [18, 13, 51],
    [20, 13, 53],
    [21, 14, 56],
    [22, 14, 58],
    [23, 15, 60],
    [24, 15, 63],
    [26, 16, 65],
    [27, 16, 68],
    [28, 16, 70],
    [30, 16, 73],
    [31, 17, 75],
    [32, 17, 77],
    [34, 17, 80],
    [35, 17, 82],
    [37, 17, 85],
    [38, 17, 87],
    [40, 17, 89],
    [42, 17, 92],
    [43, 17, 94],
    [45, 16, 96],
    [47, 16, 98],
    [48, 16, 101],
    [50, 16, 103],
    [52, 16, 104],
    [53, 15, 106],
    [55, 15, 108],
    [57, 15, 110],
    [59, 15, 111],
    [60, 15, 113],
    [62, 15, 114],
    [64, 15, 115],
    [66, 15, 116],
    [67, 15, 117],
    [69, 15, 118],
    [71, 15, 119],
    [72, 16, 120],
    [74, 16, 121],
    [75, 16, 121],
    [77, 17, 122],
    [79, 17, 123],
    [80, 18, 123],
    [82, 18, 124],
    [83, 19, 124],
    [85, 19, 125],
    [87, 20, 125],
    [88, 21, 126],
    [90, 21, 126],
    [91, 22, 126],
    [93, 23, 126],
    [94, 23, 127],
    [96, 24, 127],
    [97, 24, 127],
    [99, 25, 127],
    [101, 26, 128],
    [102, 26, 128],
    [104, 27, 128],
    [105, 28, 128],
    [107, 28, 128],
    [108, 29, 128],
    [110, 30, 129],
    [111, 30, 129],
    [113, 31, 129],
    [115, 31, 129],
    [116, 32, 129],
    [118, 33, 129],
    [119, 33, 129],
    [121, 34, 129],
    [122, 34, 129],
    [124, 35, 129],
    [126, 36, 129],
    [127, 36, 129],
    [129, 37, 129],
    [130, 37, 129],
    [132, 38, 129],
    [133, 38, 129],
    [135, 39, 129],
    [137, 40, 129],
    [138, 40, 129],
    [140, 41, 128],
    [141, 41, 128],
    [143, 42, 128],
    [145, 42, 128],
    [146, 43, 128],
    [148, 43, 128],
    [149, 44, 128],
    [151, 44, 127],
    [153, 45, 127],
    [154, 45, 127],
    [156, 46, 127],
    [158, 46, 126],
    [159, 47, 126],
    [161, 47, 126],
    [163, 48, 126],
    [164, 48, 125],
    [166, 49, 125],
    [167, 49, 125],
    [169, 50, 124],
    [171, 51, 124],
    [172, 51, 123],
    [174, 52, 123],
    [176, 52, 123],
    [177, 53, 122],
    [179, 53, 122],
    [181, 54, 121],
    [182, 54, 121],
    [184, 55, 120],
    [185, 55, 120],
    [187, 56, 119],
    [189, 57, 119],
    [190, 57, 118],
    [192, 58, 117],
    [194, 58, 117],
    [195, 59, 116],
    [197, 60, 116],
    [198, 60, 115],
    [200, 61, 114],
    [202, 62, 114],
    [203, 62, 113],
    [205, 63, 112],
    [206, 64, 112],
    [208, 65, 111],
    [209, 66, 110],
    [211, 66, 109],
    [212, 67, 109],
    [214, 68, 108],
    [215, 69, 107],
    [217, 70, 106],
    [218, 71, 105],
    [220, 72, 105],
    [221, 73, 104],
    [222, 74, 103],
    [224, 75, 102],
    [225, 76, 102],
    [226, 77, 101],
    [228, 78, 100],
    [229, 80, 99],
    [230, 81, 98],
    [231, 82, 98],
    [232, 84, 97],
    [234, 85, 96],
    [235, 86, 96],
    [236, 88, 95],
    [237, 89, 95],
    [238, 91, 94],
    [238, 93, 93],
    [239, 94, 93],
    [240, 96, 93],
    [241, 97, 92],
    [242, 99, 92],
    [243, 101, 92],
    [243, 103, 91],
    [244, 104, 91],
    [245, 106, 91],
    [245, 108, 91],
    [246, 110, 91],
    [246, 112, 91],
    [247, 113, 91],
    [247, 115, 92],
    [248, 117, 92],
    [248, 119, 92],
    [249, 121, 92],
    [249, 123, 93],
    [249, 125, 93],
    [250, 127, 94],
    [250, 128, 94],
    [250, 130, 95],
    [251, 132, 96],
    [251, 134, 96],
    [251, 136, 97],
    [251, 138, 98],
    [252, 140, 99],
    [252, 142, 99],
    [252, 144, 100],
    [252, 146, 101],
    [252, 147, 102],
    [253, 149, 103],
    [253, 151, 104],
    [253, 153, 105],
    [253, 155, 106],
    [253, 157, 107],
    [253, 159, 108],
    [253, 161, 110],
    [253, 162, 111],
    [253, 164, 112],
    [254, 166, 113],
    [254, 168, 115],
    [254, 170, 116],
    [254, 172, 117],
    [254, 174, 118],
    [254, 175, 120],
    [254, 177, 121],
    [254, 179, 123],
    [254, 181, 124],
    [254, 183, 125],
    [254, 185, 127],
    [254, 187, 128],
    [254, 188, 130],
    [254, 190, 131],
    [254, 192, 133],
    [254, 194, 134],
    [254, 196, 136],
    [254, 198, 137],
    [254, 199, 139],
    [254, 201, 141],
    [254, 203, 142],
    [253, 205, 144],
    [253, 207, 146],
    [253, 209, 147],
    [253, 210, 149],
    [253, 212, 151],
    [253, 214, 152],
    [253, 216, 154],
    [253, 218, 156],
    [253, 220, 157],
    [253, 221, 159],
    [253, 223, 161],
    [253, 225, 163],
    [252, 227, 165],
    [252, 229, 166],
    [252, 230, 168],
    [252, 232, 170],
    [252, 234, 172],
    [252, 236, 174],
    [252, 238, 176],
    [252, 240, 177],
    [252, 241, 179],
    [252, 243, 181],
    [252, 245, 183],
    [251, 247, 185],
    [251, 249, 187],
    [251, 250, 189],
    [251, 252, 191],
];

/// Standard matplotlib plasma perceptual colormap — 256 RGB entries.
pub const PLASMA_LUT: [[u8; 3]; 256] = [
    [12, 7, 134],
    [16, 7, 135],
    [19, 6, 137],
    [21, 6, 138],
    [24, 6, 139],
    [27, 6, 140],
    [29, 6, 141],
    [31, 5, 142],
    [33, 5, 143],
    [35, 5, 144],
    [37, 5, 145],
    [39, 5, 146],
    [41, 5, 147],
    [43, 5, 148],
    [45, 4, 148],
    [47, 4, 149],
    [49, 4, 150],
    [51, 4, 151],
    [52, 4, 152],
    [54, 4, 152],
    [56, 4, 153],
    [58, 4, 154],
    [59, 3, 154],
    [61, 3, 155],
    [63, 3, 156],
    [64, 3, 156],
    [66, 3, 157],
    [68, 3, 158],
    [69, 3, 158],
    [71, 2, 159],
    [73, 2, 159],
    [74, 2, 160],
    [76, 2, 161],
    [78, 2, 161],
    [79, 2, 162],
    [81, 1, 162],
    [82, 1, 163],
    [84, 1, 163],
    [86, 1, 163],
    [87, 1, 164],
    [89, 1, 164],
    [90, 0, 165],
    [92, 0, 165],
    [94, 0, 165],
    [95, 0, 166],
    [97, 0, 166],
    [98, 0, 166],
    [100, 0, 167],
    [101, 0, 167],
    [103, 0, 167],
    [104, 0, 167],
    [106, 0, 167],
    [108, 0, 168],
    [109, 0, 168],
    [111, 0, 168],
    [112, 0, 168],
    [114, 0, 168],
    [115, 0, 168],
    [117, 0, 168],
    [118, 1, 168],
    [120, 1, 168],
    [121, 1, 168],
    [123, 2, 168],
    [124, 2, 167],
    [126, 3, 167],
    [127, 3, 167],
    [129, 4, 167],
    [130, 4, 167],
    [132, 5, 166],
    [133, 6, 166],
    [134, 7, 166],
    [136, 7, 165],
    [137, 8, 165],
    [139, 9, 164],
    [140, 10, 164],
    [142, 12, 164],
    [143, 13, 163],
    [144, 14, 163],
    [146, 15, 162],
    [147, 16, 161],
    [149, 17, 161],
    [150, 18, 160],
    [151, 19, 160],
    [153, 20, 159],
    [154, 21, 158],
    [155, 23, 158],
    [157, 24, 157],
    [158, 25, 156],
    [159, 26, 155],
    [160, 27, 155],
    [162, 28, 154],
    [163, 29, 153],
    [164, 30, 152],
    [165, 31, 151],
    [167, 33, 151],
    [168, 34, 150],
    [169, 35, 149],
    [170, 36, 148],
    [172, 37, 147],
    [173, 38, 146],
    [174, 39, 145],
    [175, 40, 144],
    [176, 42, 143],
    [177, 43, 143],
    [178, 44, 142],
    [180, 45, 141],
    [181, 46, 140],
    [182, 47, 139],
    [183, 48, 138],
    [184, 50, 137],
    [185, 51, 136],
    [186, 52, 135],
    [187, 53, 134],
    [188, 54, 133],
    [189, 55, 132],
    [190, 56, 131],
    [191, 57, 130],
    [192, 59, 129],
    [193, 60, 128],
    [194, 61, 128],
    [195, 62, 127],
    [196, 63, 126],
    [197, 64, 125],
    [198, 65, 124],
    [199, 66, 123],
    [200, 68, 122],
    [201, 69, 121],
    [202, 70, 120],
    [203, 71, 119],
    [204, 72, 118],
    [205, 73, 117],
    [206, 74, 117],
    [207, 75, 116],
    [208, 77, 115],
    [209, 78, 114],
    [209, 79, 113],
    [210, 80, 112],
    [211, 81, 111],
    [212, 82, 110],
    [213, 83, 109],
    [214, 85, 109],
    [215, 86, 108],
    [215, 87, 107],
    [216, 88, 106],
    [217, 89, 105],
    [218, 90, 104],
    [219, 91, 103],
    [220, 93, 102],
    [220, 94, 102],
    [221, 95, 101],
    [222, 96, 100],
    [223, 97, 99],
    [223, 98, 98],
    [224, 100, 97],
    [225, 101, 96],
    [226, 102, 96],
    [227, 103, 95],
    [227, 104, 94],
    [228, 106, 93],
    [229, 107, 92],
    [229, 108, 91],
    [230, 109, 90],
    [231, 110, 90],
    [232, 112, 89],
    [232, 113, 88],
    [233, 114, 87],
    [234, 115, 86],
    [234, 116, 85],
    [235, 118, 84],
    [236, 119, 84],
    [236, 120, 83],
    [237, 121, 82],
    [237, 123, 81],
    [238, 124, 80],
    [239, 125, 79],
    [239, 126, 78],
    [240, 128, 77],
    [240, 129, 77],
    [241, 130, 76],
    [242, 132, 75],
    [242, 133, 74],
    [243, 134, 73],
    [243, 135, 72],
    [244, 137, 71],
    [244, 138, 71],
    [245, 139, 70],
    [245, 141, 69],
    [246, 142, 68],
    [246, 143, 67],
    [246, 145, 66],
    [247, 146, 65],
    [247, 147, 65],
    [248, 149, 64],
    [248, 150, 63],
    [248, 152, 62],
    [249, 153, 61],
    [249, 154, 60],
    [250, 156, 59],
    [250, 157, 58],
    [250, 159, 58],
    [250, 160, 57],
    [251, 162, 56],
    [251, 163, 55],
    [251, 164, 54],
    [252, 166, 53],
    [252, 167, 53],
    [252, 169, 52],
    [252, 170, 51],
    [252, 172, 50],
    [252, 173, 49],
    [253, 175, 49],
    [253, 176, 48],
    [253, 178, 47],
    [253, 179, 46],
    [253, 181, 45],
    [253, 182, 45],
    [253, 184, 44],
    [253, 185, 43],
    [253, 187, 43],
    [253, 188, 42],
    [253, 190, 41],
    [253, 192, 41],
    [253, 193, 40],
    [253, 195, 40],
    [253, 196, 39],
    [253, 198, 38],
    [252, 199, 38],
    [252, 201, 38],
    [252, 203, 37],
    [252, 204, 37],
    [252, 206, 37],
    [251, 208, 36],
    [251, 209, 36],
    [251, 211, 36],
    [250, 213, 36],
    [250, 214, 36],
    [250, 216, 36],
    [249, 217, 36],
    [249, 219, 36],
    [248, 221, 36],
    [248, 223, 36],
    [247, 224, 36],
    [247, 226, 37],
    [246, 228, 37],
    [246, 229, 37],
    [245, 231, 38],
    [245, 233, 38],
    [244, 234, 38],
    [243, 236, 38],
    [243, 238, 38],
    [242, 240, 38],
    [242, 241, 38],
    [241, 243, 38],
    [240, 245, 37],
    [240, 246, 35],
    [239, 248, 33],
];

/// Cividis colormap (optimized for color-vision deficiency) — 256 RGB entries.
pub const CIVIDIS_LUT: [[u8; 3]; 256] = [
    [0, 34, 77],
    [0, 35, 79],
    [0, 35, 80],
    [0, 36, 82],
    [0, 37, 84],
    [0, 38, 85],
    [0, 38, 87],
    [0, 39, 89],
    [0, 40, 91],
    [0, 40, 92],
    [0, 41, 94],
    [0, 42, 96],
    [0, 42, 98],
    [0, 43, 100],
    [0, 44, 102],
    [0, 44, 103],
    [0, 45, 105],
    [0, 46, 107],
    [0, 47, 109],
    [0, 47, 111],
    [0, 48, 112],
    [0, 48, 112],
    [0, 49, 112],
    [0, 49, 112],
    [4, 50, 112],
    [8, 51, 112],
    [11, 51, 112],
    [14, 52, 112],
    [17, 53, 111],
    [20, 54, 111],
    [22, 54, 111],
    [24, 55, 111],
    [26, 56, 111],
    [28, 56, 110],
    [29, 57, 110],
    [31, 58, 110],
    [33, 59, 110],
    [34, 59, 110],
    [36, 60, 110],
    [37, 61, 109],
    [39, 61, 109],
    [40, 62, 109],
    [42, 63, 109],
    [43, 63, 109],
    [44, 64, 109],
    [46, 65, 108],
    [47, 66, 108],
    [48, 66, 108],
    [49, 67, 108],
    [50, 68, 108],
    [52, 68, 108],
    [53, 69, 108],
    [54, 70, 108],
    [55, 70, 108],
    [56, 71, 108],
    [57, 72, 108],
    [58, 72, 107],
    [59, 73, 107],
    [61, 74, 107],
    [62, 75, 107],
    [63, 75, 107],
    [64, 76, 107],
    [65, 77, 107],
    [66, 77, 107],
    [67, 78, 107],
    [68, 79, 107],
    [69, 79, 107],
    [70, 80, 107],
    [71, 81, 107],
    [72, 81, 107],
    [73, 82, 107],
    [74, 83, 107],
    [75, 84, 108],
    [76, 84, 108],
    [77, 85, 108],
    [78, 86, 108],
    [78, 86, 108],
    [79, 87, 108],
    [80, 88, 108],
    [81, 88, 108],
    [82, 89, 108],
    [83, 90, 108],
    [84, 90, 108],
    [85, 91, 109],
    [86, 92, 109],
    [87, 93, 109],
    [88, 93, 109],
    [89, 94, 109],
    [89, 95, 109],
    [90, 95, 109],
    [91, 96, 110],
    [92, 97, 110],
    [93, 97, 110],
    [94, 98, 110],
    [95, 99, 110],
    [96, 100, 110],
    [97, 100, 111],
    [97, 101, 111],
    [98, 102, 111],
    [99, 102, 111],
    [100, 103, 111],
    [101, 104, 112],
    [102, 105, 112],
    [103, 105, 112],
    [104, 106, 112],
    [104, 107, 113],
    [105, 107, 113],
    [106, 108, 113],
    [107, 109, 113],
    [108, 109, 114],
    [109, 110, 114],
    [110, 111, 114],
    [110, 112, 115],
    [111, 112, 115],
    [112, 113, 115],
    [113, 114, 115],
    [114, 115, 116],
    [115, 115, 116],
    [116, 116, 117],
    [116, 117, 117],
    [117, 117, 117],
    [118, 118, 118],
    [119, 119, 118],
    [120, 120, 118],
    [121, 120, 119],
    [121, 121, 119],
    [122, 122, 119],
    [123, 123, 119],
    [124, 123, 120],
    [125, 124, 120],
    [126, 125, 120],
    [127, 125, 120],
    [128, 126, 120],
    [129, 127, 120],
    [130, 128, 120],
    [131, 128, 120],
    [132, 129, 120],
    [133, 130, 120],
    [133, 131, 120],
    [134, 131, 120],
    [135, 132, 120],
    [136, 133, 120],
    [137, 134, 120],
    [138, 134, 120],
    [139, 135, 120],
    [140, 136, 120],
    [141, 137, 120],
    [142, 137, 120],
    [143, 138, 119],
    [144, 139, 119],
    [145, 140, 119],
    [146, 140, 119],
    [147, 141, 119],
    [148, 142, 119],
    [149, 143, 119],
    [150, 143, 119],
    [151, 144, 118],
    [152, 145, 118],
    [153, 146, 118],
    [154, 147, 118],
    [155, 147, 118],
    [156, 148, 118],
    [157, 149, 117],
    [158, 150, 117],
    [159, 150, 117],
    [160, 151, 117],
    [161, 152, 116],
    [162, 153, 116],
    [163, 154, 116],
    [164, 154, 116],
    [165, 155, 115],
    [166, 156, 115],
    [167, 157, 115],
    [168, 158, 115],
    [169, 158, 114],
    [170, 159, 114],
    [171, 160, 114],
    [172, 161, 113],
    [173, 162, 113],
    [174, 162, 113],
    [175, 163, 112],
    [176, 164, 112],
    [177, 165, 112],
    [178, 166, 111],
    [179, 166, 111],
    [180, 167, 111],
    [181, 168, 110],
    [182, 169, 110],
    [183, 170, 109],
    [184, 171, 109],
    [185, 171, 109],
    [186, 172, 108],
    [187, 173, 108],
    [188, 174, 107],
    [189, 175, 107],
    [190, 176, 106],
    [191, 176, 106],
    [193, 177, 105],
    [194, 178, 105],
    [195, 179, 104],
    [196, 180, 104],
    [197, 181, 103],
    [198, 181, 103],
    [199, 182, 102],
    [200, 183, 101],
    [201, 184, 101],
    [202, 185, 100],
    [203, 186, 100],
    [204, 187, 99],
    [205, 188, 98],
    [206, 188, 98],
    [207, 189, 97],
    [208, 190, 96],
    [210, 191, 96],
    [211, 192, 95],
    [212, 193, 94],
    [213, 194, 94],
    [214, 195, 93],
    [215, 195, 92],
    [216, 196, 91],
    [217, 197, 90],
    [218, 198, 90],
    [219, 199, 89],
    [220, 200, 88],
    [222, 201, 87],
    [223, 202, 86],
    [224, 203, 85],
    [225, 204, 84],
    [226, 204, 83],
    [227, 205, 82],
    [228, 206, 81],
    [229, 207, 80],
    [230, 208, 79],
    [232, 209, 78],
    [233, 210, 77],
    [234, 211, 76],
    [235, 212, 75],
    [236, 213, 74],
    [237, 214, 72],
    [238, 215, 71],
    [239, 216, 70],
    [241, 217, 68],
    [242, 218, 67],
    [243, 218, 66],
    [244, 219, 64],
    [245, 220, 63],
    [246, 221, 61],
    [248, 222, 59],
    [249, 223, 58],
    [250, 224, 56],
    [251, 225, 54],
    [253, 226, 52],
    [253, 227, 51],
    [253, 229, 52],
    [253, 230, 54],
    [253, 231, 55],
];

/// Google turbo colormap — 256 RGB entries.
pub const TURBO_LUT: [[u8; 3]; 256] = [
    [48, 18, 59],
    [49, 21, 66],
    [50, 24, 74],
    [52, 27, 81],
    [53, 30, 88],
    [54, 33, 95],
    [55, 35, 101],
    [56, 38, 108],
    [57, 41, 114],
    [58, 44, 121],
    [59, 47, 127],
    [60, 50, 133],
    [60, 53, 139],
    [61, 55, 145],
    [62, 58, 150],
    [63, 61, 156],
    [64, 64, 161],
    [64, 67, 166],
    [65, 69, 171],
    [65, 72, 176],
    [66, 75, 181],
    [67, 78, 186],
    [67, 80, 190],
    [67, 83, 194],
    [68, 86, 199],
    [68, 88, 203],
    [69, 91, 206],
    [69, 94, 210],
    [69, 96, 214],
    [69, 99, 217],
    [70, 102, 221],
    [70, 104, 224],
    [70, 107, 227],
    [70, 109, 230],
    [70, 112, 232],
    [70, 115, 235],
    [70, 117, 237],
    [70, 120, 240],
    [70, 122, 242],
    [70, 125, 244],
    [70, 127, 246],
    [70, 130, 248],
    [69, 132, 249],
    [69, 135, 251],
    [69, 137, 252],
    [68, 140, 253],
    [67, 142, 253],
    [66, 145, 254],
    [65, 147, 254],
    [64, 150, 254],
    [63, 152, 254],
    [62, 155, 254],
    [60, 157, 253],
    [59, 160, 252],
    [57, 162, 252],
    [56, 165, 251],
    [54, 168, 249],
    [52, 170, 248],
    [51, 172, 246],
    [49, 175, 245],
    [47, 177, 243],
    [45, 180, 241],
    [43, 182, 239],
    [42, 185, 237],
    [40, 187, 235],
    [38, 189, 233],
    [37, 192, 230],
    [35, 194, 228],
    [33, 196, 225],
    [32, 198, 223],
    [30, 201, 220],
    [29, 203, 218],
    [28, 205, 215],
    [27, 207, 212],
    [26, 209, 210],
    [25, 211, 207],
    [24, 213, 204],
    [24, 215, 202],
    [23, 217, 199],
    [23, 218, 196],
    [23, 220, 194],
    [23, 222, 191],
    [24, 224, 189],
    [24, 225, 186],
    [25, 227, 184],
    [26, 228, 182],
    [27, 229, 180],
    [29, 231, 177],
    [30, 232, 175],
    [32, 233, 172],
    [34, 235, 169],
    [36, 236, 166],
    [39, 237, 163],
    [41, 238, 160],
    [44, 239, 157],
    [47, 240, 154],
    [50, 241, 151],
    [53, 243, 148],
    [56, 244, 145],
    [59, 244, 141],
    [63, 245, 138],
    [66, 246, 135],
    [70, 247, 131],
    [74, 248, 128],
    [77, 249, 124],
    [81, 249, 121],
    [85, 250, 118],
    [89, 251, 114],
    [93, 251, 111],
    [97, 252, 108],
    [101, 252, 104],
    [105, 253, 101],
    [109, 253, 98],
    [113, 253, 95],
    [116, 254, 92],
    [120, 254, 89],
    [124, 254, 86],
    [128, 254, 83],
    [132, 254, 80],
    [135, 254, 77],
    [139, 254, 75],
    [142, 254, 72],
    [146, 254, 70],
    [149, 254, 68],
    [152, 254, 66],
    [155, 253, 64],
    [158, 253, 62],
    [161, 252, 61],
    [164, 252, 59],
    [166, 251, 58],
    [169, 251, 57],
    [172, 250, 55],
    [174, 249, 55],
    [177, 248, 54],
    [179, 248, 53],
    [182, 247, 53],
    [185, 245, 52],
    [187, 244, 52],
    [190, 243, 52],
    [192, 242, 51],
    [195, 241, 51],
    [197, 239, 51],
    [200, 238, 51],
    [202, 237, 51],
    [205, 235, 52],
    [207, 234, 52],
    [209, 232, 52],
    [212, 231, 53],
    [214, 229, 53],
    [216, 227, 53],
    [218, 226, 54],
    [221, 224, 54],
    [223, 222, 54],
    [225, 220, 55],
    [227, 218, 55],
    [229, 216, 56],
    [231, 215, 56],
    [232, 213, 56],
    [234, 211, 57],
    [236, 209, 57],
    [237, 207, 57],
    [239, 205, 57],
    [240, 203, 58],
    [242, 200, 58],
    [243, 198, 58],
    [244, 196, 58],
    [246, 194, 58],
    [247, 192, 57],
    [248, 190, 57],
    [249, 188, 57],
    [249, 186, 56],
    [250, 183, 55],
    [251, 181, 55],
    [251, 179, 54],
    [252, 176, 53],
    [252, 174, 52],
    [253, 171, 51],
    [253, 169, 50],
    [253, 166, 49],
    [253, 163, 48],
    [254, 161, 47],
    [254, 158, 46],
    [254, 155, 45],
    [254, 152, 44],
    [253, 149, 43],
    [253, 146, 41],
    [253, 143, 40],
    [253, 140, 39],
    [252, 137, 38],
    [252, 134, 36],
    [251, 131, 35],
    [251, 128, 34],
    [250, 125, 32],
    [250, 122, 31],
    [249, 119, 30],
    [248, 116, 28],
    [247, 113, 27],
    [247, 110, 26],
    [246, 107, 24],
    [245, 104, 23],
    [244, 101, 22],
    [243, 99, 21],
    [242, 96, 20],
    [241, 93, 19],
    [239, 90, 17],
    [238, 88, 16],
    [237, 85, 15],
    [236, 82, 14],
    [234, 80, 13],
    [233, 77, 13],
    [232, 75, 12],
    [230, 73, 11],
    [229, 70, 10],
    [227, 68, 10],
    [226, 66, 9],
    [224, 64, 8],
    [222, 62, 8],
    [221, 60, 7],
    [219, 58, 7],
    [217, 56, 6],
    [215, 54, 6],
    [214, 52, 5],
    [212, 50, 5],
    [210, 48, 5],
    [208, 47, 4],
    [206, 45, 4],
    [203, 43, 3],
    [201, 41, 3],
    [199, 40, 3],
    [197, 38, 2],
    [195, 36, 2],
    [192, 35, 2],
    [190, 33, 2],
    [187, 31, 1],
    [185, 30, 1],
    [182, 28, 1],
    [180, 27, 1],
    [177, 25, 1],
    [174, 24, 1],
    [172, 22, 1],
    [169, 21, 1],
    [166, 20, 1],
    [163, 18, 1],
    [160, 17, 1],
    [157, 16, 1],
    [154, 14, 1],
    [151, 13, 1],
    [148, 12, 1],
    [145, 11, 1],
    [142, 10, 1],
    [139, 9, 1],
    [135, 8, 1],
    [132, 7, 1],
    [129, 6, 2],
    [125, 5, 2],
    [122, 4, 2],
];
