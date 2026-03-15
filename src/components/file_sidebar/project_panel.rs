use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::JsCast;
use crate::state::AppState;
use crate::project::BatProject;
use crate::project_store;
use crate::annotations::AudioFileMetadata;
use crate::opfs;

/// Helper: build AudioFileMetadata from a LoadedFile.
fn audio_meta_from_loaded(f: &crate::state::LoadedFile) -> AudioFileMetadata {
    AudioFileMetadata {
        sample_rate: f.audio.sample_rate,
        total_samples: f.audio.source.total_samples(),
        channels: f.audio.channels,
        duration_secs: f.audio.duration_secs,
        format: f.audio.metadata.format.to_string(),
        bits_per_sample: Some(f.audio.metadata.bits_per_sample),
        data_offset: f.audio.metadata.data_offset,
        data_size: f.audio.metadata.data_size,
    }
}

/// Save the current project to OPFS (fire-and-forget).
pub(crate) fn save_project_async(state: AppState) {
    let proj = state.current_project.get_untracked();
    if let Some(proj) = proj {
        spawn_local(async move {
            match project_store::save_project(&proj).await {
                Ok(()) => state.project_dirty.set(false),
                Err(e) => log::error!("Failed to save project: {e}"),
            }
        });
    }
}

#[component]
pub fn ProjectPanel() -> impl IntoView {
    let state = expect_context::<AppState>();

    view! {
        <div class="project-panel">
            {move || {
                let project = state.current_project.get();
                match project {
                    Some(proj) => view! { <ProjectView project=proj /> }.into_any(),
                    None => view! { <NoProjectView /> }.into_any(),
                }
            }}
        </div>
    }
}

// ─── No project open ────────────────────────────────────────────────────────

#[component]
fn NoProjectView() -> impl IntoView {
    let state = expect_context::<AppState>();
    let project_list: RwSignal<Option<Vec<(String, Option<String>)>>> = RwSignal::new(None);
    let loading_list = RwSignal::new(false);

    let on_create = move |_: web_sys::MouseEvent| {
        let files = state.files.get_untracked();
        let mut proj = BatProject::new();
        for f in files.iter() {
            if let Some(ref identity) = f.identity {
                proj.add_file(identity.clone(), Some(audio_meta_from_loaded(f)));
            }
        }
        state.current_project.set(Some(proj.clone()));
        state.project_dirty.set(true);
        save_project_async(state);
    };

    let on_load_click = move |_: web_sys::MouseEvent| {
        // Toggle the project list picker
        if project_list.get_untracked().is_some() {
            project_list.set(None);
            return;
        }
        loading_list.set(true);
        spawn_local(async move {
            match project_store::list_projects().await {
                Ok(projects) => {
                    if projects.is_empty() {
                        log::info!("No saved projects found");
                        project_list.set(Some(Vec::new()));
                    } else {
                        project_list.set(Some(projects));
                    }
                }
                Err(e) => log::error!("Failed to list projects: {e}"),
            }
            loading_list.set(false);
        });
    };

    // Import .batproj from file
    let import_ref = NodeRef::<leptos::html::Input>::new();
    let on_import_click = move |_: web_sys::MouseEvent| {
        if let Some(input) = import_ref.get() {
            let el: &web_sys::HtmlInputElement = input.as_ref();
            el.click();
        }
    };
    let on_import_change = move |ev: web_sys::Event| {
        let target = ev.target().unwrap();
        let input: web_sys::HtmlInputElement = target.unchecked_into();
        let Some(file_list) = input.files() else { return };
        let Some(file) = file_list.get(0) else { return };
        spawn_local(async move {
            let text_promise = file.text();
            let text = wasm_bindgen_futures::JsFuture::from(text_promise).await;
            match text {
                Ok(val) => {
                    if let Some(yaml_str) = val.as_string() {
                        match yaml_serde::from_str::<BatProject>(&yaml_str) {
                            Ok(proj) => {
                                state.current_project.set(Some(proj.clone()));
                                state.project_dirty.set(false);
                                // Save to OPFS so it persists
                                if let Err(e) = project_store::save_project(&proj).await {
                                    log::error!("Failed to save imported project: {e}");
                                }
                            }
                            Err(e) => log::error!("Failed to parse .batproj: {e}"),
                        }
                    }
                }
                Err(e) => log::error!("Failed to read file: {e:?}"),
            }
        });
        input.set_value("");
    };

    let has_files = move || !state.files.with(|f| f.is_empty());

    view! {
        <div class="project-panel-empty">
            <p>"No project open"</p>
            <p class="project-panel-hint">"Create a project to group files, define sequences, and share settings."</p>
            <input
                node_ref=import_ref
                type="file"
                accept=".batproj,.yaml,.yml"
                style="display:none"
                on:change=on_import_change
            />
            <div class="project-panel-actions">
                <button class="project-btn" on:click=on_create
                    disabled=move || !has_files()
                    title="Create a new project from loaded files"
                >"Create project"</button>
                <button class="project-btn project-btn-secondary" on:click=on_load_click
                    title="Load a previously saved project"
                >{move || if loading_list.get() { "Loading..." } else { "Load project" }}</button>
                <button class="project-btn project-btn-secondary" on:click=on_import_click
                    title="Import a .batproj file"
                >"Import .batproj"</button>
            </div>
            {move || {
                let list = project_list.get();
                match list {
                    None => view! { <span></span> }.into_any(),
                    Some(projects) if projects.is_empty() => {
                        view! { <p class="project-panel-hint">"No saved projects found."</p> }.into_any()
                    }
                    Some(projects) => {
                        let items: Vec<_> = projects.iter().map(|(id, name)| {
                            let id = id.clone();
                            let display = name.clone().unwrap_or_else(|| format!("Untitled ({}...)", &id[..8.min(id.len())]));
                            view! {
                                <button class="project-list-item" on:click=move |_| {
                                    let id = id.clone();
                                    spawn_local(async move {
                                        match project_store::load_project(&id).await {
                                            Ok(Some(proj)) => {
                                                state.current_project.set(Some(proj));
                                                state.project_dirty.set(false);
                                                project_list.set(None);
                                            }
                                            Ok(None) => log::warn!("Project {id} not found"),
                                            Err(e) => log::error!("Failed to load project: {e}"),
                                        }
                                    });
                                }>{display}</button>
                            }
                        }).collect();
                        view! { <div class="project-list-picker">{items}</div> }.into_any()
                    }
                }
            }}
        </div>
    }
}

