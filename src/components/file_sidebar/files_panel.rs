use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::{Clamped, JsCast};
use web_sys::{CanvasRenderingContext2d, DragEvent, HtmlCanvasElement, HtmlInputElement, ImageData, MouseEvent};
use crate::audio::playback;
use crate::audio::microphone;
use crate::audio::streaming_source;
use crate::canvas::tile_cache;
use crate::state::{AppState, FileSortMode, LoadedFile};
use crate::types::PreviewImage;

use super::file_groups;

use super::loading::{read_and_load_file, DemoEntry, fetch_demo_index, load_single_demo};

#[component]
pub(super) fn FilesPanel() -> impl IntoView {
    let state = expect_context::<AppState>();
    let is_mobile = state.is_mobile.get_untracked();
    let drag_over = RwSignal::new(false);
    let files = state.files;
    let current_idx = state.current_file_index;
    let loading_count = state.loading_count;

    let on_dragenter = move |ev: DragEvent| {
        ev.prevent_default();
        drag_over.set(true);
    };

    let on_dragover = move |ev: DragEvent| {
        ev.prevent_default();
        drag_over.set(true);
    };

    let on_dragleave = move |_: DragEvent| {
        drag_over.set(false);
    };

    let file_input_ref = NodeRef::<leptos::html::Input>::new();

    let state_for_upload = state.clone();
    let on_upload_click = move |_: web_sys::MouseEvent| {
        if let Some(input) = file_input_ref.get() {
            let el: &HtmlInputElement = input.as_ref();
            el.click();
        }
    };

    let on_file_input_change = move |ev: web_sys::Event| {
        let target = ev.target().unwrap();
        let input: HtmlInputElement = target.unchecked_into();
        let Some(file_list) = input.files() else { return };

        for i in 0..file_list.length() {
            let Some(file) = file_list.get(i) else { continue };
            let state = state_for_upload.clone();
            state.loading_count.update(|c| *c += 1);
            spawn_local(async move {
                match read_and_load_file(file, state.clone()).await {
                    Ok(()) => {}
                    Err(e) => log::error!("Failed to load file: {e}"),
                }
                state.loading_count.update(|c| *c = c.saturating_sub(1));
            });
        }

        // Reset the input so the same file can be re-selected
        input.set_value("");
    };

    let demo_entries: RwSignal<Vec<DemoEntry>> = RwSignal::new(Vec::new());
    let demo_picker_open = RwSignal::new(false);
    let demo_loading = RwSignal::new(false);

    let on_demo_click = move |_: web_sys::MouseEvent| {
        if demo_picker_open.get_untracked() {
            demo_picker_open.set(false);
            return;
        }
        if !demo_entries.get_untracked().is_empty() {
            demo_picker_open.set(true);
            return;
        }
        // Fetch the index
        demo_loading.set(true);
        spawn_local(async move {
            match fetch_demo_index().await {
                Ok(entries) => {
                    demo_entries.set(entries);
                    demo_picker_open.set(true);
                }
                Err(e) => log::error!("Failed to fetch demo index: {e}"),
            }
            demo_loading.set(false);
        });
    };


    let state_for_drop = state.clone();
    let on_drop = move |ev: DragEvent| {
        ev.prevent_default();
        drag_over.set(false);

        let Some(dt) = ev.data_transfer() else {
            log::warn!("Drop: no DataTransfer");
            return;
        };
        let Some(file_list) = dt.files() else {
            log::warn!("Drop: no files in DataTransfer");
            return;
        };

        log::info!("Drop: {} file(s)", file_list.length());

        for i in 0..file_list.length() {
            let Some(file) = file_list.get(i) else { continue };
            let state = state_for_drop.clone();
            state.loading_count.update(|c| *c += 1);
            spawn_local(async move {
                match read_and_load_file(file, state.clone()).await {
                    Ok(()) => {}
                    Err(e) => log::error!("Failed to load file: {e}"),
                }
                state.loading_count.update(|c| *c = c.saturating_sub(1));
            });
        }
    };

    view! {
        <div
            class=move || if drag_over.get() { "drop-zone drag-over" } else { "drop-zone" }
            on:dragenter=on_dragenter
            on:dragover=on_dragover
            on:dragleave=on_dragleave
            on:drop=on_drop
        >
            <input
                node_ref=file_input_ref
                type="file"
                accept=".wav,.flac,.mp3,.ogg"
                multiple=true
                style="display:none"
                on:change=on_file_input_change
            />
            {move || {
                let file_vec = files.get();
                let lc = loading_count.get();
                if file_vec.is_empty() && lc == 0 {
                    view! {
                        <div class="drop-hint">
                            {if !is_mobile { Some("Drop audio files here") } else { None }}
                            <button class="upload-btn" on:click=on_upload_click>"Browse files"</button>
                            <button class="upload-btn demo-btn" on:click=on_demo_click>
                                {move || if demo_loading.get() { "Loading..." } else { "Load demo" }}
                            </button>
                            {if state.is_tauri {
                                Some(view! {
                                    <button class="upload-btn xc-btn" on:click=move |_| {
                                        state.xc_browser_open.set(true);
                                    }>"Explore XC"</button>
                                })
                            } else {
                                None
                            }}
                            {move || {
                                if demo_picker_open.get() {
                                    let entries = demo_entries.get();
                                    let items: Vec<_> = entries.iter().map(|entry| {
                                        let entry_clone = entry.clone();
                                        let display_name = entry.filename
                                            .trim_end_matches(".wav")
                                            .trim_end_matches(".flac")
                                            .to_string();
                                        view! {
                                            <button
                                                class="demo-item"
                                                on:click=move |_| {
                                                    let entry = entry_clone.clone();
                                                    state.loading_count.update(|c| *c += 1);
                                                    spawn_local(async move {
                                                        match load_single_demo(&entry, state).await {
                                                            Ok(()) => {}
                                                            Err(e) => log::error!("Failed to load demo sound: {e}"),
                                                        }
                                                        state.loading_count.update(|c| *c = c.saturating_sub(1));
                                                    });
                                                }
                                            >
                                                {display_name}
                                            </button>
                                        }
                                    }).collect();
                                    view! {
                                        <div class="demo-picker">{items}</div>
                                    }.into_any()
                                } else {
                                    view! { <span></span> }.into_any()
                                }
                            }}
                        </div>
                    }.into_any()
                } else {
                    let is_tauri = state.is_tauri;
                    let names: Vec<String> = file_vec.iter().map(|f| f.name.clone()).collect();
                    let groups = file_groups::compute_file_groups(&names);
                    let active_group_key: Option<String> = current_idx.get()
                        .and_then(|idx| groups.get(idx))
                        .and_then(|g| g.as_ref())
                        .map(|ti| ti.group_key.clone());

                    // Compute sorted display order
                    let sort_mode = state.file_sort_mode.get();
                    let sorted_indices = compute_sorted_indices(&file_vec, sort_mode, &names, &groups);

                    let items: Vec<_> = sorted_indices.iter().map(|&i| {
                        let f = &file_vec[i];
                        let name = f.name.clone();
                        let dur = f.audio.duration_secs;
                        let sr = f.audio.sample_rate;
                        let preview = f.preview.clone();
                        let is_rec = f.is_recording;
                        let track_badge = groups.get(i).cloned().flatten();
                        let is_group_highlighted = track_badge.as_ref()
                            .map(|ti| Some(&ti.group_key) == active_group_key.as_ref())
                            .unwrap_or(false);
                        let is_streaming = streaming_source::is_streaming(f.audio.source.as_ref());
                        let is_active = move || current_idx.get() == Some(i);
                        let on_click = move |_| {
                            // Clear navigation history and bookmarks when switching files
                            state.nav_history.set(vec![]);
                            state.nav_index.set(0);
                            state.bookmarks.set(vec![]);
                            current_idx.set(Some(i));
                        };
                        let on_close = move |ev: MouseEvent| {
                            ev.stop_propagation();
                            if state.is_playing.get_untracked() && state.current_file_index.get_untracked() == Some(i) {
                                playback::stop(&state);
                            }
                            tile_cache::clear_file(i);
                            state.files.update(|files| { files.remove(i); });
                            state.current_file_index.update(|idx| {
                                *idx = match *idx {
                                    Some(cur) if cur == i => {
                                        let new_len = state.files.get_untracked().len();
                                        if new_len == 0 { None }
                                        else if i > 0 { Some(i - 1) }
                                        else { Some(0) }
                                    },
                                    Some(cur) if cur > i => Some(cur - 1),
                                    other => other,
                                };
                            });
                        };
                        let name_dl = name.clone();
                        let on_download = move |ev: MouseEvent| {
                            ev.stop_propagation();
                            let files = state.files.get_untracked();
                            if let Some(f) = files.get(i) {
                                let total = f.audio.source.total_samples() as usize;
                                let samples = f.audio.source.read_region(crate::audio::source::ChannelView::MonoMix, 0, total);
                                microphone::download_wav(&samples, f.audio.sample_rate, &name_dl);
                            }
                        };
                        let on_mark_saved = move |ev: MouseEvent| {
                            ev.stop_propagation();
                            state.files.update(|files| {
                                if let Some(f) = files.get_mut(i) {
                                    f.is_recording = false;
                                }
                            });
                        };
                        // Show unsaved badge on web recordings only
                        let show_unsaved = is_rec && !is_tauri;
                        view! {
                            <div
                                class=move || if is_active() { "file-item active" } else { "file-item" }
                                on:click=on_click
                            >
                                {preview.map(|pv| view! { <PreviewCanvas preview=pv /> })}
                                <div class="file-item-header">
                                    <div class="file-item-name">
                                        {if show_unsaved {
                                            Some(view! { <span class="file-unsaved-badge" title="Unsaved recording"></span> })
                                        } else {
                                            None
                                        }}
                                        {track_badge.map(|ti| {
                                            let cls = if is_group_highlighted {
                                                "file-badge file-badge-track highlighted"
                                            } else {
                                                "file-badge file-badge-track"
                                            };
                                            view! { <span class=cls>{format!("[{}]", ti.label)}</span> }
                                        })}
                                        {if is_streaming {
                                            Some(view! { <span class="file-badge file-badge-streaming" title="Streaming (large file)">"[~]"</span> })
                                        } else {
                                            None
                                        }}
                                        {name}
                                    </div>
                                    {if show_unsaved {
                                        Some(view! {
                                            <button class="file-download-btn" on:click=on_download title="Download WAV"
                                            >"\u{2B73}"</button>
                                            <button class="file-mark-saved-btn" on:click=on_mark_saved title="Mark as saved"
                                            >"\u{2713}"</button>
                                        })
                                    } else {
                                        None
                                    }}
                                    <button class="file-item-close" on:click=on_close>"×"</button>
                                </div>
                                <div class="file-item-info">
                                    {format!("{:.1}s  {}kHz", dur, sr / 1000)}
                                </div>
                            </div>
                        }
                    }).collect();
                    let on_add_click = move |_: web_sys::MouseEvent| {
                        if let Some(input) = file_input_ref.get() {
                            let el: &HtmlInputElement = input.as_ref();
                            el.click();
                        }
                    };
                    let show_sort = file_vec.len() > 1;
                    view! {
                        <div class="file-list">
                            {if show_sort {
                                Some(view! { <SortBar sort_mode=sort_mode /> })
                            } else {
                                None
                            }}
                            {items}
                            {move || {
                                let lc = loading_count.get();
                                if lc > 0 {
                                    view! {
                                        <div class="file-item loading">
                                            <div class="loading-spinner"></div>
                                            {format!("Loading {} file{}...", lc, if lc > 1 { "s" } else { "" })}
                                        </div>
                                    }.into_any()
                                } else {
                                    view! { <span></span> }.into_any()
                                }
                            }}
                            <button class="upload-btn add-files-btn" on:click=on_add_click>"+ Open files"</button>
                        </div>
                    }.into_any()
                }
            }}
        </div>
    }
}

