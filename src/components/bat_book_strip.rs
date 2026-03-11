use leptos::prelude::*;
use crate::state::AppState;
use crate::bat_book::data::get_manifest;
use crate::bat_book::types::{BatBookRegion, BatBookEntry};

/// Horizontal scrolling strip of bat family chips.
/// Sits between the main view and the bottom toolbar.
#[component]
pub fn BatBookStrip() -> impl IntoView {
    let state = expect_context::<AppState>();
    let region_menu_open = RwSignal::new(false);
    let scroll_ref = NodeRef::<leptos::html::Div>::new();

    let manifest = Memo::new(move |_| {
        let region = state.bat_book_region.get();
        get_manifest(region)
    });

    let on_close = move |_: web_sys::MouseEvent| {
        state.bat_book_open.set(false);
    };

    // Clicking the title also closes the strip
    let on_title_click = move |ev: web_sys::MouseEvent| {
        ev.stop_propagation();
        state.bat_book_open.set(false);
    };

    let on_config = move |ev: web_sys::MouseEvent| {
        ev.stop_propagation();
        region_menu_open.update(|v| *v = !*v);
    };

    // Convert vertical scroll to horizontal scroll in the chip area
    let on_wheel = move |ev: web_sys::WheelEvent| {
        if let Some(el) = scroll_ref.get() {
            let el: &web_sys::HtmlElement = el.as_ref();
            let delta = ev.delta_y();
            if delta.abs() > 0.0 {
                ev.prevent_default();
                el.set_scroll_left(el.scroll_left() + delta as i32);
            }
        }
    };

    view! {
        <div class="bat-book-strip" on:click=move |_| { region_menu_open.set(false); }>
            <div class="bat-book-header">
                <span class="bat-book-title" on:click=on_title_click style="cursor:pointer">"Bat Book"</span>
                <span class="bat-book-region-label">
                    {move || state.bat_book_region.get().short_label()}
                </span>
                <div class="bat-book-config-wrap">
                    <button
                        class="bat-book-config-btn"
                        on:click=on_config
                        title="Choose region"
                    >
                        "\u{2699}"
                    </button>
                    {move || region_menu_open.get().then(|| {
                        view! {
                            <div class="bat-book-region-menu">
                                {BatBookRegion::ALL.iter().map(|&r| {
                                    let is_active = move || state.bat_book_region.get() == r;
                                    view! {
                                        <button
                                            class=move || if is_active() { "bat-book-region-opt active" } else { "bat-book-region-opt" }
                                            on:click=move |ev: web_sys::MouseEvent| {
                                                ev.stop_propagation();
                                                state.bat_book_region.set(r);
                                                region_menu_open.set(false);
                                            }
                                        >
                                            {r.label()}
                                        </button>
                                    }
                                }).collect_view()}
                            </div>
                        }
                    })}
                </div>
                <div style="flex:1"></div>
                <button
                    class="bat-book-close-btn"
                    on:click=on_close
                    title="Close bat book"
                >
                    "\u{00d7}"
                </button>
            </div>
            <div class="bat-book-scroll" node_ref=scroll_ref on:wheel=on_wheel>
                {move || {
                    let m = manifest.get();
                    m.entries.iter().map(|entry| {
                        view! { <BatBookChip entry=entry.clone() /> }
                    }).collect_view()
                }}
            </div>
        </div>
    }
}

/// Compute the combined FF range from all selected entries.
/// Returns (lo, hi) or None if nothing selected.
fn combined_ff_range(state: &AppState) -> Option<(f64, f64)> {
    let ids = state.bat_book_selected_ids.get_untracked();
    if ids.is_empty() {
        return None;
    }
    let region = state.bat_book_region.get_untracked();
    let manifest = get_manifest(region);
    let mut lo = f64::MAX;
    let mut hi = f64::MIN;
    for entry in &manifest.entries {
        if ids.iter().any(|id| id == entry.id) {
            lo = lo.min(entry.freq_lo_hz);
            hi = hi.max(entry.freq_hi_hz);
        }
    }
    if lo < hi { Some((lo, hi)) } else { None }
}

/// Apply the combined FF range from selected bat book entries.
/// Shows toasts for out-of-range conditions.
/// Uses the focus stack to push/update the BatBook override layer.
fn apply_bat_book_ff(state: &AppState) {
    let Some((lo, hi)) = combined_ff_range(state) else {
        // No valid frequency range — pop the bat book override
        state.pop_bat_book_ff();
        return;
    };

    // Only apply if a file is loaded
    let files = state.files.get_untracked();
    let Some(idx) = state.current_file_index.get_untracked() else { return };
    let Some(file) = files.get(idx) else { return };
    let nyquist = file.audio.sample_rate as f64 / 2.0;

    if lo >= nyquist {
        state.show_info_toast(format!(
            "Frequency range {}\u{2013}{} kHz is above this file's Nyquist ({} kHz)",
            (lo / 1000.0) as u32,
            (hi / 1000.0) as u32,
            (nyquist / 1000.0) as u32,
        ));
        return;
    }

    let clamped_hi = hi.min(nyquist);
    if clamped_hi < hi {
        state.show_info_toast(format!(
            "Frequency range clamped to {}\u{2013}{} kHz (file Nyquist: {} kHz)",
            (lo / 1000.0) as u32,
            (clamped_hi / 1000.0) as u32,
            (nyquist / 1000.0) as u32,
        ));
    }

    state.push_bat_book_ff(lo, clamped_hi);
}

