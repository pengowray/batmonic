use crate::bat_book::auto_resolve;
use crate::bat_book::data::get_manifest;
use crate::state::store_fields::*;
use crate::state::AppState;
use leptos::prelude::*;
use wasm_bindgen::JsCast;

/// Floating reference panel on the right side of the main view.
/// Shows info about the selected bat family/families.
/// Scroll wheel on the header navigates between entries;
/// the body scrolls normally for content overflow.
#[component]
pub fn BatBookRefPanel() -> impl IntoView {
    let state = expect_context::<AppState>();

    // Which entry is "focused" when scrolling through multi-select via header
    let focused_index = RwSignal::new(0usize);

    // NodeRef for the body so we can scroll it programmatically
    let body_ref = NodeRef::<leptos::html::Div>::new();

    let selected_entries = Memo::new(move |_| {
        let sel_ids = state.bat_book.selected_ids().get();
        if sel_ids.is_empty() {
            return Vec::new();
        }
        let region = state.bat_book.region().get();
        let manifest = get_manifest(region);
        let mut entries: Vec<_> = manifest
            .entries
            .into_iter()
            .filter(|e| sel_ids.iter().any(|id| id == e.id))
            .collect();
        // Include auto-matched entries not found in the current manifest
        // (out-of-range species from a different book)
        for id in &sel_ids {
            if !entries.iter().any(|e| e.id == id.as_str()) {
                if let Some(entry) = auto_resolve::find_entry_any_book(id) {
                    entries.push(entry);
                }
            }
        }
        entries
    });

    // Reset focused_index when selection changes
    Effect::new(move |_| {
        let _ = state.bat_book.selected_ids().get();
        focused_index.set(0);
    });

    let on_close = move |ev: web_sys::MouseEvent| {
        ev.stop_propagation();
        state.bat_book.ref_open().set(false);
    };

    // Scroll the body to bring the Nth .ref-panel-entry into view
    let scroll_body_to_entry = move |index: usize| {
        let Some(body) = body_ref.get_untracked() else {
            return;
        };
        let body_el: &web_sys::HtmlElement = &body;
        let Ok(nodes) = body_el.query_selector_all(".ref-panel-entry") else {
            return;
        };
        let Some(node) = nodes.get(index as u32) else {
            return;
        };
        let Ok(el) = node.dyn_into::<web_sys::Element>() else {
            return;
        };
        let opts = web_sys::ScrollIntoViewOptions::new();
        opts.set_behavior(web_sys::ScrollBehavior::Smooth);
        opts.set_block(web_sys::ScrollLogicalPosition::Start);
        el.scroll_into_view_with_scroll_into_view_options(&opts);
    };

    // Header wheel handler: navigate entries
    let on_header_wheel = move |ev: web_sys::WheelEvent| {
        ev.prevent_default();
        ev.stop_propagation();
        let delta = ev.delta_y();
        if delta.abs() < 1.0 {
            return;
        }

        let ids = state.bat_book.selected_ids().get_untracked();

        if ids.len() > 1 {
            // Multi-select: scroll through selected entries in the body
            let entries = selected_entries.get_untracked();
            let n = entries.len();
            if n == 0 {
                return;
            }

            let cur = focused_index.get_untracked();
            let next = if delta > 0.0 {
                if cur + 1 < n {
                    cur + 1
                } else {
                    return;
                }
            } else {
                if cur > 0 {
                    cur - 1
                } else {
                    return;
                }
            };
            focused_index.set(next);
            scroll_body_to_entry(next);
        } else {
            // Single select: navigate through full manifest
            let region = state.bat_book.region().get_untracked();
            let manifest = get_manifest(region);
            if ids.is_empty() || manifest.entries.is_empty() {
                return;
            }

            let last_id = &ids[ids.len() - 1];
            let cur_idx = manifest
                .entries
                .iter()
                .position(|e| e.id == last_id.as_str());

            let next = if let Some(cur) = cur_idx {
                // Currently on a manifest entry — navigate normally
                if delta > 0.0 {
                    if cur + 1 < manifest.entries.len() {
                        cur + 1
                    } else {
                        return;
                    }
                } else {
                    if cur > 0 {
                        cur - 1
                    } else {
                        return;
                    }
                }
            } else {
                // Out-of-range species (not in manifest) — scroll into the book
                if delta > 0.0 {
                    0
                } else {
                    manifest.entries.len() - 1
                }
            };

            let new_id = manifest.entries[next].id.to_string();
            state.bat_book.selected_ids().set(vec![new_id]);

            // Apply BandFF for the new entry via focus stack
            let entry = &manifest.entries[next];
            if let Some(idx) = state.library.current_index().get_untracked() {
                let files = state.library.files().get_untracked();
                if let Some(file) = files.get(idx) {
                    let nyquist = file.audio.sample_rate as f64 / 2.0;
                    if entry.freq_lo_hz < nyquist {
                        let clamped_hi = entry.freq_hi_hz.min(nyquist);
                        state.push_bat_book_ff(entry.freq_lo_hz, clamped_hi);
                    }
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
                state.bat_book.ref_open().set(false);
            }
        }
    };

    view! {
        <div
            class="bat-book-ref-panel"
            on:touchstart=on_touchstart
            on:touchend=on_touchend
        >
            <div class="ref-panel-header" on:wheel=on_header_wheel>
                <span class="ref-panel-name">
                    {move || {
                        let entries = selected_entries.get();
                        let n = entries.len();
                        if n > 1 {
                            let fi = focused_index.get();
                            if fi == 0 {
                                // Haven't scrolled yet
                                view! {
                                    <span class="ref-panel-count">{format!("{n} selections")}</span>
                                }.into_any()
                            } else {
                                view! {
                                    <span class="ref-panel-count">{format!("{} / {}", fi + 1, n)}</span>
                                    " selections"
                                }.into_any()
                            }
                        } else if n == 1 {
                            let region = state.bat_book.region().get();
                            let manifest = get_manifest(region);
                            let total = manifest.entries.len();
                            let ids = state.bat_book.selected_ids().get();
                            let pos = ids.first()
                                .and_then(|id| manifest.entries.iter().position(|e| e.id == id.as_str()))
                                .map(|i| i + 1);
                            if let Some(pos) = pos {
                                // Species is in the current region's book
                                view! {
                                    {region.short_label()}
                                    " "
                                    <span class="ref-panel-count">{format!("{pos} / {total}")}</span>
                                }.into_any()
                            } else {
                                // Out-of-range species — don't show position
                                view! {
                                    {region.short_label()}
                                }.into_any()
                            }
                        } else {
                            view! { <span></span> }.into_any()
                        }
                    }}
                </span>
                <button class="ref-panel-close" on:click=on_close title="Close">
                    "\u{00d7}"
                </button>
            </div>
            <div class="ref-panel-body" node_ref=body_ref>
                {move || {
                    let entries = selected_entries.get();
                    entries.into_iter().map(|entry| {
                        let sci = entry.scientific_name;
                        view! {
                            <div class="ref-panel-entry">
                                <div class="ref-panel-entry-name">{entry.name}</div>
                                {(!sci.is_empty()).then(|| view! {
                                    <div class="ref-panel-sci"><i>{sci}</i></div>
                                })}
                                <div class="ref-panel-family">{entry.family}</div>
                                <div class="ref-panel-freq">{entry.freq_range_label()}</div>
                                <div class="ref-panel-call-type">"Call type: " {entry.call_type}</div>
                                <div class="ref-panel-desc">{entry.description}</div>
                            </div>
                        }
                    }).collect_view()
                }}
                <div class="ref-panel-draft-notice">
                    "Draft Only. May contain errors."
                </div>
            </div>
        </div>
    }
}
