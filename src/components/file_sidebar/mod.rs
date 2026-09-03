mod config_panel;
mod export_section;
pub(crate) mod file_badges;
pub(crate) mod file_groups;
pub(crate) mod files_panel;
mod project_panel;
pub(crate) use project_panel::save_project_async;
pub mod analysis;
pub mod harmonics;
mod loading;
pub mod metadata_panel;
pub mod mic_chooser;
pub mod privacy_settings;
pub mod psd_panel;
pub mod pulse_panel;
pub mod settings_panel;
pub(crate) mod streaming_load;
mod suggestions;

use crate::components::icons::Icon;
use crate::state::store_fields::*;
use crate::state::AppState;
use js_sys;
use leptos::prelude::*;
use std::{cell::RefCell, rc::Rc};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use crate::state::LeftSidebarTab;
pub(crate) use analysis::AnalysisPanel as SidebarAnalysisPanel;
use config_panel::ConfigPanel;
use files_panel::FilesPanel;
pub(crate) use harmonics::HarmonicsPanel;
pub(crate) use loading::{
    fetch_bytes, fetch_demo_index, fetch_text, load_named_bytes, load_native_file, load_single_demo,
};
pub(crate) use metadata_panel::MetadataPanel;
use project_panel::ProjectPanel;
pub(crate) use psd_panel::PsdPanel;
pub(crate) use pulse_panel::PulsePanel;
pub(crate) use settings_panel::{run_resonator_bench, SelectionPanel};

fn copy_to_clipboard(text: &str) {
    if let Some(window) = web_sys::window() {
        let clipboard = window.navigator().clipboard();
        let _ = clipboard.write_text(text);
    }
}

#[component]
pub fn FileSidebar() -> impl IntoView {
    let state = expect_context::<AppState>();

    // Resize drag logic
    let on_resize_start = move |ev: web_sys::MouseEvent| {
        ev.prevent_default();
        let start_x = ev.client_x() as f64;
        let start_width = state.panels.left_width().get_untracked();
        let doc = web_sys::window().unwrap().document().unwrap();
        let body = doc.body().unwrap();
        let _ = body.class_list().add_1("sidebar-resizing");

        let on_move =
            Closure::<dyn FnMut(web_sys::MouseEvent)>::new(move |ev: web_sys::MouseEvent| {
                let dx = ev.client_x() as f64 - start_x;
                let new_width = (start_width + dx).clamp(140.0, 500.0);
                state.panels.left_width().set(new_width);
            });
        let on_move_slot: Rc<RefCell<Option<Closure<dyn FnMut(web_sys::MouseEvent)>>>> =
            Rc::new(RefCell::new(Some(on_move)));
        let on_up_slot: Rc<RefCell<Option<Closure<dyn FnMut(web_sys::MouseEvent)>>>> =
            Rc::new(RefCell::new(None));

        let on_move_fn = on_move_slot
            .borrow()
            .as_ref()
            .unwrap()
            .as_ref()
            .unchecked_ref::<js_sys::Function>()
            .clone();
        let _ = doc.add_event_listener_with_callback("mousemove", &on_move_fn);

        let doc_for_up = doc.clone();
        let on_move_slot_for_up = Rc::clone(&on_move_slot);
        let on_up_slot_for_up = Rc::clone(&on_up_slot);
        let on_move_fn_for_up = on_move_fn.clone();
        let on_up =
            Closure::<dyn FnMut(web_sys::MouseEvent)>::new(move |_: web_sys::MouseEvent| {
                if let Some(body) = doc_for_up.body() {
                    let _ = body.class_list().remove_1("sidebar-resizing");
                }
                let _ =
                    doc_for_up.remove_event_listener_with_callback("mousemove", &on_move_fn_for_up);
                if let Some(on_up) = on_up_slot_for_up.borrow().as_ref() {
                    let on_up_fn = on_up.as_ref().unchecked_ref::<js_sys::Function>();
                    let _ = doc_for_up
                        .remove_event_listener_with_callback_and_bool("mouseup", on_up_fn, true);
                }
                on_move_slot_for_up.borrow_mut().take();
                on_up_slot_for_up.borrow_mut().take();
            });

        let on_up_fn = on_up.as_ref().unchecked_ref::<js_sys::Function>().clone();
        *on_up_slot.borrow_mut() = Some(on_up);
        let _ = doc.add_event_listener_with_callback_and_bool("mouseup", &on_up_fn, true);
    };

    let sidebar_class = move || {
        let mut cls = String::from("sidebar");
        if state.panels.left_collapsed().get() {
            cls.push_str(" collapsed");
        }
        if state.status.is_mobile().get() {
            cls.push_str(" mobile-overlay");
        }
        cls
    };

    view! {
        <div class=sidebar_class>
            <div class="sidebar-tabs">
                <button
                    class=move || if state.panels.left_tab().get() == LeftSidebarTab::Files {
                        "sidebar-header-label active"
                    } else {
                        "sidebar-header-label"
                    }
                    on:click=move |_| {
                        state.panels.left_tab().set(LeftSidebarTab::Files);
                    }
                    title="Files"
                >
                    "Files"
                </button>
                {move || state.project.enabled().get().then(|| view! {
                    <button
                        class=move || if state.panels.left_tab().get() == LeftSidebarTab::Project {
                            "sidebar-header-label active"
                        } else {
                            "sidebar-header-label"
                        }
                        on:click=move |_| {
                            state.panels.left_tab().set(LeftSidebarTab::Project);
                        }
                        title="Project (beta)"
                    >
                        "Project"
                        {move || {
                            if state.project.current().with(|p| p.is_some()) {
                                Some(view! { <span class="project-tab-dot">{"\u{25CF}"}</span> })
                            } else {
                                None
                            }
                        }}
                    </button>
                })}
                <button
                    class=move || if state.panels.left_tab().get() == LeftSidebarTab::Settings {
                        "sidebar-settings-btn active"
                    } else {
                        "sidebar-settings-btn"
                    }
                    on:click=move |_| {
                        let current = state.panels.left_tab().get();
                        if current == LeftSidebarTab::Settings {
                            state.panels.left_tab().set(LeftSidebarTab::Files);
                        } else {
                            state.panels.left_tab().set(LeftSidebarTab::Settings);
                        }
                    }
                    title=move || if state.panels.left_tab().get() == LeftSidebarTab::Settings {
                        "Back to files"
                    } else {
                        "Settings"
                    }
                >
                    <Icon kind=Icon::Settings />
                </button>
            </div>
            {move || {
                match state.panels.left_tab().get() {
                    LeftSidebarTab::Files => view! { <FilesPanel /> }.into_any(),
                    LeftSidebarTab::Project if state.project.enabled().get() => view! { <ProjectPanel /> }.into_any(),
                    LeftSidebarTab::Project => {
                        state.panels.left_tab().set(LeftSidebarTab::Files);
                        view! { <FilesPanel /> }.into_any()
                    }
                    LeftSidebarTab::Settings => view! { <ConfigPanel /> }.into_any(),
                }
            }}
            <div
                class=move || if state.status.is_mobile().get() { "sidebar-resize-handle hidden" } else { "sidebar-resize-handle" }
                on:mousedown=on_resize_start
            ></div>
        </div>
    }
}