#[component]
fn BatBookChip(entry: BatBookEntry) -> impl IntoView {
    let state = expect_context::<AppState>();
    let entry_id = entry.id.to_string();
    let entry_id_for_click = entry_id.clone();
    let name = entry.name;
    let freq_label = entry.freq_range_label();
    let call_type = entry.call_type;

    let is_selected = {
        let eid = entry_id.clone();
        move || {
            state.bat_book_selected_ids.get().iter().any(|id| id == &eid)
        }
    };

    let on_click = move |ev: web_sys::MouseEvent| {
        ev.stop_propagation();
        let ctrl = ev.ctrl_key() || ev.meta_key();
        let shift = ev.shift_key();
        let eid = entry_id_for_click.clone();

        let was_selected = state.bat_book_selected_ids.get_untracked().iter().any(|id| id == &eid);

        if was_selected && !ctrl && !shift {
            // Click selected bat again: deselect and restore previous FF
            state.bat_book_selected_ids.set(Vec::new());
            state.bat_book_ref_open.set(false);
            state.bat_book_last_clicked_id.set(None);
            // Pop the bat book override — restores user's previous FF if not adopted
            state.pop_bat_book_ff();
            return;
        }

        if ctrl && was_selected {
            // Ctrl/Cmd-click an already-selected bat: remove from selection
            state.bat_book_selected_ids.update(|ids| ids.retain(|id| id != &eid));
            if state.bat_book_selected_ids.get_untracked().is_empty() {
                state.bat_book_ref_open.set(false);
                state.bat_book_last_clicked_id.set(None);
                state.pop_bat_book_ff();
            } else {
                // Recalculate combined range
                apply_bat_book_ff(&state);
            }
            state.bat_book_last_clicked_id.set(Some(eid));
            return;
        }

        if shift {
            // Shift-click: range select from last clicked to this entry
            let region = state.bat_book_region.get_untracked();
            let manifest = get_manifest(region);
            let last_id = state.bat_book_last_clicked_id.get_untracked();
            let anchor = last_id.as_deref().unwrap_or("");
            let anchor_idx = manifest.entries.iter().position(|e| e.id == anchor);
            let click_idx = manifest.entries.iter().position(|e| e.id == eid.as_str());

            if let (Some(a), Some(b)) = (anchor_idx, click_idx) {
                let lo = a.min(b);
                let hi = a.max(b);
                let range_ids: Vec<String> = manifest.entries[lo..=hi]
                    .iter()
                    .map(|e| e.id.to_string())
                    .collect();
                if ctrl {
                    // Shift+Ctrl: add range to existing selection
                    state.bat_book_selected_ids.update(|ids| {
                        for rid in &range_ids {
                            if !ids.iter().any(|id| id == rid) {
                                ids.push(rid.clone());
                            }
                        }
                    });
                } else {
                    // Shift only: replace selection with range
                    state.bat_book_selected_ids.set(range_ids);
                }
            } else {
                // No anchor or entry not found — treat as normal click
                state.bat_book_selected_ids.set(vec![eid.clone()]);
            }
        } else if ctrl {
            // Ctrl/Cmd-click: add to selection
            state.bat_book_selected_ids.update(|ids| {
                if !ids.iter().any(|id| id == &eid) {
                    ids.push(eid.clone());
                }
            });
        } else {
            // Normal click: replace selection
            state.bat_book_selected_ids.set(vec![eid.clone()]);
        }

        state.bat_book_last_clicked_id.set(Some(eid));
        state.bat_book_ref_open.set(true);
        apply_bat_book_ff(&state);
    };

    let class = move || {
        if is_selected() {
            use crate::focus_stack::FocusSource;
            if state.focus_stack.get().is_adopted(FocusSource::BatBook) {
                "bat-book-chip selected adopted"
            } else {
                "bat-book-chip selected"
            }
        } else {
            "bat-book-chip"
        }
    };

    view! {
        <button class=class on:click=on_click>
            <span class="bat-book-chip-name">{name}</span>
            <span class="bat-book-chip-freq">{freq_label} " " {call_type}</span>
        </button>
    }
}
