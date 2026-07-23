//! 2D colormap lookup tables.
//!
//! A 2D colormap maps two byte values (primary, secondary) to an RGB triple.
//! Used for visualizations that encode two dimensions of data in color:
//! - Flow: primary = intensity, secondary = shift direction
//! - Chromagram: primary = pitch class intensity, secondary = note intensity

use crate::canvas::colors::flow_rgb;

/// A 2D colormap: 256 × 256 → RGB lookup table (192 KB).
#[derive(Clone)]
pub struct Colormap2D {
    /// Row-major: `lut[secondary * 256 + primary]`.
    lut: Vec<[u8; 3]>,
}

impl Colormap2D {
    /// Look up the color for given (primary, secondary) byte values.
    #[inline]
    pub fn apply(&self, primary: u8, secondary: u8) -> [u8; 3] {
        self.lut[secondary as usize * 256 + primary as usize]
    }
}

/// Convert HSL (h in degrees 0–360, s and l in 0.0–1.0) to RGB [0–255].
pub fn hsl_to_rgb(h: f32, s: f32, l: f32) -> [u8; 3] {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let h_prime = h / 60.0;
    let x = c * (1.0 - (h_prime % 2.0 - 1.0).abs());
    let (r1, g1, b1) = match h_prime as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    [
        ((r1 + m) * 255.0).clamp(0.0, 255.0) as u8,
        ((g1 + m) * 255.0).clamp(0.0, 255.0) as u8,
        ((b1 + m) * 255.0).clamp(0.0, 255.0) as u8,
    ]
}

/// Build a flow colormap.
///
/// - Primary axis (0–255): pixel intensity (greyscale magnitude).
/// - Secondary axis (0–255): shift direction — 128 = neutral,
///   0 = max downward (blue), 255 = max upward (red).
///
/// The `intensity_gate`, `flow_gate`, and `opacity` parameters control
/// thresholds and color strength, matching the existing `flow_rgb` logic.
pub fn build_flow_colormap(intensity_gate: f32, flow_gate: f32, opacity: f32) -> Colormap2D {
    let mut lut = vec![[0u8; 3]; 256 * 256];

    for sec in 0..256u16 {
        // Map secondary byte to shift in [-1, 1]
        let shift = (sec as f32 - 128.0) / 128.0;

        for pri in 0..256u16 {
            let grey = pri as u8;
            let rgb = flow_rgb(grey, shift, intensity_gate, flow_gate, opacity, 3.0, 1.0);
            lut[sec as usize * 256 + pri as usize] = rgb;
        }
    }

    Colormap2D { lut }
}

/// Build a chromagram colormap (warm orange-to-white, no pitch distinction).
///
/// - Primary axis (0–255): overall pitch class intensity.
/// - Secondary axis (0–255): specific note (octave) intensity.
///
/// When both are high: bright white/yellow. When class is high but note is low:
/// dim warm color (energy in the pitch class, but not this specific octave).
/// When both are low: black.
pub fn build_chromagram_colormap() -> Colormap2D {
    // Cached: the LUT is a pure constant (no runtime input), so build it once
    // per thread instead of every render. [render-path perf]
    thread_local! { static CACHE: Colormap2D = build_chromagram_colormap_inner(); }
    CACHE.with(|c| c.clone())
}

fn build_chromagram_colormap_inner() -> Colormap2D {
    let mut lut = vec![[0u8; 3]; 256 * 256];

    for sec in 0..256u16 {
        let note = sec as f32 / 255.0; // specific note intensity

        for pri in 0..256u16 {
            let class = pri as f32 / 255.0; // overall pitch class intensity

            // Base brightness from class intensity
            // Note intensity adds contrast within the band
            let brightness = class * 0.4 + note * 0.6;
            // Saturation: high class + low note → warm desaturated; high note → vivid
            let saturation = if class > 0.01 {
                (note / class).min(1.0)
            } else {
                0.0
            };

            // HSL-ish mapping: warm orange-to-white
            // Low saturation: grey/white. High saturation: orange/yellow.
            let r = (brightness * (0.6 + 0.4 * saturation) * 255.0).min(255.0) as u8;
            let g = (brightness * (0.3 + 0.5 * saturation) * 255.0).min(255.0) as u8;
            let b = (brightness * (0.1 + 0.1 * saturation) * 255.0).min(255.0) as u8;

            lut[sec as usize * 256 + pri as usize] = [r, g, b];
        }
    }

    Colormap2D { lut }
}

