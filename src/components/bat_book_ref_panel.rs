use leptos::prelude::*;
use crate::state::AppState;
use crate::bat_book::data::get_manifest;

/// Floating reference panel on the right side of the main view.
/// Shows info about the selected bat family/families.
/// Scroll wheel navigates through entries.
#[component]
pub fn BatBookRefPanel() -> impl IntoView {
    let state = expect_context::<AppState>();

    let selected_entries = Memo::new(move |_| {
        let sel_ids = state.bat_book_selected_ids.get();
        if sel_ids.is_empty() {
            return Vec::new();
        }
        let region = state.bat_book_region.get();
        let manifest = get_manifest(region);
        manifest.entries.into_iter()
            .filter(|e| sel_ids.iter().any(|id| id == e.id))
            .collect::<Vec<_>>()
    });

    let on_close = move |ev: web_sys::MouseEvent| {
        ev.stop_propagation();
        state.bat_book_ref_open.set(false);
    };

    // Scroll wheel: navigate to prev/next entry in the full manifest
    // Disabled when multiple bats are selected (allow normal scroll instead)
    let on_wheel = move |ev: web_sys::WheelEvent| {
        let ids = state.bat_book_selected_ids.get_untracked();
        if ids.len() > 1 {
            // Multiple selected — let the panel scroll normally
            return;
        }
        ev.prevent_default();
        ev.stop_propagation();
        let delta = ev.delta_y();
        if delta.abs() < 1.0 { return; }

        let region = state.bat_book_region.get_untracked();
        let manifest = get_manifest(region);
        let ids = state.bat_book_selected_ids.get_untracked();
        if ids.is_empty() || manifest.entries.is_empty() { return; }

        // Find the index of the last selected entry
        let last_id = &ids[ids.len() - 1];
        let cur_idx = manifest.entries.iter().position(|e| e.id == last_id.as_str());
        let Some(cur) = cur_idx else { return };

        let next = if delta > 0.0 {
            // scroll down = next
            if cur + 1 < manifest.entries.len() { cur + 1 } else { return }
        } else {
            // scroll up = prev
            if cur > 0 { cur - 1 } else { return }
        };

        let new_id = manifest.entries[next].id.to_string();

        state.bat_book_selected_ids.set(vec![new_id]);

        // Apply FF for the new entry via focus stack
        let entry = &manifest.entries[next];
        if let Some(idx) = state.current_file_index.get_untracked() {
            let files = state.files.get_untracked();
            if let Some(file) = files.get(idx) {
                let nyquist = file.audio.sample_rate as f64 / 2.0;
                if entry.freq_lo_hz < nyquist {
                    let clamped_hi = entry.freq_hi_hz.min(nyquist);
                    state.push_bat_book_ff(entry.freq_lo_hz, clamped_hi);
                }
            }
        }
    };

    // Swipe-up to dismiss
    let touch_start_y = RwSignal::new(0.0f64);
    let on_touchstart = move |ev: web_sys::TouchEvent| {
        if let Some(touch) = ev.touches().get(0) {
            touch_start_y.set(touch.client_y() as f64);
        }
    };
    let on_touchend = move |ev: web_sys::TouchEvent| {
        if let Some(touch) = ev.changed_touches().get(0) {
            let dy = touch_start_y.get_untracked() - touch.client_y() as f64;
            if dy > 60.0 {
                state.bat_book_ref_open.set(false);
            }
        }
    };

    view! {
        <div
            class="bat-book-ref-panel"
            on:touchstart=on_touchstart
            on:touchend=on_touchend
            on:wheel=on_wheel
        >
            <div class="ref-panel-header">
                <span class="ref-panel-name">
                    {move || {
                        let n = selected_entries.get().len();
                        if n > 1 {
                            format!("{n} selections")
                        } else {
                            String::new()
                        }
                    }}
                </span>
                <button class="ref-panel-close" on:click=on_close title="Close">
                    "\u{00d7}"
                </button>
            </div>
            <div class="ref-panel-body">
                {move || {
                    let entries = selected_entries.get();
                    entries.into_iter().map(|entry| {
                        let sci = entry.scientific_name;
                        let commonness_label = entry.commonness.map(|c| c.label());
                        view! {
                            <div class="ref-panel-entry">
                                <div class="ref-panel-entry-name">{entry.name}</div>
                                {(!sci.is_empty()).then(|| view! {
                                    <div class="ref-panel-sci"><i>{sci}</i></div>
                                })}
                                <div class="ref-panel-family">{entry.family}</div>
                                {commonness_label.map(|label| view! {
                                    <div class="ref-panel-commonness">{label}</div>
                                })}
                                <div class="ref-panel-freq">{entry.freq_range_label()}</div>
                                <div class="ref-panel-call-type">"Call type: " {entry.call_type}</div>
                                <div class="ref-panel-desc">{entry.description}</div>
                            </div>
                        }
                    }).collect_view()
                }}
                <div class="ref-panel-draft-notice">
                    "Draft Only. Details are approximate at best."
                </div>
            </div>
        </div>
    }
}
