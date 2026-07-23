use crate::annotations::{AnnotationId, AnnotationKind};
use crate::canvas::spectrogram_renderer::freq_to_y;
use crate::components::file_sidebar::settings_panel::{delete_annotation, update_annotation_label};
use crate::state::store_fields::*;
use crate::state::AppState;
use leptos::prelude::*;
use wasm_bindgen::JsCast;

/// Floating label editor anchored to the selected annotation's top-left corner on the spectrogram.
/// Shown when `state.annotations.editing()` is true and exactly one annotation is selected.
#[component]
pub fn AnnotationLabelEditor() -> impl IntoView {
    let state = expect_context::<AppState>();

    // Position + initial label, reactive.
    let editor_state = Signal::derive(move || {
        if !state.annotations.editing().get() {
            return None;
        }
        let ids = state.annotations.selected_ids().get();
        if ids.len() != 1 {
            return None;
        }
        let id = ids[0].clone();
        let idx = state.library.current_index().get()?;
        let file_id = state.current_file_id_tracked()?;
        let store = state.annotations.store().get();
        let set = store.get(file_id)?;
        let ann = set.annotations.iter().find(|a| a.id == id)?;
        let region = match &ann.kind {
            AnnotationKind::Region(r) => r,
            _ => return None,
        };

        let files = state.library.files().get();
        let file = files.get(idx)?;
        let time_res = file.spectrogram.time_resolution;
        let file_max_freq = file.spectrogram.max_freq;

        let scroll = state.view.scroll_offset().get();
        let zoom = state.view.zoom_level().get();
        let canvas_w = state.viewmode.spectrogram_canvas_width().get();
        let min_freq = state.view.min_display_freq().get().unwrap_or(0.0);
        let max_freq = state.view.max_display_freq().get().unwrap_or(file_max_freq);
        let canvas_h = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| {
                d.query_selector(".spectrogram-container canvas")
                    .ok()
                    .flatten()
            })
            .map(|el| el.get_bounding_client_rect().height())
            .unwrap_or(400.0);

        let visible_time = (canvas_w / zoom) * time_res;
        let start_time = scroll;
        let px_per_sec = canvas_w / visible_time;
        let x0 = ((region.time_start - start_time) * px_per_sec).max(0.0);
        let y0 = match region.freq_high {
            Some(fh) => freq_to_y(fh, min_freq, max_freq, canvas_h).max(0.0),
            None => 0.0,
        };

        let is_default = ann.label_default.unwrap_or(false);
        let initial = if is_default {
            String::new()
        } else {
            region.label.clone().unwrap_or_default()
        };

        Some((id, x0, y0, initial))
    });

    view! {
        {move || {
            let (id, x, y, initial) = match editor_state.get() {
                Some(t) => t,
                None => return None,
            };
            let id_enter = id.clone();
            let id_blur = id.clone();
            let id_esc = id.clone();
            let input_ref = NodeRef::<leptos::html::Input>::new();
            Effect::new(move |_| {
                if let Some(el) = input_ref.get() {
                    let _ = el.focus();
                    let _ = el.select();
                }
            });
            let save_from = move |el: &web_sys::HtmlInputElement, aid: &AnnotationId| {
                let val = el.value();
                let label = if val.trim().is_empty() { None } else { Some(val.trim().to_string()) };
                update_annotation_label(state, aid, label);
                state.annotations.is_new_edit().set(false);
                state.annotations.editing().set(false);
            };
            Some(view! {
                <input
                    class="annotation-inline-label-editor"
                    type="text"
                    placeholder="Label..."
                    node_ref=input_ref
                    prop:value=initial
                    style=format!("position: absolute; left: {:.0}px; top: {:.0}px;", x + 2.0, y + 1.0)
                    on:click=|ev: web_sys::MouseEvent| ev.stop_propagation()
                    on:mousedown=|ev: web_sys::MouseEvent| ev.stop_propagation()
                    on:pointerdown=|ev: web_sys::PointerEvent| ev.stop_propagation()
                    on:keydown=move |ev: web_sys::KeyboardEvent| {
                        if ev.key() == "Enter" {
                            ev.prevent_default();
                            let input = ev.target().unwrap().unchecked_into::<web_sys::HtmlInputElement>();
                            save_from(&input, &id_enter);
                        } else if ev.key() == "Escape" {
                            ev.prevent_default();
                            ev.stop_propagation();
                            if state.annotations.is_new_edit().get_untracked() {
                                delete_annotation(state, &id_esc);
                            }
                            state.annotations.is_new_edit().set(false);
                            state.annotations.editing().set(false);
                        }
                    }
                    on:focusout=move |ev: web_sys::FocusEvent| {
                        if !state.annotations.editing().get_untracked() { return; }
                        let input = ev.target().unwrap().unchecked_into::<web_sys::HtmlInputElement>();
                        save_from(&input, &id_blur);
                    }
                />
            })
        }}
    }
}
