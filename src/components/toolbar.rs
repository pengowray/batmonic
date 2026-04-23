use leptos::prelude::*;
use wasm_bindgen::JsCast;
use crate::state::{AppState, RightSidebarTab, ListenMode, MicAcquisitionState, RecordReadyState};
use crate::audio::streaming_source;
use crate::components::file_sidebar::file_groups;
use crate::components::file_sidebar::file_badges::{FileBadgeData, FileBadgeRow, parse_cc_license, get_xc_field};

#[component]
pub fn Toolbar() -> impl IntoView {
    let state = expect_context::<AppState>();
    let show_about = state.show_about;
    let seq_dropdown_open = RwSignal::new(false);
    let show_cc_modal = RwSignal::new(false);
    let track_dropdown_open = RwSignal::new(false);

    let is_tauri = state.is_tauri;
    let overflow_menu_open = RwSignal::new(false);
    let toolbar_ref = NodeRef::<leptos::html::Div>::new();

    // Set --toolbar-h CSS variable on document root so mobile overlays know toolbar height
    Effect::new(move |_| {
        if let Some(el) = toolbar_ref.get() {
            let h = el.offset_height();
            if let Some(doc_el) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.document_element()) {
                let _ = doc_el.unchecked_ref::<web_sys::HtmlElement>().style().set_property("--toolbar-h", &format!("{}px", h));
            }
        }
    });

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

    // Derived: is current file unsaved (recording not yet saved by user)
    let is_unsaved = Memo::new(move |_| {
        let files = state.files.get();
        let idx = state.current_file_index.get();
        idx.and_then(|i| files.get(i))
            .map(|f| f.is_recording)
            .unwrap_or(false)
    });

    // Derived: badge data for current file
    let current_badge_data = Memo::new(move |_| {
        let files = state.files.get();
        let idx = state.current_file_index.get()?;
        let f = files.get(idx)?;
        let names: Vec<String> = files.iter().map(|f| f.name.clone()).collect();
        let groups = file_groups::compute_all_groups(&names, &files);
        let gi = groups.get(idx)?;

        let cc_info = f.xc_metadata.as_ref().and_then(|meta| {
            let lic = get_xc_field(meta, "License")?;
            let label = parse_cc_license(&lic)?;
            Some(label)
        });

        Some(FileBadgeData {
            sample_rate: f.audio.sample_rate,
            bits_per_sample: f.audio.metadata.bits_per_sample,
            is_float: f.audio.metadata.is_float,
            duration_secs: f.audio.duration_secs,
            is_unsaved: f.is_recording,
            is_streaming: streaming_source::is_streaming(f.audio.source.as_ref()),
            track: gi.track.clone(),
            sequence: gi.sequence.clone(),
            cc_license: cc_info,
            cc_tooltip: None, // toolbar renders CC separately
            file_index: idx,
        })
    });

    // Derived: sequence group files (for dropdown)
    let seq_group_files = Memo::new(move |_| {
        let files = state.files.get();
        let idx = state.current_file_index.get()?;
        let names: Vec<String> = files.iter().map(|f| f.name.clone()).collect();
        let groups = file_groups::compute_all_groups(&names, &files);
        let cur_seq = groups.get(idx)?.sequence.as_ref()?;
        let key = cur_seq.sequence_key.clone();
        let tlabel = cur_seq.track_label.clone();
        let mut matches: Vec<(usize, String, u32)> = groups.iter().enumerate()
            .filter_map(|(i, g)| {
                let s = g.sequence.as_ref()?;
                if s.sequence_key == key && s.track_label == tlabel {
                    Some((i, files[i].name.clone(), s.sequence_number))
                } else {
                    None
                }
            })
            .collect();
        matches.sort_by_key(|(_, _, n)| *n);
        Some(matches)
    });

    // Derived: track group files (for dropdown)
    let track_group_files = Memo::new(move |_| {
        let files = state.files.get();
        let idx = state.current_file_index.get()?;
        let names: Vec<String> = files.iter().map(|f| f.name.clone()).collect();
        let groups = file_groups::compute_all_groups(&names, &files);
        let cur_track = groups.get(idx)?.track.as_ref()?;
        let key = cur_track.group_key.clone();
        let matches: Vec<(usize, String, String)> = groups.iter().enumerate()
            .filter_map(|(i, g)| {
                let t = g.track.as_ref()?;
                if t.group_key == key {
                    Some((i, files[i].name.clone(), t.label.clone()))
                } else {
                    None
                }
            })
            .collect();
        Some(matches)
    });

    // Derived: status prefix for document title
    let status_prefix = Memo::new(move |_| {
        let recording = state.mic_recording.get();
        let listening = state.mic_listening.get();
        let playing = state.is_playing.get();
        let rec_ready = state.record_ready_state.get();
        let listen_mode = state.listen_mode.get();
        let acq_state = state.mic_acquisition_state.get();

        let mut parts = Vec::new();

        // "Ready to record" dialog active
        if rec_ready == RecordReadyState::AwaitingConfirmation {
            return Some("\u{23F8} Rec ready\u{2026}".to_string()); // ⏸ Rec ready…
        }

        // ReadyMic listen mode
        if listening && listen_mode == ListenMode::ReadyMic && acq_state == MicAcquisitionState::Ready {
            return Some("\u{23F8} Mic ready\u{2026}".to_string()); // ⏸ Mic ready…
        }

        if recording {
            parts.push("\u{1F534}"); // 🔴
        }
        if listening {
            if listen_mode == ListenMode::Normal || listen_mode == ListenMode::ReadyMic {
                parts.push("\u{1F3A4}"); // 🎤
            } else {
                parts.push("\u{1F3A4}\u{1F987}"); // 🎤🦇
            }
        } else if playing && !recording {
            parts.push("\u{25B6}\u{FE0F}"); // ▶️
        }
        if parts.is_empty() { None } else { Some(parts.join("")) }
    });

    // Derived: recording file name
    let recording_file_name = Memo::new(move |_| {
        let recording = state.mic_recording.get();
        let listening = state.mic_listening.get();
        if recording || (recording && listening) {
            let files = state.files.get();
            state.mic_live_file_idx.get()
                .and_then(|idx| files.get(idx).map(|f| f.name.clone()))
                .or_else(|| file_name.get())
        } else {
            None
        }
    });

    // Derived: center text
    let center_text = Memo::new(move |_| {
        let recording = state.mic_recording.get();
        let listening = state.mic_listening.get();

        if recording {
            let _ = state.mic_timer_tick.get(); // subscribe to timer ticks
            let start = state.mic_recording_start_time.get_untracked().unwrap_or(0.0);
            let now = js_sys::Date::now();
            let secs = (now - start) / 1000.0;
            let dur = crate::format_time::format_duration_compact(secs);
            let name = recording_file_name.get().unwrap_or_default();
            if name.is_empty() {
                format!("Recording ({})", dur)
            } else {
                format!("Recording ({}) \u{2014} {}", dur, name)
            }
        } else if listening {
            "Listening...".to_string()
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

        let title = if recording {
            let _ = state.mic_timer_tick.get(); // subscribe for live updates
            let start = state.mic_recording_start_time.get_untracked().unwrap_or(0.0);
            let now = js_sys::Date::now();
            let secs = (now - start) / 1000.0;
            let dur = crate::format_time::format_duration_compact(secs);
            let pfx = prefix.as_deref().unwrap_or("");
            format!("{} Recording ({}) - Oversample", pfx, dur)
        } else {
            match (prefix.as_deref(), listening, name.as_deref()) {
                (Some(pfx), true, _) => format!("{} Listening... - Oversample", pfx),
                (Some(pfx), false, Some(name)) => format!("{} {} - Oversample", pfx, name),
                (Some(pfx), false, None) => format!("{} Oversample", pfx),
                (None, _, Some(name)) => format!("{} - Oversample", name),
                (None, _, None) => "Oversample".to_string(),
            }
        };

        if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
            doc.set_title(&title);
        }
    });

    // Close dropdowns on click-outside
    Effect::new(move |_| {
        // Close dropdowns when file changes
        let _ = state.current_file_index.get();
        seq_dropdown_open.set(false);
        track_dropdown_open.set(false);
    });

    // Download/save handler for toolbar
    let on_toolbar_download = move |_: web_sys::MouseEvent| {
        let files = state.files.get_untracked();
        if let Some(idx) = state.current_file_index.get_untracked() {
            if let Some(f) = files.get(idx) {
                if is_tauri {
                    // On Tauri, the backend already saved to disk — just clear unsaved state
                    state.status_message.set(Some("Recording saved".into()));
                } else {
                    // On web, trigger browser download with preserved GUANO + cue markers
                    let total = f.audio.source.total_samples() as usize;
                    let samples = f.audio.source.read_region(crate::audio::source::ChannelView::MonoMix, 0, total);
                    crate::audio::wav_encoder::download_recording_wav(
                        &samples, f.audio.sample_rate, &f.name,
                        f.audio.metadata.guano.as_ref(), &f.wav_markers,
                    );
                }
                // Clear unsaved state
                state.files.update(|files| {
                    if let Some(f) = files.get_mut(idx) {
                        f.is_recording = false;
                    }
                });
            }
        }
    };

    view! {
        <div class="toolbar" node_ref=toolbar_ref>
            // Left: sidebar tab button
            <button
                class=move || if !state.sidebar_collapsed.get() {
                    "toolbar-sidebar-tab toolbar-sidebar-tab-left active"
                } else {
                    "toolbar-sidebar-tab toolbar-sidebar-tab-left"
                }
                on:click=move |ev: web_sys::MouseEvent| {
                    ev.stop_propagation();
                    state.sidebar_collapsed.update(|c| *c = !*c);
                    if !state.sidebar_collapsed.get_untracked() && state.is_mobile.get_untracked() {
                        state.right_sidebar_collapsed.set(true);
                    }
                }
                title=move || if state.sidebar_collapsed.get() { "Show sidebar" } else { "Hide sidebar" }
            >{"\u{25E7}"}</button>

            // Center: brand + filename (row 1) + badges (row 2)
            <div class="toolbar-title-center">
                // Row 1: brand + status icons + filename
                <div class="toolbar-title-row">
                    <span
                        class="toolbar-brand"
                        style=move || {
                            let mut s = String::from("cursor: pointer");
                            let mobile = state.is_mobile.get();
                            if mobile {
                                let has_file = file_name.get().is_some();
                                let recording = state.mic_recording.get();
                                let listening = state.mic_listening.get();
                                if has_file || recording || listening {
                                    s.push_str("; display: none");
                                }
                            }
                            s
                        }
                        on:click=move |_| show_about.set(true)
                        title="About"
                    ><b>"Oversample"</b>" "<span style="font-style: italic; opacity: 0.45; font-weight: 300;">"beta"</span></span>

                    <span class="toolbar-status-icons">
                        {move || state.mic_recording.get().then(|| view! {
                            <span class="toolbar-rec-dot"></span>
                        })}
                        {move || {
                            let listening = state.mic_listening.get();
                            let playing = state.is_playing.get();
                            let recording = state.mic_recording.get();

                            if listening {
                                let listen_mode = state.listen_mode.get();
                                if listen_mode == ListenMode::Normal {
                                    Some("\u{1F3A4}".to_string())
                                } else {
                                    Some("\u{1F3A4}\u{1F987}".to_string())
                                }
                            } else if playing && !recording {
                                Some("\u{25B6}\u{FE0F}".to_string())
                            } else {
                                None
                            }
                        }}
                    </span>

                    <span
                        class="toolbar-title-filename"
                        title=move || {
                            let name = file_name.get().unwrap_or_default();
                            if name.is_empty() { String::new() } else { name }
                        }
                    >
                        {move || is_unsaved.get().then(|| view! {
                            <span class="file-unsaved-asterisk" title="Unsaved recording">"*"</span>
                        })}
                        {move || center_text.get()}
                    </span>

                </div>

                // Row 2: badge row (replaces old info-row)
                <div class="toolbar-badge-row">
                    {move || {
                        let has_file = file_name.get().is_some();
                        if !has_file {
                            return None;
                        }

                        let badge_data = current_badge_data.get();
                        let cc = cc_license.get();
                        let unsaved = is_unsaved.get();

                        Some(view! {
                            // FileBadgeRow (sample rate, bit depth, duration, streaming, seq, track)
                            {badge_data.map(|data| {
                                let on_seq = Callback::new(move |()| {
                                    track_dropdown_open.set(false);
                                    seq_dropdown_open.update(|v| *v = !*v);
                                });
                                let on_track = Callback::new(move |()| {
                                    seq_dropdown_open.set(false);
                                    track_dropdown_open.update(|v| *v = !*v);
                                });
                                view! {
                                    <FileBadgeRow
                                        data=data
                                        context="toolbar"
                                        show_group_badges=Signal::derive(move || true)
                                        group_dropdowns=true
                                        on_seq_click=on_seq
                                        on_track_click=on_track
                                    />
                                }
                            })}

                            // Unsaved badge + download button (toolbar-specific)
                            {unsaved.then(|| {
                                let (btn_label, btn_title) = if is_tauri {
                                    ("\u{1F4BE} Save", "Save recording")
                                } else {
                                    ("\u{1F4BE} Download", "Download WAV")
                                };
                                view! {
                                    <span class="toolbar-unsaved-badge">"* File unsaved"</span>
                                    <button
                                        class="toolbar-download-btn"
                                        title=btn_title
                                        on:click=on_toolbar_download
                                    >{btn_label}</button>
                                }
                            })}

                            // CC badge — subtle text, opens license modal on click
                            {cc.map(|cc_label| {
                                let short_label = cc_label.strip_prefix("CC ").unwrap_or(&cc_label).to_string();
                                view! {
                                    <button
                                        class="toolbar-cc-badge"
                                        title="License details"
                                        on:click=move |e: web_sys::MouseEvent| {
                                            e.stop_propagation();
                                            show_cc_modal.set(true);
                                        }
                                    >
                                        <span class="toolbar-cc-icon"></span>
                                        <span class="toolbar-cc-label">{short_label}</span>
                                    </button>
                                }
                            })}
                        })
                    }}

                    // Sequence dropdown panel
                    {move || seq_dropdown_open.get().then(|| {
                        let items = seq_group_files.get().unwrap_or_default();
                        let current = state.current_file_index.get();
                        view! {
                            <div class="badge-dropdown-panel">
                                {items.into_iter().map(|(idx, name, seq_num)| {
                                    let is_current = current == Some(idx);
                                    let cls = if is_current { "badge-dropdown-item active" } else { "badge-dropdown-item" };
                                    view! {
                                        <div
                                            class=cls
                                            on:click=move |e: web_sys::MouseEvent| {
                                                e.stop_propagation();
                                                state.current_file_index.set(Some(idx));
                                                seq_dropdown_open.set(false);
                                            }
                                        >
                                            <span class="file-badge file-badge-seq">{format!("#{}", seq_num)}</span>
                                            <span>{name}</span>
                                        </div>
                                    }
                                }).collect::<Vec<_>>()}
                            </div>
                        }
                    })}

                    // Track dropdown panel
                    {move || track_dropdown_open.get().then(|| {
                        let items = track_group_files.get().unwrap_or_default();
                        let current = state.current_file_index.get();
                        view! {
                            <div class="badge-dropdown-panel">
                                {items.into_iter().map(|(idx, name, label)| {
                                    let is_current = current == Some(idx);
                                    let cls = if is_current { "badge-dropdown-item active" } else { "badge-dropdown-item" };
                                    view! {
                                        <div
                                            class=cls
                                            on:click=move |e: web_sys::MouseEvent| {
                                                e.stop_propagation();
                                                state.current_file_index.set(Some(idx));
                                                track_dropdown_open.set(false);
                                            }
                                        >
                                            <span class="file-badge file-badge-track">{format!("[{}]", label)}</span>
                                            <span>{name}</span>
                                        </div>
                                    }
                                }).collect::<Vec<_>>()}
                            </div>
                        }
                    })}
                </div>
            </div>

            // Right: overflow menu + right sidebar tab
            <div class="toolbar-right-group">
                <div class="toolbar-overflow-wrap">
                    <button
                        class="toolbar-overflow-btn"
                        on:click=move |ev: web_sys::MouseEvent| {
                            ev.stop_propagation();
                            overflow_menu_open.update(|v| *v = !*v);
                        }
                        title="More actions"
                    >"\u{2026}"</button>
                    {move || overflow_menu_open.get().then(|| view! {
                        <div class="toolbar-overflow-backdrop" on:click=move |_| overflow_menu_open.set(false)></div>
                        <div class="toolbar-overflow-menu" on:click=move |_| overflow_menu_open.set(false)>
                            <button
                                class="toolbar-overflow-item"
                                on:click=move |_| state.undo_annotations()
                                disabled=move || !state.can_undo()
                            >
                                <span class="toolbar-overflow-icon">{"\u{21B6}"}</span>
                                "Undo"
                            </button>
                            <button
                                class="toolbar-overflow-item"
                                on:click=move |_| state.redo_annotations()
                                disabled=move || !state.can_redo()
                            >
                                <span class="toolbar-overflow-icon">{"\u{21B7}"}</span>
                                "Redo"
                            </button>
                            <div class="toolbar-overflow-separator"></div>
                            <button
                                class="toolbar-overflow-item"
                                on:click=move |_| {
                                    let idx = state.nav_index.get_untracked();
                                    if idx == 0 { return; }
                                    let new_idx = idx - 1;
                                    state.nav_index.set(new_idx);
                                    let hist = state.nav_history.get_untracked();
                                    if let Some(entry) = hist.get(new_idx) {
                                        state.suspend_follow();
                                        state.scroll_offset.set(entry.scroll_offset);
                                        state.zoom_level.set(entry.zoom_level);
                                    }
                                }
                                disabled=move || state.nav_index.get() == 0
                            >
                                <span class="toolbar-overflow-icon">"←"</span>
                                "Back"
                            </button>
                            <button
                                class="toolbar-overflow-item"
                                on:click=move |_| {
                                    let idx = state.nav_index.get_untracked();
                                    let hist = state.nav_history.get_untracked();
                                    if idx + 1 >= hist.len() { return; }
                                    let new_idx = idx + 1;
                                    state.nav_index.set(new_idx);
                                    if let Some(entry) = hist.get(new_idx) {
                                        state.suspend_follow();
                                        state.scroll_offset.set(entry.scroll_offset);
                                        state.zoom_level.set(entry.zoom_level);
                                    }
                                }
                                disabled=move || {
                                    let idx = state.nav_index.get();
                                    let len = state.nav_history.get().len();
                                    idx + 1 >= len
                                }
                            >
                                <span class="toolbar-overflow-icon">"→"</span>
                                "Forward"
                            </button>
                        </div>
                    })}
                </div>
                <button
                    class=move || if !state.right_sidebar_collapsed.get() {
                        "toolbar-sidebar-tab toolbar-sidebar-tab-right active"
                    } else {
                        "toolbar-sidebar-tab toolbar-sidebar-tab-right"
                    }
                    on:click=move |ev: web_sys::MouseEvent| {
                        ev.stop_propagation();
                        state.right_sidebar_collapsed.update(|c| *c = !*c);
                        if !state.right_sidebar_collapsed.get_untracked() && state.is_mobile.get_untracked() {
                            state.sidebar_collapsed.set(true);
                        }
                    }
                    title=move || if state.right_sidebar_collapsed.get() { "Show info panel" } else { "Hide info panel" }
                >{"\u{25E8}"}</button>
            </div>

            // CC License modal
            {move || show_cc_modal.get().then(|| {
                let meta = xc_metadata.get().unwrap_or_default();
                let field = |key: &str| get_xc_field(&meta, key);
                let species = field("Species").unwrap_or_default();
                let sci_name = field("Scientific name").unwrap_or_default();
                let recordist = field("Recordist").unwrap_or_default();
                let date = field("Date").unwrap_or_default();
                let attribution = field("Attribution").unwrap_or_default();
                let url = field("URL").unwrap_or_default();
                let lic_url = field("License").unwrap_or_default();
                let cc_label = parse_cc_license(&lic_url).unwrap_or_default();
                let device = field("Device").unwrap_or_default();
                let mic = field("Microphone").unwrap_or_default();
                let method = field("Method").unwrap_or_default();
                let quality = field("Quality").unwrap_or_default();
                let location = field("Location").unwrap_or_default();
                let country = field("Country").unwrap_or_default();
                let loc_display = if !location.is_empty() && !country.is_empty() {
                    format!("{}, {}", location, country)
                } else if !location.is_empty() {
                    location
                } else {
                    country
                };
                let is_demo = {
                    let files = state.files.get();
                    let idx = state.current_file_index.get();
                    idx.and_then(|i| files.get(i)).map(|f| f.is_demo).unwrap_or(false)
                };

                view! {
                    <div class="modal-overlay" on:click=move |_| show_cc_modal.set(false)>
                        <div class="modal-dialog cc-modal" on:click=move |ev: web_sys::MouseEvent| ev.stop_propagation()>
                            <div class="modal-header">
                                <span class="modal-title">
                                    <span class="toolbar-cc-icon" style="width: 16px; height: 16px; margin-right: 6px;"></span>
                                    {if cc_label.is_empty() { "License".to_string() } else { format!("Creative Commons {cc_label}") }}
                                </span>
                                <button class="modal-close" on:click=move |_| show_cc_modal.set(false)>{"\u{00D7}"}</button>
                            </div>
                            <div class="modal-body">
                                // Species heading
                                {(!species.is_empty()).then(|| view! {
                                    <div class="cc-species">
                                        {species}
                                        {(!sci_name.is_empty()).then(|| view! {
                                            <span class="cc-sci-name">{format!(" ({})", sci_name)}</span>
                                        })}
                                    </div>
                                })}

                                // Key fields
                                <div class="cc-fields">
                                    {(!recordist.is_empty()).then(|| view! {
                                        <div class="cc-row">
                                            <span class="cc-label">"Recordist"</span>
                                            <span class="cc-value">{recordist}</span>
                                        </div>
                                    })}
                                    {(!date.is_empty()).then(|| view! {
                                        <div class="cc-row">
                                            <span class="cc-label">"Date"</span>
                                            <span class="cc-value">{date}</span>
                                        </div>
                                    })}
                                    {(!loc_display.is_empty()).then(|| view! {
                                        <div class="cc-row">
                                            <span class="cc-label">"Location"</span>
                                            <span class="cc-value">{loc_display}</span>
                                        </div>
                                    })}
                                    {(!device.is_empty()).then(|| view! {
                                        <div class="cc-row">
                                            <span class="cc-label">"Device"</span>
                                            <span class="cc-value">{device}</span>
                                        </div>
                                    })}
                                    {(!mic.is_empty()).then(|| view! {
                                        <div class="cc-row">
                                            <span class="cc-label">"Microphone"</span>
                                            <span class="cc-value">{mic}</span>
                                        </div>
                                    })}
                                    {(!method.is_empty()).then(|| view! {
                                        <div class="cc-row">
                                            <span class="cc-label">"Method"</span>
                                            <span class="cc-value">{method}</span>
                                        </div>
                                    })}
                                    {(!quality.is_empty()).then(|| view! {
                                        <div class="cc-row">
                                            <span class="cc-label">"Quality"</span>
                                            <span class="cc-value">{quality}</span>
                                        </div>
                                    })}
                                </div>

                                // Attribution
                                {(!attribution.is_empty()).then(|| view! {
                                    <div class="cc-attribution">{attribution}</div>
                                })}

                                // License link
                                {(!lic_url.is_empty()).then(|| {
                                    let label = if cc_label.is_empty() { lic_url.clone() } else { format!("Creative Commons {cc_label}") };
                                    view! {
                                        <div class="cc-license-link">
                                            <a href=lic_url target="_blank" rel="noopener">{label}</a>
                                        </div>
                                    }
                                })}

                                // Links row
                                <div class="cc-links">
                                    {(!url.is_empty()).then(|| view! {
                                        <a class="cc-link-btn" href=url target="_blank" rel="noopener">"Open on Xeno\u{2011}Canto \u{2197}"</a>
                                    })}
                                    <button class="cc-link-btn" on:click=move |e: web_sys::MouseEvent| {
                                        e.stop_propagation();
                                        show_cc_modal.set(false);
                                        state.right_sidebar_tab.set(RightSidebarTab::Metadata);
                                        state.right_sidebar_collapsed.set(false);
                                    }>"More metadata\u{2026}"</button>
                                </div>

                                // Demo archive notice
                                {is_demo.then(|| view! {
                                    <div class="cc-demo-notice">
                                        "Retrieved from the "
                                        <a href="https://github.com/pengowray/bat-demo-sounds" target="_blank" rel="noopener">"bat demo sounds archive"</a>
                                        "."
                                    </div>
                                })}
                            </div>
                        </div>
                    </div>
                }
            })}

            {move || show_about.get().then(|| view! {
                <div class="about-overlay" on:click=move |_| show_about.set(false)>
                    <div class="about-dialog" on:click=move |ev: web_sys::MouseEvent| ev.stop_propagation()>
                        <img src="about-icon.png" alt="Oversample" style="width: 200px; height: 200px; display: block; margin: 0 auto 12px auto;"/>
                        <div class="about-header" style="text-align: center;">
                            <span class="about-title"><b>"Oversample"</b></span>
                            <div style="font-size: 12px; color: #bbb; margin-top: 4px;">
                                {
                                    let ver = env!("CARGO_PKG_VERSION");
                                    let hash = env!("GIT_HASH");
                                    if ver.starts_with("0.") {
                                        format!("v{ver} (beta) [{hash}]")
                                    } else {
                                        format!("v{ver} [{hash}]")
                                    }
                                }
                            </div>
                            <div style="font-size: 11px; color: #aaa; margin-top: 2px;">"by Pengo Wray"</div>
                        </div>
                        <p class="about-desc">"Bat call viewer and acoustic analysis tool."</p>
                        <div style="margin-top: 12px; font-size: 11px; color: #999; line-height: 1.8;">
                            "Thanks to the libraries and code that make this possible:"
                            <div style="margin-top: 6px; columns: 2; column-gap: 16px;">
                                <div><a href="https://leptos.dev" target="_blank" style="color: #8cf; text-decoration: none;">"Leptos"</a></div>
                                <div><a href="https://tauri.app" target="_blank" style="color: #8cf; text-decoration: none;">"Tauri"</a></div>
                                <div><a href="https://crates.io/crates/realfft" target="_blank" style="color: #8cf; text-decoration: none;">"RealFFT"</a></div>
                                <div><a href="https://github.com/jhartquist/resonators" target="_blank" style="color: #8cf; text-decoration: none;">"resonators"</a></div>
                                <div><a href="https://crates.io/crates/cpal" target="_blank" style="color: #8cf; text-decoration: none;">"cpal"</a></div>
                                <div><a href="https://crates.io/crates/hound" target="_blank" style="color: #8cf; text-decoration: none;">"Hound"</a></div>
                                <div><a href="https://crates.io/crates/claxon" target="_blank" style="color: #8cf; text-decoration: none;">"Claxon"</a></div>
                                <div><a href="https://crates.io/crates/lewton" target="_blank" style="color: #8cf; text-decoration: none;">"Lewton"</a></div>
                                <div><a href="https://crates.io/crates/symphonia" target="_blank" style="color: #8cf; text-decoration: none;">"Symphonia"</a></div>
                                <div><a href="https://crates.io/crates/wasm-bindgen" target="_blank" style="color: #8cf; text-decoration: none;">"wasm-bindgen"</a></div>
                                <div><a href="https://crates.io/crates/web-sys" target="_blank" style="color: #8cf; text-decoration: none;">"web-sys"</a></div>
                            </div>
                            <div style="margin-top: 8px;">"and "<a href="https://github.com/jmears63/batgizmo-app-public" target="_blank" style="color: #8cf; text-decoration: none;">"batgizmo"</a>"."</div>
                            <div style="margin-top: 8px;">"App icon by "<a href="https://twitter.com/lordspikyfish" target="_blank" style="color: #8cf; text-decoration: none;">"spiky.fish"</a>"."</div>
                        </div>
                        <button class="about-close" on:click=move |_| show_about.set(false)>"Close"</button>
                    </div>
                </div>
            })}
        </div>
    }
}