#[component]
fn PreviewCanvas(preview: PreviewImage) -> impl IntoView {
    let canvas_ref = NodeRef::<leptos::html::Canvas>::new();
    let pv = preview.clone();

    Effect::new(move || {
        let Some(el) = canvas_ref.get() else { return };
        let canvas: &HtmlCanvasElement = el.as_ref();
        canvas.set_width(pv.width);
        canvas.set_height(pv.height);
        let ctx = canvas
            .get_context("2d")
            .unwrap()
            .unwrap()
            .dyn_into::<CanvasRenderingContext2d>()
            .unwrap();
        let clamped = Clamped(pv.pixels.as_slice());
        if let Ok(img) = ImageData::new_with_u8_clamped_array_and_sh(clamped, pv.width, pv.height) {
            let _ = ctx.put_image_data(&img, 0.0, 0.0);
        }
    });

    view! {
        <canvas
            node_ref=canvas_ref
            class="file-preview-canvas"
        />
    }
}

#[component]
fn SortBar(sort_mode: FileSortMode) -> impl IntoView {
    let state = expect_context::<AppState>();
    let sort_signal = state.file_sort_mode;

    let options: Vec<_> = FileSortMode::ALL.iter().map(|&mode| {
        let label = mode.label();
        let is_selected = mode == sort_mode;
        view! {
            <option value=label selected=is_selected>{label}</option>
        }
    }).collect();

    let on_change = move |ev: web_sys::Event| {
        let target = ev.target().unwrap();
        let select: web_sys::HtmlSelectElement = target.unchecked_into();
        let val = select.value();
        let mode = FileSortMode::ALL.iter().find(|m| m.label() == val).copied().unwrap_or_default();
        sort_signal.set(mode);
    };

    view! {
        <div class="file-sort-bar">
            <span class="file-sort-label">"Sort:"</span>
            <select class="file-sort-select" on:change=on_change>
                {options}
            </select>
        </div>
    }
}