/// Build 12 chromagram colormaps, one per pitch class, each with a distinct hue.
///
/// Naturals (C, D, E, F, G, A, B) get higher saturation; sharps/flats are muted.
/// Hues are spread across the spectrum avoiding pure red (0°) and pure blue (240°).
pub fn build_chromagram_pitch_class_colormaps() -> [Colormap2D; 12] {
    thread_local! { static CACHE: [Colormap2D; 12] = build_chromagram_pitch_class_colormaps_inner(); }
    CACHE.with(|c| c.clone())
}

fn build_chromagram_pitch_class_colormaps_inner() -> [Colormap2D; 12] {
    // Hues in degrees: C=50, C#=75, D=100, D#=130, E=160, F=190,
    // F#=215, G=260, G#=285, A=310, A#=335, B=40
    const HUES: [f32; 12] = [
        50.0, 75.0, 100.0, 130.0, 160.0, 190.0, 215.0, 260.0, 285.0, 310.0, 335.0, 40.0,
    ];
    // Naturals: C(0), D(2), E(4), F(5), G(7), A(9), B(11)
    const IS_NATURAL: [bool; 12] = [
        true, false, true, false, true, true, false, true, false, true, false, true,
    ];

    std::array::from_fn(|pc| {
        let hue = HUES[pc];
        let base_sat = if IS_NATURAL[pc] { 0.85 } else { 0.5 };

        let mut lut = vec![[0u8; 3]; 256 * 256];
        for sec in 0..256u16 {
            let note = sec as f32 / 255.0;
            for pri in 0..256u16 {
                let class = pri as f32 / 255.0;
                let brightness = class * 0.4 + note * 0.6;
                let saturation = if class > 0.01 {
                    base_sat * (note / class).min(1.0)
                } else {
                    0.0
                };
                let [r, g, b] = hsl_to_rgb(hue, saturation, brightness * 0.5);
                lut[sec as usize * 256 + pri as usize] = [r, g, b];
            }
        }
        Colormap2D { lut }
    })
}

/// Build 12 solid chromagram colormaps, one per pitch class.
///
/// Like `build_chromagram_pitch_class_colormaps` but all octaves within a pitch
/// class are rendered identically — brightness depends only on the overall pitch
/// class intensity (R channel), ignoring per-octave detail (G channel).
pub fn build_chromagram_solid_colormaps() -> [Colormap2D; 12] {
    thread_local! { static CACHE: [Colormap2D; 12] = build_chromagram_solid_colormaps_inner(); }
    CACHE.with(|c| c.clone())
}

fn build_chromagram_solid_colormaps_inner() -> [Colormap2D; 12] {
    const HUES: [f32; 12] = [
        50.0, 75.0, 100.0, 130.0, 160.0, 190.0, 215.0, 260.0, 285.0, 310.0, 335.0, 40.0,
    ];
    const IS_NATURAL: [bool; 12] = [
        true, false, true, false, true, true, false, true, false, true, false, true,
    ];

    std::array::from_fn(|pc| {
        let hue = HUES[pc];
        let base_sat = if IS_NATURAL[pc] { 0.85 } else { 0.5 };

        let mut lut = vec![[0u8; 3]; 256 * 256];
        for sec in 0..256u16 {
            // sec (G channel / note intensity) is ignored for solid mode
            for pri in 0..256u16 {
                let class = pri as f32 / 255.0;
                let brightness = class;
                let saturation = if brightness > 0.01 { base_sat } else { 0.0 };
                let [r, g, b] = hsl_to_rgb(hue, saturation, brightness * 0.5);
                lut[sec as usize * 256 + pri as usize] = [r, g, b];
            }
        }
        Colormap2D { lut }
    })
}

