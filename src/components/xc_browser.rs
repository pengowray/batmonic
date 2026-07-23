//! Xeno-Canto browser — **Tauri-only, by necessity, not an oversight**.
//!
//! Every operation here calls a native `xc_*` command (reqwest backend, shared
//! `xc-lib`) unconditionally — there is deliberately no browser `fetch()` path.
//! The XC API (`xeno-canto.org/api/3`) and its audio CDN do not send permissive
//! CORS headers and the project ships no proxy, so a browser fetch from
//! app.oversample.com would fail with a CORS error. The "Explore XC" entry
//! point is gated behind `state.is_tauri` (files_panel.rs), so this UI is never
//! reachable on the web build.
//!
//! The web build's only XC affordance is the `#XC<id>` URL-hash deep-link
//! (app.rs), which searches the curated demo index — NOT the live API. A real
//! browser XC mode would require a CORS-enabled proxy (a new feature, not a fix).

use crate::state::store_fields::*;
use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::prelude::*;
use crate::state::AppState;
use crate::tauri_bridge::{tauri_invoke, tauri_invoke_with_args};

const XC_GROUPS: &[&str] = &["bats", "birds", "frogs", "grasshoppers", "land mammals"];

const XC_COUNTRIES_RAW: &str = include_str!("../data/countries.txt");

fn xc_countries() -> Vec<&'static str> {
    XC_COUNTRIES_RAW.lines().filter(|l| !l.is_empty()).collect()
}

// ── Helper to call tauri_invoke with a JS object of args ─────────────

async fn invoke_with(cmd: &str, args: &js_sys::Object) -> Result<JsValue, String> {
    tauri_invoke(cmd, &args.into()).await
}

fn js_obj() -> js_sys::Object {
    js_sys::Object::new()
}

fn set_str(obj: &js_sys::Object, key: &str, val: &str) {
    js_sys::Reflect::set(obj, &JsValue::from_str(key), &JsValue::from_str(val)).ok();
}

// ── Data types (mirror Tauri response shapes) ────────────────────────

#[derive(Clone, Debug)]
struct SpeciesInfo {
    genus: String,
    sp: String,
    en: String,
    _fam: String,
    recording_count: u32,
}

#[derive(Clone, Debug)]
struct RecordingInfo {
    id: u64,
    en: String,
    _genus: String,
    _sp: String,
    q: String,
    length: String,
    cnt: String,
    loc: String,
    rec: String,
    date: String,
    sound_type: String,
    smp: String,
    dvc: String,
    mic: String,
}

#[derive(Clone, Debug)]
struct CachedFile {
    path: String,
    filename: String,
    _xc_id: u64,
    metadata: Vec<(String, String)>,
    hashes: Option<crate::state::SidecarHashes>,
}

// ── Parse helpers ────────────────────────────────────────────────────

fn parse_species_list(val: &JsValue) -> Vec<SpeciesInfo> {
    let taxonomy: oversample_ipc::xc::XcGroupTaxonomy =
        match serde_wasm_bindgen::from_value(val.clone()) {
            Ok(t) => t,
            Err(_) => return Vec::new(),
        };
    taxonomy
        .species
        .into_iter()
        .map(|s| SpeciesInfo {
            genus: s.genus,
            sp: s.sp,
            en: s.en,
            _fam: s.fam,
            recording_count: s.recording_count,
        })
        .collect()
}

fn parse_recordings(val: &JsValue) -> Vec<RecordingInfo> {
    let result: oversample_ipc::xc::XcSearchResult =
        match serde_wasm_bindgen::from_value(val.clone()) {
            Ok(r) => r,
            Err(_) => return Vec::new(),
        };
    result
        .recordings
        .into_iter()
        .map(|r| RecordingInfo {
            id: r.id_num(),
            en: r.en,
            _genus: r.genus,
            _sp: r.sp,
            q: r.q,
            length: r.length,
            cnt: r.cnt,
            loc: r.loc,
            rec: r.rec,
            date: r.date,
            sound_type: r.sound_type,
            smp: r.smp,
            dvc: r.dvc,
            mic: r.mic,
        })
        .collect()
}

