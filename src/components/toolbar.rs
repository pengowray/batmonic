use leptos::prelude::*;
use crate::state::{AppState, RightSidebarTab, ListenMode};

/// Parse a CC license URL/string (from XC metadata "lic" field) into a short label.
/// e.g. "//creativecommons.org/licenses/by-nc-sa/4.0/" -> "CC BY-NC-SA 4.0"
fn parse_cc_license(lic: &str) -> Option<String> {
    let lower = lic.to_lowercase();
    // Match URLs like //creativecommons.org/licenses/by-nc-sa/4.0/
    // or text like "CC BY-NC-SA 4.0" or "CC-BY-NC 4.0"
    if lower.contains("creativecommons.org/licenses/") {
        // Extract the path part after /licenses/
        if let Some(idx) = lower.find("/licenses/") {
            let rest = &lic[idx + 10..]; // after "/licenses/"
            let parts: Vec<&str> = rest.trim_matches('/').split('/').collect();
            if parts.len() >= 2 {
                let license_type = parts[0].to_uppercase();
                let version = parts[1];
                return Some(format!("CC {} {}", license_type, version));
            } else if !parts.is_empty() {
                return Some(format!("CC {}", parts[0].to_uppercase()));
            }
        }
    }
    // Already in short form like "CC BY-NC-SA 4.0" or "CC-BY-NC 4.0"
    if lower.starts_with("cc") {
        return Some(lic.to_string());
    }
    None
}

/// Get XC metadata field value by key from the loaded file's metadata pairs.
fn get_xc_field(metadata: &[(String, String)], key: &str) -> Option<String> {
    metadata.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
}