// ─── Project open ───────────────────────────────────────────────────────────

#[component]
fn ProjectView(project: BatProject) -> impl IntoView {
    let state = expect_context::<AppState>();
    let proj_name = project.name.clone().unwrap_or_default();
    let file_count = project.files.len();
    let created = project.created_at.clone().unwrap_or_default();
    let has_tauri_metadata = project.files.iter().any(|f| f.metadata_from_tauri);
    let notes_text = project.notes.clone().unwrap_or_default();
    let seq_count = project.sequences.len();
    let mt_count = project.multitrack_groups.len();
    let timeline_count = project.timelines.len();
    let timelines_clone = project.timelines.clone();

    // Track which project files are currently loaded
    let loaded_files = state.files.get_untracked();
    let file_statuses: Vec<(crate::project::ProjectFile, bool)> = project.files.iter().map(|pf| {
        let is_loaded = loaded_files.iter().any(|lf| {
            lf.identity.as_ref().map_or(false, |id| {
                if let (Some(a), Some(b)) = (&pf.identity.spot_hash_b3, &id.spot_hash_b3) {
                    a == b
                } else {
                    pf.identity.filename == id.filename && pf.identity.file_size == id.file_size
                }
            })
        });
        (pf.clone(), is_loaded)
    }).collect();
    let loaded_count = file_statuses.iter().filter(|(_, loaded)| *loaded).count();

    // Check for loaded files not yet in the project
    let new_files_count = {
        let proj_clone = project.clone();
        loaded_files.iter().filter(|lf| {
            lf.identity.as_ref().map_or(false, |id| proj_clone.find_file(id).is_none())
        }).count()
    };

    // ── Event handlers ──

    let on_name_change = move |ev: web_sys::Event| {
        let target = ev.target().unwrap();
        let input: web_sys::HtmlInputElement = target.unchecked_into();
        let new_name = input.value();
        state.current_project.update(|p| {
            if let Some(proj) = p {
                proj.name = if new_name.is_empty() { None } else { Some(new_name) };
                proj.touch();
            }
        });
        state.project_dirty.set(true);
    };

    let on_notes_change = move |ev: web_sys::Event| {
        let target = ev.target().unwrap();
        let textarea: web_sys::HtmlTextAreaElement = target.unchecked_into();
        let new_notes = textarea.value();
        state.current_project.update(|p| {
            if let Some(proj) = p {
                proj.notes = if new_notes.is_empty() { None } else { Some(new_notes) };
                proj.touch();
            }
        });
        state.project_dirty.set(true);
    };

    let on_save = move |_: web_sys::MouseEvent| { save_project_async(state); };

    let on_export = move |_: web_sys::MouseEvent| {
        let proj = state.current_project.get_untracked();
        if let Some(proj) = proj {
            match project_store::export_project_yaml(&proj) {
                Ok(yaml) => {
                    download_text(&yaml, &format!("{}.batproj",
                        proj.name.as_deref().unwrap_or("project")));
                }
                Err(e) => log::error!("Failed to export project: {e}"),
            }
        }
    };

    let on_close = move |_: web_sys::MouseEvent| {
        state.current_project.set(None);
        state.project_dirty.set(false);
    };

    // Merge .batm sidecars from loaded files into the project
    let merge_status: RwSignal<Option<String>> = RwSignal::new(None);
    let on_merge_batm = move |_: web_sys::MouseEvent| {
        merge_status.set(Some("Scanning...".to_string()));
        spawn_local(async move {
            let loaded = state.files.get_untracked();
            let mut merged_count = 0u32;
            let mut skipped = 0u32;

            for f in loaded.iter() {
                let Some(ref identity) = f.identity else { continue };
                let key = opfs::opfs_key(identity);

                // Check if already merged
                let already = state.current_project.with_untracked(|p| {
                    p.as_ref().map_or(false, |proj| proj.was_merged(&key))
                });
                if already { skipped += 1; continue; }

                // Try to load the .batm
                match opfs::load_batm_by_key(&key).await {
                    Ok(Some(set)) => {
                        let did_merge = state.current_project.try_update(|p| {
                            p.as_mut().map_or(false, |proj| proj.merge_batm(&set, &key))
                        }).unwrap_or(false);
                        if did_merge {
                            merged_count += 1;
                            state.project_dirty.set(true);
                        }
                    }
                    Ok(None) => {} // No sidecar for this file
                    Err(e) => log::warn!("Failed to load .batm {key}: {e}"),
                }
            }

            let msg = if merged_count > 0 {
                format!("Merged {merged_count} sidecar(s)")
            } else if skipped > 0 {
                "Already merged".to_string()
            } else {
                "No sidecars found".to_string()
            };
            merge_status.set(Some(msg));
        });
    };

    let merge_count = project.merge_history.len();

    // Add new loaded files that aren't in the project yet
    let on_sync_files = move |_: web_sys::MouseEvent| {
        let loaded = state.files.get_untracked();
        state.current_project.update(|p| {
            let Some(proj) = p else { return };
            for f in loaded.iter() {
                if let Some(ref identity) = f.identity {
                    if proj.find_file(identity).is_none() {
                        proj.add_file(identity.clone(), Some(audio_meta_from_loaded(f)));
                    }
                }
            }
        });
        state.project_dirty.set(true);
    };

    // ── Build file items ──

    let file_items: Vec<_> = file_statuses.iter().map(|(pf, is_loaded)| {
        let filename = pf.identity.filename.clone();
        let duration = pf.audio_metadata.as_ref().map(|m| m.duration_secs);
        let sample_rate = pf.audio_metadata.as_ref().map(|m| m.sample_rate);
        let has_annotations = !pf.annotations.is_empty();
        let has_noise = pf.noise_profile.is_some();
        let time_offset = pf.time_offset_secs;
        let from_tauri = pf.metadata_from_tauri;
        let loaded = *is_loaded;

        let status_cls = if loaded { "project-file-item" } else { "project-file-item missing" };

        view! {
            <div class=status_cls>
                <div class="project-file-name">
                    {if !loaded {
                        Some(view! { <span class="project-file-status" title="Not loaded">{"\u{25CB} "}</span> })
                    } else {
                        Some(view! { <span class="project-file-status loaded" title="Loaded">{"\u{25CF} "}</span> })
                    }}
                    {filename}
                    {if from_tauri {
                        Some(view! { <span class="file-badge file-badge-tauri" title="Metadata from desktop">"T"</span> })
                    } else {
                        None
                    }}
                </div>
                <div class="project-file-info">
                    {duration.map(|d| format!("{d:.1}s")).unwrap_or_default()}
                    {sample_rate.map(|sr| format!("  {}kHz", sr / 1000)).unwrap_or_default()}
                    {if has_annotations { " annot." } else { "" }}
                    {if has_noise { " NR" } else { "" }}
                    {if time_offset != 0.0 { format!("  offset {time_offset:+.1}s") } else { String::new() }}
                </div>
            </div>
        }
    }).collect();

    let created_short = created.get(..10).unwrap_or(&created).to_string();

    // ── View ──

    view! {
        <div class="project-view">
            // Name
            <div class="project-name-row">
                <input
                    type="text"
                    class="project-name-input"
                    value=proj_name
                    placeholder="Project name"
                    on:change=on_name_change
                />
            </div>

            // Meta line
            <div class="project-meta">
                <span>{format!("{file_count} file(s)")}</span>
                {if loaded_count < file_count {
                    Some(view! { <span class="project-meta-warn">{format!(" ({} missing)", file_count - loaded_count)}</span> })
                } else {
                    None
                }}
                <span class="project-meta-sep">{"\u{00B7}"}</span>
                <span title=format!("Created: {created}")>{created_short}</span>
                {move || {
                    if state.project_dirty.get() {
                        Some(view! { <span class="project-unsaved">" (unsaved)"</span> })
                    } else {
                        None
                    }
                }}
            </div>

            // Detected groupings
            {if seq_count > 0 || mt_count > 0 {
                let parts: Vec<String> = [
                    if seq_count > 0 { Some(format!("{seq_count} sequence(s)")) } else { None },
                    if mt_count > 0 { Some(format!("{mt_count} multitrack group(s)")) } else { None },
                ].into_iter().flatten().collect();
                Some(view! {
                    <div class="project-groupings">
                        {parts.join(", ")}
                    </div>
                })
            } else {
                None
            }}

            // Tauri hint
            {if !state.is_tauri && !has_tauri_metadata {
                Some(view! {
                    <div class="project-tauri-hint">
                        "Open in desktop app to enrich with file system dates."
                    </div>
                })
            } else {
                None
            }}

            // New files banner
            {if new_files_count > 0 {
                Some(view! {
                    <div class="project-new-files-banner">
                        <span>{format!("{new_files_count} loaded file(s) not in project")}</span>
                        <button class="project-btn-inline" on:click=on_sync_files>"Add to project"</button>
                    </div>
                })
            } else {
                None
            }}

            // File list
            <div class="project-file-list">
                <div class="project-section-header">"Files"</div>
                {file_items}
            </div>

            // Timelines
            {if timeline_count > 0 || seq_count > 0 {
                let timeline_items: Vec<_> = timelines_clone.iter().map(|tl| {
                    let label = tl.label.clone().unwrap_or_else(|| format!("Timeline ({})", tl.entries.len()));
                    let entry_count = tl.entries.len();
                    view! {
                        <div class="project-file-item">
                            <div class="project-file-name">{label}</div>
                            <div class="project-file-info">{format!("{} files", entry_count)}</div>
                        </div>
                    }
                }).collect();
                Some(view! {
                    <div class="project-timelines-section">
                        <div class="project-section-header">"Timelines"</div>
                        {if timeline_items.is_empty() {
                            Some(view! {
                                <div class="project-panel-hint" style="margin: 4px 0;">
                                    "Select files in the Files tab and click Create Timeline."
                                </div>
                            })
                        } else {
                            None
                        }}
                        {timeline_items}
                    </div>
                })
            } else {
                None
            }}

            // Notes
            <div class="project-notes-section">
                <div class="project-section-header">"Notes"</div>
                <textarea
                    class="project-notes-input"
                    placeholder="Project notes..."
                    on:change=on_notes_change
                >{notes_text}</textarea>
            </div>

            // Merge .batm sidecars
            <div class="project-merge-section">
                <div class="project-section-header">"Annotations"</div>
                <div class="project-merge-row">
                    <button class="project-btn-inline" on:click=on_merge_batm
                        title="Import annotations from .batm sidecar files into this project"
                    >"Merge .batm sidecars"</button>
                    {if merge_count > 0 {
                        Some(view! { <span class="project-merge-count">{format!("{merge_count} merged")}</span> })
                    } else {
                        None
                    }}
                </div>
                {move || {
                    merge_status.get().map(|msg| view! {
                        <div class="project-merge-status">{msg}</div>
                    })
                }}
            </div>

            // Actions
            <div class="project-panel-actions">
                <button class="project-btn" on:click=on_save
                    disabled=move || !state.project_dirty.get()
                >"Save"</button>
                <button class="project-btn project-btn-secondary" on:click=on_export>"Export"</button>
                <button class="project-btn project-btn-danger" on:click=on_close>"Close"</button>
            </div>
        </div>
    }
}

// ─── Helpers ────────────────────────────────────────────────────────────────

/// Trigger a browser file download with text content.
fn download_text(content: &str, filename: &str) {
    let Some(window) = web_sys::window() else { return };
    let Some(document) = window.document() else { return };

    let blob_parts = js_sys::Array::new();
    blob_parts.push(&wasm_bindgen::JsValue::from_str(content));
    let Ok(blob) = web_sys::Blob::new_with_str_sequence(&blob_parts) else { return };
    let Ok(url) = web_sys::Url::create_object_url_with_blob(&blob) else { return };

    let Ok(a) = document.create_element("a") else { return };
    let _ = a.set_attribute("href", &url);
    let _ = a.set_attribute("download", filename);
    let _ = a.set_attribute("style", "display:none");
    let Some(body) = document.body() else { return };
    let _ = body.append_child(&a);
    if let Some(el) = a.dyn_ref::<web_sys::HtmlElement>() {
        el.click();
    }
    let _ = body.remove_child(&a);
    let _ = web_sys::Url::revoke_object_url(&url);
}
