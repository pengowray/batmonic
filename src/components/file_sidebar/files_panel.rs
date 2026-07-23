use crate::state::store_fields::*;
use std::collections::HashMap;
use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::{Clamped, JsCast};
use web_sys::{CanvasRenderingContext2d, DragEvent, HtmlCanvasElement, HtmlInputElement, ImageData, MouseEvent};
use crate::audio::playback;
use crate::audio::streaming_source;
use crate::canvas::tile_cache;
use crate::state::{AppState, FileSortMode, LoadedFile};
use crate::types::PreviewImage;
use super::file_groups;
use super::file_badges;
use crate::format_time::format_duration_compact;

use super::loading::{read_and_load_file, load_native_file, DemoEntry, fetch_demo_index, find_open_demo, load_single_demo};
use super::suggestions::BatsForYou;

/// Reconcile the index-keyed state that `Vec::remove(removed)` invalidates,
/// shared by every file-removal path (the close button via [`remove_file_at`]
/// and the stop-listening cleanup in `live_recording`):
///
/// - the positional tile/spectral caches — cleared wholesale, because their
///   keys are `(file_idx, …)` and the files that shift down would otherwise
///   read their old neighbour's tiles under the reused index (a wrong-file
///   render). They self-heal by lazily re-rendering the visible tiles.
/// - `selected_file_indices` — drop `removed`, shift higher indices down.
/// - `active_timeline` / `active_timeline_track` — segments embed `file_index`,
///   so the timeline is rebuilt from the reconciled selection; dropping below
///   two files tears it (and the track selection) down.
///
/// Callers remain responsible for `current_file_index` / `mic_live_file_idx`,
/// which have path-specific fixup rules. Must be called AFTER `files` is
/// mutated (the timeline rebuild reads the post-removal list).
pub(crate) fn reconcile_after_file_removed(state: &AppState, removed: usize) {
    tile_cache::clear_all_caches();
    crate::canvas::spectral_store::clear();
    state.timeline.selected_file_indices().update(|sel| {
        sel.retain(|&x| x != removed);
        for x in sel.iter_mut() {
            if *x > removed { *x -= 1; }
        }
    });
    if state.timeline.active().get_untracked().is_some() {
        let sel = state.timeline.selected_file_indices().get_untracked();
        let rebuilt = state.library.files().with_untracked(|files| {
            crate::timeline::TimelineView::from_files(&sel, files)
        });
        if rebuilt.is_none() {
            state.timeline.active_track().set(None);
        }
        state.timeline.active().set(rebuilt);
    }
}

