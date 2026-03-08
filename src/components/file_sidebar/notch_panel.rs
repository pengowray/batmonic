use leptos::prelude::*;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;
use std::sync::Arc;
use crate::audio::source::ChannelView;
use crate::state::AppState;
use crate::dsp::notch::{self, NoiseProfile, DetectionConfig};

async fn yield_to_browser() {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        if let Some(w) = web_sys::window() {
            let _ = w.set_timeout_with_callback(&resolve);
        }
    });
    let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
}

/// Apply a deserialized NoiseProfile to app state (shared by import and preset load).
fn apply_noise_profile(state: AppState, profile: NoiseProfile) {
    let files = state.files.get_untracked();
    let idx = state.current_file_index.get_untracked();
    let nyquist = idx
        .and_then(|i| files.get(i))
        .map(|f| f.audio.sample_rate as f64 / 2.0)
        .unwrap_or(f64::MAX);

    let mut bands = profile.bands;
    for band in bands.iter_mut() {
        if band.center_hz >= nyquist {
            band.enabled = false;
        }
    }

    let count = bands.len();
    let has_floor = profile.noise_floor.is_some();
    state.notch_bands.set(bands);
    state.notch_profile_name.set(profile.name);
    if count > 0 {
        state.notch_enabled.set(true);
    }

    if let Some(floor) = profile.noise_floor {
        state.noise_reduce_floor.set(Some(floor));
        state.noise_reduce_enabled.set(true);
    }

    state.notch_harmonic_suppression.set(profile.harmonic_suppression);

    let msg = match (count > 0, has_floor) {
        (true, true) => format!("Loaded {} band{} + noise floor", count, if count == 1 { "" } else { "s" }),
        (true, false) => format!("Loaded {} band{}", count, if count == 1 { "" } else { "s" }),
        (false, true) => "Loaded noise floor".to_string(),
        (false, false) => "Profile was empty".to_string(),
    };
    state.show_info_toast(msg);
}

/// Build a NoiseProfile from current state. Returns None if nothing to save.
fn build_current_profile(state: AppState) -> Option<(NoiseProfile, String)> {
    let bands = state.notch_bands.get_untracked();
    let noise_floor = state.noise_reduce_floor.get_untracked();
    if bands.is_empty() && noise_floor.is_none() {
        return None;
    }

    let files = state.files.get_untracked();
    let idx = state.current_file_index.get_untracked();
    let sample_rate = idx
        .and_then(|i| files.get(i))
        .map(|f| f.audio.sample_rate)
        .unwrap_or(0);

    let name = state.notch_profile_name.get_untracked();
    let profile_name = if name.is_empty() {
        // Derive default name from current filename
        idx.and_then(|i| files.get(i))
            .map(|f| {
                let base = f.name.rsplit('/').next().unwrap_or(&f.name);
                let base = base.rsplit('\\').next().unwrap_or(base);
                base.rsplit_once('.').map(|(n, _)| n).unwrap_or(base).to_string()
            })
            .unwrap_or_else(|| "Noise Profile".to_string())
    } else {
        name
    };

    let created = js_sys::Date::new_0()
        .to_iso_string()
        .as_string()
        .unwrap_or_default();

    let profile = NoiseProfile {
        name: profile_name.clone(),
        bands,
        source_sample_rate: sample_rate,
        created,
        noise_floor,
        harmonic_suppression: state.notch_harmonic_suppression.get_untracked(),
    };

    Some((profile, profile_name))
}

