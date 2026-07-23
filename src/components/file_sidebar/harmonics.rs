use crate::dsp::harmonics;
use crate::state::store_fields::*;
use crate::state::AppState;
use leptos::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement};

#[component]
pub(crate) fn HarmonicsPanel() -> impl IntoView {
    let state = expect_context::<AppState>();

    let harmonics = Memo::new(move |_| {
        let files = state.library.files().get();
        let idx = state.library.current_index().get();
        idx.and_then(|i| files.get(i).cloned())
            .map(|file| harmonics::analyze_harmonics(&file.audio, &file.spectrogram))
    });

    view! {
        <div class="sidebar-panel">
            {move || {
                match harmonics.get() {
                    None => view! {
                        <div class="sidebar-panel-empty">"No file selected"</div>
                    }.into_any(),
                    Some(h) => {
                        let coherence_pct = format!("{:.0}%", h.phase_coherence_mean * 100.0);
                        let coherence_label = if h.phase_coherence_mean >= 0.65 {
                            "High (natural)"
                        } else if h.phase_coherence_mean >= 0.45 {
                            "Moderate"
                        } else {
                            "Low (processed)"
                        };
                        let coherence_color = if h.phase_coherence_mean >= 0.65 {
                            "#4c8"
                        } else if h.phase_coherence_mean >= 0.45 {
                            "#fc8"
                        } else {
                            "#f64"
                        };

                        let ratio_text = format!("{:.2}×", h.harmonic_coherence_ratio);
                        let ratio_label = if h.harmonic_coherence_ratio >= 0.9 {
                            "On-par with noise floor"
                        } else if h.harmonic_coherence_ratio >= 0.7 {
                            "Slightly reduced"
                        } else {
                            "Reduced — possible artifacts"
                        };

                        let fund_text = match h.fundamental_freq {
                            Some(f) => format!("{:.1} kHz", f / 1000.0),
                            None => "Not detected".to_string(),
                        };
                        let decay_text = format!("{:.2}", h.decay_exponent);
                        let decay_label = if h.decay_exponent >= 0.8 && h.decay_exponent <= 2.5 {
                            "Natural range"
                        } else if h.decay_exponent > 2.5 {
                            "Steep (processed?)"
                        } else {
                            "Shallow (possible aliasing)"
                        };

                        let flux_mean_text = format!("{:.4}", h.flux_mean);
                        let flux_peak_text = format!("{:.4}", h.flux_peak);
                        let preringing_text = if h.preringing_count == 0 {
                            "None".to_string()
                        } else {
                            format!("{} frame(s)", h.preringing_count)
                        };
                        let preringing_color = if h.preringing_count == 0 { "#4c8" } else { "#f64" };
                        let staircase_pct = format!("{:.0}%", h.staircasing_score * 100.0);
                        let staircase_color = if h.staircasing_score < 0.3 {
                            "#4c8"
                        } else if h.staircasing_score < 0.5 {
                            "#fc8"
                        } else {
                            "#f64"
                        };

                        let indicators = h.artifact_indicators.clone();
                        let all_clear = indicators.len() == 1
                            && indicators[0].contains("No significant");

                        let amplitudes_for_chart = h.harmonic_amplitudes.clone();
                        let anomalies_for_chart = h.decay_anomaly_indices.clone();
                        let decay_exp_for_chart = h.decay_exponent;
                        let flux_for_chart = h.flux_per_frame.clone();
                        let flux_peak_for_chart = h.flux_peak;
                        let flux_mean_for_chart = h.flux_mean;
                        let preringing_count_for_chart = h.preringing_count;

                        view! {
                            // --- Phase Coherence ---
                            <div class="setting-group">
                                <div class="setting-group-title">"Phase Coherence"</div>
                                <div class="analysis-stats">
                                    <div class="analysis-stat"
                                        title="Mean phase coherence across all frequency bins above the noise floor. \
                                               Above 65% suggests natural signal; below 45% suggests heavy processing.">
                                        <span class="analysis-stat-value"
                                            style=format!("color:{}", coherence_color)>
                                            {coherence_pct}
                                        </span>
                                        <span class="analysis-stat-label">"Mean coherence"</span>
                                    </div>
                                    <div class="analysis-stat"
                                        title="Overall verdict based on mean coherence score.">
                                        <span class="analysis-stat-value">{coherence_label}</span>
                                        <span class="analysis-stat-label">"Assessment"</span>
                                    </div>
                                    <div class="analysis-stat"
                                        title="Ratio of coherence at the detected harmonic frequencies vs. the overall \
                                               mean. Values below 1.0× mean the harmonic bins are less coherent than the \
                                               background — a sign of synthetic harmonics introduced by processing.">
                                        <span class="analysis-stat-value">{ratio_text}</span>
                                        <span class="analysis-stat-label">"Harmonic ratio"</span>
                                    </div>
                                </div>
                                <div class="analysis-warning" style="color:#888;font-style:italic">
                                    {ratio_label}
                                </div>
                            </div>

                            // --- Harmonic Decay ---
                            <div class="setting-group">
                                <div class="setting-group-title">"Harmonic Decay"</div>
                                <div style="padding:2px 12px 6px;font-size:10px;color:#666;line-height:1.5">
                                    "Natural overtones follow a power-law: each harmonic (2f, 3f\u{2026}) has less \
                                    energy than the one below it, roughly A\u{2099} \u{221d} 1/n\u{1d45}. Pitch-shifting \
                                    can produce alias harmonics that violate this — a higher overtone equalling or \
                                    exceeding the one below it is a red flag. The dashed curve on the chart shows the \
                                    fitted decay law; red bars are anomalies."
                                </div>
                                <div class="analysis-stats">
                                    <div class="analysis-stat"
                                        title="The fundamental frequency detected via Harmonic Product Spectrum. \
                                               For bat calls this is typically 20–100 kHz.">
                                        <span class="analysis-stat-value">{fund_text}</span>
                                        <span class="analysis-stat-label">"Fundamental"</span>
                                    </div>
                                    <div class="analysis-stat"
                                        title="Power-law decay exponent \u{3B1} fitted to A\u{2099} \u{2248} 1/n\u{1d45}. \
                                               Natural sounds typically fall in the range 0.8–2.5. Very low values \
                                               (flat decay) suggest aliasing; very high values suggest steep roll-off \
                                               from aggressive filtering.">
                                        <span class="analysis-stat-value">{decay_text}</span>
                                        <span class="analysis-stat-label">"\u{03B1} exponent"</span>
                                    </div>
                                    <div class="analysis-stat"
                                        title="Whether each successive harmonic is strictly weaker than the previous. \
                                               Non-monotonic decay (No) means at least one overtone has more energy \
                                               than the one below it — a common artifact of aliasing.">
                                        <span class="analysis-stat-value">
                                            {if h.decay_is_monotonic { "Yes" } else { "No" }}
                                        </span>
                                        <span class="analysis-stat-label">"Monotonic"</span>
                                    </div>
                                </div>
                                <div class="analysis-warning" style="color:#888;font-style:italic">
                                    {decay_label}
                                </div>
                                // Harmonic decay bar chart
                                {if !amplitudes_for_chart.is_empty() {
                                    view! {
                                        <HarmonicDecayChart
                                            amplitudes=amplitudes_for_chart
                                            anomaly_indices=anomalies_for_chart
                                            decay_exponent=decay_exp_for_chart
                                        />
                                    }.into_any()
                                } else {
                                    view! { <span></span> }.into_any()
                                }}
                            </div>

                            // --- Spectral Flux ---
                            <div class="setting-group">
                                <div class="setting-group-title">"Spectral Flux"</div>
                                <div style="padding:2px 12px 6px;font-size:10px;color:#666;line-height:1.5">
                                    "Spectral flux measures how much the frequency content changes between frames. \
                                    Pre-ringing is energy that appears just before a sudden onset — a fingerprint of \
                                    FFT windowing smear in heavily processed audio. Staircasing is when the peak \
                                    frequency stays stuck at the same bin despite energy changing, revealing that \
                                    frequency has been quantised into discrete steps."
                                </div>
                                <div class="analysis-stats">
                                    <div class="analysis-stat"
                                        title="Average frame-to-frame onset flux (half-wave rectified). \
                                               Higher values mean more rapid spectral change overall.">
                                        <span class="analysis-stat-value">{flux_mean_text}</span>
                                        <span class="analysis-stat-label">"Mean flux"</span>
                                    </div>
                                    <div class="analysis-stat"
                                        title="Maximum onset flux across all frame transitions. \
                                               Used as a reference to scale the pre-ringing and staircasing thresholds.">
                                        <span class="analysis-stat-value">{flux_peak_text}</span>
                                        <span class="analysis-stat-label">"Peak flux"</span>
                                    </div>
                                    <div class="analysis-stat"
                                        title="Number of frames with significant flux that immediately precede a \
                                               much larger onset. In clean recordings this should be zero; in \
                                               STFT-processed audio the window smear can deposit energy in adjacent \
                                               frames before the true transient.">
                                        <span class="analysis-stat-value"
                                            style=format!("color:{}", preringing_color)>
                                            {preringing_text}
                                        </span>
                                        <span class="analysis-stat-label">"Pre-ringing"</span>
                                    </div>
                                    <div class="analysis-stat"
                                        title="Fraction of active transitions where the peak frequency bin does not \
                                               move despite energy changing. High values indicate the frequency sweep \
                                               is advancing in discrete steps (a staircase pattern) rather than \
                                               smoothly, which is characteristic of pitch-shifted audio.">
                                        <span class="analysis-stat-value"
                                            style=format!("color:{}", staircase_color)>
                                            {staircase_pct}
                                        </span>
                                        <span class="analysis-stat-label">"Staircasing"</span>
                                    </div>
                                </div>
                                {if !flux_for_chart.is_empty() {
                                    view! {
                                        <FluxTimelineChart
                                            flux=flux_for_chart
                                            flux_peak=flux_peak_for_chart
                                            flux_mean=flux_mean_for_chart
                                            preringing_count=preringing_count_for_chart
                                        />
                                    }.into_any()
                                } else {
                                    view! { <span></span> }.into_any()
                                }}
                                <div style="padding:2px 12px 0;font-size:9px;color:#555">
                                    "Grey area = flux over time \u{2014} dashed line = mean \u{2014} red dots = pre-ringing"
                                </div>
                            </div>

                            // --- Artifact Indicators ---
                            <div class="setting-group">
                                <div class="setting-group-title">"Findings"</div>
                                <div style="padding: 4px 12px">
                                    {indicators.into_iter().map(|msg| {
                                        let color = if all_clear { "#4c8" } else { "#fc8" };
                                        view! {
                                            <div style=format!(
                                                "font-size:11px;color:{};padding:2px 0;line-height:1.4",
                                                color
                                            )>
                                                {if all_clear { "\u{2713} " } else { "\u{26a0} " }}
                                                {msg}
                                            </div>
                                        }
                                    }).collect::<Vec<_>>()}
                                </div>
                                <div style="padding:4px 12px 8px;font-size:10px;color:#555;line-height:1.5">
                                    "These metrics detect processing artifacts but cannot make a definitive judgement — \
                                    some natural recordings have low phase coherence (broadband noise, complex calls), \
                                    and clean pitch-shifted audio may score well. Use alongside the heatmap and your \
                                    own knowledge of the recording."
                                </div>
                            </div>
                        }.into_any()
                    }
                }
            }}
        </div>
    }
}

