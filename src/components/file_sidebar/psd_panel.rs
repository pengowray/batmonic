use leptos::prelude::*;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement};
use crate::audio::source::{ChannelView, DEFAULT_ANALYSIS_WINDOW_SECS};
use crate::audio::playback::effective_selection;
use crate::state::{AppState, RightSidebarTab};
use crate::dsp::psd::{self, PsdResult};
use crate::annotations::{
    Annotation, AnnotationKind, AnnotationSet, Group, Region,
    generate_uuid, now_iso8601,
};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq)]
enum PsdFreqRangeMode {
    Auto,
    All,
    Selection,
    FF,
}

/// Colors for peak markers (primary, then secondary peaks).
const PEAK_COLORS: &[&str] = &[
    "#ff4444", "#ff8844", "#ffaa44", "#44aaff", "#aa44ff", "#44ffaa", "#ff44aa", "#88ff44",
];

fn peak_color(i: usize) -> &'static str {
    PEAK_COLORS[i.min(PEAK_COLORS.len() - 1)]
}

fn hex_to_rgba(hex: &str, alpha: f64) -> String {
    let hex = hex.trim_start_matches('#');
    let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(255);
    let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(255);
    let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(255);
    format!("rgba({},{},{},{})", r, g, b, alpha)
}

#[component]
pub(crate) fn PsdPanel() -> impl IntoView {
    let state = expect_context::<AppState>();

    let psd_result: RwSignal<Option<PsdResult>> = RwSignal::new(None);
    let is_computing = RwSignal::new(false);
    let compute_gen = RwSignal::new(0u32);
    let analysis_is_full = RwSignal::new(false);
    let file_is_long = RwSignal::new(false);
    let using_selection = RwSignal::new(false);
    let log_scale = RwSignal::new(false);
    let freq_range_mode = RwSignal::new(PsdFreqRangeMode::Auto);

    // Trigger recomputation when tab is active and inputs change
    Effect::new(move || {
        let tab = state.right_sidebar_tab.get();
        if tab != RightSidebarTab::Psd {
            // Clear hover overlays when leaving the tab
            if !state.psd_hover_freqs.get_untracked().is_empty() {
                state.psd_hover_freqs.set(Vec::new());
            }
            return;
        }

        let _files = state.files.get();
        let _idx = state.current_file_index.get();
        let _sel = state.selection.get();
        let _nfft = state.psd_nfft.get();
        let _eq = state.psd_apply_eq.get();
        let _notch = state.psd_apply_notch.get();
        let _nr = state.psd_apply_nr.get();
        let _fr = freq_range_mode.get();
        // Subscribe to selection/annotation changes when freq range mode cares
        let _ = state.selection.get();
        let _ = state.selected_annotation_ids.get();

        // Subscribe to relevant filter params when toggles are on
        if state.psd_apply_eq.get_untracked() {
            let _ = state.filter_enabled.get();
            let _ = state.filter_freq_low.get();
            let _ = state.filter_freq_high.get();
            let _ = state.filter_band_mode.get();
        }
        if state.psd_apply_notch.get_untracked() {
            let _ = state.notch_enabled.get();
            let _ = state.notch_bands.get();
        }
        if state.psd_apply_nr.get_untracked() {
            let _ = state.noise_reduce_enabled.get();
        }

        let files = state.files.get_untracked();
        let idx = state.current_file_index.get_untracked();
        if idx.and_then(|i| files.get(i)).is_none() {
            psd_result.set(None);
            is_computing.set(false);
            return;
        }

        // Yield to the browser before mutating the panel state so the
        // triggering UI event fully unwinds before any DOM is rebuilt.
        spawn_local(async move {
            yield_to_browser().await;
            start_psd_compute(
                state,
                psd_result,
                is_computing,
                compute_gen,
                analysis_is_full,
                file_is_long,
                using_selection,
                freq_range_mode,
                false,
            );
        });
    });

    // Annotate all peaks
    let annotate_peaks = move |_: web_sys::MouseEvent| {
        let result = psd_result.get_untracked();
        let file_idx = state.current_file_index.get_untracked();
        let selection = state.selection.get_untracked();
        if let (Some(psd), Some(idx)) = (result, file_idx) {
            if psd.peaks.is_empty() { return; }
            let time_start;
            let time_end;
            if let Some(sel) = selection {
                time_start = sel.time_start;
                time_end = sel.time_end;
            } else {
                time_start = 0.0;
                let files = state.files.get_untracked();
                time_end = files.get(idx).map(|f| f.audio.duration_secs).unwrap_or(0.0);
            }

            state.snapshot_annotations();

            let group_id = generate_uuid();
            let mut annotations = vec![Annotation {
                id: group_id.clone(),
                kind: AnnotationKind::Group(Group {
                    label: Some("PSD Analysis".to_string()),
                    color: None,
                    collapsed: Some(false),
                }),
                created_at: now_iso8601(),
                modified_at: now_iso8601(),
                notes: Some(format!(
                    "NFFT={}, {} frames, {} peaks",
                    psd.nfft, psd.frame_count, psd.peaks.len()
                )),
                parent_id: None,
                sort_order: None,
                tags: Vec::new(),
            }];

            // Add selection bounds as the first child annotation
            if let Some(sel) = selection {
                if let (Some(flo), Some(fhi)) = (sel.freq_low, sel.freq_high) {
                    annotations.push(Annotation {
                        id: generate_uuid(),
                        kind: AnnotationKind::Region(Region {
                            time_start,
                            time_end,
                            freq_low: Some(flo),
                            freq_high: Some(fhi),
                            label: Some(format!("Selection {:.1}\u{2013}{:.1} kHz", flo / 1000.0, fhi / 1000.0)),
                            color: Some("#ffcc33".to_string()),
                            locked: None,
                        }),
                        created_at: now_iso8601(),
                        modified_at: now_iso8601(),
                        notes: None,
                        parent_id: Some(group_id.clone()),
                        sort_order: Some(-1.0),
                        tags: Vec::new(),
                    });
                }
            }

            for (i, peak) in psd.peaks.iter().enumerate() {
                let sort = i as f64 * 3.0;
                let color = peak_color(i).to_string();

                // Peak frequency region
                let peak_half_bw = psd.freq_resolution;
                annotations.push(Annotation {
                    id: generate_uuid(),
                    kind: AnnotationKind::Region(Region {
                        time_start,
                        time_end,
                        freq_low: Some((peak.freq_hz - peak_half_bw).max(0.0)),
                        freq_high: Some(peak.freq_hz + peak_half_bw),
                        label: Some(format!("F{} {:.1} kHz ({:.1} dB)", i + 1, peak.freq_hz / 1000.0, peak.power_db)),
                        color: Some(color.clone()),
                        locked: None,
                    }),
                    created_at: now_iso8601(),
                    modified_at: now_iso8601(),
                    notes: None,
                    parent_id: Some(group_id.clone()),
                    sort_order: Some(sort),
                    tags: Vec::new(),
                });

                if let Some((lo, hi)) = peak.bw_6db {
                    annotations.push(Annotation {
                        id: generate_uuid(),
                        kind: AnnotationKind::Region(Region {
                            time_start,
                            time_end,
                            freq_low: Some(lo),
                            freq_high: Some(hi),
                            label: Some(format!("F{} -6 dB: {:.1} kHz", i + 1, (hi - lo) / 1000.0)),
                            color: Some("#44aa66".to_string()),
                            locked: None,
                        }),
                        created_at: now_iso8601(),
                        modified_at: now_iso8601(),
                        notes: None,
                        parent_id: Some(group_id.clone()),
                        sort_order: Some(sort + 1.0),
                        tags: Vec::new(),
                    });
                }

                if let Some((lo, hi)) = peak.bw_10db {
                    annotations.push(Annotation {
                        id: generate_uuid(),
                        kind: AnnotationKind::Region(Region {
                            time_start,
                            time_end,
                            freq_low: Some(lo),
                            freq_high: Some(hi),
                            label: Some(format!("F{} -10 dB: {:.1} kHz", i + 1, (hi - lo) / 1000.0)),
                            color: Some("#aaaa44".to_string()),
                            locked: None,
                        }),
                        created_at: now_iso8601(),
                        modified_at: now_iso8601(),
                        notes: None,
                        parent_id: Some(group_id.clone()),
                        sort_order: Some(sort + 2.0),
                        tags: Vec::new(),
                    });
                }
            }

            state.annotation_store.update(|store| {
                store.ensure_len(idx + 1);
                if store.sets[idx].is_none() {
                    let new_set = state.files.with_untracked(|files| {
                        files.get(idx).map(|f| {
                            let id = f.identity.clone().unwrap_or_else(|| {
                                crate::file_identity::identity_layer1(&f.name, f.audio.metadata.file_size as u64)
                            });
                            AnnotationSet::new_with_metadata(id, &f.audio, f.cached_peak_db, f.cached_full_peak_db)
                        })
                    });
                    if let Some(set) = new_set {
                        store.sets[idx] = Some(set);
                    }
                }
                if let Some(ref mut set) = store.sets[idx] {
                    for ann in annotations {
                        set.annotations.push(ann);
                    }
                }
            });
            state.annotations_dirty.set(true);
            state.show_info_toast("PSD peaks annotated");
        }
    };

    // Clear hover when mouse leaves the panel
    let on_panel_leave = move |_: web_sys::MouseEvent| {
        state.psd_hover_freqs.set(Vec::new());
    };

    view! {
        <div class="sidebar-panel" on:mouseleave=on_panel_leave>
            // Controls
            <div class="setting-group">
                <div class="setting-group-title">"Power Spectral Density (PSD)"</div>
                <div class="psd-controls-row">
                    <label class="setting-label">"NFFT:"</label>
                    <select
                        class="psd-nfft-select"
                        on:change=move |ev: web_sys::Event| {
                            let target = ev.target().unwrap();
                            let select: web_sys::HtmlSelectElement = target.unchecked_into();
                            if let Ok(val) = select.value().parse::<usize>() {
                                state.psd_nfft.set(val);
                            }
                        }
                    >
                        {[256, 512, 1024, 2048, 4096].iter().map(|&n| {
                            let selected = move || state.psd_nfft.get() == n;
                            view! {
                                <option value=n.to_string() selected=selected>{n.to_string()}</option>
                            }
                        }).collect::<Vec<_>>()}
                    </select>
                    // Log / Linear radio buttons
                    <div class="psd-btn-group" style="margin-left:auto">
                        <button
                            class=move || if !log_scale.get() { "psd-btn psd-btn-active" } else { "psd-btn" }
                            on:click=move |_| log_scale.set(false)
                            title="Linear frequency scale"
                        >"Linear"</button>
                        <button
                            class=move || if log_scale.get() { "psd-btn psd-btn-active" } else { "psd-btn" }
                            on:click=move |_| log_scale.set(true)
                            title="Logarithmic frequency scale"
                        >"Log"</button>
                    </div>
                </div>
            </div>

            // Filter toggle buttons + Freq range
            <div class="psd-filter-toggles">
                <div class="psd-btn-group">
                    <button
                        class=move || {
                            if !state.filter_enabled.get() { "psd-btn psd-btn-disabled" }
                            else if state.psd_apply_eq.get() { "psd-btn psd-btn-active" }
                            else { "psd-btn" }
                        }
                        on:click=move |_| {
                            if state.filter_enabled.get_untracked() {
                                state.psd_apply_eq.update(|v| *v = !*v);
                            }
                        }
                        title="Apply EQ filter to PSD"
                    >"EQ"</button>
                    <button
                        class=move || {
                            if !state.notch_enabled.get() { "psd-btn psd-btn-disabled" }
                            else if state.psd_apply_notch.get() { "psd-btn psd-btn-active" }
                            else { "psd-btn" }
                        }
                        on:click=move |_| {
                            if state.notch_enabled.get_untracked() {
                                state.psd_apply_notch.update(|v| *v = !*v);
                            }
                        }
                        title="Apply notch filter to PSD"
                    >"Notch"</button>
                    <button
                        class=move || {
                            if !state.noise_reduce_enabled.get() { "psd-btn psd-btn-disabled" }
                            else if state.psd_apply_nr.get() { "psd-btn psd-btn-active" }
                            else { "psd-btn" }
                        }
                        on:click=move |_| {
                            if state.noise_reduce_enabled.get_untracked() {
                                state.psd_apply_nr.update(|v| *v = !*v);
                            }
                        }
                        title="Apply noise reduction to PSD"
                    >"NR"</button>
                </div>
                <span class="psd-controls-sep"></span>
                <label class="setting-label" style="font-size:10px;color:#888">"Freq range"</label>
                <div class="psd-btn-group">
                    <button
                        class=move || if freq_range_mode.get() == PsdFreqRangeMode::Auto { "psd-btn psd-btn-active" } else { "psd-btn" }
                        on:click=move |_| freq_range_mode.set(PsdFreqRangeMode::Auto)
                        title="Auto: use selection if available, otherwise use frequency focus range"
                    >"Auto"</button>
                    <button
                        class=move || if freq_range_mode.get() == PsdFreqRangeMode::All { "psd-btn psd-btn-active" } else { "psd-btn" }
                        on:click=move |_| freq_range_mode.set(PsdFreqRangeMode::All)
                        title="No frequency range restriction for peak detection"
                    >"All"</button>
                    <button
                        class=move || {
                            let mode = freq_range_mode.get();
                            let has_sel = effective_selection(&state).and_then(|s| match (s.freq_low, s.freq_high) {
                                (Some(lo), Some(hi)) if lo < hi => Some(()),
                                _ => None,
                            }).is_some();
                            if mode == PsdFreqRangeMode::Selection && has_sel { "psd-btn psd-btn-active" }
                            else if mode == PsdFreqRangeMode::Selection { "psd-btn psd-btn-active psd-btn-dimmed" }
                            else if !has_sel { "psd-btn psd-btn-disabled" }
                            else { "psd-btn" }
                        }
                        on:click=move |_| freq_range_mode.set(PsdFreqRangeMode::Selection)
                        title="Use selection or selected annotation frequency range"
                    >"Sel"</button>
                    <button
                        class=move || {
                            let mode = freq_range_mode.get();
                            let has_ff = state.hfr_enabled.get();
                            if mode == PsdFreqRangeMode::FF && has_ff { "psd-btn psd-btn-active" }
                            else if mode == PsdFreqRangeMode::FF { "psd-btn psd-btn-active psd-btn-dimmed" }
                            else if !has_ff { "psd-btn psd-btn-disabled" }
                            else { "psd-btn" }
                        }
                        on:click=move |_| freq_range_mode.set(PsdFreqRangeMode::FF)
                        title="Use frequency focus (HF) range"
                    >"FF"</button>
                </div>
            </div>

            // Scope badge — uses fixed-height container to prevent layout shift
            <div class="psd-status-row">
            {move || {
                if is_computing.get() {
                    view! { <span class="psd-status-text">"Computing\u{2026}"</span> }.into_any()
                } else if using_selection.get() && psd_result.get().is_some() {
                    view! { <span class="analysis-scope-badge">"Selection"</span> }.into_any()
                } else if file_is_long.get() && !analysis_is_full.get() && psd_result.get().is_some() {
                    view! {
                        <span class="analysis-scope-badge">"First 30s"</span>
                        <button
                            class="analysis-full-btn"
                            on:click=move |_| {
                                spawn_local(async move {
                                    yield_to_browser().await;
                                    start_psd_compute(
                                        state,
                                        psd_result,
                                        is_computing,
                                        compute_gen,
                                        analysis_is_full,
                                        file_is_long,
                                        using_selection,
                                        freq_range_mode,
                                        true,
                                    );
                                });
                            }
                        >
                            "Analyze full file"
                        </button>
                    }.into_any()
                } else if analysis_is_full.get() && file_is_long.get() && psd_result.get().is_some() {
                    view! { <span class="analysis-scope-badge analysis-scope-full">"Full file"</span> }.into_any()
                } else if psd_result.get().is_none() && !is_computing.get() {
                    let has_file = {
                        let files = state.files.get();
                        let idx = state.current_file_index.get();
                        idx.and_then(|i| files.get(i)).is_some()
                    };
                    if !has_file {
                        view! { <span class="psd-status-text">"No file loaded"</span> }.into_any()
                    } else {
                        view! { <span></span> }.into_any()
                    }
                } else {
                    view! { <span></span> }.into_any()
                }
            }}
            </div>

            // PSD Chart
            {move || {
                let result = psd_result.get();
                let is_log = log_scale.get();
                if let Some(psd) = result {
                    view! {
                        <PsdChart psd=psd log_scale=is_log />
                    }.into_any()
                } else {
                    view! { <span></span> }.into_any()
                }
            }}

            // Peak table + annotate button
            {move || {
                let result = psd_result.get();
                match result {
                    None => view! { <span></span> }.into_any(),
                    Some(psd) => {
                        let frames_text = format!("{} frames, {:.1} Hz/bin", psd.frame_count, psd.freq_resolution);

                        let peak_rows: Vec<_> = psd.peaks.iter().enumerate().map(|(i, peak)| {
                            let color = peak_color(i).to_string();
                            let freq_hz = peak.freq_hz;
                            let power_db = peak.power_db;

                            let hover_color = color.clone();
                            let bw_6 = peak.bw_6db;
                            let bw_10 = peak.bw_10db;

                            // Peak-only hover (for dot, kHz, dB columns)
                            let on_enter_peak = {
                                let hover_color = hover_color.clone();
                                move |_: web_sys::MouseEvent| {
                                    let freqs = vec![
                                        (freq_hz, format!("{:.1}k", freq_hz / 1000.0), hover_color.clone()),
                                    ];
                                    state.psd_hover_freqs.set(freqs);
                                }
                            };
                            // -6 dB column hover: peak + -6 dB range
                            let on_enter_6db = {
                                let hover_color = hover_color.clone();
                                move |_: web_sys::MouseEvent| {
                                    let mut freqs = vec![
                                        (freq_hz, format!("{:.1}k", freq_hz / 1000.0), hover_color.clone()),
                                    ];
                                    if let Some((lo, hi)) = bw_6 {
                                        freqs.push((lo, "-6dB lo".to_string(), "#44aa66".to_string()));
                                        freqs.push((hi, "-6dB hi".to_string(), "#44aa66".to_string()));
                                    }
                                    state.psd_hover_freqs.set(freqs);
                                }
                            };
                            // -10 dB column hover: peak + -10 dB range
                            let on_enter_10db = {
                                let hover_color = hover_color.clone();
                                move |_: web_sys::MouseEvent| {
                                    let mut freqs = vec![
                                        (freq_hz, format!("{:.1}k", freq_hz / 1000.0), hover_color.clone()),
                                    ];
                                    if let Some((lo, hi)) = bw_10 {
                                        freqs.push((lo, "-10dB lo".to_string(), "#aaaa44".to_string()));
                                        freqs.push((hi, "-10dB hi".to_string(), "#aaaa44".to_string()));
                                    }
                                    state.psd_hover_freqs.set(freqs);
                                }
                            };
                            let on_leave = move |_: web_sys::MouseEvent| {
                                state.psd_hover_freqs.set(Vec::new());
                            };

                            let bw_6_text = match peak.bw_6db {
                                Some((lo, hi)) => format!("{:.1}k ({:.1}\u{2013}{:.1})", (hi - lo) / 1000.0, lo / 1000.0, hi / 1000.0),
                                None => "\u{2014}".to_string(),
                            };
                            let bw_10_text = match peak.bw_10db {
                                Some((lo, hi)) => format!("{:.1}k ({:.1}\u{2013}{:.1})", (hi - lo) / 1000.0, lo / 1000.0, hi / 1000.0),
                                None => "\u{2014}".to_string(),
                            };

                            view! {
                                <tr class="psd-peak-row"
                                    on:mouseleave=on_leave
                                >
                                    <td class="psd-peak-idx" on:mouseenter=on_enter_peak.clone()>
                                        <span class="psd-peak-dot" style=format!("background:{}", color)></span>
                                    </td>
                                    <td class="psd-peak-freq" on:mouseenter=on_enter_peak.clone()>{format!("{:.1}", freq_hz / 1000.0)}</td>
                                    <td class="psd-peak-power" on:mouseenter=on_enter_peak>{format!("{:.1}", power_db)}</td>
                                    <td class="psd-peak-bw" on:mouseenter=on_enter_6db>{bw_6_text}</td>
                                    <td class="psd-peak-bw" on:mouseenter=on_enter_10db>{bw_10_text}</td>
                                </tr>
                            }
                        }).collect();

                        view! {
                            <div class="setting-group">
                                <div class="setting-group-title">"Peaks"</div>
                                <table class="psd-peak-table">
                                    <thead>
                                        <tr>
                                            <th></th>
                                            <th title="Peak frequency (kHz)">"kHz"</th>
                                            <th title="Power (dB)">"dB"</th>
                                            <th title="-6 dB bandwidth">"-6 dB"</th>
                                            <th title="-10 dB bandwidth">"-10 dB"</th>
                                        </tr>
                                    </thead>
                                    <tbody>{peak_rows}</tbody>
                                </table>
                                <div class="psd-meta-text">{frames_text}</div>
                                <button
                                    class="analysis-full-btn"
                                    style="margin-top:4px"
                                    on:click=annotate_peaks
                                    title="Add peak frequency and bandwidth annotations for all peaks"
                                >
                                    "Add peak annotations"
                                </button>
                            </div>
                        }.into_any()
                    }
                }
            }}
        </div>
    }
}

