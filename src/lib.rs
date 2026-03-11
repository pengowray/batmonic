pub mod types;
pub mod dsp;
pub mod audio;
pub mod canvas;
pub mod components;
pub mod state;
pub mod focus_stack;
pub mod tauri_bridge;
pub mod bat_book;
pub mod annotations;
pub mod file_identity;
pub mod format_time;
pub mod opfs;

use leptos::prelude::*;
use components::app::App;

#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn main() {
    console_error_panic_hook::set_once();
    let _ = console_log::init_with_level(log::Level::Debug);

    mount_to_body(App);
}
