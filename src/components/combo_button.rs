use leptos::prelude::*;
use wasm_bindgen::prelude::*;

use crate::state::AppState;

/// Reusable horizontal split button: left side performs an action (toggle),
/// right side shows a reactive label + dropdown arrow and opens a menu.
/// Press-and-hold on either side also opens the menu (mobile-friendly).
#[component]
pub fn ComboButton(
    /// Category label on the left button (e.g. "View", "HFR")
    left_label: &'static str,
    /// Value text on the left button (e.g. "ON"/"OFF"); empty string hides it
    #[prop(into)]
    left_value: Signal<String>,
    /// Called when the left button is clicked
    left_click: Callback<web_sys::MouseEvent>,
    /// CSS class for the left button (reactive, for active/open states)
    #[prop(into)]
    left_class: Signal<&'static str>,
    /// Label on the right button (e.g. "Spec", "PS", "\u{2014}")
    #[prop(into)]
    right_value: Signal<String>,
    /// CSS class for the right button
    #[prop(into)]
    right_class: Signal<&'static str>,
    /// Whether the dropdown is currently open
    #[prop(into)]
    is_open: Signal<bool>,
    /// Called to toggle the dropdown
    toggle_menu: Callback<()>,
    /// Tooltip for the left button
    #[prop(default = "")]
    left_title: &'static str,
    /// Tooltip for the right button
    #[prop(default = "")]
    right_title: &'static str,
    /// "below" or "above" — direction the panel opens
    #[prop(default = "below")]
    menu_direction: &'static str,
    /// Extra inline style for the dropdown panel (e.g. "min-width: 210px;")
    #[prop(default = "")]
    panel_style: &'static str,
    /// Dropdown panel content
    children: Children,
) -> impl IntoView {
    let state = expect_context::<AppState>();

    // -- press-and-hold timer --
    let hold_timer: RwSignal<Option<i32>> = RwSignal::new(None);
    // Tracks whether the hold timer already fired (long press opened the menu)
    let hold_fired: RwSignal<bool> = RwSignal::new(false);

    let start_hold = move || {
        cancel_hold_inner(hold_timer);
        hold_fired.set(false);
        let window = web_sys::window().unwrap();
        let toggle = toggle_menu;
        let cb = Closure::wrap(Box::new(move || {
            hold_fired.set(true);
            toggle.run(());
        }) as Box<dyn Fn()>);
        if let Ok(id) = window.set_timeout_with_callback_and_timeout_and_arguments_0(
            cb.as_ref().unchecked_ref(),
            400,
        ) {
            hold_timer.set(Some(id));
        }
        cb.forget();
    };

    let cancel_hold = move || {
        cancel_hold_inner(hold_timer);
    };

    let show_left_value = true;

    // Pre-render children so we can use FnOnce Children inside a reactive context
    let panel_content = children();

    // Panel visibility style: hidden when closed, positioned when open
    let hidden_style = format!("display: none; {panel_style}");
    let visible_style = if menu_direction == "above" {
        format!("bottom: calc(100% + 2px); left: 0; {panel_style}")
    } else {
        format!("top: calc(100% + 2px); left: 0; {panel_style}")
    };

    view! {
        <div
            class="combo-btn-row"
            style=move || format!(
                "pointer-events: {};",
                if state.mouse_in_label_area.get() { "none" } else { "auto" }
            )
            on:click=|ev: web_sys::MouseEvent| ev.stop_propagation()
        >
            // ── Left button ──
            <button
                class=move || left_class.get()
                title=left_title
                on:click=move |ev: web_sys::MouseEvent| {
                    cancel_hold();
                    left_click.run(ev);
                }
                on:mousedown=move |_| start_hold()
                on:mouseup=move |_| cancel_hold()
                on:touchstart=move |ev: web_sys::TouchEvent| {
                    ev.prevent_default();
                    start_hold();
                }
                on:touchend=move |ev: web_sys::TouchEvent| {
                    cancel_hold();
                    // Short tap: fire the primary action (click is suppressed by preventDefault)
                    if !hold_fired.get_untracked() {
                        // Synthesize a MouseEvent for the callback
                        let me = web_sys::MouseEvent::new("click").unwrap();
                        left_click.run(me);
                    }
                    ev.prevent_default();
                }
                on:touchmove=move |_| cancel_hold()
                on:contextmenu=move |ev: web_sys::MouseEvent| ev.prevent_default()
            >
                <span class="layer-btn-category">{left_label}</span>
                {show_left_value.then(|| view! {
                    <span class="layer-btn-value">{move || {
                        let v = left_value.get();
                        if v.is_empty() { "\u{00A0}".to_string() } else { v }
                    }}</span>
                })}
            </button>

            // ── Right button ──
            <button
                class=move || right_class.get()
                title=right_title
                on:click=move |_: web_sys::MouseEvent| {
                    cancel_hold();
                    toggle_menu.run(());
                }
                on:mousedown=move |_| start_hold()
                on:mouseup=move |_| cancel_hold()
                on:touchstart=move |ev: web_sys::TouchEvent| {
                    ev.prevent_default();
                    start_hold();
                }
                on:touchend=move |ev: web_sys::TouchEvent| {
                    cancel_hold();
                    // Short tap: toggle the menu (click is suppressed by preventDefault)
                    if !hold_fired.get_untracked() {
                        toggle_menu.run(());
                    }
                    ev.prevent_default();
                }
                on:touchmove=move |_| cancel_hold()
                on:contextmenu=move |ev: web_sys::MouseEvent| ev.prevent_default()
            >
                <span class="layer-btn-value">{move || right_value.get()}</span>
                <span class="combo-btn-arrow">{"\u{25BE}"}</span>
            </button>

            // ── Dropdown panel (always in DOM, toggled via display) ──
            <div
                class="layer-panel"
                style=move || if is_open.get() { visible_style.clone() } else { hidden_style.clone() }
            >
                {panel_content}
            </div>
        </div>
    }
}

fn cancel_hold_inner(hold_timer: RwSignal<Option<i32>>) {
    if let Some(id) = hold_timer.get_untracked() {
        if let Some(window) = web_sys::window() {
            window.clear_timeout_with_handle(id);
        }
        hold_timer.set(None);
    }
}