#[component]
pub fn Toolbar() -> impl IntoView {
    let state = expect_context::<AppState>();
    let show_about = RwSignal::new(false);

    let is_mobile = state.is_mobile.get_untracked();

    // Derived: current file name
    let file_name = Memo::new(move |_| {
        let files = state.files.get();
        let idx = state.current_file_index.get()?;
        files.get(idx).map(|f| f.name.clone())
    });

    // Derived: XC metadata of current file
    let xc_metadata = Memo::new(move |_| {
        let files = state.files.get();
        let idx = state.current_file_index.get()?;
        files.get(idx).and_then(|f| f.xc_metadata.clone())
    });

    // Derived: CC license info (short label)
    let cc_license = Memo::new(move |_| {
        let meta = xc_metadata.get()?;
        let lic = get_xc_field(&meta, "License")?;
        parse_cc_license(&lic)
    });

    // Derived: attribution text for tooltip
    let attribution = Memo::new(move |_| {
        let meta = xc_metadata.get()?;
        get_xc_field(&meta, "Attribution")
    });

    // Derived: status prefix (recording/playing/listening indicators)
    let status_prefix = Memo::new(move |_| {
        let recording = state.mic_recording.get();
        let listening = state.mic_listening.get();
        let playing = state.is_playing.get();

        let mut parts = Vec::new();

        if recording {
            parts.push("\u{1F534}"); // 🔴
        }

        if listening {
            let listen_mode = state.listen_mode.get();
            if listen_mode == ListenMode::Normal {
                parts.push("\u{1F442}"); // 👂 (ear — 1:1 passthrough)
            } else {
                parts.push("\u{1F987}"); // 🦇 (bat — HFR processing)
            }
        } else if playing && !recording {
            parts.push("\u{25B6}\u{FE0F}"); // ▶️
        }

        if parts.is_empty() {
            None
        } else {
            Some(parts.join(""))
        }
    });

    // Derived: recording file name when both recording and listening
    let recording_file_name = Memo::new(move |_| {
        let recording = state.mic_recording.get();
        let listening = state.mic_listening.get();
        if recording && listening {
            // Show recording file name
            let files = state.files.get();
            state.mic_live_file_idx.get()
                .and_then(|idx| files.get(idx).map(|f| f.name.clone()))
                .or_else(|| file_name.get())
        } else if recording {
            // Just recording — show normal file name or recording name
            let files = state.files.get();
            state.mic_live_file_idx.get()
                .and_then(|idx| files.get(idx).map(|f| f.name.clone()))
                .or_else(|| file_name.get())
        } else {
            None
        }
    });

    // Derived: full title text (for both recording+listening and normal states)
    let center_text = Memo::new(move |_| {
        let recording = state.mic_recording.get();
        let listening = state.mic_listening.get();

        if recording && listening {
            // Show recording file name (not "Listening...")
            recording_file_name.get().unwrap_or_default()
        } else if listening {
            "Listening...".to_string()
        } else if recording {
            recording_file_name.get().unwrap_or_default()
        } else {
            file_name.get().unwrap_or_default()
        }
    });

    // Effect: update document title
    Effect::new(move |_| {
        let prefix = status_prefix.get();
        let name = file_name.get();
        let recording = state.mic_recording.get();
        let listening = state.mic_listening.get();

        let title = match (prefix.as_deref(), listening && !recording, name.as_deref()) {
            (Some(pfx), true, _) => format!("{} Listening... - Batmonic", pfx),
            (Some(pfx), false, Some(name)) => format!("{} {} - Batmonic", pfx, name),
            (Some(pfx), false, None) => format!("{} Batmonic", pfx),
            (None, _, Some(name)) => format!("{} - Batmonic", name),
            (None, _, None) => "Batmonic".to_string(),
        };

        if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
            doc.set_title(&title);
        }
    });

    view! {
        <div class="toolbar">
            // Left: mobile menu + brand
            {if is_mobile {
                Some(view! {
                    <button
                        class="toolbar-menu-btn"
                        on:click=move |ev: web_sys::MouseEvent| {
                            ev.stop_propagation();
                            state.sidebar_collapsed.update(|c| *c = !*c);
                        }
                        title="Menu"
                    >"\u{2630}"</button>
                })
            } else {
                None
            }}
            <span
                class="toolbar-brand"
                style=move || if !is_mobile && state.sidebar_collapsed.get() { "margin-left: 24px; cursor: pointer" } else { "cursor: pointer" }
                on:click=move |_| show_about.set(true)
                title="About"
            ><b>"Batmonic"</b></span>

            // Center: status indicators + file name (or "Listening...")
            <div class="toolbar-title-center">
                // Status indicators (recording dot, play icon, listen icon)
                {move || status_prefix.get().map(|pfx| view! {
                    <span class="toolbar-status-icons">{pfx}</span>
                })}

                // File name (with middle ellipsis via CSS)
                <span
                    class="toolbar-title-filename"
                    title=move || {
                        let name = file_name.get().unwrap_or_default();
                        if name.is_empty() { String::new() } else { name }
                    }
                >
                    {move || center_text.get()}
                </span>

                // Info / CC license button (right side of center)
                {move || {
                    let has_file = file_name.get().is_some();
                    if !has_file {
                        return None;
                    }

                    let cc = cc_license.get();
                    let attr = attribution.get();

                    if let Some(cc_label) = cc {
                        // CC license badge — clicking opens info panel
                        let tooltip = attr.unwrap_or_else(|| cc_label.clone());
                        Some(leptos::either::Either::Left(view! {
                            <button
                                class="toolbar-cc-badge"
                                title=tooltip
                                on:click=move |_| {
                                    state.right_sidebar_tab.set(RightSidebarTab::Metadata);
                                    state.right_sidebar_collapsed.set(false);
                                }
                            >
                                <span class="toolbar-cc-icon">"CC"</span>
                                <span class="toolbar-cc-label">{cc_label}</span>
                            </button>
                        }))
                    } else {
                        // Plain info icon
                        let title_str = "File info".to_string();
                        Some(leptos::either::Either::Right(view! {
                            <button
                                class="toolbar-info-btn"
                                title=title_str
                                on:click=move |_| {
                                    state.right_sidebar_tab.set(RightSidebarTab::Metadata);
                                    state.right_sidebar_collapsed.set(false);
                                }
                            >
                                {"\u{24D8}"} // ⓘ
                            </button>
                        }))
                    }
                }}
            </div>

            // Undo/Redo buttons
            <div class="toolbar-undo-redo">
                <button
                    class="toolbar-undo-btn"
                    title="Undo (Ctrl+Z)"
                    on:click=move |_| state.undo_annotations()
                    disabled=move || !state.can_undo()
                >{"\u{21B6}"}</button>
                <button
                    class="toolbar-undo-btn"
                    title="Redo (Ctrl+Shift+Z)"
                    on:click=move |_| state.redo_annotations()
                    disabled=move || !state.can_redo()
                >{"\u{21B7}"}</button>
            </div>

            // Right sidebar button (mobile only)
            {if is_mobile {
                Some(view! {
                    <button
                        class="toolbar-menu-btn"
                        on:click=move |ev: web_sys::MouseEvent| {
                            ev.stop_propagation();
                            state.right_sidebar_collapsed.update(|c| *c = !*c);
                            // Close left sidebar when opening right
                            if !state.right_sidebar_collapsed.get_untracked() {
                                state.sidebar_collapsed.set(true);
                            }
                        }
                        title="Info panel"
                    >"\u{2630}"</button>
                })
            } else {
                None
            }}

            {move || show_about.get().then(|| view! {
                <div class="about-overlay" on:click=move |_| show_about.set(false)>
                    <div class="about-dialog" on:click=move |ev: web_sys::MouseEvent| ev.stop_propagation()>
                        <div class="about-header">
                            <span class="about-title"><b>"Batmonic"</b>" by Pengo Wray"</span>
                            <span class="about-version">{concat!("v", env!("CARGO_PKG_VERSION"))}</span>
                        </div>
                        <p class="about-desc">"Bat call viewer and acoustic analysis tool."</p>
                        <div style="margin-top: 12px; font-size: 11px; color: #999; line-height: 1.8;">
                            "Thanks to the libraries that make this possible:"
                            <div style="margin-top: 6px; columns: 2; column-gap: 16px;">
                                <div><a href="https://leptos.dev" target="_blank" style="color: #8cf; text-decoration: none;">"Leptos"</a>""</div>
                                <div><a href="https://crates.io/crates/realfft" target="_blank" style="color: #8cf; text-decoration: none;">"RealFFT"</a></div>
                                <div><a href="https://crates.io/crates/hound" target="_blank" style="color: #8cf; text-decoration: none;">"Hound"</a></div>
                                <div><a href="https://crates.io/crates/claxon" target="_blank" style="color: #8cf; text-decoration: none;">"Claxon"</a></div>
                                <div><a href="https://crates.io/crates/lewton" target="_blank" style="color: #8cf; text-decoration: none;">"Lewton"</a></div>
                                <div><a href="https://crates.io/crates/symphonia" target="_blank" style="color: #8cf; text-decoration: none;">"Symphonia"</a></div>
                                <div><a href="https://github.com/jmears63/batgizmo-app-public" target="_blank" style="color: #8cf; text-decoration: none;">"batgizmo-app"</a></div>
                            </div>
                        </div>
                        <button class="about-close" on:click=move |_| show_about.set(false)>"Close"</button>
                    </div>
                </div>
            })}
        </div>
    }
}