#[component]
fn HarmonicDecayChart(
    amplitudes: Vec<f32>,
    anomaly_indices: Vec<usize>,
    decay_exponent: f32,
) -> impl IntoView {
    let canvas_ref = NodeRef::<leptos::html::Canvas>::new();
    let amps = amplitudes.clone();
    let anom = anomaly_indices.clone();
    let alpha = decay_exponent;

    Effect::new(move || {
        let Some(el) = canvas_ref.get() else { return };
        let canvas: &HtmlCanvasElement = el.as_ref();
        let w = 220u32;
        let h = 80u32;
        canvas.set_width(w);
        canvas.set_height(h);
        let ctx = canvas
            .get_context("2d")
            .unwrap()
            .unwrap()
            .dyn_into::<CanvasRenderingContext2d>()
            .unwrap();

        // Background
        ctx.set_fill_style_str("#111");
        ctx.fill_rect(0.0, 0.0, w as f64, h as f64);

        if amps.is_empty() {
            return;
        }

        let n = amps.len();
        let bar_w = (w as f64 - 16.0) / n as f64;
        let chart_h = h as f64 - 12.0;
        let x_off = 8.0;

        // Draw power-law reference curve in dim white
        ctx.set_stroke_style_str("rgba(200,200,200,0.35)");
        ctx.set_line_width(1.0);
        ctx.begin_path();
        for i in 0..=n {
            let n_i = (i + 1) as f32;
            let expected = 1.0f32 / n_i.powf(alpha);
            let y = chart_h - expected as f64 * chart_h + 4.0;
            let x = x_off + i as f64 * bar_w + bar_w * 0.5;
            if i == 0 {
                ctx.move_to(x, y);
            } else {
                ctx.line_to(x, y);
            }
        }
        ctx.stroke();

        // Draw bars
        for (i, &amp) in amps.iter().enumerate() {
            let is_anomaly = anom.contains(&i);
            let color = if is_anomaly { "#f64" } else { "#4a8" };
            ctx.set_fill_style_str(color);
            let bar_h = (amp as f64 * chart_h).max(1.0);
            let x = x_off + i as f64 * bar_w + 1.0;
            let y = chart_h - bar_h + 4.0;
            ctx.fill_rect(x, y, bar_w - 2.0, bar_h);

            // Harmonic label
            ctx.set_fill_style_str("#888");
            ctx.set_font("8px monospace");
            let label = format!("H{}", i + 1);
            let _ = ctx.fill_text(&label, x + 1.0, h as f64 - 1.0);
        }
    });

    view! {
        <div style="padding:4px 12px 0">
            <canvas
                node_ref=canvas_ref
                style="width:100%;height:80px;display:block;border-radius:3px"
            />
        </div>
    }
}

