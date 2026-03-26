use leptos::prelude::*;
use crate::state::{AppState, CanvasTool, SpectrogramHandle};
use crate::annotations::AnnotationKind;

/// Format a frequency value for display (e.g. "45.0 kHz" or "800 Hz").
fn fmt_freq(f: f64) -> String {
    if f >= 1000.0 {
        format!("{:.1} kHz", f / 1000.0)
    } else {
        format!("{:.0} Hz", f)
    }
}

/// Format a selection/annotation's dimensions: duration and optional freq range.
fn format_selection_dims(duration: f64, freq_low: Option<f64>, freq_high: Option<f64>) -> String {
    let dur_str = crate::format_time::format_duration(duration, 3);
    match (freq_low, freq_high) {
        (Some(fl), Some(fh)) => format!(
            "Duration: {}   Freq range: {:.0} – {:.0} kHz",
            dur_str, fl / 1000.0, fh / 1000.0
        ),
        _ => format!("Duration: {}", dur_str),
    }
}

#[component]
pub fn AnalysisPanel() -> impl IntoView {
    let state = expect_context::<AppState>();

    let selection_dims = move || {
        let selection = state.selection.get()?;
        let d = selection.time_end - selection.time_start;
        if d > 0.0001 {
            Some(format_selection_dims(d, selection.freq_low, selection.freq_high))
        } else {
            None
        }
    };

    let annotation_dims = move || {
        let ids = state.selected_annotation_ids.get();
        if ids.is_empty() { return None; }
        let idx = state.current_file_index.get()?;
        let store = state.annotation_store.get();
        let set = store.sets.get(idx)?.as_ref()?;
        // Show dims for single selected annotation
        if ids.len() == 1 {
            let ann = set.annotations.iter().find(|a| a.id == ids[0])?;
            match &ann.kind {
                AnnotationKind::Region(r) => {
                    let d = r.time_end - r.time_start;
                    if d > 0.0001 {
                        Some(format_selection_dims(d, r.freq_low, r.freq_high))
                    } else {
                        None
                    }
                }
                _ => None,
            }
        } else {
            Some(format!("{} annotations selected", ids.len()))
        }
    };

    view! {
        <div class="analysis-panel">
            {move || {
                let has_file = state.current_file_index.get().is_some() || state.active_timeline.get().is_some();

                if !has_file {
                    return view! {
                        <span style="color: #555">"Load a file..."</span>
                    }.into_any();
                }

                // Selection dimensions take priority
                if let Some(dims) = selection_dims() {
                    return view! {
                        <span>{dims}</span>
                    }.into_any();
                }

                // Selected annotation dimensions
                if let Some(dims) = annotation_dims() {
                    return view! {
                        <span style="color: #aaa">{dims}</span>
                    }.into_any();
                }

                // FF / HET handle interaction
                if let Some(handle) = state.spec_drag_handle.get() {
                    let msg = match handle {
                        SpectrogramHandle::FfUpper | SpectrogramHandle::FfLower | SpectrogramHandle::FfMiddle => {
                            let lo = state.ff_freq_lo.get();
                            let hi = state.ff_freq_hi.get();
                            format!("Frequency focus: {} – {}", fmt_freq(lo), fmt_freq(hi))
                        }
                        SpectrogramHandle::HetCenter => {
                            let f = state.het_frequency.get();
                            format!("Heterodyne: {}", fmt_freq(f))
                        }
                        SpectrogramHandle::HetBandUpper | SpectrogramHandle::HetBandLower => {
                            let f = state.het_frequency.get();
                            let c = state.het_cutoff.get();
                            format!("Heterodyne: {} ± {}", fmt_freq(f), fmt_freq(c))
                        }
                    };
                    return view! {
                        <span style="color: #888">{msg}</span>
                    }.into_any();
                }

                // Axis drag
                if let (Some(start), Some(current)) = (state.axis_drag_start_freq.get(), state.axis_drag_current_freq.get()) {
                    let lo = start.min(current);
                    let hi = start.max(current);
                    let msg = format!("Selecting frequency range: {} – {}", fmt_freq(lo), fmt_freq(hi));
                    return view! {
                        <span style="color: #888">{msg}</span>
                    }.into_any();
                }

                // Annotation resize drag
                if state.annotation_drag_handle.get().is_some() {
                    return view! {
                        <span style="color: #888">"Resizing annotation..."</span>
                    }.into_any();
                }

                // Drag in progress
                if state.is_dragging.get() {
                    let msg = match state.canvas_tool.get() {
                        CanvasTool::Hand => "Panning...",
                        CanvasTool::Selection => "Selecting...",
                    };
                    return view! {
                        <span style="color: #888">{msg}</span>
                    }.into_any();
                }

                // Hovering label area
                if state.mouse_in_label_area.get() {
                    return view! {
                        <span style="color: #666">"Drag to set frequency focus"</span>
                    }.into_any();
                }

                // Mouse on spectrogram: show time and frequency
                let freq = state.mouse_freq.get();
                let time = state.cursor_time.get();
                if let (Some(f), Some(t)) = (freq, time) {
                    return view! {
                        <span style="color: #777">{format!("{:.3}s  {}", t, fmt_freq(f))}</span>
                    }.into_any();
                }

                // Loading files
                let loading = state.loading_files.get();
                if !loading.is_empty() {
                    let msg = if loading.len() == 1 {
                        let entry = &loading[0];
                        let stage = match &entry.stage {
                            crate::state::LoadingStage::Decoding => "Decoding".to_string(),
                            crate::state::LoadingStage::Preview => "Generating preview".to_string(),
                            crate::state::LoadingStage::Spectrogram(pct) => format!("Spectrogram {}%", pct),
                            crate::state::LoadingStage::Finalizing => "Finalizing".to_string(),
                            crate::state::LoadingStage::Streaming => "Streaming".to_string(),
                        };
                        format!("Loading: {} ({})", entry.name, stage)
                    } else {
                        format!("Loading {} files...", loading.len())
                    };
                    return view! {
                        <span style="color: #888">{msg}</span>
                    }.into_any();
                }

                // Hash computing
                if state.hash_computing.get() {
                    return view! {
                        <span style="color: #666">"Computing file identity..."</span>
                    }.into_any();
                }

                // Default: empty
                view! {
                    <span></span>
                }.into_any()
            }}
        </div>
    }
}
