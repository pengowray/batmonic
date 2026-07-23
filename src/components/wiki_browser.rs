//! Import audio from Wikimedia Commons and Wikipedia.
//!
//! Unlike the Xeno-Canto browser next door, this works on every build: the
//! Wikimedia API and media hosts both allow anonymous cross-origin requests,
//! so the web build fetches them directly (see `crate::wikimedia`).
//!
//! One box takes anything: a file page URL, a direct media URL, a `File:…`
//! title, an article URL (whose audio files get listed), or free text to
//! search Commons with. Files declaring a time expansion factor (P14424) are
//! flagged in the list and corrected on load.

use crate::state::store_fields::*;
use crate::state::AppState;
use crate::wikimedia::{self, WikiFile, WikiTarget};
use leptos::prelude::*;
use leptos::task::spawn_local;

fn format_size(bytes: u64) -> String {
    const MB: f64 = 1_000_000.0;
    if bytes >= 1_000_000 {
        format!("{:.1} MB", bytes as f64 / MB)
    } else {
        format!("{} kB", bytes / 1000)
    }
}

#[component]
pub fn WikiBrowser() -> impl IntoView {
    let state = expect_context::<AppState>();

    let input = RwSignal::new(String::new());
    let results: RwSignal<Vec<WikiFile>> = RwSignal::new(Vec::new());
    let searching = RwSignal::new(false);
    let error_msg: RwSignal<Option<String>> = RwSignal::new(None);
    // Title of the file currently downloading, so its row can show progress.
    let loading_title: RwSignal<Option<String>> = RwSignal::new(None);
    let searched = RwSignal::new(false);

    let close = move || state.dialogs.wiki_browser_open().set(false);

    let load_one = move |file: WikiFile| {
        if loading_title.get_untracked().is_some() {
            return;
        }
        loading_title.set(Some(file.title.clone()));
        spawn_local(async move {
            let outcome = wikimedia::load_file(state, &file).await;
            loading_title.set(None);
            if outcome.is_ok() {
                state.dialogs.wiki_browser_open().set(false);
            }
        });
    };

    let submit = move || {
        let text = input.get_untracked().trim().to_string();
        let target = match wikimedia::parse_input(&text) {
            Ok(t) => t,
            Err(e) => {
                error_msg.set(Some(e));
                return;
            }
        };
        searching.set(true);
        error_msg.set(None);
        results.set(Vec::new());
        spawn_local(async move {
            // A link that names one specific file needs no result list —
            // fetch it and load it in a single step.
            let single = matches!(target, WikiTarget::File { .. });
            match wikimedia::resolve(&target).await {
                Ok(files) if files.is_empty() => {
                    error_msg.set(Some(
                        "No audio files found. Wikipedia articles often have none \u{2014} try searching Commons instead.".into(),
                    ));
                }
                Ok(files) => {
                    if single && files.len() == 1 {
                        load_one(files[0].clone());
                    } else {
                        results.set(files);
                    }
                }
                Err(e) => error_msg.set(Some(e)),
            }
            searching.set(false);
            searched.set(true);
        });
    };

    view! {
        <div class="xc-modal-overlay" on:click=move |_| close()>
            <div class="xc-modal" on:click=|ev: web_sys::MouseEvent| ev.stop_propagation()>
                <div class="xc-modal-header">
                    <span class="xc-modal-title">"Import from Wikipedia"</span>
                    <button class="xc-modal-close" on:click=move |_| close()>{"\u{00D7}"}</button>
                </div>

                <div class="xc-section">
                    <p class="xc-info">
                        "Paste a Wikimedia Commons or Wikipedia link \u{2014} a file page, a direct media URL, or an article to list its audio \u{2014} or type words to search "
                        <a href="https://commons.wikimedia.org" target="_blank">"Commons"</a>
                        ". Recordings tagged with a time expansion factor are corrected to their true frequencies on load."
                    </p>
                    <div class="xc-search-bar">
                        <input
                            class="xc-input xc-search-input"
                            r#type="text"
                            placeholder="commons.wikimedia.org/wiki/File:... or \"bat echolocation\""
                            prop:value=move || input.get()
                            on:input=move |ev| input.set(event_target_value(&ev))
                            on:keydown=move |ev: web_sys::KeyboardEvent| {
                                if ev.key() == "Enter" { submit(); }
                            }
                        />
                        <button
                            class="xc-btn"
                            disabled=move || searching.get()
                            on:click=move |_| submit()
                        >
                            {move || if searching.get() { "Searching\u{2026}" } else { "Go" }}
                        </button>
                    </div>
                </div>

                {move || error_msg.get().map(|msg| view! {
                    <div class="xc-error">
                        <span>{msg}</span>
                        <button class="xc-error-dismiss" on:click=move |_| error_msg.set(None)>
                            {"\u{00D7}"}
                        </button>
                    </div>
                })}

                {move || searching.get().then(|| view! {
                    <div class="xc-loading">"Asking Wikimedia\u{2026}"</div>
                })}

                {move || {
                    let files = results.get();
                    (!files.is_empty()).then(|| {
                        let rows = files.into_iter().map(|file| {
                            let for_click = file.clone();
                            let title = file.title.clone();
                            let te_badge = file.time_expansion.map(|te| view! {
                                <span
                                    class="wiki-te-badge"
                                    title="Declared time expansion factor (P14424) \u{2014} corrected on load"
                                >
                                    {wikimedia::format_factor(te)}
                                </span>
                            });
                            let detail = {
                                let mut parts = Vec::new();
                                if let Some(d) = file.corrected_duration_secs() {
                                    parts.push(crate::format_time::format_duration(d, 1));
                                }
                                if file.size > 0 {
                                    parts.push(format_size(file.size));
                                }
                                parts.join(" \u{00B7} ")
                            };
                            view! {
                                <div class="wiki-row">
                                    <div class="wiki-row-main">
                                        <span class="wiki-row-name">{file.name.clone()}</span>
                                        {te_badge}
                                    </div>
                                    <span class="wiki-row-detail">{detail}</span>
                                    <a
                                        class="wiki-row-link"
                                        href=file.description_url.clone()
                                        target="_blank"
                                        title="Open the file page on Wikimedia"
                                        on:click=|ev: web_sys::MouseEvent| ev.stop_propagation()
                                    >"info"</a>
                                    <button
                                        class="xc-btn xc-btn-small xc-btn-load"
                                        disabled=move || loading_title.get().is_some()
                                        on:click=move |_| load_one(for_click.clone())
                                    >
                                        {move || if loading_title.get().as_deref() == Some(title.as_str()) {
                                            "Loading\u{2026}"
                                        } else {
                                            "Load"
                                        }}
                                    </button>
                                </div>
                            }
                        }).collect::<Vec<_>>();
                        view! { <div class="xc-recordings-list">{rows}</div> }
                    })
                }}

                {move || (searched.get() && !searching.get() && results.get().is_empty()
                    && error_msg.get().is_none()).then(|| view! {
                    <div class="xc-info">"Nothing to show yet."</div>
                })}
            </div>
        </div>
    }
}
