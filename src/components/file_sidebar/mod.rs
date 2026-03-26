pub(crate) mod file_groups;
pub(crate) mod file_badges;
mod files_panel;
mod config_panel;
mod export_section;
mod project_panel;
pub(crate) use project_panel::save_project_async;
pub mod settings_panel;
pub mod analysis;
pub mod metadata_panel;
pub mod harmonics;
pub mod notch_panel;
pub mod psd_panel;
pub mod pulse_panel;
mod loading;
mod streaming_load;
mod mic_chooser;

use leptos::prelude::*;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use js_sys;
use std::{cell::RefCell, rc::Rc};
use crate::state::AppState;

use files_panel::FilesPanel;
use config_panel::ConfigPanel;
use project_panel::ProjectPanel;
use crate::state::LeftSidebarTab;
pub(crate) use settings_panel::SelectionPanel;
pub(crate) use analysis::AnalysisPanel as SidebarAnalysisPanel;
pub(crate) use metadata_panel::MetadataPanel;
pub(crate) use harmonics::HarmonicsPanel;
pub(crate) use notch_panel::NotchPanel;
pub(crate) use psd_panel::PsdPanel;
pub(crate) use pulse_panel::PulsePanel;
pub(crate) use loading::{load_named_bytes, load_native_file, fetch_demo_index, load_single_demo};

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
        let start_width = state.sidebar_width.get_untracked();
        let doc = web_sys::window().unwrap().document().unwrap();
        let body = doc.body().unwrap();
        let _ = body.class_list().add_1("sidebar-resizing");

        let on_move = Closure::<dyn FnMut(web_sys::MouseEvent)>::new(move |ev: web_sys::MouseEvent| {
            let dx = ev.client_x() as f64 - start_x;
            let new_width = (start_width + dx).clamp(140.0, 500.0);
            state.sidebar_width.set(new_width);
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
        let on_up = Closure::<dyn FnMut(web_sys::MouseEvent)>::new(move |_: web_sys::MouseEvent| {
            if let Some(body) = doc_for_up.body() {
                let _ = body.class_list().remove_1("sidebar-resizing");
            }
            let _ = doc_for_up.remove_event_listener_with_callback("mousemove", &on_move_fn_for_up);
            if let Some(on_up) = on_up_slot_for_up.borrow().as_ref() {
                let on_up_fn = on_up.as_ref().unchecked_ref::<js_sys::Function>();
                let _ = doc_for_up.remove_event_listener_with_callback_and_bool("mouseup", on_up_fn, true);
            }
            on_move_slot_for_up.borrow_mut().take();
            on_up_slot_for_up.borrow_mut().take();
        });

        let on_up_fn = on_up.as_ref().unchecked_ref::<js_sys::Function>().clone();
        *on_up_slot.borrow_mut() = Some(on_up);
        let _ = doc.add_event_listener_with_callback_and_bool("mouseup", &on_up_fn, true);
    };

    let is_mobile = state.is_mobile.get_untracked();

    let sidebar_class = move || {
        let mut cls = String::from("sidebar");
        if state.sidebar_collapsed.get() {
            cls.push_str(" collapsed");
        }
        if is_mobile {
            cls.push_str(" mobile-overlay");
        }
        cls
    };

    view! {
        <div class=sidebar_class>
            <div class="sidebar-tabs">
                {if !is_mobile {
                    Some(view! {
                        <button
                            class="sidebar-tab sidebar-collapse-btn"
                            on:click=move |_| {
                                state.sidebar_collapsed.update(|c| *c = !*c);
                            }
                            title=move || if state.sidebar_collapsed.get() { "Show sidebar" } else { "Hide sidebar" }
                        >
                            {"\u{25E7}"}
                        </button>
                    })
                } else {
                    None
                }}
                <button
                    class=move || if state.left_sidebar_tab.get() == LeftSidebarTab::Files {
                        "sidebar-header-label active"
                    } else {
                        "sidebar-header-label"
                    }
                    on:click=move |_| {
                        state.left_sidebar_tab.set(LeftSidebarTab::Files);
                    }
                    title="Files"
                >
                    "Files"
                </button>
                {move || state.projects_enabled.get().then(|| view! {
                    <button
                        class=move || if state.left_sidebar_tab.get() == LeftSidebarTab::Project {
                            "sidebar-header-label active"
                        } else {
                            "sidebar-header-label"
                        }
                        on:click=move |_| {
                            state.left_sidebar_tab.set(LeftSidebarTab::Project);
                        }
                        title="Project (beta)"
                    >
                        "Project"
                        {move || {
                            if state.current_project.with(|p| p.is_some()) {
                                Some(view! { <span class="project-tab-dot">{"\u{25CF}"}</span> })
                            } else {
                                None
                            }
                        }}
                    </button>
                })}
                <button
                    class=move || if state.left_sidebar_tab.get() == LeftSidebarTab::Settings {
                        "sidebar-settings-btn active"
                    } else {
                        "sidebar-settings-btn"
                    }
                    on:click=move |_| {
                        let current = state.left_sidebar_tab.get();
                        if current == LeftSidebarTab::Settings {
                            state.left_sidebar_tab.set(LeftSidebarTab::Files);
                        } else {
                            state.left_sidebar_tab.set(LeftSidebarTab::Settings);
                        }
                    }
                    title=move || if state.left_sidebar_tab.get() == LeftSidebarTab::Settings {
                        "Back to files"
                    } else {
                        "Settings"
                    }
                >
                    {"\u{2699}"}
                </button>
            </div>
            {move || {
                match state.left_sidebar_tab.get() {
                    LeftSidebarTab::Files => view! { <FilesPanel /> }.into_any(),
                    LeftSidebarTab::Project if state.projects_enabled.get() => view! { <ProjectPanel /> }.into_any(),
                    LeftSidebarTab::Project => {
                        state.left_sidebar_tab.set(LeftSidebarTab::Files);
                        view! { <FilesPanel /> }.into_any()
                    }
                    LeftSidebarTab::Settings => view! { <ConfigPanel /> }.into_any(),
                }
            }}
            {if !is_mobile {
                Some(view! { <div class="sidebar-resize-handle" on:mousedown=on_resize_start></div> })
            } else {
                None
            }}
        </div>
    }
}
