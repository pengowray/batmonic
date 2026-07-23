//! Viewport-aware popup positioning.
//!
//! `PopupPanel` portals its content to `<body>` and computes placement against
//! the viewport: flip to the opposite side when the preferred side has no
//! room, shift along the cross-axis to stay on-screen, and cap `max-height`
//! to the remaining space. This replaces the ad-hoc `position: absolute` +
//! fixed pixel offsets pattern that broke on narrow viewports (notably
//! Android portrait) when neither below-left nor below-right had room.
//!
//! Why portals: previously, popups lived inside the trigger's parent (e.g.
//! `.hearing-bar`), which meant any clipping ancestor or z-index neighbour
//! could cover them. Portalling to `<body>` cuts that knot — popups always
//! sit above everything else.

use leptos::portal::Portal;
use leptos::prelude::*;
use wasm_bindgen::prelude::*;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Side {
    Below,
    Above,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Align {
    /// Align popup's left edge with anchor's left edge.
    Start,
    /// Align popup's right edge with anchor's right edge.
    End,
}

/// Resolved placement, in viewport coordinates (suitable for `position: fixed`).
#[derive(Clone, Copy, Debug)]
pub struct Placement {
    pub left: f64,
    pub top: f64,
    pub max_width: f64,
    pub max_height: f64,
}

/// Pure positioning math. Inputs are in viewport-relative CSS pixels.
///
/// `anchor` is `(left, top, right, bottom)`. `margin` is the gap between the
/// anchor edge and the popup along the chosen side. `edge_pad` is the minimum
/// gap between the popup and any viewport edge.
pub fn compute_placement(
    anchor: (f64, f64, f64, f64),
    panel_size: (f64, f64),
    viewport: (f64, f64),
    preferred_side: Side,
    preferred_align: Align,
    margin: f64,
    edge_pad: f64,
) -> Placement {
    let (a_left, a_top, a_right, a_bottom) = anchor;
    let (p_w, p_h) = panel_size;
    let (v_w, v_h) = viewport;

    // Flip: stick with the preferred side if it fits, otherwise pick the side
    // with more room.
    let space_below = (v_h - a_bottom - edge_pad).max(0.0);
    let space_above = (a_top - edge_pad).max(0.0);
    let needed = p_h + margin;
    let resolved_side = match preferred_side {
        Side::Below if space_below >= needed => Side::Below,
        Side::Above if space_above >= needed => Side::Above,
        _ => {
            if space_below >= space_above {
                Side::Below
            } else {
                Side::Above
            }
        }
    };

    let (top, side_space) = match resolved_side {
        Side::Below => (a_bottom + margin, space_below - margin),
        Side::Above => {
            let h = p_h.min((space_above - margin).max(0.0));
            (a_top - margin - h, space_above - margin)
        }
    };
    let max_height = side_space.max(0.0);

    let raw_left = match preferred_align {
        Align::Start => a_left,
        Align::End => a_right - p_w,
    };
    let max_left = (v_w - p_w - edge_pad).max(edge_pad);
    let left = raw_left.clamp(edge_pad, max_left);
    let max_width = (v_w - 2.0 * edge_pad).max(0.0);

    Placement {
        left,
        top: top.max(edge_pad),
        max_width,
        max_height,
    }
}

/// Schedule `cb` to run on the next animation frame. The closure is consumed.
fn raf_once<F: FnOnce() + 'static>(cb: F) {
    let Some(win) = web_sys::window() else { return };
    let closure = Closure::once_into_js(cb);
    let _ = win.request_animation_frame(closure.unchecked_ref());
}

/// Add a window resize listener; the returned `i32` is a handle the caller can
/// use with `remove_event_listener_with_callback_and_event_listener_options`
/// (we don't bother with cleanup here because popups close on outside-click
/// and the listener leak is bounded by the number of distinct popup
/// instances — tiny in practice).
fn add_resize_listener<F: Fn() + 'static>(cb: F) -> Option<(web_sys::Window, Closure<dyn Fn()>)> {
    let win = web_sys::window()?;
    let closure = Closure::wrap(Box::new(cb) as Box<dyn Fn()>);
    win.add_event_listener_with_callback("resize", closure.as_ref().unchecked_ref())
        .ok()?;
    Some((win, closure))
}