/// Remove the file at `idx` from the list and reconcile EVERY piece of state
/// that keys off a file's positional index, because `Vec::remove(i)` shifts
/// all files after `i` down by one.
///
/// Index-bearing state that must be fixed up on removal (keep this list in
/// sync with the body):
/// - `current_file_index`  — the focused file
/// - `mic_live_file_idx`   — the live-capture slot
/// - `selected_file_indices` — the multi-select set (timeline builder)
/// - `active_timeline` / `active_timeline_track` — segments hold `file_index`
/// - the positional tile/spectral caches (main/flow/reassign/chroma/resonator
///   + `spectral_store`), whose keys are `(file_idx, …)`. Clearing only the
///   removed file's tiles is NOT enough: the files that shift down would then
///   read their old neighbour's tiles under the now-reused index (a wrong-file
///   render). We clear all positional caches on removal; they self-heal by
///   re-rendering the visible tiles.
///
/// Also stops playback if the closed file was being played, and restores the
/// new current file's viewport.
///
/// Used for the synchronous close-button path and for the post-stop close
/// path that runs after an async stop_listening / stop_recording.
fn remove_file_at(state: &AppState, i: usize) {
    if state.playback.is_playing().get_untracked() && state.library.current_index().get_untracked() == Some(i) {
        playback::stop(state);
    }
    let was_current = state.library.current_index().get_untracked() == Some(i);
    state.library.files().update(|files| {
        if i < files.len() {
            files.remove(i);
        }
    });
    // Read files len OUTSIDE the current_index().update() guard — siblings in
    // the `library` store share one borrow cell (inner read would conflict).
    let new_len = state.library.files().get_untracked().len();
    state.library.current_index().update(|idx| {
        *idx = match *idx {
            Some(cur) if cur == i => {
                if new_len == 0 { None }
                else if i > 0 { Some(i - 1) }
                else { Some(0) }
            },
            Some(cur) if cur > i => Some(cur - 1),
            other => other,
        };
    });
    state.mic.live_file_idx().update(|idx| {
        *idx = match *idx {
            Some(cur) if cur == i => None,
            Some(cur) if cur > i => Some(cur - 1),
            other => other,
        };
    });
    // Reconcile the positional caches + multi-select + timeline (shared with
    // the stop-listening removal path in live_recording).
    reconcile_after_file_removed(state, i);
    // If closing the current file left current_file_index unchanged (e.g.
    // closing file 0 when file 1 slides into slot 0), the per-file
    // vertical-zoom sync Effect won't fire — so reload the new current
    // file's stored viewport manually.
    if was_current {
        let new_idx = state.library.current_index().get_untracked();
        let (min, max) = if let Some(n) = new_idx {
            state.library.files().with_untracked(|files| {
                files.get(n)
                    .map(|f| (f.min_display_freq, f.max_display_freq))
                    .unwrap_or((None, None))
            })
        } else {
            (None, None)
        };
        if state.view.min_display_freq().get_untracked() != min {
            state.view.min_display_freq().set(min);
        }
        if state.view.max_display_freq().get_untracked() != max {
            state.view.max_display_freq().set(max);
        }
    }
}

