use leptos::prelude::*;
use crate::state::{AppState, CanvasTool};

#[component]
pub fn AnalysisPanel() -> impl IntoView {
    let state = expect_context::<AppState>();

    let duration = move || {
        let selection = state.selection.get()?;
        let d = selection.time_end - selection.time_start;
        if d > 0.0001 { Some(d) } else { None }
    };

    view! {
        <div class="analysis-panel">
            {move || {
                let has_file = state.current_file_index.get().is_some();

                if !has_file {
                    return view! {
                        <span style="color: #555">"Load a file..."</span>
                    }.into_any();
                }

                // Selection duration takes priority
                if let Some(d) = duration() {
                    return view! {
                        <span>{format!("{:.3}s", d)}</span>
                    }.into_any();
                }

                // FF handle interaction
                if state.spec_drag_handle.get().is_some() {
                    return view! {
                        <span style="color: #888">"Adjusting frequency focus"</span>
                    }.into_any();
                }

                // Axis drag
                if state.axis_drag_start_freq.get().is_some() {
                    return view! {
                        <span style="color: #888">"Selecting frequency range..."</span>
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
                    let freq_str = if f >= 1000.0 {
                        format!("{:.1} kHz", f / 1000.0)
                    } else {
                        format!("{:.0} Hz", f)
                    };
                    return view! {
                        <span style="color: #777">{format!("{:.3}s  {}", t, freq_str)}</span>
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
