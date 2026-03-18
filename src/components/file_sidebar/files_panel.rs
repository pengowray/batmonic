use std::collections::HashMap;
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
use crate::timeline::TimelineView;
use crate::format_time::format_duration_compact;

use super::loading::{read_and_load_file, DemoEntry, fetch_demo_index, load_single_demo};

#[component]
pub(super) fn FilesPanel() -> impl IntoView {
    let state = expect_context::<AppState>();
    let is_mobile = state.is_mobile.get_untracked();
    let drag_over = RwSignal::new(false);
    let files = state.files;
    let current_idx = state.current_file_index;
    let loading_files = state.loading_files;

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
            let load_id = state.loading_start(&file.name());
            spawn_local(async move {
                match read_and_load_file(file, state.clone(), load_id).await {
                    Ok(()) => {}
                    Err(e) => log::error!("Failed to load file: {e}"),
                }
                state.loading_done(load_id);
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
            let load_id = state.loading_start(&file.name());
            spawn_local(async move {
                match read_and_load_file(file, state.clone(), load_id).await {
                    Ok(()) => {}
                    Err(e) => log::error!("Failed to load file: {e}"),
                }
                state.loading_done(load_id);
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
                let loading_empty = loading_files.with(|v| v.is_empty());
                if file_vec.is_empty() && loading_empty {
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
                                                    let load_id = state.loading_start(&entry.filename);
                                                    spawn_local(async move {
                                                        match load_single_demo(&entry, state, load_id).await {
                                                            Ok(()) => {}
                                                            Err(e) => log::error!("Failed to load demo sound: {e}"),
                                                        }
                                                        state.loading_done(load_id);
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
                    let group_infos = file_groups::compute_all_groups(&names, &file_vec);
                    let active_group_key: Option<String> = current_idx.get()
                        .and_then(|idx| group_infos.get(idx))
                        .and_then(|g| g.track.as_ref())
                        .map(|ti| ti.group_key.clone());

                    // Compute sorted display order
                    let sort_mode = state.file_sort_mode.get();
                    let sorted_indices = compute_sorted_indices(&file_vec, sort_mode, &names, &group_infos);

                    let mut items: Vec<leptos::tachys::view::any_view::AnyView> = Vec::new();
                    for (pos, &i) in sorted_indices.iter().enumerate() {
                        // Insert gap separator between consecutive sequence files
                        if pos > 0 {
                            let prev_i = sorted_indices[pos - 1];
                            if let (Some(prev_seq), Some(cur_seq)) = (
                                group_infos[prev_i].sequence.as_ref(),
                                group_infos[i].sequence.as_ref(),
                            ) {
                                if prev_seq.sequence_key == cur_seq.sequence_key {
                                    let gap_view = if let Some(gap) = cur_seq.gap_from_prev_secs {
                                        let (label, cls) = if gap.abs() < 0.01 {
                                            ("continuous".to_string(), "seq-gap-row continuous")
                                        } else if gap > 60.0 {
                                            (format!("{} gap", format_duration_compact(gap)), "seq-gap-row large-gap")
                                        } else {
                                            (format!("{} gap", format_duration_compact(gap)), "seq-gap-row")
                                        };
                                        view! { <div class=cls>{label}</div> }.into_any()
                                    } else {
                                        view! { <div class="seq-gap-row unknown">"? gap"</div> }.into_any()
                                    };
                                    items.push(gap_view);
                                }
                            }
                        }
                    {
                        let f = &file_vec[i];
                        let name = f.name.clone();
                        let dur = f.audio.duration_secs;
                        let sr = f.audio.sample_rate;
                        let preview = f.preview.clone();
                        let is_rec = f.is_recording;
                        let gi = &group_infos[i];
                        let track_badge = gi.track.clone();
                        let seq_badge = gi.sequence.clone();
                        let is_group_highlighted = track_badge.as_ref()
                            .map(|ti| Some(&ti.group_key) == active_group_key.as_ref())
                            .unwrap_or(false);
                        let is_streaming = streaming_source::is_streaming(f.audio.source.as_ref());
                        let is_active = move || current_idx.get() == Some(i);
                        let is_selected = move || state.selected_file_indices.with(|sel| sel.contains(&i));
                        let on_click = move |ev: MouseEvent| {
                            let ctrl = ev.ctrl_key() || ev.meta_key();
                            let shift = ev.shift_key();

                            if ctrl {
                                // Toggle this file in multi-selection
                                state.selected_file_indices.update(|sel| {
                                    if let Some(pos) = sel.iter().position(|&x| x == i) {
                                        sel.remove(pos);
                                    } else {
                                        sel.push(i);
                                    }
                                });
                                return;
                            }

                            if shift {
                                // Range select from current_file_index (or first selected) to i
                                let anchor = current_idx.get_untracked().unwrap_or(0);
                                let (lo, hi) = if anchor <= i { (anchor, i) } else { (i, anchor) };
                                // Use the sorted_indices order for the range
                                state.selected_file_indices.set((lo..=hi).collect());
                                return;
                            }

                            // Plain click: switch to single file, clear multi-selection
                            state.selected_file_indices.set(Vec::new());
                            state.active_timeline.set(None);
                            state.active_timeline_track.set(None);
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
                        let file_view = view! {
                            <div
                                class=move || {
                                    let mut cls = "file-item".to_string();
                                    if is_active() { cls.push_str(" active"); }
                                    if is_selected() { cls.push_str(" selected"); }
                                    cls
                                }
                                on:click=on_click
                            >
                                {preview.map(|pv| {
                    let show = state.show_file_previews;
                    view! {
                        <Show when=move || show.get()>
                            <PreviewCanvas preview=pv.clone() />
                        </Show>
                    }
                })}
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
                                        {seq_badge.map(|si| {
                                            view! { <span class="file-badge file-badge-seq"
                                                title=si.gap_from_prev_secs.map(|g| format!("Gap: {}", format_duration_compact(g))).unwrap_or_default()
                                            >{format!("#{}", si.sequence_number)}</span> }
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
                                    {format!("{}  {}kHz", format_duration_compact(dur), sr / 1000)}
                                </div>
                            </div>
                        }.into_any();
                        items.push(file_view);
                    }}
                    let on_add_click = move |_: web_sys::MouseEvent| {
                        if let Some(input) = file_input_ref.get() {
                            let el: &HtmlInputElement = input.as_ref();
                            el.click();
                        }
                    };
                    let show_sort = file_vec.len() > 1;

                    let on_create_timeline = move |_: web_sys::MouseEvent| {
                        let sel = state.selected_file_indices.get_untracked();
                        if sel.len() < 2 { return; }
                        let files = state.files.get_untracked();
                        if let Some(tv) = TimelineView::from_files(&sel, &files) {
                            // Save to project if one exists
                            state.current_project.update(|p| {
                                let Some(proj) = p else { return };
                                let entries: Vec<_> = tv.segments.iter().filter_map(|seg| {
                                    let loaded = files.get(seg.file_index)?;
                                    let identity = loaded.identity.as_ref()?;
                                    let proj_idx = proj.find_file(identity)?;
                                    Some(crate::project::TimelineEntry {
                                        file_index: proj_idx,
                                        start_epoch_ms: tv.origin_epoch_ms + seg.timeline_offset_secs * 1000.0,
                                        duration_secs: seg.duration_secs,
                                        multitrack_group_id: None,
                                    })
                                }).collect();
                                if !entries.is_empty() {
                                    proj.timelines.push(crate::project::TimelineDefinition {
                                        id: crate::annotations::generate_uuid(),
                                        entries,
                                        label: None,
                                    });
                                    proj.touch();
                                }
                            });
                            state.project_dirty.set(true);

                            state.active_timeline.set(Some(tv));
                            state.active_timeline_track.set(None);
                            state.current_file_index.set(None);
                        }
                    };

                    let on_exit_timeline = move |_: web_sys::MouseEvent| {
                        state.active_timeline.set(None);
                        state.active_timeline_track.set(None);
                        state.selected_file_indices.set(Vec::new());
                        // Restore to first file if none active
                        if state.current_file_index.get_untracked().is_none() && !state.files.with_untracked(|f| f.is_empty()) {
                            state.current_file_index.set(Some(0));
                        }
                    };

                    view! {
                        <div class="file-list">
                            {if show_sort {
                                Some(view! { <SortBar sort_mode=sort_mode /> })
                            } else {
                                None
                            }}
                            // Timeline creation / active banner
                            {move || {
                                if state.active_timeline.with(|t| t.is_some()) {
                                    let seg_count = state.active_timeline.with(|t| {
                                        t.as_ref().map(|tv| tv.segments.len()).unwrap_or(0)
                                    });
                                    let total_dur = state.active_timeline.with(|t| {
                                        t.as_ref().map(|tv| tv.total_duration_secs).unwrap_or(0.0)
                                    });
                                    view! {
                                        <div class="timeline-banner">
                                            <span class="timeline-banner-label">
                                                {format!("Timeline: {} files, {}", seg_count, format_duration_compact(total_dur))}
                                            </span>
                                            <button class="timeline-exit-btn" on:click=on_exit_timeline
                                                title="Exit timeline view"
                                            >"\u{00D7}"</button>
                                        </div>
                                    }.into_any()
                                } else {
                                    let sel_count = state.selected_file_indices.with(|s| s.len());
                                    if sel_count >= 2 {
                                        view! {
                                            <div class="timeline-create-bar">
                                                <span>{format!("{} files selected", sel_count)}</span>
                                                <button class="timeline-create-btn" on:click=on_create_timeline
                                                    title="Create a stitched timeline from selected files"
                                                >"Create Timeline"</button>
                                            </div>
                                        }.into_any()
                                    } else {
                                        view! { <span></span> }.into_any()
                                    }
                                }
                            }}
                            {items}
                            {move || {
                                let entries = loading_files.get();
                                if entries.is_empty() {
                                    return view! { <span></span> }.into_any();
                                }
                                let items: Vec<_> = entries.iter().map(|entry| {
                                    let stage_text = match &entry.stage {
                                        crate::state::LoadingStage::Decoding => "Decoding\u{2026}".to_string(),
                                        crate::state::LoadingStage::Preview => "Preview\u{2026}".to_string(),
                                        crate::state::LoadingStage::Spectrogram(pct) => format!("Spectrogram {pct}%"),
                                        crate::state::LoadingStage::Finalizing => "Finalizing\u{2026}".to_string(),
                                        crate::state::LoadingStage::Streaming => "Streaming\u{2026}".to_string(),
                                    };
                                    let pct = if let crate::state::LoadingStage::Spectrogram(p) = entry.stage { p } else { 0 };
                                    let show_bar = matches!(entry.stage, crate::state::LoadingStage::Spectrogram(_));
                                    let short_name = if entry.name.len() > 28 {
                                        format!("{}\u{2026}", &entry.name[..27])
                                    } else {
                                        entry.name.clone()
                                    };
                                    view! {
                                        <div class="file-item loading">
                                            <div class="loading-spinner"></div>
                                            <div class="loading-info">
                                                <span class="loading-name">{short_name}</span>
                                                <span class="loading-stage">{stage_text}</span>
                                                {if show_bar {
                                                    Some(view! {
                                                        <div class="loading-bar">
                                                            <div class="loading-bar-fill"
                                                                 style=format!("width:{}%", pct)></div>
                                                        </div>
                                                    })
                                                } else {
                                                    None
                                                }}
                                            </div>
                                        </div>
                                    }
                                }).collect::<Vec<_>>();
                                view! { <div>{items}</div> }.into_any()
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

    let show_previews = state.show_file_previews;
    let on_toggle_previews = move |_: web_sys::MouseEvent| {
        show_previews.update(|v| *v = !*v);
    };

    view! {
        <div class="file-sort-bar">
            <span class="file-sort-label">"Sort:"</span>
            <select class="file-sort-select" on:change=on_change>
                {options}
            </select>
            <button
                class=move || if show_previews.get() { "file-preview-toggle active" } else { "file-preview-toggle" }
                title=move || if show_previews.get() { "Hide previews" } else { "Show previews" }
                on:click=on_toggle_previews
            >
                "\u{1F5BC}\u{FE0E}"
            </button>
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

/// Get a combined group key for sorting: (sequence_key, track_label).
/// Files with sequence or track info get grouped; others return None.
fn combined_group_key(gi: &file_groups::FileGroupInfo) -> Option<(String, String)> {
    match (&gi.sequence, &gi.track) {
        (Some(seq), Some(track)) => Some((seq.sequence_key.clone(), track.label.clone())),
        (Some(seq), None) => Some((seq.sequence_key.clone(), String::new())),
        (None, Some(track)) => Some((track.group_key.clone(), track.label.clone())),
        (None, None) => None,
    }
}

/// Compute display indices sorted according to the selected mode.
fn compute_sorted_indices(
    files: &[LoadedFile],
    mode: FileSortMode,
    names: &[String],
    groups: &[file_groups::FileGroupInfo],
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
                let ga = groups[a].track.as_ref().map(|ti| (&ti.group_key, &ti.label));
                let gb = groups[b].track.as_ref().map(|ti| (&ti.group_key, &ti.label));
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
        FileSortMode::GroupedAdded => {
            // Groups (sequences/multitracks) first, ordered by earliest add_order in each group.
            // Within a group: by sequence_number then track label.
            // Ungrouped files after, in add_order.
            let mut group_min_order: HashMap<String, usize> = HashMap::new();
            for (i, gi) in groups.iter().enumerate() {
                if let Some((key, _)) = combined_group_key(gi) {
                    let order = files[i].add_order;
                    group_min_order.entry(key).and_modify(|m| *m = (*m).min(order)).or_insert(order);
                }
            }
            indices.sort_by(|&a, &b| {
                let ka = combined_group_key(&groups[a]);
                let kb = combined_group_key(&groups[b]);
                match (&ka, &kb) {
                    (Some((key_a, _)), Some((key_b, _))) => {
                        let order_a = group_min_order.get(key_a).copied().unwrap_or(usize::MAX);
                        let order_b = group_min_order.get(key_b).copied().unwrap_or(usize::MAX);
                        order_a.cmp(&order_b)
                            .then_with(|| key_a.cmp(key_b))
                            .then_with(|| {
                                let seq_a = groups[a].sequence.as_ref().map(|s| s.sequence_number).unwrap_or(0);
                                let seq_b = groups[b].sequence.as_ref().map(|s| s.sequence_number).unwrap_or(0);
                                seq_a.cmp(&seq_b)
                            })
                            .then_with(|| {
                                let la = groups[a].track.as_ref().map(|t| &t.label);
                                let lb = groups[b].track.as_ref().map(|t| &t.label);
                                la.cmp(&lb)
                            })
                    }
                    (Some(_), None) => std::cmp::Ordering::Less,
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    (None, None) => files[a].add_order.cmp(&files[b].add_order),
                }
            });
        }
        FileSortMode::ByDateGrouped => {
            // Chronological, but sequences/multitracks kept together.
            // Groups ordered by their earliest recording start time.
            let mut group_min_time: HashMap<String, f64> = HashMap::new();
            for (i, gi) in groups.iter().enumerate() {
                if let Some((key, _)) = combined_group_key(gi) {
                    let t = files[i].recording_start_epoch_ms().unwrap_or(f64::MAX);
                    group_min_time.entry(key).and_modify(|m| *m = m.min(t)).or_insert(t);
                }
            }
            indices.sort_by(|&a, &b| {
                let ka = combined_group_key(&groups[a]);
                let kb = combined_group_key(&groups[b]);
                let time_a = match &ka {
                    Some((key, _)) => *group_min_time.get(key).unwrap_or(&f64::MAX),
                    None => files[a].recording_start_epoch_ms().unwrap_or(f64::MAX),
                };
                let time_b = match &kb {
                    Some((key, _)) => *group_min_time.get(key).unwrap_or(&f64::MAX),
                    None => files[b].recording_start_epoch_ms().unwrap_or(f64::MAX),
                };
                time_a.partial_cmp(&time_b)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| {
                        let seq_a = groups[a].sequence.as_ref().map(|s| s.sequence_number).unwrap_or(0);
                        let seq_b = groups[b].sequence.as_ref().map(|s| s.sequence_number).unwrap_or(0);
                        seq_a.cmp(&seq_b)
                    })
                    .then_with(|| {
                        let la = groups[a].track.as_ref().map(|t| &t.label);
                        let lb = groups[b].track.as_ref().map(|t| &t.label);
                        la.cmp(&lb)
                    })
            });
        }
    }
    indices
}