#[component]
pub(super) fn FilesPanel() -> impl IntoView {
    let state = expect_context::<AppState>();
    let drag_over = RwSignal::new(false);
    let files = state.library.files();
    let current_idx = state.library.current_index();
    let loading_files = state.library.loading();

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

    let state_for_upload = state;
    let on_upload_click = move |_: web_sys::MouseEvent| {
        if state.is_tauri && !state.status.is_mobile().get_untracked() {
            // Tauri desktop: use native file dialog to get real filesystem paths
            let state = state_for_upload;
            spawn_local(async move {
                let args = js_sys::Object::new();
                match crate::tauri_bridge::tauri_invoke("open_file_dialog", &args.into()).await {
                    Ok(result) => {
                        let paths: Vec<String> = js_sys::Array::from(&result)
                            .iter()
                            .filter_map(|v| v.as_string())
                            .collect();
                        for path in paths {
                            let name = path.rsplit(['/', '\\']).next().unwrap_or(&path).to_string();
                            let state = state;
                            let load_id = state.loading_start(&name);
                            spawn_local(async move {
                                match load_native_file(path, state, load_id).await {
                                    Ok(()) => {}
                                    Err(e) => log::error!("Failed to load file: {e}"),
                                }
                                state.loading_done(load_id);
                            });
                        }
                    }
                    Err(e) => log::error!("File dialog error: {e}"),
                }
            });
        } else if let Some(input) = file_input_ref.get() {
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
            let state = state_for_upload;
            let load_id = state.loading_start(&file.name());
            spawn_local(async move {
                match read_and_load_file(file, state, load_id).await {
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
    // Default suggestions panel to expanded when the file list starts empty so
    // first-run users see "Bats For You"; collapse when files are present so
    // the panel doesn't push the file list off-screen.
    let bats_expanded = RwSignal::new(state.library.files().with_untracked(|f| f.is_empty()));

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


    let state_for_drop = state;
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
            let state = state_for_drop;
            let file_name = file.name();
            let load_id = state.loading_start(&file_name);
            spawn_local(async move {
                match read_and_load_file(file, state, load_id).await {
                    Ok(()) => {}
                    Err(e) => {
                        log::error!("Failed to load {}: {}", file_name, e);
                        state.show_error_toast(&format!("Couldn't open {file_name}: {e}"));
                    }
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
                accept=".wav,.w4v,.flac,.mp3,.ogg,.m4a,.m4b,.zc"
                multiple=true
                style="display:none"
                on:change=on_file_input_change
            />
            {move || {
                let file_vec = files.get();
                let loading_empty = loading_files.with(|v| v.is_empty());
                let no_files = file_vec.is_empty() && loading_empty;
                let is_tauri = state.is_tauri;

                // Build the file rows only when there are files. The action
                // footer + suggestions render in both states from the same code
                // path below so the menu is identical pre- and post-load.
                let names: Vec<String> = file_vec.iter().map(|f| f.name.clone()).collect();
                let group_infos = file_groups::compute_all_groups(&names, &file_vec);
                let active_group_key: Option<String> = current_idx.get()
                    .and_then(|idx| group_infos.get(idx))
                    .and_then(|g| g.track.as_ref())
                    .map(|ti| ti.group_key.clone());
                let active_seq_key: Option<(String, String)> = current_idx.get()
                    .and_then(|idx| group_infos.get(idx))
                    .and_then(|g| g.sequence.as_ref())
                    .map(|s| (s.sequence_key.clone(), s.track_label.clone()));

                let sort_mode = state.library.sort_mode().get();
                let sorted_indices = compute_sorted_indices(&file_vec, sort_mode, &names, &group_infos);

                let mut items: Vec<leptos::tachys::view::any_view::AnyView> = Vec::new();
                for (_pos, &i) in sorted_indices.iter().enumerate() {
                    {
                        let f = &file_vec[i];
                        let name = f.name.clone();
                        let preview = f.preview.clone();
                        let is_rec = f.is_recording;
                        let gi = &group_infos[i];
                        let track_badge = gi.track.clone();
                        let seq_badge = gi.sequence.clone();
                        let is_streaming = streaming_source::is_streaming(f.audio.source.as_ref());
                        let file_loading_id = f.loading_id;
                        let is_active = move || current_idx.get() == Some(i);
                        let is_selected = move || state.timeline.selected_file_indices().with(|sel| sel.contains(&i));

                        // Determine if group badges should show for this file
                        let show_groups = seq_badge.as_ref()
                            .map(|s| active_seq_key.as_ref()
                                .map(|(k, t)| k == &s.sequence_key && t == &s.track_label)
                                .unwrap_or(false))
                            .unwrap_or(false)
                            || track_badge.as_ref()
                            .map(|t| Some(&t.group_key) == active_group_key.as_ref())
                            .unwrap_or(false);

                        // Build badge data
                        let cc_info = f.xc_metadata.as_ref().and_then(|meta| {
                            let lic = file_badges::get_xc_field(meta, "License")?;
                            let label = file_badges::parse_cc_license(&lic)?;
                            let attr = file_badges::get_xc_field(meta, "Attribution");
                            let tooltip = if let Some(attr_text) = attr {
                                format!("Creative Commons {} \u{2014} {}", label, attr_text)
                            } else {
                                format!("Creative Commons {}", label)
                            };
                            Some((label, tooltip))
                        });
                        let badge_data = file_badges::FileBadgeData {
                            sample_rate: f.audio.sample_rate,
                            bits_per_sample: f.audio.metadata.bits_per_sample,
                            is_float: f.audio.metadata.is_float,
                            duration_secs: f.audio.duration_secs,
                            is_unsaved: is_rec && !is_tauri,
                            is_streaming,
                            track: track_badge,
                            sequence: seq_badge,
                            cc_license: cc_info.as_ref().map(|(l, _)| l.clone()),
                            cc_tooltip: cc_info.map(|(_, t)| t),
                            file_index: i,
                        };

                        let on_click = move |ev: MouseEvent| {
                            let ctrl = ev.ctrl_key() || ev.meta_key();
                            let shift = ev.shift_key();

                            if ctrl {
                                state.timeline.selected_file_indices().update(|sel| {
                                    if let Some(pos) = sel.iter().position(|&x| x == i) {
                                        sel.remove(pos);
                                    } else {
                                        sel.push(i);
                                    }
                                });
                                return;
                            }

                            if shift {
                                let anchor = current_idx.get_untracked().unwrap_or(0);
                                let (lo, hi) = if anchor <= i { (anchor, i) } else { (i, anchor) };
                                state.timeline.selected_file_indices().set((lo..=hi).collect());
                                return;
                            }

                            state.timeline.selected_file_indices().set(Vec::new());
                            state.timeline.active().set(None);
                            state.timeline.active_track().set(None);
                            state.viewmode.nav_history().set(std::collections::VecDeque::new());
                            state.viewmode.nav_index().set(0);
                            state.viewmode.bookmarks().set(vec![]);
                            current_idx.set(Some(i));
                        };
                        let on_close = move |ev: MouseEvent| {
                            ev.stop_propagation();
                            // Closing the active live doc while listening or
                            // recording: stop the mic cleanly first, then
                            // remove the file. We can't remove synchronously
                            // because finalize_recording / cleanup_listen_file
                            // run on the live slot and would race a sync remove.
                            //
                            // Capture the file's add_order so we can find it
                            // again after the await — concurrent file ops or
                            // finalize_recording could shift its index.
                            let is_live_doc = state.mic.live_file_idx().get_untracked() == Some(i);
                            let mic_active = state.mic.listening().get_untracked()
                                || state.mic.recording().get_untracked();
                            if is_live_doc && mic_active {
                                let add_order = state.library.files().with_untracked(|files| {
                                    files.get(i).map(|f| f.add_order)
                                });
                                spawn_local(async move {
                                    if state.mic.recording().get_untracked() {
                                        // Stops + finalizes (saves) the recording.
                                        // The file stays in the list with the
                                        // recorded audio after this await.
                                        crate::audio::microphone::toggle_record(&state).await;
                                    } else if state.mic.listening().get_untracked() {
                                        // Stops listening; cleanup_listen_file
                                        // removes the file from the list.
                                        crate::audio::microphone::toggle_listen(&state).await;
                                    }
                                    // Listening case: file is already gone, no-op.
                                    // Recording case: finalize kept the file —
                                    // honor the close by removing it now.
                                    if let Some(target) = add_order {
                                        if let Some(idx) = state.library.files().with_untracked(|files| {
                                            files.iter().position(|f| f.add_order == target)
                                        }) {
                                            remove_file_at(&state, idx);
                                        }
                                    }
                                });
                                return;
                            }
                            remove_file_at(&state, i);
                        };
                        let name_dl = name.clone();
                        let on_download = move |_: ()| {
                            let files = state.library.files().get_untracked();
                            if let Some(f) = files.get(i) {
                                let total = f.audio.source.total_samples() as usize;
                                let samples = f.audio.source.read_region(crate::audio::source::ChannelView::MonoMix, 0, total);
                                crate::audio::wav_encoder::download_recording_wav(
                                    &samples, f.audio.sample_rate, &name_dl,
                                    f.audio.metadata.guano.as_ref(), &f.wav_markers,
                                );
                            }
                            // Clear unsaved state after download
                            state.library.files().update(|files| {
                                if let Some(f) = files.get_mut(i) {
                                    f.is_recording = false;
                                }
                            });
                        };
                        let on_toggle_readonly = move |ev: MouseEvent| {
                            ev.stop_propagation();
                            state.library.files().update(|files| {
                                if let Some(f) = files.get_mut(i) {
                                    f.read_only = !f.read_only;
                                }
                            });
                        };
                        let batm_badge_state = move || {
                            state.library.files().with(|files| {
                                files.get(i).map(|f| (f.read_only, f.had_sidecar)).unwrap_or((false, false))
                            })
                        };
                        let show_unsaved = is_rec && !is_tauri;
                        let has_download = is_rec && !is_tauri;
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
                    let show = state.library.show_previews();
                    view! {
                        <Show when=move || show.get()>
                            <PreviewCanvas preview=pv.clone() />
                        </Show>
                    }
                })}
                                <div class="file-item-header">
                                    <div class="file-item-name">
                                        {if is_tauri {
                                            Some(view! {
                                                <span
                                                    class="file-badge file-badge-batm"
                                                    style="cursor: pointer;"
                                                    style:display=move || {
                                                        let (ro, sidecar) = batm_badge_state();
                                                        if ro || sidecar { "inline" } else { "none" }
                                                    }
                                                    on:click=on_toggle_readonly
                                                    title=move || {
                                                        let (ro, sidecar) = batm_badge_state();
                                                        match (ro, sidecar) {
                                                            (true, true) => "View-only \u{2014} .batm sidecar not being updated. Click to enable editing.",
                                                            (true, false) => "View-only \u{2014} annotations won\u{2019}t be saved. Click to enable editing.",
                                                            (false, true) => ".batm sidecar next to audio file is being updated. Click for view-only.",
                                                            (false, false) => "",
                                                        }.to_string()
                                                    }
                                                >
                                                    {move || {
                                                        let (ro, sidecar) = batm_badge_state();
                                                        match (ro, sidecar) {
                                                            (true, true) => "[.batm view]",
                                                            (true, false) => "[view]",
                                                            (false, true) => "[.batm]",
                                                            (false, false) => "",
                                                        }
                                                    }}
                                                </span>
                                            })
                                        } else {
                                            None
                                        }}
                                        {if show_unsaved {
                                            Some(view! { <span class="file-unsaved-asterisk" title="Unsaved recording">"*"</span> })
                                        } else {
                                            None
                                        }}
                                        {name}
                                    </div>
                                    <button class="file-item-close" on:click=on_close>"×"</button>
                                </div>
                                <div class="file-item-info">
                                    <file_badges::FileBadgeRow
                                        data=badge_data
                                        context="file-menu"
                                        show_group_badges=Signal::derive(move || show_groups)
                                        show_download=has_download
                                        on_download=Callback::new(on_download)
                                    />
                                </div>
                                {file_loading_id.map(|lid| {
                                    view! {
                                        <div class="file-item-loading">
                                            {move || {
                                                let entries = loading_files.get();
                                                let entry = entries.iter().find(|e| e.id == lid);
                                                if let Some(entry) = entry {
                                                    let stage_text = match &entry.stage {
                                                        crate::state::LoadingStage::Decoding => "Decoding\u{2026}".to_string(),
                                                        crate::state::LoadingStage::Preview => "Preview\u{2026}".to_string(),
                                                        crate::state::LoadingStage::Spectrogram(pct) => format!("Spectrogram {pct}%"),
                                                        crate::state::LoadingStage::Finalizing => "Finalizing\u{2026}".to_string(),
                                                        crate::state::LoadingStage::Streaming => "Streaming\u{2026}".to_string(),
                                                    };
                                                    let pct = if let crate::state::LoadingStage::Spectrogram(p) = entry.stage { p } else { 0 };
                                                    let show_bar = matches!(entry.stage, crate::state::LoadingStage::Spectrogram(_));
                                                    view! {
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
                                                    }.into_any()
                                                } else {
                                                    view! { <span></span> }.into_any()
                                                }
                                            }}
                                        </div>
                                    }
                                })}
                            </div>
                        }.into_any();
                        items.push(file_view);
                    }}
                    let show_sort = file_vec.len() > 1;

                    let on_exit_timeline = move |_: web_sys::MouseEvent| {
                        state.timeline.active().set(None);
                        state.timeline.active_track().set(None);
                        state.timeline.selected_file_indices().set(Vec::new());
                        // Restore to first file if none active
                        if state.library.current_index().get_untracked().is_none() && !state.library.files().with_untracked(|f| f.is_empty()) {
                            state.library.current_index().set(Some(0));
                        }
                    };

                    view! {
                        <div class="file-list">
                            {state.status.is_mobile().get().then(|| view! {
                                <div
                                    style="padding: 12px 12px 8px; cursor: pointer; user-select: none; -webkit-user-select: none;"
                                    on:click=move |_| state.dialogs.about().set(true)
                                >
                                    <span style="font-weight: bold; font-size: 14px; color: #ddd;">"Oversample"</span>
                                    " "
                                    <span style="font-style: italic; font-size: 14px; opacity: 0.45; font-weight: 300; color: #ddd;">"beta"</span>
                                </div>
                            })}
                            {if show_sort {
                                Some(view! { <SortBar sort_mode=sort_mode /> })
                            } else {
                                None
                            }}
                            // Active timeline banner
                            {state.timeline.active().with(|t| t.is_some()).then(|| {
                                let (seg_count, total_dur) = state.timeline.active().with(|t| {
                                    t.as_ref().map(|tv| (tv.segments.len(), tv.total_duration_secs))
                                        .unwrap_or((0, 0.0))
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
                                }
                            })}
                            {(!no_files).then(|| view! {
                                <div>{items}</div>
                            })}
                            // Show loading entries that don't yet have a file in the list
                            // (still decoding/streaming before the file is added)
                            {
                                let entries = loading_files.get();
                                let file_loading_ids: Vec<u64> = files.with(|f| {
                                    f.iter().filter_map(|file| file.loading_id).collect()
                                });
                                let orphan_items: Vec<_> = entries.iter()
                                    .filter(|e| !file_loading_ids.contains(&e.id))
                                    .map(|entry| {
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
                                                    {show_bar.then(|| view! {
                                                        <div class="loading-bar">
                                                            <div class="loading-bar-fill"
                                                                 style=format!("width:{}%", pct)></div>
                                                        </div>
                                                    })}
                                                </div>
                                            </div>
                                        }
                                    }).collect::<Vec<_>>();
                                (!orphan_items.is_empty()).then(|| view! { <div>{orphan_items}</div> })
                            }
                            // Empty state — faded bat + headline + hint when
                            // the file list (and any in-flight loads) are
                            // empty. Action buttons below give the user
                            // somewhere to go on both desktop and mobile.
                            {no_files.then(|| view! {
                                <div class="files-empty-state">
                                    <img class="files-empty-logo"
                                         src="about-icon.png"
                                         alt=""
                                         aria-hidden="true"/>
                                    <div class="files-empty-title">"No recordings open"</div>
                                    <div class="files-empty-sub">"Plug in a mic or drop a .wav"</div>
                                </div>
                            })}
                            // ── Mic detected chip ──
                            // Shows when a USB mic is connected but no backend
                            // is set yet (i.e. the user hasn't picked it). Click
                            // opens the chooser; X dismisses for the session.
                            // Reappears on the next USB-connect transition.
                            {move || {
                                let usb = state.mic.usb_connected().get();
                                let has_backend = state.mic.backend().get().is_some();
                                let dismissed = state.mic.chip_dismissed().get();
                                let strat = state.mic.strategy().get();
                                let show = state.is_tauri
                                    && usb
                                    && !has_backend
                                    && !dismissed
                                    && strat != crate::state::MicStrategy::None;
                                show.then(|| {
                                    let name = state.mic.device_name().get_untracked()
                                        .unwrap_or_else(|| "USB mic".to_string());
                                    view! {
                                        <div class="mic-detected-chip"
                                            role="button"
                                            tabindex="0"
                                            on:click=move |_| {
                                                state.mic.pending_action().set(None);
                                                state.mic.show_chooser().set(true);
                                            }
                                        >
                                            <span class="mic-led ready" aria-hidden="true"></span>
                                            <span class="mic-detected-text">
                                                {format!("Mic detected: {name}")}
                                                <span class="mic-detected-hint">"\u{00A0}\u{2014} tap to use"</span>
                                            </span>
                                            <button
                                                class="mic-detected-close"
                                                title="Dismiss"
                                                on:click=move |ev: web_sys::MouseEvent| {
                                                    ev.stop_propagation();
                                                    state.mic.chip_dismissed().set(true);
                                                }
                                            >{"\u{00D7}"}</button>
                                        </div>
                                    }
                                })
                            }}
                            // Unified action footer — same buttons available
                            // whether or not files are loaded. Was previously
                            // split into "empty state" vs "with files" panels
                            // with different sets of actions.
                            <div class="file-actions">
                                <button class="upload-btn add-files-btn" on:click=on_upload_click>
                                    "+ Open files"
                                </button>
                                {(state.mic.strategy().get() != crate::state::MicStrategy::None).then(|| view! {
                                    <button
                                        class="upload-btn add-files-btn"
                                        title="Open the mic and create an empty live document. Adjust HFR mode/range/bandpass before pressing Listen or Record."
                                        on:click=move |_| {
                                            spawn_local(async move {
                                                crate::audio::microphone::arm_live_doc(&state).await;
                                            });
                                        }
                                    >"+ New live recording"</button>
                                })}
                                <button class="upload-btn add-files-btn" on:click=on_demo_click>
                                    {move || if demo_loading.get() { "Loading\u{2026}" } else { "Load demo" }}
                                </button>
                                {is_tauri.then(|| view! {
                                    <button class="upload-btn add-files-btn xc-btn" on:click=move |_| {
                                        state.dialogs.xc_browser_open().set(true);
                                    }>"Explore XC"</button>
                                })}
                                <button
                                    class="upload-btn add-files-btn xc-btn"
                                    title="Load a WAV or other recording from Wikimedia Commons or Wikipedia, correcting any declared time expansion factor"
                                    on:click=move |_| {
                                        state.dialogs.wiki_browser_open().set(true);
                                    }
                                >"From Wikipedia"</button>
                            </div>
                            // Demo picker (inline, when triggered)
                            {move || demo_picker_open.get().then(|| {
                                let entries = demo_entries.get();
                                let bat_entries: Vec<DemoEntry> = entries.iter()
                                    .filter(|e| e.is_bat())
                                    .cloned()
                                    .collect();
                                let has_bats = !bat_entries.is_empty();
                                let random_bat_btn = has_bats.then(|| view! {
                                    <button
                                        class="demo-item demo-random-bat"
                                        on:click=move |_| {
                                            // Prefer a bat that isn't already open; if all are
                                            // open, fall back to the full pool and jump-to-existing.
                                            let unopened: Vec<DemoEntry> = bat_entries.iter()
                                                .filter(|e| find_open_demo(state, &e.filename).is_none())
                                                .cloned()
                                                .collect();
                                            let pool = if unopened.is_empty() { bat_entries.clone() } else { unopened };
                                            let idx = (js_sys::Math::random() * pool.len() as f64) as usize;
                                            let entry = pool[idx.min(pool.len() - 1)].clone();
                                            if let Some(open_idx) = find_open_demo(state, &entry.filename) {
                                                state.library.current_index().set(Some(open_idx));
                                                return;
                                            }
                                            let label = entry.en.clone()
                                                .unwrap_or_else(|| entry.filename.clone());
                                            let load_id = state.loading_start(&format!("Random bat: {label}"));
                                            spawn_local(async move {
                                                match load_single_demo(&entry, state, load_id).await {
                                                    Ok(()) => {}
                                                    Err(e) => log::error!("Failed to load random bat: {e}"),
                                                }
                                                state.loading_done(load_id);
                                            });
                                        }
                                    >
                                        "Random bat"
                                    </button>
                                });
                                let demo_items: Vec<_> = entries.iter().map(|entry| {
                                    let entry_clone = entry.clone();
                                    let display_name = entry.en.clone().unwrap_or_else(|| {
                                        entry.filename
                                            .trim_end_matches(".wav")
                                            .trim_end_matches(".w4v")
                                            .trim_end_matches(".flac")
                                            .trim_end_matches(".mp3")
                                            .to_string()
                                    });
                                    view! {
                                        <button
                                            class="demo-item"
                                            on:click=move |_| {
                                                let entry = entry_clone.clone();
                                                if let Some(idx) = find_open_demo(state, &entry.filename) {
                                                    state.library.current_index().set(Some(idx));
                                                    demo_picker_open.set(false);
                                                    return;
                                                }
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
                                    <div class="demo-picker">
                                        {random_bat_btn}
                                        {demo_items}
                                    </div>
                                }
                            })}
                        </div>
                    }.into_any()
            }}
            // Kept outside the reactive closure above so it isn't unmounted +
            // remounted on every files signal update (e.g. duration_secs
            // ticking up during listen/record), which would reset its local
            // signals and re-fetch metadata, causing the per-card details
            // line to flicker.
            <BatsForYou demo_entries=demo_entries expanded=bats_expanded />
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
    let sort_signal = state.library.sort_mode();

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

    let show_previews = state.library.show_previews();
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