/// Lightweight view of `XcSearchResult` that skips the (potentially large)
/// recordings array when only the pagination/count scalars are needed.
#[derive(serde::Deserialize)]
struct SearchMeta {
    #[serde(default = "one")]
    num_pages: u32,
    #[serde(default = "one")]
    page: u32,
    #[serde(default)]
    num_recordings: u32,
}
fn one() -> u32 {
    1
}

fn parse_num_pages(val: &JsValue) -> u32 {
    serde_wasm_bindgen::from_value::<SearchMeta>(val.clone())
        .map(|m| m.num_pages)
        .unwrap_or(1)
}

fn parse_current_page(val: &JsValue) -> u32 {
    serde_wasm_bindgen::from_value::<SearchMeta>(val.clone())
        .map(|m| m.page)
        .unwrap_or(1)
}

fn parse_num_recordings(val: &JsValue) -> u32 {
    serde_wasm_bindgen::from_value::<SearchMeta>(val.clone())
        .map(|m| m.num_recordings)
        .unwrap_or(0)
}

fn format_sample_rate(smp: &str) -> String {
    match smp.parse::<u64>() {
        Ok(hz) if hz >= 1000 => format!("{}kHz", hz / 1000),
        Ok(hz) => format!("{hz}Hz"),
        Err(_) => smp.to_string(),
    }
}

fn parse_cached_file(val: &JsValue) -> Option<CachedFile> {
    let cf: oversample_ipc::xc::XcCachedFile = serde_wasm_bindgen::from_value(val.clone()).ok()?;
    Some(CachedFile {
        path: cf.path,
        filename: cf.filename,
        _xc_id: cf.xc_id,
        metadata: cf.metadata,
        hashes: cf.hashes,
    })
}

// ── View states ──────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq)]
enum BrowserView {
    ApiKeyPrompt,
    GroupBrowse,
    SpeciesRecordings { genus: String, species: String, en: String },
    SearchResults,
}

// ── Component ────────────────────────────────────────────────────────