/// Build 10 chromagram colormaps, one per octave, using a rainbow from warm to cool.
///
/// Octave 0 (lowest) = warm orange (30°), octave 9 (highest) = violet (270°).
/// Same rainbow pattern repeats for every pitch class band.
pub fn build_chromagram_octave_colormaps() -> [Colormap2D; 10] {
    thread_local! { static CACHE: [Colormap2D; 10] = build_chromagram_octave_colormaps_inner(); }
    CACHE.with(|c| c.clone())
}

fn build_chromagram_octave_colormaps_inner() -> [Colormap2D; 10] {
    std::array::from_fn(|oct| {
        // Rainbow from warm (30°) to cool (270°)
        let hue = 30.0 + (oct as f32 / 9.0) * 240.0;

        let mut lut = vec![[0u8; 3]; 256 * 256];
        for sec in 0..256u16 {
            let note = sec as f32 / 255.0;
            for pri in 0..256u16 {
                let class = pri as f32 / 255.0;
                let brightness = class * 0.4 + note * 0.6;
                let saturation = if brightness > 0.01 { 0.7 } else { 0.0 };
                let [r, g, b] = hsl_to_rgb(hue, saturation, brightness * 0.5);
                lut[sec as usize * 256 + pri as usize] = [r, g, b];
            }
        }
        Colormap2D { lut }
    })
}

/// Build a phase coherence 2D colormap.
///
/// - Primary axis (0–255): pixel intensity (magnitude greyscale).
/// - Secondary axis (0–255): signed phase deviation direction — 128 = coherent
///   (no deviation), 0 = max backward deviation (blue), 255 = max forward
///   deviation (red).
///
/// Quiet pixels stay black. Coherent pixels (secondary ~128) are bright
/// blue-white. Deviating pixels shift toward red (forward) or blue (backward).
pub fn build_phase_coherence_colormap() -> Colormap2D {
    let mut lut = vec![[0u8; 3]; 256 * 256];

    for sec in 0..256u16 {
        // Map secondary byte to signed deviation in [-1, 1]
        let dev = (sec as f32 - 128.0) / 128.0;
        let abs_dev = dev.abs();

        for pri in 0..256u16 {
            let intensity = pri as f32 / 255.0;

            // Gate: very quiet pixels stay black
            if intensity < 0.015 {
                continue; // [0,0,0] already
            }

            // Gamma-adjusted brightness
            let bright = intensity.powf(0.75);

            // Coherent (abs_dev near 0): bright blue-white (matching existing scheme)
            // Deviating: shift toward red (positive) or deep blue (negative)
            let coherent_mix = (1.0 - abs_dev * 2.0).max(0.0); // 1 at center, 0 at |dev|>=0.5

            // Coherent color: pale blue-white (#c8e8ff at full brightness)
            let coh_r = 0.78;
            let coh_g = 0.91;
            let coh_b = 1.0;

            let (r, g, b) = if coherent_mix > 0.0 && abs_dev < 0.3 {
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
                // Backward deviation → blue
                let strength = ((abs_dev - 0.1) / 0.9).clamp(0.0, 1.0);
                let r = bright * coh_r * (1.0 - strength * 0.85);
                let g = bright * coh_g * (1.0 - strength * 0.5);
                let b = bright * (coh_b + strength * 0.0); // already 1.0
                (r, g, b)
            };

            lut[sec as usize * 256 + pri as usize] = [
                (r * 255.0).clamp(0.0, 255.0) as u8,
                (g * 255.0).clamp(0.0, 255.0) as u8,
                (b * 255.0).clamp(0.0, 255.0) as u8,
            ];
        }
    }

    Colormap2D { lut }
}