/// Extract a GUANO timestamp string from a file's metadata, if present.
fn guano_timestamp(f: &LoadedFile) -> Option<String> {
    let guano = f.audio.metadata.guano.as_ref()?;
    guano.fields.iter()
        .find(|(k, _)| k == "Timestamp")
        .map(|(_, v)| v.clone())
}

/// Compute display indices sorted according to the selected mode.
fn compute_sorted_indices(
    files: &[LoadedFile],
    mode: FileSortMode,
    names: &[String],
    groups: &[Option<file_groups::TrackInfo>],
) -> Vec<usize> {
    let mut indices: Vec<usize> = (0..files.len()).collect();
    match mode {
        FileSortMode::AddOrder => {
            indices.sort_by_key(|&i| files[i].add_order);
        }
        FileSortMode::ByName => {
            indices.sort_by(|&a, &b| {
                names[a].to_lowercase().cmp(&names[b].to_lowercase())
            });
        }
        FileSortMode::ByDate => {
            indices.sort_by(|&a, &b| {
                let da = files[a].last_modified_ms.unwrap_or(f64::MAX);
                let db = files[b].last_modified_ms.unwrap_or(f64::MAX);
                da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
            });
        }
        FileSortMode::ByMetadataDate => {
            indices.sort_by(|&a, &b| {
                let ta = guano_timestamp(&files[a]).unwrap_or_default();
                let tb = guano_timestamp(&files[b]).unwrap_or_default();
                ta.cmp(&tb)
            });
        }
        FileSortMode::Grouped => {
            indices.sort_by(|&a, &b| {
                let ga = groups[a].as_ref().map(|ti| (&ti.group_key, &ti.label));
                let gb = groups[b].as_ref().map(|ti| (&ti.group_key, &ti.label));
                match (ga, gb) {
                    (Some((gk_a, l_a)), Some((gk_b, l_b))) => {
                        gk_a.cmp(gk_b).then_with(|| l_a.cmp(l_b))
                    }
                    (Some(_), None) => std::cmp::Ordering::Less,
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    (None, None) => names[a].to_lowercase().cmp(&names[b].to_lowercase()),
                }
            });
        }
    }
    indices
}
