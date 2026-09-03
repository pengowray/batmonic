// View Bar — sits below the Hearing Bar (above the main canvas) and owns
// the visualization-layer controls: which view fills the canvas, which
// overlays (annotations, bat book) are visible, and which canvas tool is
// active for click/drag interactions.
//
// Kept deliberately flexible — flex-wrap + a `.bar-spacer` separator lets
// new toggles be added on either end without breaking narrow layouts.

use crate::components::icons::Icon;
use crate::state::store_fields::*;
use leptos::prelude::*;

use crate::components::app::MainViewButton;
use crate::components::xform_button::XformButton;
use crate::state::{ActiveFocus, AppState, Bar, CanvasTool, LayerPanel};

fn layer_opt_class(active: bool) -> &'static str {
    if active {
        "layer-panel-opt sel"
    } else {
        "layer-panel-opt"
    }
}

fn toggle_panel(state: &AppState, panel: LayerPanel) {
    state.panels.layer_panel_open().update(|p| {
        *p = if *p == Some(panel) { None } else { Some(panel) };
    });
}

/// Stacked overlay toggles — "Annotations" over "Bat book" — rendered as two
/// compact checkboxes so they read as simple "show this overlay" switches
/// rather than intimidating mode buttons. Annotations require a loaded file;
/// the bat book is a reference and is always available.
#[component]
fn OverlayToggles() -> impl IntoView {
    let state = expect_context::<AppState>();
    let has_file = Signal::derive(move || {
        state.library.current_index().get().is_some() || state.timeline.active().get().is_some()
    });

    let anno_on = move || state.annotations.visible().get();
    let toggle_anno = move |_: web_sys::Event| {
        let new_visible = !state.annotations.visible().get_untracked();
        state.annotations.visible().set(new_visible);
        if !new_visible {
            // Drop annotation focus/selection and clear interaction state.
            if state.interaction.active_focus().get_untracked() == Some(ActiveFocus::Annotations) {
                state.interaction.active_focus().set(None);
            }
            if !state.annotations.selected_ids().get_untracked().is_empty() {
                state.annotations.selected_ids().set(Vec::new());
            }
            state.annotations.hover_handle().set(None);
            state.annotations.drag_handle().set(None);
            state.annotations.editing().set(false);
            state.annotations.is_new_edit().set(false);
        }
    };

    let book_on = move || state.bat_book.open().get() || state.bat_book.ref_open().get();
    let toggle_book = move |_: web_sys::Event| {
        let on = book_on();
        state.bat_book.open().set(!on);
        state.bat_book.ref_open().set(!on);
    };

    view! {
        <div class="overlay-toggles">
            <label
                class="overlay-check"
                class:disabled=move || !has_file.get()
                title=move || if anno_on() { "Hide annotations" } else { "Show annotations" }
            >
                <input
                    type="checkbox"
                    prop:checked=move || anno_on()
                    prop:disabled=move || !has_file.get()
                    on:change=toggle_anno
                />
                <span>"Annotations"</span>
            </label>
            <label
                class="overlay-check"
                title=move || if book_on() { "Hide bat book" } else { "Show bat book" }
            >
                <input
                    type="checkbox"
                    prop:checked=move || book_on()
                    on:change=toggle_book
                />
                <span>"Bat book"</span>
            </label>
        </div>
    }
}

/// Canvas tool selector (Hand / Selection). Dropdown opens downward since
/// this lives in the top-positioned View Bar (not the bottom toolbar).
#[component]
fn ToolCombo() -> impl IntoView {
    let state = expect_context::<AppState>();
    let is_open = move || state.panels.layer_panel_open().get() == Some(LayerPanel::Tool);
    let no_file = move || {
        state.library.current_index().get().is_none() && state.timeline.active().get().is_none()
    };

    view! {
        <div style="position: relative;">
            <button
                class=move || {
                    if no_file() { "layer-btn tool-combo-btn disabled" }
                    else if is_open() { "layer-btn tool-combo-btn open" }
                    else { "layer-btn tool-combo-btn" }
                }
                on:click=move |_| { if !no_file() { toggle_panel(&state, LayerPanel::Tool); } }
                title=move || match state.interaction.canvas_tool().get() {
                    CanvasTool::Hand => "Tool: Hand (pan)",
                    CanvasTool::Selection => "Tool: Selection",
                }
            >
                <span class="layer-btn-category">"\u{00A0}"</span>
                <span class="layer-btn-value tool-combo-icon">{move || match state.interaction.canvas_tool().get() {
                    CanvasTool::Hand => view! { <Icon kind=Icon::Hand /> },
                    CanvasTool::Selection => view! { <Icon kind=Icon::Select /> },
                }}</span>
            </button>
            <Show when=move || is_open()>
                <div class="layer-panel" style="top: calc(100% + 4px); right: 0;">
                    <div class="layer-panel-title">"Tool"</div>
                    <button
                        class=move || layer_opt_class(state.interaction.canvas_tool().get() == CanvasTool::Hand)
                        on:click=move |_| {
                            state.interaction.canvas_tool().set(CanvasTool::Hand);
                            state.panels.layer_panel_open().set(None);
                        }
                    ><Icon kind=Icon::Hand />"\u{00A0}\u{00A0}Hand (pan)"</button>
                    <button
                        class=move || layer_opt_class(state.interaction.canvas_tool().get() == CanvasTool::Selection)
                        on:click=move |_| {
                            state.interaction.canvas_tool().set(CanvasTool::Selection);
                            state.panels.layer_panel_open().set(None);
                        }
                    ><Icon kind=Icon::Select />"\u{00A0}\u{00A0}Selection"</button>
                </div>
            </Show>
        </div>
    }
}

#[component]
pub fn ViewBar() -> impl IntoView {
    let state = expect_context::<AppState>();
    let has_file = move || {
        state.library.current_index().get().is_some() || state.timeline.active().get().is_some()
    };
    let controls_ref = NodeRef::<leptos::html::Div>::new();
    let pan = crate::components::bar_pan::BarPan::new(controls_ref);

    view! {
        // Stop clicks/taps inside the bar from bubbling to .main's
        // "close all panels" handler — without this, opening a custom
        // button's panel (Tool, etc.) immediately re-closes it.
        // ComboButton has its own stop_propagation, so the combos in
        // here used to be fine; the Tool button is a plain <button>
        // that doesn't, hence the menu was getting eaten.
        <div class="view-bar"
            class:panel-open=move || matches!(state.panels.layer_panel_open().get().map(LayerPanel::bar), Some(Bar::View))
            on:click=|ev: web_sys::MouseEvent| ev.stop_propagation()
            on:touchstart=|ev: web_sys::TouchEvent| ev.stop_propagation()
        >
            <span class="bar-label">"VIEW"</span>
            <div class="bar-controls"
                node_ref=controls_ref
                on:pointerdown=move |ev| pan.on_pointerdown(ev)
                on:pointermove=move |ev| pan.on_pointermove(ev)
                on:pointerup=move |ev| pan.on_pointerup(ev)
                on:pointercancel=move |ev| pan.on_pointerup(ev)
                on:wheel=move |ev| pan.on_wheel(ev)
            >
                <MainViewButton />
                <div class="bar-sep"></div>
                <XformButton />
                <div class="bar-sep"></div>
                <OverlayToggles />
                <div class="bar-spacer"></div>
                // Always shown (no longer hidden on mobile or when the window
                // is narrow) so the user can never get stranded without a way
                // back to the Hand tool. Icon-only to keep it compact.
                {move || has_file().then(|| view! { <ToolCombo /> })}
            </div>
        </div>
    }
}