// ── PSD Chart Canvas ────────────────────────────────────────────────────────

fn draw_psd_canvas(
    canvas: &HtmlCanvasElement,
    power_db: &[f64],
    freq_res: f64,
    sample_rate: u32,
    peaks: &[psd::PsdPeak],
    peak_freq_range: Option<(f64, f64)>,
    log_scale: bool,
) {
    let parent_width = canvas.parent_element()
        .map(|p| p.client_width() as u32)
        .unwrap_or(250);
    let w = parent_width.max(150);
    let h = 200u32;

    // Skip drawing if parent has no real width yet (sidebar still opening)
    if parent_width < 50 {
        return;
    }

    canvas.set_width(w);
    canvas.set_height(h);

    let ctx = canvas
        .get_context("2d")
        .unwrap()
        .unwrap()
        .dyn_into::<CanvasRenderingContext2d>()
        .unwrap();

    ctx.set_fill_style_str("#111");
    ctx.fill_rect(0.0, 0.0, w as f64, h as f64);

    if power_db.is_empty() {
        return;
    }

    let margin_left = 36.0;
    let margin_right = 8.0;
    let margin_top = 18.0;
    let margin_bottom = 24.0;
    let chart_w = w as f64 - margin_left - margin_right;
    let chart_h = h as f64 - margin_top - margin_bottom;

    let nyquist = sample_rate as f64 / 2.0;
    let n_bins = power_db.len();

    let db_max = power_db.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let db_min = (db_max - 80.0).max(-200.0);
    let db_range = db_max - db_min;
    if db_range <= 0.0 { return; }

    let min_log_freq = 100.0f64;
    let freq_to_x = |freq: f64| -> f64 {
        if log_scale {
            if freq <= 0.0 { return margin_left; }
            let f = freq.max(min_log_freq);
            let log_min = min_log_freq.log10();
            let log_max = nyquist.log10();
            let frac = (f.log10() - log_min) / (log_max - log_min);
            margin_left + frac.clamp(0.0, 1.0) * chart_w
        } else {
            margin_left + (freq / nyquist) * chart_w
        }
    };

    let db_to_y = |db: f64| -> f64 {
        let frac = (db - db_min) / db_range;
        margin_top + (1.0 - frac.clamp(0.0, 1.0)) * chart_h
    };

    // dB grid
    ctx.set_stroke_style_str("rgba(255,255,255,0.1)");
    ctx.set_fill_style_str("#666");
    ctx.set_font("9px monospace");
    ctx.set_line_width(0.5);
    let db_step = if db_range > 60.0 { 20.0 } else { 10.0 };
    let mut db_tick = (db_min / db_step).ceil() * db_step;
    while db_tick <= db_max {
        let y = db_to_y(db_tick);
        ctx.begin_path();
        ctx.move_to(margin_left, y);
        ctx.line_to(w as f64 - margin_right, y);
        ctx.stroke();
        let label = format!("{:.0}", db_tick);
        let _ = ctx.fill_text(&label, 2.0, y + 3.0);
        db_tick += db_step;
    }

    // Frequency grid
    let freq_ticks: Vec<f64> = if log_scale {
        let mut ticks = Vec::new();
        for &base in &[100.0, 1000.0, 10000.0, 100000.0] {
            for mult in 1..10 {
                let f = base * mult as f64;
                if f >= min_log_freq && f <= nyquist {
                    ticks.push(f);
                }
            }
        }
        ticks
    } else {
        let step = if nyquist > 100_000.0 {
            50_000.0
        } else if nyquist > 50_000.0 {
            25_000.0
        } else if nyquist > 20_000.0 {
            10_000.0
        } else {
            5_000.0
        };
        let mut ticks = Vec::new();
        let mut f = step;
        while f < nyquist {
            ticks.push(f);
            f += step;
        }
        ticks
    };

    for &freq in &freq_ticks {
        let x = freq_to_x(freq);
        if x < margin_left || x > w as f64 - margin_right { continue; }

        let is_major = if log_scale {
            freq == 100.0 || freq == 1000.0 || freq == 10000.0 || freq == 100000.0
        } else {
            true
        };

        if is_major {
            ctx.set_stroke_style_str("rgba(255,255,255,0.12)");
        } else {
            ctx.set_stroke_style_str("rgba(255,255,255,0.04)");
        }
        ctx.begin_path();
        ctx.move_to(x, margin_top);
        ctx.line_to(x, h as f64 - margin_bottom);
        ctx.stroke();

        if is_major {
            ctx.set_fill_style_str("#666");
            let label = if freq >= 1000.0 {
                format!("{:.0}k", freq / 1000.0)
            } else {
                format!("{:.0}", freq)
            };
            let _ = ctx.fill_text(&label, x - 8.0, h as f64 - 6.0);
        }
    }

    // Draw frequency range constraint shading (dim regions outside range)
    if let Some((lo, hi)) = peak_freq_range {
        ctx.set_fill_style_str("rgba(255,255,255,0.05)");
        let x_lo = freq_to_x(lo);
        let x_hi = freq_to_x(hi);
        // Dim left side
        ctx.fill_rect(margin_left, margin_top, x_lo - margin_left, chart_h);
        // Dim right side
        ctx.fill_rect(x_hi, margin_top, (w as f64 - margin_right) - x_hi, chart_h);
        // Draw range boundary lines
        ctx.set_stroke_style_str("rgba(255,200,50,0.4)");
        ctx.set_line_width(1.0);
        let _ = ctx.set_line_dash(&JsValue::from(js_sys::Array::of2(
            &JsValue::from(2.0), &JsValue::from(2.0),
        )));
        ctx.begin_path();
        ctx.move_to(x_lo, margin_top);
        ctx.line_to(x_lo, h as f64 - margin_bottom);
        ctx.move_to(x_hi, margin_top);
        ctx.line_to(x_hi, h as f64 - margin_bottom);
        ctx.stroke();
        let _ = ctx.set_line_dash(&JsValue::from(js_sys::Array::new()));
    }

    // Draw bandwidth shading for the primary peak only (to keep chart clean)
    if let Some(peak) = peaks.first() {
        if let Some((lo, hi)) = peak.bw_10db {
            let x1 = freq_to_x(lo);
            let x2 = freq_to_x(hi);
            ctx.set_fill_style_str("rgba(170,170,68,0.15)");
            ctx.fill_rect(x1, margin_top, x2 - x1, chart_h);
        }
        if let Some((lo, hi)) = peak.bw_6db {
            let x1 = freq_to_x(lo);
            let x2 = freq_to_x(hi);
            ctx.set_fill_style_str("rgba(68,170,102,0.2)");
            ctx.fill_rect(x1, margin_top, x2 - x1, chart_h);
        }
    }

    // PSD curve
    ctx.set_stroke_style_str("#4dd");
    ctx.set_line_width(1.5);
    ctx.begin_path();
    let mut started = false;
    for (i, &pdb) in power_db.iter().enumerate().take(n_bins) {
        let freq = i as f64 * freq_res;
        if log_scale && freq < min_log_freq { continue; }
        let x = freq_to_x(freq);
        let y = db_to_y(pdb);
        if !started {
            ctx.move_to(x, y);
            started = true;
        } else {
            ctx.line_to(x, y);
        }
    }
    ctx.stroke();

    // Draw all peak markers
    for (i, peak) in peaks.iter().enumerate() {
        let color = peak_color(i);
        let px = freq_to_x(peak.freq_hz);
        let py = db_to_y(peak.power_db);

        // Dashed vertical line
        let alpha = if i == 0 { 0.6 } else { 0.35 };
        ctx.set_stroke_style_str(&hex_to_rgba(color, alpha));
        ctx.set_line_width(1.0);
        let _ = ctx.set_line_dash(&JsValue::from(js_sys::Array::of2(
            &JsValue::from(3.0),
            &JsValue::from(3.0),
        )));
        ctx.begin_path();
        ctx.move_to(px, margin_top);
        ctx.line_to(px, h as f64 - margin_bottom);
        ctx.stroke();
        let _ = ctx.set_line_dash(&JsValue::from(js_sys::Array::new()));

        // Dot
        ctx.set_fill_style_str(color);
        ctx.begin_path();
        let radius = if i == 0 { 3.0 } else { 2.5 };
        let _ = ctx.arc(px, py, radius, 0.0, std::f64::consts::TAU);
        ctx.fill();

        // Label (only for first 3 to avoid clutter)
        if i < 3 {
            ctx.set_fill_style_str(color);
            ctx.set_font("9px monospace");
            let label = format!("{:.1}k", peak.freq_hz / 1000.0);
            let _ = ctx.fill_text(&label, px + 5.0, py - 4.0);
        }
    }

    // Chart border
    ctx.set_stroke_style_str("rgba(255,255,255,0.2)");
    ctx.set_line_width(1.0);
    ctx.stroke_rect(margin_left, margin_top, chart_w, chart_h);
}