#[component]
pub fn XcBrowser() -> impl IntoView {
    let state = expect_context::<AppState>();

    let view = RwSignal::new(BrowserView::ApiKeyPrompt);
    let api_key_input = RwSignal::new(String::new());
    let has_key = RwSignal::new(false);
    let selected_group = RwSignal::new("bats".to_string());
    let country_input = RwSignal::new(String::new());
    let species_list: RwSignal<Vec<SpeciesInfo>> = RwSignal::new(Vec::new());
    let recordings: RwSignal<Vec<RecordingInfo>> = RwSignal::new(Vec::new());
    let loading = RwSignal::new(false);
    let error_msg: RwSignal<Option<String>> = RwSignal::new(None);
    let taxonomy_age: RwSignal<Option<String>> = RwSignal::new(None);
    let search_input = RwSignal::new(String::new());
    let recordings_page = RwSignal::new(1u32);
    let recordings_total_pages = RwSignal::new(1u32);
    let downloading: RwSignal<Option<u64>> = RwSignal::new(None);
    let recordings_total: RwSignal<u32> = RwSignal::new(0);
    let cached_ids: RwSignal<std::collections::HashSet<u64>> = RwSignal::new(std::collections::HashSet::new());

    // Country combobox state
    let country_dropdown_open = RwSignal::new(false);
    let country_filter_text = RwSignal::new(String::new());
    let country_highlight_idx = RwSignal::new(0usize);
    let countries = xc_countries();
    let filtered_countries = Memo::new(move |_| {
        let filter = country_filter_text.get().to_lowercase();
        if filter.is_empty() {
            countries.clone()
        } else {
            countries.iter().copied()
                .filter(|c| c.to_lowercase().contains(&filter))
                .collect::<Vec<_>>()
        }
    });

    // Check if API key is already set
    spawn_local(async move {
        if let Ok(val) = crate::tauri_bridge::tauri_invoke_no_args("xc_get_api_key").await {
            if val.is_string() && !val.as_string().unwrap_or_default().is_empty() {
                has_key.set(true);
                view.set(BrowserView::GroupBrowse);
            }
        }
    });

    let on_close = move |_: web_sys::MouseEvent| {
        state.dialogs.xc_browser_open().set(false);
    };

    // Prevent click on modal content from closing it
    let on_content_click = move |ev: web_sys::MouseEvent| {
        ev.stop_propagation();
    };

    let on_save_key = move |_: web_sys::MouseEvent| {
        let key = api_key_input.get_untracked().trim().to_string();
        if key.is_empty() {
            return;
        }
        spawn_local(async move {
            match tauri_invoke_with_args(
                "xc_set_api_key",
                &oversample_ipc::xc::XcSetApiKeyArgs { key: key.clone() },
            ).await {
                Ok(_) => {
                    has_key.set(true);
                    view.set(BrowserView::GroupBrowse);
                    error_msg.set(None);
                }
                Err(e) => error_msg.set(Some(format!("Failed to save key: {e}"))),
            }
        });
    };

    let load_group = move || {
        let group = selected_group.get_untracked();
        let country = {
            let c = country_input.get_untracked().trim().to_string();
            if c.is_empty() { None } else { Some(c) }
        };
        loading.set(true);
        error_msg.set(None);
        species_list.set(Vec::new());

        spawn_local(async move {
            match tauri_invoke_with_args(
                "xc_browse_group",
                &oversample_ipc::xc::XcGroupArgs { group: group.clone(), country: country.clone() },
            ).await {
                Ok(val) => {
                    species_list.set(parse_species_list(&val));
                }
                Err(e) => error_msg.set(Some(e)),
            }

            // Get cache age
            if let Ok(val) = tauri_invoke_with_args(
                "xc_taxonomy_age",
                &oversample_ipc::xc::XcGroupArgs { group: group.clone(), country: country.clone() },
            ).await {
                taxonomy_age.set(val.as_string());
            }

            loading.set(false);
        });
    };

    let on_load_group = move |_: web_sys::MouseEvent| {
        // Commit any pending filter text
        let text = country_filter_text.get_untracked().trim().to_string();
        country_input.set(text);
        country_dropdown_open.set(false);
        load_group();
    };

    let on_refresh = move |_: web_sys::MouseEvent| {
        let group = selected_group.get_untracked();
        let country = {
            let c = country_input.get_untracked().trim().to_string();
            if c.is_empty() { None } else { Some(c) }
        };
        loading.set(true);
        error_msg.set(None);

        spawn_local(async move {
            match tauri_invoke_with_args(
                "xc_refresh_taxonomy",
                &oversample_ipc::xc::XcGroupArgs { group: group.clone(), country: country.clone() },
            ).await {
                Ok(val) => {
                    species_list.set(parse_species_list(&val));
                    taxonomy_age.set(Some("just now".to_string()));
                }
                Err(e) => error_msg.set(Some(e)),
            }
            loading.set(false);
        });
    };

    let check_cached = move |ids: Vec<u64>| {
        spawn_local(async move {
            let mut set = std::collections::HashSet::new();
            for id in ids {
                if let Ok(val) = tauri_invoke_with_args(
                    "xc_is_cached",
                    &oversample_ipc::xc::XcIdArgs { id },
                ).await {
                    if val.as_bool().unwrap_or(false) {
                        set.insert(id);
                    }
                }
            }
            cached_ids.set(set);
        });
    };

    let load_species_recordings = move |genus: String, species: String, en: String| {
        view.set(BrowserView::SpeciesRecordings {
            genus: genus.clone(),
            species: species.clone(),
            en,
        });
        loading.set(true);
        recordings.set(Vec::new());
        recordings_page.set(1);
        error_msg.set(None);

        spawn_local(async move {
            match tauri_invoke_with_args(
                "xc_species_recordings",
                &oversample_ipc::xc::XcSpeciesArgs {
                    genus: genus.clone(),
                    species: species.clone(),
                    page: None,
                },
            ).await {
                Ok(val) => {
                    recordings.set(parse_recordings(&val));
                    recordings_page.set(parse_current_page(&val));
                    recordings_total_pages.set(parse_num_pages(&val));
                    recordings_total.set(parse_num_recordings(&val));
                    let ids: Vec<u64> = recordings.try_get_untracked().unwrap_or_default().iter().map(|r| r.id).collect();
                    check_cached(ids);
                }
                Err(e) => error_msg.set(Some(e)),
            }
            loading.set(false);
        });
    };

    let on_search = move |_: web_sys::MouseEvent| {
        let query = search_input.get_untracked().trim().to_string();
        if query.is_empty() {
            return;
        }
        view.set(BrowserView::SearchResults);
        loading.set(true);
        recordings.set(Vec::new());
        error_msg.set(None);

        spawn_local(async move {
            match tauri_invoke_with_args(
                "xc_search",
                &oversample_ipc::xc::XcSearchArgs { query: query.clone(), page: None },
            ).await {
                Ok(val) => {
                    recordings.set(parse_recordings(&val));
                    recordings_page.set(parse_current_page(&val));
                    recordings_total_pages.set(parse_num_pages(&val));
                    recordings_total.set(parse_num_recordings(&val));
                    let ids: Vec<u64> = recordings.try_get_untracked().unwrap_or_default().iter().map(|r| r.id).collect();
                    check_cached(ids);
                }
                Err(e) => error_msg.set(Some(e)),
            }
            loading.set(false);
        });
    };

    let on_search_keydown = move |ev: web_sys::KeyboardEvent| {
        if ev.key() == "Enter" {
            on_search(ev.unchecked_into());
        }
    };

    let on_country_keydown = move |ev: web_sys::KeyboardEvent| {
        match ev.key().as_str() {
            "Enter" => {
                if country_dropdown_open.get_untracked() {
                    let list = filtered_countries.get_untracked();
                    let idx = country_highlight_idx.get_untracked();
                    if idx == 0 {
                        country_input.set(String::new());
                        country_filter_text.set(String::new());
                    } else if let Some(name) = list.get(idx - 1) {
                        country_input.set(name.to_string());
                        country_filter_text.set(name.to_string());
                    }
                    country_dropdown_open.set(false);
                } else {
                    let text = country_filter_text.get_untracked().trim().to_string();
                    country_input.set(text);
                }
                load_group();
            }
            "Escape" => {
                country_dropdown_open.set(false);
                country_filter_text.set(country_input.get_untracked());
            }
            "ArrowDown" => {
                ev.prevent_default();
                if !country_dropdown_open.get_untracked() {
                    country_dropdown_open.set(true);
                }
                let max = filtered_countries.get_untracked().len();
                country_highlight_idx.update(|i| *i = (*i + 1).min(max));
            }
            "ArrowUp" => {
                ev.prevent_default();
                country_highlight_idx.update(|i| *i = i.saturating_sub(1));
            }
            _ => {}
        }
    };

    let on_back = move |_: web_sys::MouseEvent| {
        view.set(BrowserView::GroupBrowse);
        recordings.set(Vec::new());
    };

    let load_recordings_page = move |page_num: u32| {
        let current_view = view.get_untracked();
        loading.set(true);
        recordings.set(Vec::new());
        error_msg.set(None);

        spawn_local(async move {
            let result = match &current_view {
                BrowserView::SpeciesRecordings { genus, species, .. } => {
                    tauri_invoke_with_args(
                        "xc_species_recordings",
                        &oversample_ipc::xc::XcSpeciesArgs {
                            genus: genus.clone(),
                            species: species.clone(),
                            page: Some(page_num),
                        },
                    ).await
                }
                BrowserView::SearchResults => {
                    tauri_invoke_with_args(
                        "xc_search",
                        &oversample_ipc::xc::XcSearchArgs {
                            query: search_input.get_untracked(),
                            page: Some(page_num),
                        },
                    ).await
                }
                _ => return,
            };

            match result {
                Ok(val) => {
                    recordings.set(parse_recordings(&val));
                    recordings_page.set(parse_current_page(&val));
                    recordings_total_pages.set(parse_num_pages(&val));
                    recordings_total.set(parse_num_recordings(&val));
                    let ids: Vec<u64> = recordings.try_get_untracked().unwrap_or_default().iter().map(|r| r.id).collect();
                    check_cached(ids);
                }
                Err(e) => error_msg.set(Some(e)),
            }
            loading.set(false);
        });
    };

    let download_and_load = move |id: u64| {
        downloading.set(Some(id));
        error_msg.set(None);
        spawn_local(async move {
            let result: Result<(), String> = async {
                let val = tauri_invoke_with_args(
                    "xc_download",
                    &oversample_ipc::xc::XcIdArgs { id },
                ).await?;
                let cached = parse_cached_file(&val)
                    .ok_or_else(|| "Failed to parse download result".to_string())?;

                // Read raw file bytes via efficient binary IPC
                let path_args = js_obj();
                set_str(&path_args, "path", &cached.path);
                let bytes_val = invoke_with("read_file_bytes", &path_args).await?;

                // Convert ArrayBuffer/Uint8Array → Vec<u8>
                let bytes: Vec<u8> = if let Ok(ab) = bytes_val.dyn_into::<js_sys::ArrayBuffer>() {
                    js_sys::Uint8Array::new(&ab).to_vec()
                } else {
                    return Err("read_file_bytes did not return ArrayBuffer".to_string());
                };

                // Use the standard loading pipeline (WASM-side decode, spectrogram, etc.)
                let load_id = state.loading_start(&cached.filename);
                let load_result = crate::components::file_sidebar::load_named_bytes(
                    cached.filename.clone(),
                    &bytes,
                    Some(cached.metadata),
                    cached.hashes,
                    None,
                    None,
                    state,
                    load_id,
                    false,
                ).await;
                state.loading_done(load_id);
                load_result?;

                // Switch to the newly loaded file
                let file_count = state.library.files().with_untracked(|files| files.len());
                if file_count > 0 {
                    state.library.current_index().set(Some(file_count - 1));
                }

                cached_ids.update(|s| { s.insert(id); });
                state.dialogs.xc_browser_open().set(false);
                Ok(())
            }.await;

            if let Err(e) = result {
                log::error!("Failed to load XC{id}: {e}");
                error_msg.set(Some(format!("Failed to load: {e}")));
            }
            downloading.set(None);
        });
    };

    view! {
        <div class="xc-modal-overlay" on:click=on_close>
            <div class="xc-modal" on:click=on_content_click>
                <div class="xc-modal-header">
                    <span class="xc-modal-title">"Explore Xeno-Canto"</span>
                    <button class="xc-modal-close" on:click=on_close>{"\u{00D7}"}</button>
                </div>

                // Error display
                {move || error_msg.get().map(|msg| view! {
                    <div class="xc-error">
                        <span>{msg}</span>
                        <button class="xc-error-dismiss" on:click=move |_| error_msg.set(None)>{"\u{00D7}"}</button>
                    </div>
                })}

                // Download progress indicator
                {move || downloading.get().map(|id| view! {
                    <div class="xc-downloading">{format!("Downloading XC{id}\u{2026}")}</div>
                })}

                // API key prompt
                {move || {
                    if view.get() == BrowserView::ApiKeyPrompt {
                        Some(view! {
                            <div class="xc-section">
                                <p class="xc-info">
                                    "Enter your Xeno-Canto API key. You can get one by creating a free account at "
                                    <a href="https://xeno-canto.org" target="_blank">"xeno-canto.org"</a>
                                    " and going to your account settings."
                                </p>
                                <div class="xc-key-form">
                                    <input
                                        type="password"
                                        class="xc-input"
                                        placeholder="API key"
                                        on:input=move |ev| {
                                            let val = event_target_value(&ev);
                                            api_key_input.set(val);
                                        }
                                    />
                                    <button class="xc-btn" on:click=on_save_key>"Save key"</button>
                                </div>
                            </div>
                        })
                    } else {
                        None
                    }
                }}

                // Main browse/search view
                {move || {
                    let current_view = view.get();
                    if !has_key.get() { return None; }

                    Some(view! {
                        <div class="xc-section">
                            // Search bar
                            <div class="xc-search-bar">
                                <input
                                    type="text"
                                    class="xc-input xc-search-input"
                                    placeholder="Search (e.g. Myotis, bat, Australia...)"
                                    prop:value=move || search_input.get()
                                    on:input=move |ev| search_input.set(event_target_value(&ev))
                                    on:keydown=on_search_keydown
                                />
                                <button class="xc-btn" on:click=on_search>"Search"</button>
                            </div>

                            // Group/country filters
                            {move || {
                                if matches!(current_view, BrowserView::GroupBrowse) {
                                    Some(view! {
                                        <div class="xc-filters">
                                            <label>"Group: "</label>
                                            <select
                                                class="xc-select"
                                                on:change=move |ev| {
                                                    selected_group.set(event_target_value(&ev));
                                                }
                                            >
                                                {XC_GROUPS.iter().map(|g| {
                                                    let g = g.to_string();
                                                    let g2 = g.clone();
                                                    let g3 = g.clone();
                                                    view! {
                                                        <option
                                                            value=g.clone()
                                                            selected=move || selected_group.get() == g2
                                                        >{g3}</option>
                                                    }
                                                }).collect::<Vec<_>>()}
                                            </select>
                                            <label>" Country: "</label>
                                            <div
                                                class="xc-combobox"
                                                tabindex="-1"
                                                on:focusout=move |ev: web_sys::FocusEvent| {
                                                    // Check if the new focus target is still inside this combobox
                                                    if let Some(related) = ev.related_target() {
                                                        if let Some(el) = ev.current_target() {
                                                            let container: web_sys::HtmlElement = el.unchecked_into();
                                                            let related_node: web_sys::Node = related.unchecked_into();
                                                            if container.contains(Some(&related_node)) {
                                                                return;
                                                            }
                                                        }
                                                    }
                                                    country_dropdown_open.set(false);
                                                    country_filter_text.set(country_input.get_untracked());
                                                }
                                            >
                                                <input
                                                    type="text"
                                                    class="xc-input xc-country-input"
                                                    placeholder="All"
                                                    prop:value=move || country_filter_text.get()
                                                    on:input=move |ev| {
                                                        let val = event_target_value(&ev);
                                                        country_filter_text.set(val);
                                                        country_highlight_idx.set(0);
                                                        country_dropdown_open.set(true);
                                                    }
                                                    on:focus=move |_| {
                                                        country_filter_text.set(country_input.get_untracked());
                                                        country_dropdown_open.set(true);
                                                    }
                                                    on:keydown=on_country_keydown
                                                />
                                                <button
                                                    class="xc-combobox-toggle"
                                                    tabindex="-1"
                                                    on:mousedown=move |ev: web_sys::MouseEvent| {
                                                        ev.prevent_default();
                                                        country_dropdown_open.update(|v| *v = !*v);
                                                    }
                                                >
                                                    {"\u{25BE}"}
                                                </button>
                                                {move || country_dropdown_open.get().then(|| {
                                                    let list = filtered_countries.get();
                                                    view! {
                                                        <div class="xc-combobox-dropdown">
                                                            <button
                                                                class=move || if country_input.get().is_empty() {
                                                                    "xc-combobox-option sel"
                                                                } else {
                                                                    "xc-combobox-option"
                                                                }
                                                                on:mousedown=move |ev: web_sys::MouseEvent| {
                                                                    ev.prevent_default();
                                                                    country_input.set(String::new());
                                                                    country_filter_text.set(String::new());
                                                                    country_dropdown_open.set(false);
                                                                }
                                                            >
                                                                "All (no filter)"
                                                            </button>
                                                            {list.into_iter().enumerate().map(|(i, name)| {
                                                                let name_owned = name.to_string();
                                                                let name_for_set = name_owned.clone();
                                                                let name_for_cls = name_owned.clone();
                                                                view! {
                                                                    <button
                                                                        class=move || {
                                                                            let mut cls = "xc-combobox-option".to_string();
                                                                            if country_input.get() == name_for_cls {
                                                                                cls.push_str(" sel");
                                                                            }
                                                                            if country_highlight_idx.get() == i + 1 {
                                                                                cls.push_str(" highlight");
                                                                            }
                                                                            cls
                                                                        }
                                                                        on:mousedown=move |ev: web_sys::MouseEvent| {
                                                                            ev.prevent_default();
                                                                            country_input.set(name_for_set.clone());
                                                                            country_filter_text.set(name_for_set.clone());
                                                                            country_dropdown_open.set(false);
                                                                        }
                                                                    >
                                                                        {name_owned}
                                                                    </button>
                                                                }
                                                            }).collect::<Vec<_>>()}
                                                        </div>
                                                    }
                                                })}
                                            </div>
                                            <button class="xc-btn" on:click=on_load_group>"Go"</button>
                                        </div>
                                        <div class="xc-cache-info">
                                            {move || taxonomy_age.get().map(|age| view! {
                                                <span class="xc-cache-age">{"Cached: "}{age}</span>
                                            })}
                                            <button class="xc-btn xc-btn-small" on:click=on_refresh>"Refresh"</button>
                                        </div>
                                    })
                                } else {
                                    None
                                }
                            }}

                            // Back button for sub-views
                            {move || {
                                if !matches!(view.get(), BrowserView::GroupBrowse | BrowserView::ApiKeyPrompt) {
                                    Some(view! {
                                        <button class="xc-btn xc-btn-back" on:click=on_back>
                                            {"\u{2190} Back to species"}
                                        </button>
                                    })
                                } else {
                                    None
                                }
                            }}
                        </div>
                    })
                }}

                // Loading indicator
                {move || loading.get().then(|| view! {
                    <div class="xc-loading">"Loading..."</div>
                })}

                // Species list (group browse view)
                {move || {
                    if view.get() != BrowserView::GroupBrowse { return None; }
                    let list = species_list.get();
                    if list.is_empty() && !loading.get() { return None; }

                    let count = list.len();
                    Some(view! {
                        <div class="xc-result-summary">{format!("{count} species")}</div>
                        <div class="xc-species-list">
                            <div class="xc-list-header">
                                <span class="xc-col-name">"Species"</span>
                                <span class="xc-col-sci">"Scientific name"</span>
                                <span class="xc-col-count">"Recs"</span>
                            </div>
                            {list.into_iter().map(|sp| {
                                let genus = sp.genus.clone();
                                let species = sp.sp.clone();
                                let en = sp.en.clone();
                                let load_sp = load_species_recordings;
                                view! {
                                    <button
                                        class="xc-species-row"
                                        on:click=move |_| {
                                            load_sp(genus.clone(), species.clone(), en.clone());
                                        }
                                    >
                                        <span class="xc-col-name">{sp.en}</span>
                                        <span class="xc-col-sci">{format!("{} {}", sp.genus, sp.sp)}</span>
                                        <span class="xc-col-count">{sp.recording_count}</span>
                                    </button>
                                }
                            }).collect::<Vec<_>>()}
                        </div>
                    })
                }}

                // Recordings list (species or search view)
                {move || {
                    let current_view = view.get();
                    if !matches!(current_view, BrowserView::SpeciesRecordings { .. } | BrowserView::SearchResults) {
                        return None;
                    }
                    let recs = recordings.get();
                    let title = match &current_view {
                        BrowserView::SpeciesRecordings { en, genus, species } => {
                            format!("{en} ({genus} {species})")
                        }
                        BrowserView::SearchResults => "Search results".to_string(),
                        _ => String::new(),
                    };

                    Some(view! {
                        <div class="xc-recordings-header">
                            {title}
                            <span class="xc-result-count">
                                {move || {
                                    let total = recordings_total.get();
                                    if total > 0 {
                                        format!(" \u{2014} {} recordings", total)
                                    } else {
                                        String::new()
                                    }
                                }}
                            </span>
                        </div>
                        <div class="xc-recordings-list">
                            <div class="xc-rec-header">
                                <span class="xc-rec-id">"ID"</span>
                                <span class="xc-rec-species">"Species"</span>
                                <span class="xc-rec-quality">"Q"</span>
                                <span class="xc-rec-length">"Len"</span>
                                <span class="xc-rec-loc">"Location"</span>
                                <span class="xc-rec-action"></span>
                            </div>
                            {recs.into_iter().map(|rec| {
                                let id = rec.id;
                                let dl = download_and_load;
                                let q_class = match rec.q.as_str() {
                                    "A" => "xc-rec-quality xc-q-a",
                                    "B" => "xc-rec-quality xc-q-b",
                                    "C" => "xc-rec-quality xc-q-c",
                                    "D" => "xc-rec-quality xc-q-d",
                                    "E" => "xc-rec-quality xc-q-e",
                                    _ => "xc-rec-quality",
                                };
                                let has_details = !rec.sound_type.is_empty()
                                    || !rec.smp.is_empty()
                                    || !rec.date.is_empty()
                                    || !rec.rec.is_empty()
                                    || !rec.dvc.is_empty()
                                    || !rec.mic.is_empty();
                                view! {
                                    <div class="xc-rec-row">
                                        <div class="xc-rec-main">
                                            <span class="xc-rec-id">
                                                <a
                                                    href=format!("https://xeno-canto.org/{}", rec.id)
                                                    target="_blank"
                                                    class="xc-rec-link"
                                                    on:click=move |ev: web_sys::MouseEvent| ev.stop_propagation()
                                                >
                                                    {format!("XC{}", rec.id)}
                                                </a>
                                            </span>
                                            <span class="xc-rec-species">{rec.en}</span>
                                            <span class=q_class>{rec.q}</span>
                                            <span class="xc-rec-length">{rec.length}</span>
                                            <span class="xc-rec-loc" title=rec.loc.clone()>{rec.cnt}</span>
                                            <span class="xc-rec-action">
                                                {move || cached_ids.get().contains(&id).then(|| view! {
                                                    <span class="xc-rec-cached" title="Cached locally">{"\u{2713}"}</span>
                                                })}
                                                <button
                                                    class="xc-btn xc-btn-load"
                                                    disabled=move || downloading.get().is_some()
                                                    on:click=move |_| dl(id)
                                                >
                                                    {move || if downloading.get() == Some(id) { "Downloading\u{2026}" } else { "Load" }}
                                                </button>
                                            </span>
                                        </div>
                                        {has_details.then(|| view! {
                                            <div class="xc-rec-detail">
                                                {(!rec.sound_type.is_empty()).then(|| view! {
                                                    <span class="xc-rec-tag">{rec.sound_type}</span>
                                                })}
                                                {(!rec.smp.is_empty()).then(|| view! {
                                                    <span class="xc-rec-tag xc-rec-smp">{format_sample_rate(&rec.smp)}</span>
                                                })}
                                                {(!rec.date.is_empty()).then(|| view! {
                                                    <span class="xc-rec-tag">{rec.date}</span>
                                                })}
                                                {(!rec.rec.is_empty()).then(|| view! {
                                                    <span class="xc-rec-tag xc-rec-recordist">{rec.rec}</span>
                                                })}
                                                {(!rec.dvc.is_empty()).then(|| view! {
                                                    <span class="xc-rec-tag" title="Device">{rec.dvc}</span>
                                                })}
                                                {(!rec.mic.is_empty()).then(|| view! {
                                                    <span class="xc-rec-tag" title="Microphone">{rec.mic}</span>
                                                })}
                                            </div>
                                        })}
                                    </div>
                                }
                            }).collect::<Vec<_>>()}
                        </div>

                        // Pagination
                        {move || {
                            let total = recordings_total_pages.get();
                            if total <= 1 { return None; }
                            Some(view! {
                                <div class="xc-pagination">
                                    <button
                                        class="xc-btn xc-btn-small"
                                        disabled=move || recordings_page.get() <= 1
                                        on:click=move |_| load_recordings_page(recordings_page.get_untracked().saturating_sub(1))
                                    >
                                        {"\u{2190} Prev"}
                                    </button>
                                    <span class="xc-page-info">
                                        {move || format!("Page {} of {}", recordings_page.get(), recordings_total_pages.get())}
                                    </span>
                                    <button
                                        class="xc-btn xc-btn-small"
                                        disabled=move || recordings_page.get() >= recordings_total_pages.get()
                                        on:click=move |_| load_recordings_page(recordings_page.get_untracked() + 1)
                                    >
                                        {"Next \u{2192}"}
                                    </button>
                                </div>
                            })
                        }}
                    })
                }}
            </div>
        </div>
    }
}