/// Smart popup wrapper. The caller supplies a `NodeRef` pointing at the
/// anchor element (typically the trigger button or its row container) and a
/// signal for open/closed.
///
/// When `is_open` is true, the children are rendered into a portal under
/// `<body>` at a viewport-aware position. The popup re-measures on window
/// resize while open. When `is_open` is false, the portal is unmounted.
///
/// `extra_style` is appended to the inline style on the popup `<div>` (useful
/// for `min-width`, etc.). `class` defaults to `"layer-panel"` so the popup
/// inherits the existing visual style.
#[component]
pub fn PopupPanel(
    is_open: Signal<bool>,
    anchor: NodeRef<leptos::html::Div>,
    #[prop(default = Side::Below)] preferred_side: Side,
    #[prop(default = Align::Start)] preferred_align: Align,
    #[prop(default = "")] extra_style: &'static str,
    #[prop(default = "layer-panel")] class: &'static str,
    children: ChildrenFn,
) -> impl IntoView {
    let placement: RwSignal<Option<Placement>> = RwSignal::new(None);
    let panel_ref = NodeRef::<leptos::html::Div>::new();

    // Measurement: runs once the panel is mounted (on rAF after is_open flips),
    // and again on every window resize while open.
    let measure = move || {
        // `try_get_untracked`: `measure` is deferred via a leaked `raf_once` (and
        // re-run from the resize listener), so it can fire after PopupPanel is
        // disposed, which disposes these NodeRefs. The outer `Some` guards
        // disposal; the inner guards the element not being mounted yet.
        let Some(Some(anchor_el)) = anchor.try_get_untracked() else {
            return;
        };
        let Some(Some(panel_el)) = panel_ref.try_get_untracked() else {
            return;
        };
        let Some(win) = web_sys::window() else { return };

        let a = anchor_el.get_bounding_client_rect();
        let p = panel_el.get_bounding_client_rect();

        let v_w = win
            .inner_width()
            .ok()
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let v_h = win
            .inner_height()
            .ok()
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);

        let resolved = compute_placement(
            (a.left(), a.top(), a.right(), a.bottom()),
            (p.width(), p.height()),
            (v_w, v_h),
            preferred_side,
            preferred_align,
            4.0,
            4.0,
        );
        placement.set(Some(resolved));
    };

    Effect::new(move |_| {
        if !is_open.get() {
            placement.set(None);
            return;
        }
        // Defer one frame so the portal has actually mounted the children.
        raf_once(measure);
        // Attach a resize listener for the lifetime of this open state.
        // SendWrapper is needed because wasm-bindgen Closure is !Send (which
        // is irrelevant on single-threaded WASM but is enforced by Leptos's
        // cleanup API). When is_open flips back to false the effect re-runs
        // and the cleanup fires.
        if let Some((win, closure)) = add_resize_listener(move || {
            raf_once(measure);
        }) {
            let pair = send_wrapper::SendWrapper::new((win, closure));
            Owner::on_cleanup(move || {
                let (win, closure) = pair.take();
                let _ = win.remove_event_listener_with_callback(
                    "resize",
                    closure.as_ref().unchecked_ref(),
                );
            });
        }
    });

    // Stash children in a StoredValue so we can call them from inside the
    // `Show`/`Portal` reactive closures without tripping the FnOnce-vs-Fn
    // capture rules in the `view!` macro expansion.
    let children = StoredValue::new(children);

    view! {
        <Show when=move || is_open.get() fallback=|| ()>
            <Portal>
                <div
                    node_ref=panel_ref
                    class=class
                    style=move || match placement.get() {
                        Some(p) => format!(
                            "position: fixed; left: {}px; top: {}px; max-width: {}px; max-height: {}px; {extra_style}",
                            p.left, p.top, p.max_width, p.max_height,
                        ),
                        // Render off-screen for the first paint so we can
                        // measure intrinsic size without a visible flash at
                        // the wrong position.
                        None => format!(
                            "position: fixed; left: -9999px; top: -9999px; visibility: hidden; {extra_style}"
                        ),
                    }
                >
                    {move || children.with_value(|c| c())}
                </div>
            </Portal>
        </Show>
    }
}