#[component]
pub(crate) fn NotchPanel() -> impl IntoView {
    let state = expect_context::<AppState>();
    let sensitivity = RwSignal::new(6.0f64); // prominence threshold
    let saved_presets: RwSignal<Vec<String>> = RwSignal::new(Vec::new());

    // Load saved presets list on Tauri
    if state.is_tauri {
        spawn_local(async move {
            let args = js_sys::Object::new();
            if let Ok(result) = crate::tauri_bridge::tauri_invoke("list_noise_presets", &args.into()).await {
                let arr = js_sys::Array::from(&result);
                let list: Vec<String> = (0..arr.length())
                    .filter_map(|i| arr.get(i).as_string())
                    .collect();
                saved_presets.set(list);
            }
        });
    }

    // Detect noise bands
    let on_detect = move |_: web_sys::MouseEvent| {
        let files = state.files.get_untracked();
        let idx = state.current_file_index.get_untracked();
        let Some(file) = idx.and_then(|i| files.get(i).cloned()) else {
            state.show_error_toast("No file loaded");
            return;
        };

        state.notch_detecting.set(true);
        let threshold = sensitivity.get_untracked();
        let total = file.audio.source.total_samples() as usize;
        let samples = Arc::new(file.audio.source.read_region(ChannelView::MonoMix, 0, total));
        let sample_rate = file.audio.sample_rate;
        let duration = file.audio.duration_secs;

        spawn_local(async move {
            yield_to_browser().await;

            let config = DetectionConfig {
                analysis_duration_secs: if duration > 30.0 { 10.0 } else { duration },
                prominence_threshold: threshold,
                ..DetectionConfig::default()
            };

            let bands = notch::detect_noise_bands_async(&samples, sample_rate, &config).await;
            let count = bands.len();
            state.notch_bands.set(bands);
            if count > 0 {
                state.notch_enabled.set(true);
                state.show_info_toast(format!("Found {} noise band{}", count, if count == 1 { "" } else { "s" }));
            } else {
                state.show_info_toast("No persistent noise bands detected");
            }
            state.notch_detecting.set(false);
        });
    };

    // Toggle individual band
    let toggle_band = move |index: usize| {
        state.notch_bands.update(|bands| {
            if let Some(band) = bands.get_mut(index) {
                band.enabled = !band.enabled;
            }
        });
    };

    // Remove individual band
    let remove_band = move |index: usize| {
        state.notch_bands.update(|bands| {
            if index < bands.len() {
                bands.remove(index);
            }
        });
    };

    // Enable/disable all
    let set_all_enabled = move |enabled: bool| {
        state.notch_bands.update(|bands| {
            for band in bands.iter_mut() {
                band.enabled = enabled;
            }
        });
    };

    // Clear all bands
    let clear_all = move |_: web_sys::MouseEvent| {
        state.notch_bands.set(Vec::new());
        state.notch_enabled.set(false);
    };

    // Learn noise floor for spectral subtraction
    let on_learn_floor = move |_: web_sys::MouseEvent| {
        let files = state.files.get_untracked();
        let idx = state.current_file_index.get_untracked();
        let Some(file) = idx.and_then(|i| files.get(i).cloned()) else {
            state.show_error_toast("No file loaded");
            return;
        };

        state.noise_reduce_learning.set(true);
        let total = file.audio.source.total_samples() as usize;
        let samples = Arc::new(file.audio.source.read_region(ChannelView::MonoMix, 0, total));
        let sample_rate = file.audio.sample_rate;
        let duration = file.audio.duration_secs;

        spawn_local(async move {
            yield_to_browser().await;
            let analysis_secs = if duration > 30.0 { 10.0 } else { duration };
            let floor = crate::dsp::spectral_sub::learn_noise_floor_async(
                &samples, sample_rate, analysis_secs,
            ).await;
            if let Some(f) = floor {
                state.noise_reduce_floor.set(Some(f));
                state.noise_reduce_enabled.set(true);
                state.show_info_toast("Noise floor learned");
            } else {
                state.show_error_toast("Not enough audio to learn noise floor");
            }
            state.noise_reduce_learning.set(false);
        });
    };

    // Noise reduction strength slider handler
    let on_strength_change = move |ev: web_sys::Event| {
        let target: web_sys::HtmlInputElement = ev.target().unwrap().unchecked_into();
        if let Ok(val) = target.value().parse::<f64>() {
            state.noise_reduce_strength.set(val / 100.0); // slider 0–300 → 0.0–3.0
        }
    };

    // Sensitivity slider handler
    let on_sensitivity_change = move |ev: web_sys::Event| {
        let target: web_sys::HtmlInputElement = ev.target().unwrap().unchecked_into();
        if let Ok(val) = target.value().parse::<f64>() {
            sensitivity.set((150.0 - val) / 10.0); // slider 30–120 → threshold 12.0–3.0 (inverted: higher sensitivity = lower threshold = more bands)
        }
    };

    // Harmonic suppression slider handler
    let on_harmonic_change = move |ev: web_sys::Event| {
        let target: web_sys::HtmlInputElement = ev.target().unwrap().unchecked_into();
        if let Ok(val) = target.value().parse::<f64>() {
            state.notch_harmonic_suppression.set(val / 100.0); // slider 0–100 → 0.0–1.0
        }
    };

    // Profile name handler
    let on_name_change = move |ev: web_sys::Event| {
        let target: web_sys::HtmlInputElement = ev.target().unwrap().unchecked_into();
        state.notch_profile_name.set(target.value());
    };

    // Save preset (Tauri only)
    let on_save_preset = move |_: web_sys::MouseEvent| {
        let Some((profile, profile_name)) = build_current_profile(state) else {
            state.show_error_toast("Nothing to save");
            return;
        };

        let Ok(yaml) = yaml_serde::to_string(&profile) else {
            state.show_error_toast("Failed to serialize profile");
            return;
        };

        spawn_local(async move {
            let args = js_sys::Object::new();
            let _ = js_sys::Reflect::set(&args, &JsValue::from_str("name"), &JsValue::from_str(&profile_name));
            let _ = js_sys::Reflect::set(&args, &JsValue::from_str("json"), &JsValue::from_str(&yaml));
            match crate::tauri_bridge::tauri_invoke("save_noise_preset", &args.into()).await {
                Ok(_) => {
                    state.show_info_toast(format!("Saved preset: {}", profile_name));
                    // Refresh list
                    let args2 = js_sys::Object::new();
                    if let Ok(result) = crate::tauri_bridge::tauri_invoke("list_noise_presets", &args2.into()).await {
                        let arr = js_sys::Array::from(&result);
                        let list: Vec<String> = (0..arr.length())
                            .filter_map(|i| arr.get(i).as_string())
                            .collect();
                        saved_presets.set(list);
                    }
                }
                Err(e) => state.show_error_toast(format!("Save failed: {e}")),
            }
        });
    };

    // Load preset (Tauri only)
    let load_preset = move |filename: String| {
        spawn_local(async move {
            let args = js_sys::Object::new();
            let _ = js_sys::Reflect::set(&args, &JsValue::from_str("name"), &JsValue::from_str(&filename));
            match crate::tauri_bridge::tauri_invoke("load_noise_preset", &args.into()).await {
                Ok(result) => {
                    let text = result.as_string().unwrap_or_default();
                    // Try YAML first, fall back to JSON for legacy .json presets
                    let parsed = if filename.ends_with(".json") {
                        serde_json::from_str::<NoiseProfile>(&text).map_err(|e| e.to_string())
                    } else {
                        yaml_serde::from_str::<NoiseProfile>(&text).map_err(|e| e.to_string())
                    };
                    match parsed {
                        Ok(profile) => apply_noise_profile(state, profile),
                        Err(e) => state.show_error_toast(format!("Invalid preset: {e}")),
                    }
                }
                Err(e) => state.show_error_toast(format!("Load failed: {e}")),
            }
        });
    };

    // Delete preset (Tauri only)
    let delete_preset = move |filename: String| {
        spawn_local(async move {
            let args = js_sys::Object::new();
            let _ = js_sys::Reflect::set(&args, &JsValue::from_str("name"), &JsValue::from_str(&filename));
            match crate::tauri_bridge::tauri_invoke("delete_noise_preset", &args.into()).await {
                Ok(_) => {
                    state.show_info_toast("Preset deleted");
                    // Refresh list
                    let args2 = js_sys::Object::new();
                    if let Ok(result) = crate::tauri_bridge::tauri_invoke("list_noise_presets", &args2.into()).await {
                        let arr = js_sys::Array::from(&result);
                        let list: Vec<String> = (0..arr.length())
                            .filter_map(|i| arr.get(i).as_string())
                            .collect();
                        saved_presets.set(list);
                    }
                }
                Err(e) => state.show_error_toast(format!("Delete failed: {e}")),
            }
        });
    };

    view! {
        <div class="sidebar-panel notch-panel">
            // === Notch Filter ===
            <div class="setting-group">
                <div class="setting-row">
                    <label class="setting-label" style="flex: 1; cursor: pointer;">
                        <input
                            type="checkbox"
                            prop:checked=move || state.notch_enabled.get()
                            on:change=move |ev: web_sys::Event| {
                                let target: web_sys::HtmlInputElement = ev.target().unwrap().unchecked_into();
                                state.notch_enabled.set(target.checked());
                            }
                        />
                        " Notch Filter"
                    </label>
                </div>
            </div>

            // Detection
            <div class="setting-group">
                <div class="setting-group-title">"Detection"</div>
                <div class="setting-row" style="gap: 4px;">
                    <button
                        class="sidebar-btn"
                        style="flex: 1;"
                        on:click=on_detect
                        disabled=move || state.notch_detecting.get() || state.current_file_index.get().is_none()
                    >
                        {move || if state.notch_detecting.get() {
                            "Detecting..."
                        } else {
                            "Detect Noise"
                        }}
                    </button>
                </div>
                <div class="setting-row">
                    <span class="setting-label">"Sensitivity"</span>
                    <input
                        type="range"
                        class="setting-slider"
                        min="30"
                        max="120"
                        step="5"
                        prop:value=move || (150.0 - sensitivity.get() * 10.0) as i32
                        on:input=on_sensitivity_change
                        title=move || format!("Threshold: {:.1}x ({:.0} dB)", sensitivity.get(), 20.0 * sensitivity.get().log10())
                    />
                </div>
            </div>

            // Band list
            <div class="setting-group">
                <div class="setting-group-title">
                    {move || {
                        let bands = state.notch_bands.get();
                        let enabled = bands.iter().filter(|b| b.enabled).count();
                        if bands.is_empty() {
                            "Bands".to_string()
                        } else {
                            format!("Bands ({}/{})", enabled, bands.len())
                        }
                    }}
                </div>
                {move || {
                    let bands = state.notch_bands.get();
                    if bands.is_empty() {
                        view! {
                            <div class="setting-row" style="opacity: 0.5; font-size: 11px;">
                                "No bands detected yet"
                            </div>
                        }.into_any()
                    } else {
                        let items: Vec<_> = bands.iter().enumerate().map(|(i, band)| {
                            let center = band.center_hz;
                            let strength = band.strength_db;
                            let enabled = band.enabled;
                            let bandwidth = band.bandwidth_hz;
                            view! {
                                <div class="notch-band-row"
                                    style="display: flex; align-items: center; gap: 4px; padding: 2px 0; font-size: 11px;"
                                    on:mouseenter=move |_| state.notch_hovering_band.set(Some(i))
                                    on:mouseleave=move |_| state.notch_hovering_band.set(None)
                                >
                                    <input
                                        type="checkbox"
                                        checked=enabled
                                        on:change=move |_| toggle_band(i)
                                        style="margin: 0;"
                                    />
                                    <span style="flex: 1; white-space: nowrap;">
                                        {if center >= 1000.0 {
                                            format!("{:.1} kHz", center / 1000.0)
                                        } else {
                                            format!("{:.0} Hz", center)
                                        }}
                                    </span>
                                    <span style="opacity: 0.6; font-size: 10px; white-space: nowrap;" title=format!("BW: {:.0} Hz", bandwidth)>
                                        {format!("+{:.0}dB", strength)}
                                    </span>
                                    <button
                                        class="notch-remove-btn"
                                        style="background: none; border: none; color: inherit; opacity: 0.4; cursor: pointer; padding: 0 2px; font-size: 12px;"
                                        on:click=move |_: web_sys::MouseEvent| remove_band(i)
                                        title="Remove band"
                                    >
                                        {"\u{00D7}"}
                                    </button>
                                </div>
                            }
                        }).collect();
                        view! {
                            <div class="notch-band-list" style="max-height: 200px; overflow-y: auto;">
                                {items}
                            </div>
                        }.into_any()
                    }
                }}
                {move || {
                    let bands = state.notch_bands.get();
                    if bands.is_empty() {
                        view! { <span></span> }.into_any()
                    } else {
                        view! {
                            <div class="setting-row" style="gap: 4px; margin-top: 4px;">
                                <button
                                    class="sidebar-btn"
                                    style="flex: 1; font-size: 10px;"
                                    on:click=move |_: web_sys::MouseEvent| set_all_enabled(true)
                                >
                                    "All On"
                                </button>
                                <button
                                    class="sidebar-btn"
                                    style="flex: 1; font-size: 10px;"
                                    on:click=move |_: web_sys::MouseEvent| set_all_enabled(false)
                                >
                                    "All Off"
                                </button>
                                <button
                                    class="sidebar-btn"
                                    style="flex: 1; font-size: 10px;"
                                    on:click=clear_all
                                >
                                    "Clear"
                                </button>
                            </div>
                        }.into_any()
                    }
                }}
            </div>

            // === Noise Reduction (spectral subtraction) ===
            <div class="setting-group">
                <div class="setting-row">
                    <label class="setting-label" style="flex: 1; cursor: pointer;">
                        <input
                            type="checkbox"
                            prop:checked=move || state.noise_reduce_enabled.get()
                            on:change=move |ev: web_sys::Event| {
                                let target: web_sys::HtmlInputElement = ev.target().unwrap().unchecked_into();
                                state.noise_reduce_enabled.set(target.checked());
                            }
                        />
                        " Noise Reduction"
                    </label>
                </div>
                <div class="setting-row" style="font-size: 10px; opacity: 0.5; margin-top: -2px;">
                    "Spectral subtraction"
                </div>
                <div class="setting-row" style="gap: 4px;">
                    <button
                        class="sidebar-btn"
                        style="flex: 1;"
                        on:click=on_learn_floor
                        disabled=move || state.noise_reduce_learning.get() || state.current_file_index.get().is_none()
                    >
                        {move || if state.noise_reduce_learning.get() {
                            "Learning..."
                        } else {
                            "Learn Noise Floor"
                        }}
                    </button>
                </div>
                <div class="setting-row">
                    <span class="setting-label">"Strength"</span>
                    <input
                        type="range"
                        class="setting-slider"
                        min="0"
                        max="300"
                        step="5"
                        prop:value=move || (state.noise_reduce_strength.get() * 100.0) as i32
                        on:input=on_strength_change
                        title=move || format!("{:.0}%", state.noise_reduce_strength.get() * 100.0)
                    />
                </div>
                {move || {
                    let floor = state.noise_reduce_floor.get();
                    if let Some(f) = floor {
                        view! {
                            <div class="setting-row" style="font-size: 11px; opacity: 0.7;">
                                {format!("{} bins, {:.1}s analyzed", f.bin_magnitudes.len(), f.analysis_duration_secs)}
                            </div>
                            <div class="setting-row" style="gap: 4px; margin-top: 2px;">
                                <button
                                    class="sidebar-btn"
                                    style="flex: 1; font-size: 10px;"
                                    on:click=move |_: web_sys::MouseEvent| {
                                        state.noise_reduce_floor.set(None);
                                        state.noise_reduce_enabled.set(false);
                                    }
                                >
                                    "Clear Floor"
                                </button>
                            </div>
                        }.into_any()
                    } else {
                        view! {
                            <div class="setting-row" style="opacity: 0.5; font-size: 11px;">
                                "No noise floor learned"
                            </div>
                        }.into_any()
                    }
                }}
            </div>

            // === Harmonic Suppression ===
            {move || {
                let has_bands = !state.notch_bands.get().is_empty();
                let has_floor = state.noise_reduce_floor.get().is_some();
                if has_bands || has_floor {
                    view! {
                        <div class="setting-group">
                            <div class="setting-row">
                                <span class="setting-label">"Harmonic suppression"</span>
                                <input
                                    type="range"
                                    class="setting-slider"
                                    min="0"
                                    max="100"
                                    step="5"
                                    prop:value=move || (state.notch_harmonic_suppression.get() * 100.0) as i32
                                    on:input=on_harmonic_change
                                    title=move || {
                                        let v = state.notch_harmonic_suppression.get();
                                        if v == 0.0 {
                                            "Off".to_string()
                                        } else {
                                            format!("{:.0}% ({:.0} dB)", v * 100.0, -48.0 * v)
                                        }
                                    }
                                />
                            </div>
                            <div class="setting-row" style="font-size: 10px; opacity: 0.6;">
                                {move || {
                                    let v = state.notch_harmonic_suppression.get();
                                    if v == 0.0 {
                                        "Attenuate 2\u{00D7} & 3\u{00D7} harmonics of noise".to_string()
                                    } else {
                                        format!("{:.0}% ({:.0} dB at 2\u{00D7} & 3\u{00D7})", v * 100.0, -48.0 * v)
                                    }
                                }}
                            </div>
                        </div>
                    }.into_any()
                } else {
                    view! { <span></span> }.into_any()
                }
            }}

            // === Profile management ===
            <div class="setting-group">
                <div class="setting-group-title">"Profile"</div>
                <div class="setting-row">
                    <input
                        type="text"
                        class="setting-input"
                        style="flex: 1; font-size: 11px; padding: 2px 4px; background: var(--bg-secondary, #333); color: inherit; border: 1px solid #555; border-radius: 3px;"
                        placeholder="Profile name"
                        prop:value=move || state.notch_profile_name.get()
                        on:input=on_name_change
                    />
                </div>
                // Tauri-only: Save Preset + preset list
                {if state.is_tauri {
                    Some(view! {
                        <div class="setting-row" style="gap: 4px; margin-top: 4px;">
                            <button
                                class="sidebar-btn"
                                style="flex: 1;"
                                on:click=on_save_preset
                                disabled=move || state.notch_bands.get().is_empty() && state.noise_reduce_floor.get().is_none()
                            >
                                "Save Preset"
                            </button>
                        </div>
                        {move || {
                            let presets = saved_presets.get();
                            if presets.is_empty() {
                                view! { <span></span> }.into_any()
                            } else {
                                let items: Vec<_> = presets.iter().map(|p| {
                                    let filename_load = p.clone();
                                    let filename_del = p.clone();
                                    let display = p.trim_end_matches(".batm").trim_end_matches(".json").replace('_', " ");
                                    let display_title = display.clone();
                                    view! {
                                        <div style="display: flex; align-items: center; gap: 4px; padding: 1px 0; font-size: 11px;">
                                            <button
                                                class="sidebar-btn"
                                                style="flex: 1; font-size: 10px; text-align: left; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;"
                                                on:click=move |_: web_sys::MouseEvent| {
                                                    let f = filename_load.clone();
                                                    load_preset(f);
                                                }
                                                title=format!("Load {}", display_title)
                                            >
                                                {display}
                                            </button>
                                            <button
                                                style="background: none; border: none; color: inherit; opacity: 0.4; cursor: pointer; padding: 0 2px; font-size: 12px;"
                                                on:click=move |_: web_sys::MouseEvent| {
                                                    let f = filename_del.clone();
                                                    delete_preset(f);
                                                }
                                                title="Delete preset"
                                            >
                                                {"\u{00D7}"}
                                            </button>
                                        </div>
                                    }
                                }).collect();
                                view! {
                                    <div class="setting-group-title" style="margin-top: 4px;">"Saved Presets"</div>
                                    <div style="max-height: 150px; overflow-y: auto;">
                                        {items}
                                    </div>
                                }.into_any()
                            }
                        }}
                    })
                } else {
                    None
                }}
            </div>
        </div>
    }
}