#[component]
fn FluxTimelineChart(
    flux: Vec<f32>,
    flux_peak: f32,
    flux_mean: f32,
    preringing_count: usize,
) -> impl IntoView {
    let canvas_ref = NodeRef::<leptos::html::Canvas>::new();
    let flux_data = flux.clone();
    let peak = flux_peak;
    let mean = flux_mean;
    let precount = preringing_count;

    Effect::new(move || {
        let Some(el) = canvas_ref.get() else { return };
        let canvas: &HtmlCanvasElement = el.as_ref();
        let w = 220u32;
        let h = 60u32;
        canvas.set_width(w);
        canvas.set_height(h);
        let ctx = canvas
            .get_context("2d")
            .unwrap()
            .unwrap()
            .dyn_into::<CanvasRenderingContext2d>()
            .unwrap();

        // Background
        ctx.set_fill_style_str("#111");
        ctx.fill_rect(0.0, 0.0, w as f64, h as f64);

        if flux_data.is_empty() || peak < 1e-10 {
            return;
        }

        let n = flux_data.len();
        let chart_h = h as f64 - 4.0;

        // Filled area
        ctx.set_fill_style_str("rgba(80,120,200,0.5)");
        ctx.begin_path();
        ctx.move_to(0.0, h as f64);
        for (i, &f) in flux_data.iter().enumerate() {
            let x = i as f64 / n as f64 * w as f64;
            let y = chart_h - (f / peak) as f64 * chart_h + 2.0;
            ctx.line_to(x, y);
        }
        ctx.line_to(w as f64, h as f64);
        ctx.close_path();
        ctx.fill();

        // Mean line
        if mean > 0.0 {
            let mean_y = chart_h - (mean / peak) as f64 * chart_h + 2.0;
            ctx.set_stroke_style_str("rgba(200,200,100,0.6)");
            ctx.set_line_width(1.0);
            ctx.begin_path();
            ctx.move_to(0.0, mean_y);
            ctx.line_to(w as f64, mean_y);
            ctx.stroke();
        }

        // Pre-ringing markers (red dots)
        if precount > 0 {
            let onset_threshold = peak * 0.4;
            let preflux_threshold = peak * 0.12;
            let look_ahead = 5usize;
            ctx.set_fill_style_str("#f64");
            for t in 0..flux_data.len() {
                let f = flux_data[t];
                if f < preflux_threshold || f >= onset_threshold {
                    continue;
                }
                let window_end = (t + 1 + look_ahead).min(flux_data.len());
                let has_onset = flux_data[t + 1..window_end]
                    .iter()
                    .any(|&v| v > onset_threshold);
                if has_onset {
                    let x = t as f64 / n as f64 * w as f64;
                    let y = chart_h - (f / peak) as f64 * chart_h + 2.0;
                    ctx.begin_path();
                    let _ = ctx.arc(x, y, 3.0, 0.0, std::f64::consts::TAU);
                    ctx.fill();
                }
            }
        }
    });

    view! {
        <div style="padding:4px 12px 0">
            <canvas
                node_ref=canvas_ref
                style="width:100%;height:60px;display:block;border-radius:3px"
            />
        </div>
    }
}