#[component]
fn PsdChart(psd: PsdResult, log_scale: bool) -> impl IntoView {
    let canvas_ref = NodeRef::<leptos::html::Canvas>::new();
    let power_db = psd.power_db.clone();
    let freq_res = psd.freq_resolution;
    let sample_rate = psd.sample_rate;
    let peaks = psd.peaks.clone();
    let peak_freq_range = psd.peak_freq_range;

    Effect::new(move || {
        let Some(el) = canvas_ref.get() else { return };
        let canvas: &HtmlCanvasElement = el.as_ref();
        let canvas = canvas.clone();

        // Draw immediately
        draw_psd_canvas(&canvas, &power_db, freq_res, sample_rate, &peaks, peak_freq_range, log_scale);

        // Set up ResizeObserver to redraw when the container resizes
        // (handles sidebar open animation, window resize, etc.)
        let power_db2 = power_db.clone();
        let peaks2 = peaks.clone();
        let canvas2 = canvas.clone();
        let cb = wasm_bindgen::closure::Closure::<dyn Fn(js_sys::Array)>::new(move |_entries: js_sys::Array| {
            draw_psd_canvas(&canvas2, &power_db2, freq_res, sample_rate, &peaks2, peak_freq_range, log_scale);
        });
        if let Ok(observer) = web_sys::ResizeObserver::new(cb.as_ref().unchecked_ref()) {
            if let Some(parent) = canvas.parent_element() {
                observer.observe(&parent);
            }
            // Store observer on the canvas element to prevent GC and tie its
            // lifetime to the DOM node. When the component unmounts and the
            // canvas is removed, the observer becomes inert.
            let _ = js_sys::Reflect::set(
                &canvas,
                &JsValue::from_str("__psd_resize_obs"),
                &observer,
            );
        }
        cb.forget();
    });

    view! {
        <div class="psd-chart-wrap">
            <canvas
                node_ref=canvas_ref
                style="width:100%;height:200px;display:block;border-radius:3px"
            />
        </div>
    }
}

