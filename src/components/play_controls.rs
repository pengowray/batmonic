use leptos::prelude::*;
use crate::state::{AppState, StatusLevel};
use crate::viewport;

/// App-level toast display — always visible regardless of whether a file is open.
#[component]
pub fn ToastDisplay() -> impl IntoView {
    let state = expect_context::<AppState>();

    view! {
        <div class="app-toast-container">
            // Persistent "Buffering…" indicator while the streaming decoder
            // is behind the playhead. Distinct from transient status toasts
            // so it doesn't auto-dismiss.
            {move || state.is_buffering.get().then(|| {
                view! {
                    <span class="status-toast status-toast-info">"Buffering\u{2026}"</span>
                }
            })}
            {move || state.status_message.get().map(|msg| {
                let state2 = state;
                wasm_bindgen_futures::spawn_local(async move {
                    let p = js_sys::Promise::new(&mut |resolve, _| {
                        if let Some(w) = web_sys::window() {
                            let _ = w.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 3000);
                        }
                    });
                    wasm_bindgen_futures::JsFuture::from(p).await.ok();
                    state2.status_message.set(None);
                });
                let cls = if state.status_level.get_untracked() == StatusLevel::Info {
                    "status-toast status-toast-info"
                } else {
                    "status-toast"
                };
                view! {
                    <span class=cls>{msg}</span>
                }
            })}
        </div>
    }
}

/// Bookmark popup — floated in main-overlays so it appears over the canvas.
#[component]
pub fn BookmarkPopup() -> impl IntoView {
    let state = expect_context::<AppState>();

    view! {
        {move || state.show_bookmark_popup.get().then(|| {
            let bms = state.bookmarks.get();
            let recent: Vec<_> = bms.iter().rev().take(8).cloned().collect();
            view! {
                <div class="bookmark-popup"
                    on:click=|ev: web_sys::MouseEvent| ev.stop_propagation()
                >
                    <div class="bookmark-popup-title">"Bookmarks"</div>
                    {recent.into_iter().map(|bm| {
                        let t = bm.time;
                        let state2 = state;
                        view! {
                            <button class="bookmark-item"
                                on:click=move |_| {
                                    let zoom = state2.zoom_level.get_untracked();
                                    let files = state2.files.get_untracked();
                                    let idx = state2.current_file_index.get_untracked();
                                    let time_res = idx.and_then(|i| files.get(i))
                                        .map(|f| f.spectrogram.time_resolution)
                                        .unwrap_or(0.001);
                                    let canvas_w = 800.0_f64;
                                    let visible_time = viewport::visible_time(canvas_w, zoom, time_res);
                                    let new_scroll = (t - visible_time * 0.1).max(0.0);
                                    state2.suspend_follow();
                                    state2.scroll_offset.set(new_scroll);
                                    state2.show_bookmark_popup.set(false);
                                }
                            >{crate::format_time::format_time_display(t, 2)}</button>
                        }
                    }).collect_view()}
                    <button class="bookmark-popup-close"
                        on:click=move |_| state.show_bookmark_popup.set(false)
                    >"Dismiss"</button>
                </div>
            }
        })}
    }
}
