pub mod annotations;
pub mod audio;
pub mod bat_book;
pub mod canvas;
pub mod components;
pub mod dsp;
pub mod file_identity;
pub mod focus_stack;
pub mod format_time;
pub mod opfs;
pub mod project;
pub mod project_store;
pub mod scope;
pub mod settings;
pub mod state;
pub mod tauri_bridge;
pub mod test_hook;
pub mod timeline;
pub mod types;
pub mod viewport;
pub mod web_util;
pub mod wikimedia;

use components::app::App;
use leptos::prelude::*;

#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn main() {
    console_error_panic_hook::set_once();
    let _ = console_log::init_with_level(log::Level::Debug);

    if cfg!(debug_assertions) {
        log::warn!(
            "Oversample is running in DEBUG WASM mode. Audio rendering is much \
             slower and the app can hit spurious WASM panics that don't happen \
             in release. Run `trunk serve --release` (or `trunk build --release`)."
        );
    }

    mount_to_body(App);
}
