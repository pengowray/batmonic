use leptos::prelude::*;
use wasm_bindgen::prelude::*;

use crate::state::AppState;

/// Reusable horizontal split button: left side performs an action (toggle),
/// right side shows a reactive label + dropdown arrow and opens a menu.
/// Press-and-hold on either side also opens the menu (mobile-friendly).
#[component]
pub fn ComboButton(
    /// Category label on the left button (e.g. "View", "HF")
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
    /// Optional reactive label for the right button's category text (small top text)
    #[prop(optional, into)]
    right_label: Option<Signal<String>>,
    /// "below" or "above" — direction the panel opens
    #[prop(default = "below")]
    menu_direction: &'static str,
    /// Extra inline style for the dropdown panel (e.g. "min-width: 210px;")
    #[prop(default = "")]
    panel_style: &'static str,
    /// Optional callback for long-press on the left button.  When provided
    /// and the callback returns, the long-press fires this instead of opening
    /// the dropdown menu.
    #[prop(optional, into)]
    left_long_press: Option<Callback<web_sys::MouseEvent>>,
    /// Dropdown panel content
    children: Children,
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
                // Store gesture start time so the callback can compensate for hold duration
                let start = hold_start_ms.get_untracked();
                state.mic_gesture_start_ms.set(Some(start));
                let me = web_sys::MouseEvent::new("longpress").unwrap();
                lp.run(me);
            } else {
                toggle.run(());
            }
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
            class=move || if is_open.get() { "combo-btn-row open" } else { "combo-btn-row" }
            style=move || format!(
                "pointer-events: {};",
                if state.mouse_in_label_area.get() { "none" } else { "auto" }
            )
            on:click=|ev: web_sys::MouseEvent| ev.stop_propagation()
            on:touchstart=|ev: web_sys::TouchEvent| ev.stop_propagation()
        >
            // ── Left button ──
            <button
                class=move || left_class.get()
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
                    ev.prevent_default();
                    if let Some(touch) = ev.touches().get(0) {
                        touch_start_xy.set((touch.client_x() as f64, touch.client_y() as f64));
                    }
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
                on:touchmove=move |ev: web_sys::TouchEvent| {
                    // Only cancel hold if finger moved more than 10px (prevents
                    // accidental cancellation from natural finger jitter on mobile)
                    if let Some(touch) = ev.touches().get(0) {
                        let (sx, sy) = touch_start_xy.get_untracked();
                        let dx = touch.client_x() as f64 - sx;
                        let dy = touch.client_y() as f64 - sy;
                        if (dx * dx + dy * dy) > 100.0 {
                            cancel_hold();
                        }
                    }
                }
                on:contextmenu=move |ev: web_sys::MouseEvent| ev.prevent_default()
            >
                <span class="combo-btn-text combo-btn-text-left">
                    <span class="layer-btn-category">{move || {
                        let value = left_value.get();
                        if value.is_empty() || left_label.is_empty() {
                            "\u{00A0}".to_string()
                        } else {
                            left_label.to_string()
                        }
                    }}</span>
                    <span class="layer-btn-value">{move || {
                        let value = left_value.get();
                        if !value.is_empty() {
                            value
                        } else if !left_label.is_empty() {
                            left_label.to_string()
                        } else {
                            "\u{00A0}".to_string()
                        }
                    }}</span>
                </span>
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
                    if let Some(touch) = ev.touches().get(0) {
                        touch_start_xy.set((touch.client_x() as f64, touch.client_y() as f64));
                    }
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
                on:touchmove=move |ev: web_sys::TouchEvent| {
                    if let Some(touch) = ev.touches().get(0) {
                        let (sx, sy) = touch_start_xy.get_untracked();
                        let dx = touch.client_x() as f64 - sx;
                        let dy = touch.client_y() as f64 - sy;
                        if (dx * dx + dy * dy) > 100.0 {
                            cancel_hold();
                        }
                    }
                }
                on:contextmenu=move |ev: web_sys::MouseEvent| ev.prevent_default()
            >
                <span class="combo-btn-text combo-btn-text-right">
                    <span class="layer-btn-category">{move || {
                        match right_label {
                            Some(sig) => {
                                let v = sig.get();
                                if v.is_empty() { "\u{00A0}".to_string() } else { v }
                            }
                            None => "\u{00A0}".to_string(),
                        }
                    }}</span>
                    <span class="layer-btn-value">{move || right_value.get()}</span>
                </span>
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
