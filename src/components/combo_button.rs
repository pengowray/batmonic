use crate::state::store_fields::*;
use leptos::prelude::*;
use wasm_bindgen::prelude::*;

use crate::components::icons::Icon;
use crate::components::popup::{Align, PopupPanel, Side};
use crate::state::AppState;

/// Reusable horizontal split button: left side performs an action (toggle),
/// right side shows a reactive label + dropdown arrow and opens a menu.
/// Press-and-hold on either side also opens the menu (mobile-friendly).
#[component]
pub fn ComboButton(
    /// Category label on the left button (e.g. "View", "HF")
    left_label: &'static str,
    /// Optional reactive override for the left category label. When set, it
    /// takes precedence over the static `left_label`. Used by the Play
    /// button to show the current playback mode (e.g. "HET", "TE").
    #[prop(optional, into)]
    left_label_dyn: Option<Signal<String>>,
    /// Value text on the left button (e.g. "ON"/"OFF"); empty string hides it
    #[prop(into)]
    left_value: Signal<String>,
    /// Optional icon drawn before `left_value` on the left button. Counts
    /// as a value for layout purposes, so an icon-only button (Play) keeps
    /// its category label above it.
    #[prop(optional, into)]
    left_icon: Option<Signal<Option<Icon>>>,
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
    /// Optional reactive label for the right button's category text (small top text)
    #[prop(optional, into)]
    right_label: Option<Signal<String>>,
    /// "below" or "above" — direction the panel opens
    #[prop(default = "below")]
    menu_direction: &'static str,
    /// "left" or "right" — horizontal edge the panel is anchored to
    #[prop(default = "left")]
    panel_align: &'static str,
    /// Extra inline style for the dropdown panel (e.g. "min-width: 210px;")
    #[prop(default = "")]
    panel_style: &'static str,
    /// Optional callback for long-press on the left button.  When provided
    /// and the callback returns, the long-press fires this instead of opening
    /// the dropdown menu.
    #[prop(optional, into)]
    left_long_press: Option<Callback<web_sys::MouseEvent>>,
    /// Dropdown panel content. Uses `ChildrenFn` (callable multiple times) so
    /// the panel content can be unmounted / re-mounted when the popup opens
    /// and closes via the portal.
    children: ChildrenFn,
) -> impl IntoView {
    let state = expect_context::<AppState>();

    // -- press-and-hold timer --
    let hold_timer: RwSignal<Option<i32>> = RwSignal::new(None);
    // Tracks whether the hold timer already fired (long press opened the menu)
    let hold_fired: RwSignal<bool> = RwSignal::new(false);
    // Timestamp when the hold gesture started (for long-press duration compensation)
    let hold_start_ms: RwSignal<f64> = RwSignal::new(0.0);
    // Initial touch position for movement threshold
    let touch_start_xy: RwSignal<(f64, f64)> = RwSignal::new((0.0, 0.0));
    // True once a touch has moved far enough to count as a drag/scroll
    // (rather than a tap). When set, touchend does NOT fire the button
    // action and does NOT preventDefault — letting the parent's
    // horizontal scroll (mobile toolbars) proceed naturally.
    let touch_dragging: RwSignal<bool> = RwSignal::new(false);

    let start_hold = move || {
        cancel_hold_inner(hold_timer);
        hold_fired.set(false);
        hold_start_ms.set(js_sys::Date::now());
        let window = web_sys::window().unwrap();
        let toggle = toggle_menu;
        let long_press = left_long_press;
        let cb = Closure::wrap(Box::new(move || {
            hold_fired.set(true);
            if let Some(lp) = long_press {
                // Store gesture start time so the callback can compensate for hold
                // duration. `try_get_untracked`: this leaked 400ms timer can fire
                // after the ComboButton is disposed mid-hold, disposing this
                // signal; bail rather than panic on the disposed read.
                let Some(start) = hold_start_ms.try_get_untracked() else {
                    return;
                };
                state.mic.gesture_start_ms().set(Some(start));
                let me = web_sys::MouseEvent::new("longpress").unwrap();
                lp.run(me);
            } else {
                toggle.run(());
            }
        }) as Box<dyn Fn()>);
        if let Ok(id) = window
            .set_timeout_with_callback_and_timeout_and_arguments_0(cb.as_ref().unchecked_ref(), 400)
        {
            hold_timer.set(Some(id));
        }
        cb.forget();
    };

    let cancel_hold = move || {
        cancel_hold_inner(hold_timer);
    };

    // Convert preference props to popup enums.
    let preferred_side = if menu_direction == "above" {
        Side::Above
    } else {
        Side::Below
    };
    let preferred_align = if panel_align == "right" {
        Align::End
    } else {
        Align::Start
    };

    // NodeRef for the .combo-btn-row container — the PopupPanel uses this as
    // the anchor for viewport-aware placement.
    let row_ref = NodeRef::<leptos::html::Div>::new();

    view! {
        <div
            node_ref=row_ref
            class=move || if is_open.get() { "combo-btn-row open" } else { "combo-btn-row" }
            style=move || format!(
                "pointer-events: {};",
                if state.interaction.mouse_in_label_area().get() { "none" } else { "auto" }
            )
            on:click=|ev: web_sys::MouseEvent| ev.stop_propagation()
            on:touchstart=|ev: web_sys::TouchEvent| ev.stop_propagation()
        >
            // ── Left button ──
            <button
                class=move || format!("{} lock-grow", left_class.get())
                title=left_title
                on:click=move |ev: web_sys::MouseEvent| {
                    cancel_hold();
                    // Don't fire click if long-press already fired
                    if !hold_fired.get_untracked() {
                        left_click.run(ev);
                    }
                }
                on:mousedown=move |_| start_hold()
                on:mouseup=move |_| cancel_hold()
                on:touchstart=move |ev: web_sys::TouchEvent| {
                    // Do NOT preventDefault — that blocks the parent's
                    // horizontal scroll on mobile toolbars. Tap vs. drag
                    // is resolved in touchmove / touchend below.
                    touch_dragging.set(false);
                    if let Some(touch) = ev.touches().get(0) {
                        touch_start_xy.set((touch.client_x() as f64, touch.client_y() as f64));
                    }
                    start_hold();
                }
                on:touchend=move |ev: web_sys::TouchEvent| {
                    cancel_hold();
                    // Fire the primary action only for a genuine tap (no
                    // drag, no long-press). preventDefault here suppresses
                    // the synthetic mouse click that would otherwise
                    // double-fire on:click. For a drag we do neither, so
                    // the scroll the browser just performed stands.
                    if !touch_dragging.get_untracked() && !hold_fired.get_untracked() {
                        let me = web_sys::MouseEvent::new("click").unwrap();
                        left_click.run(me);
                        ev.prevent_default();
                    }
                }
                on:touchmove=move |ev: web_sys::TouchEvent| {
                    // >10px movement = drag/scroll: cancel the hold timer
                    // and mark this gesture so touchend won't fire the tap.
                    if let Some(touch) = ev.touches().get(0) {
                        let (sx, sy) = touch_start_xy.get_untracked();
                        let dx = touch.client_x() as f64 - sx;
                        let dy = touch.client_y() as f64 - sy;
                        if (dx * dx + dy * dy) > 100.0 {
                            touch_dragging.set(true);
                            cancel_hold();
                        }
                    }
                }
                on:contextmenu=move |ev: web_sys::MouseEvent| ev.prevent_default()
            >
                <span class="combo-btn-text combo-btn-text-left">
                    <span class="layer-btn-category fit-text" data-fit-max="9" data-fit-min="7">{move || {
                        let value = left_value.get();
                        let has_icon = left_icon.is_some_and(|s| s.get().is_some());
                        let label = left_label_dyn.map(|s| s.get()).unwrap_or_else(|| left_label.to_string());
                        if (value.is_empty() && !has_icon) || label.is_empty() {
                            "\u{00A0}".to_string()
                        } else {
                            label
                        }
                    }}</span>
                    <span class="layer-btn-value fit-text" data-fit-max="13" data-fit-min="9">{move || {
                        let value = left_value.get();
                        let icon = left_icon.and_then(|s| s.get());
                        let text = if !value.is_empty() || icon.is_some() {
                            value
                        } else {
                            let label = left_label_dyn.map(|s| s.get()).unwrap_or_else(|| left_label.to_string());
                            if !label.is_empty() { label } else { "\u{00A0}".to_string() }
                        };
                        let gap = (icon.is_some() && !text.is_empty()).then_some("\u{00A0}");
                        view! {
                            {icon.map(|kind| view! { <Icon kind=kind /> })}
                            {gap}
                            {text}
                        }
                    }}</span>
                </span>
            </button>

            // ── Right button ──
            <button
                class=move || format!("{} lock-grow", right_class.get())
                title=right_title
                on:click=move |_: web_sys::MouseEvent| {
                    cancel_hold();
                    toggle_menu.run(());
                }
                on:mousedown=move |_| start_hold()
                on:mouseup=move |_| cancel_hold()
                on:touchstart=move |ev: web_sys::TouchEvent| {
                    // Do NOT preventDefault — preserves parent scroll.
                    touch_dragging.set(false);
                    if let Some(touch) = ev.touches().get(0) {
                        touch_start_xy.set((touch.client_x() as f64, touch.client_y() as f64));
                    }
                    start_hold();
                }
                on:touchend=move |ev: web_sys::TouchEvent| {
                    cancel_hold();
                    // Genuine tap only: toggle the menu and suppress the
                    // synthetic click. A drag scrolls instead.
                    if !touch_dragging.get_untracked() && !hold_fired.get_untracked() {
                        toggle_menu.run(());
                        ev.prevent_default();
                    }
                }
                on:touchmove=move |ev: web_sys::TouchEvent| {
                    if let Some(touch) = ev.touches().get(0) {
                        let (sx, sy) = touch_start_xy.get_untracked();
                        let dx = touch.client_x() as f64 - sx;
                        let dy = touch.client_y() as f64 - sy;
                        if (dx * dx + dy * dy) > 100.0 {
                            touch_dragging.set(true);
                            cancel_hold();
                        }
                    }
                }
                on:contextmenu=move |ev: web_sys::MouseEvent| ev.prevent_default()
            >
                <span class="combo-btn-text combo-btn-text-right">
                    <span class="layer-btn-category fit-text" data-fit-max="9" data-fit-min="7">{move || {
                        match right_label {
                            Some(sig) => {
                                let v = sig.get();
                                if v.is_empty() { "\u{00A0}".to_string() } else { v }
                            }
                            None => "\u{00A0}".to_string(),
                        }
                    }}</span>
                    <span class="layer-btn-value fit-text" data-fit-max="13" data-fit-min="9">{move || right_value.get()}</span>
                </span>
                <span class="combo-btn-arrow">{"\u{25E2}"}</span>
            </button>

            // ── Dropdown panel ──
            //
            // Rendered into a portal under <body> via PopupPanel so it
            // escapes any clipping ancestor (overflow:hidden on the toolbar,
            // narrow sidebar, etc.) and so we don't have to manually manage
            // z-index stacking contexts. Placement is computed against the
            // viewport with flip + shift, so panels that don't fit below or
            // beside the trigger reflow into the available space instead of
            // clipping off-screen.
            <PopupPanel
                is_open=is_open
                anchor=row_ref
                preferred_side=preferred_side
                preferred_align=preferred_align
                extra_style=panel_style
            >
                {children()}
            </PopupPanel>
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