fn start_psd_compute(
    state: AppState,
    psd_result: RwSignal<Option<PsdResult>>,
    is_computing: RwSignal<bool>,
    compute_gen: RwSignal<u32>,
    analysis_is_full: RwSignal<bool>,
    file_is_long: RwSignal<bool>,
    using_selection: RwSignal<bool>,
    freq_range_mode: RwSignal<PsdFreqRangeMode>,
    full_file: bool,
) {
    let files = state.files.get_untracked();
    let idx = state.current_file_index.get_untracked();
    let file = idx.and_then(|i| files.get(i).cloned());
    let Some(file) = file else { return; };

    // Don't clear psd_result to None here — setting it synchronously inside
    // the Effect cascade tears down the peak-table DOM (and its event-handler
    // closures) while Leptos is still processing the triggering event, which
    // causes "closure invoked recursively or after being dropped".
    // Instead, keep the old result visible until the new one arrives.
    is_computing.set(true);
    compute_gen.update(|g| *g += 1);
    let generation = compute_gen.get_untracked();

    let sample_rate = file.audio.sample_rate;
    let total = file.audio.source.total_samples() as usize;
    if total == 0 {
        is_computing.set(false);
        return;
    }

    let nfft = state.psd_nfft.get_untracked();
    let apply_eq = state.psd_apply_eq.get_untracked();
    let apply_notch = state.psd_apply_notch.get_untracked();
    let apply_nr = state.psd_apply_nr.get_untracked();

    // Determine sample range
    let selection = state.selection.get_untracked();
    let (start_sample, end_sample, is_sel) = if let Some(sel) = selection {
        let s = (sel.time_start * sample_rate as f64) as usize;
        let e = ((sel.time_end * sample_rate as f64) as usize).min(total);
        (s, e, true)
    } else {
        let max_samples = (DEFAULT_ANALYSIS_WINDOW_SECS * sample_rate as f64) as usize;
        let is_long = total > max_samples;
        file_is_long.set(is_long);
        if full_file || !is_long {
            analysis_is_full.set(true);
            (0, total, false)
        } else {
            analysis_is_full.set(false);
            (0, max_samples, false)
        }
    };
    using_selection.set(is_sel);

    let mut samples: Vec<f32> = file.audio.source.read_region(
        ChannelView::MonoMix,
        start_sample as u64,
        end_sample.saturating_sub(start_sample),
    );

    if apply_notch && state.notch_enabled.get_untracked() {
        let bands = state.notch_bands.get_untracked();
        let harm_supp = state.notch_harmonic_suppression.get_untracked();
        if !bands.is_empty() {
            samples = crate::dsp::notch::apply_notch_filters(&samples, sample_rate, &bands, harm_supp);
        }
    }

    if apply_nr && state.noise_reduce_enabled.get_untracked() {
        if let Some(nf) = state.noise_reduce_floor.get_untracked() {
            let strength = state.noise_reduce_strength.get_untracked();
            samples = crate::dsp::spectral_sub::apply_spectral_subtraction(
                &samples, sample_rate, &nf, strength, 0.01, 0.0,
            );
        }
    }

    if apply_eq && state.filter_enabled.get_untracked() {
        let freq_low = state.filter_freq_low.get_untracked();
        let freq_high = state.filter_freq_high.get_untracked();
        let db_below = state.filter_db_below.get_untracked();
        let db_selected = state.filter_db_selected.get_untracked();
        let db_harmonics = state.filter_db_harmonics.get_untracked();
        let db_above = state.filter_db_above.get_untracked();
        let band_mode = state.filter_band_mode.get_untracked();
        samples = crate::dsp::filters::apply_eq_filter(
            &samples, sample_rate, freq_low, freq_high,
            db_below, db_selected, db_harmonics, db_above, band_mode,
        );
    }

    let peak_freq_range = {
        let mode = freq_range_mode.get_untracked();
        match mode {
            PsdFreqRangeMode::All => None,
            PsdFreqRangeMode::Selection => {
                effective_selection(&state).and_then(|s| match (s.freq_low, s.freq_high) {
                    (Some(lo), Some(hi)) if lo < hi => Some((lo, hi)),
                    _ => None,
                })
            }
            PsdFreqRangeMode::FF => {
                if state.hfr_enabled.get_untracked() {
                    let lo = state.filter_freq_low.get_untracked();
                    let hi = state.filter_freq_high.get_untracked();
                    if lo < hi { Some((lo, hi)) } else { None }
                } else {
                    None
                }
            }
            PsdFreqRangeMode::Auto => {
                // Try selection first, then FF
                let sel_range = effective_selection(&state).and_then(|s| match (s.freq_low, s.freq_high) {
                    (Some(lo), Some(hi)) if lo < hi => Some((lo, hi)),
                    _ => None,
                });
                sel_range.or_else(|| {
                    if state.hfr_enabled.get_untracked() {
                        let lo = state.filter_freq_low.get_untracked();
                        let hi = state.filter_freq_high.get_untracked();
                        if lo < hi { Some((lo, hi)) } else { None }
                    } else {
                        None
                    }
                })
            }
        }
    };

    let samples = Arc::new(samples);

    spawn_local(async move {
        let result = psd::compute_psd_async(
            &samples, sample_rate, nfft, peak_freq_range,
            crate::canvas::tile_cache::yield_to_browser,
            &move || compute_gen.get_untracked() != generation,
        ).await;
        if compute_gen.get_untracked() != generation {
            return;
        }
        if let Some(r) = result {
            psd_result.set(Some(r));
        }
        is_computing.set(false);
    });
}

async fn yield_to_browser() {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        let win = web_sys::window().unwrap();
        let cb = wasm_bindgen::closure::Closure::once_into_js(move || {
            let _ = resolve.call0(&JsValue::NULL);
        });
        let _ = win.set_timeout_with_callback_and_timeout_and_arguments_0(
            cb.unchecked_ref(),
            0,
        );
    });
    let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
}
