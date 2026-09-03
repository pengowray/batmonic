// Listen combo button — lives at the right end of the Hearing Bar.
//
// Left half: toggles live mic listening; status text reflects acquire /
//   ready / muted state.
// Right half (caret): "…" — opens this popup of listen-specific knobs.
//
// Mode (HET/PS/PV/ZC/1:1), heterodyne freq/cutoff, factors, bandpass,
// and the Output On/Mute toggle live in the Mode and HFR buttons. This
// popup only carries settings that have nowhere else to go:
//   • PS/PV overlap-save buffer size (latency vs smoothness tradeoff)

use crate::audio::microphone;
use crate::components::combo_button::ComboButton;
use crate::components::icons::Icon;
use crate::state::store_fields::*;
use crate::state::{
    AppState, LayerPanel, MicAcquisitionState, MicStrategy, PlaybackMode, RecordReadyState,
};
use leptos::prelude::*;

fn layer_opt_class(active: bool) -> &'static str {
    if active {
        "layer-panel-opt sel"
    } else {
        "layer-panel-opt"
    }
}

fn toggle_panel(state: &AppState, panel: LayerPanel) {
    state.panels.layer_panel_open().update(|p| {
        *p = if *p == Some(panel) { None } else { Some(panel) };
    });
}

#[component]
pub fn ListenButton() -> impl IntoView {
    let state = expect_context::<AppState>();

    let listen_is_open = Signal::derive(move || {
        state.panels.layer_panel_open().get() == Some(LayerPanel::ListenMode)
    });

    let listen_left_class = Signal::derive(move || {
        if state.mic.strategy().get() == MicStrategy::None {
            return "layer-btn combo-btn-left disabled";
        }
        let is_listening_ready = state.mic.listening().get()
            && state.mic.acquisition_state().get() == MicAcquisitionState::Ready;
        let is_rec_ready =
            state.mic.record_ready_state().get() == RecordReadyState::AwaitingConfirmation;
        if is_listening_ready || is_rec_ready {
            "layer-btn combo-btn-left mic-armed"
        } else {
            "layer-btn combo-btn-left"
        }
    });
    let listen_right_class = Signal::derive(move || {
        if listen_is_open.get() {
            "layer-btn combo-btn-right open"
        } else {
            "layer-btn combo-btn-right"
        }
    });

    let listen_left_value = Signal::derive(move || {
        if state.mic.record_ready_state().get() == RecordReadyState::AwaitingConfirmation {
            "Rec ready\u{2026}".to_string()
        } else if state.mic.acquisition_state().get() == MicAcquisitionState::Acquiring {
            "Readying\u{2026}".to_string()
        } else if state.mic.listening().get() && state.mic.mute_output().get() {
            if state.mic.acquisition_state().get() == MicAcquisitionState::Ready {
                "Muted".to_string()
            } else {
                "Readying\u{2026}".to_string()
            }
        } else {
            // Button label is stable — USB mic detection is communicated via
            // the toast + file-panel chip + green LED on the Mic button.
            "Listen".to_string()
        }
    });
    // Icon beside the label: pause while parked/muted, mic otherwise.
    let listen_left_icon = Signal::derive(move || {
        let rec_ready =
            state.mic.record_ready_state().get() == RecordReadyState::AwaitingConfirmation;
        let muted_ready = state.mic.listening().get()
            && state.mic.mute_output().get()
            && state.mic.acquisition_state().get() == MicAcquisitionState::Ready;
        if rec_ready || muted_ready {
            Some(Icon::Pause)
        } else if state.mic.acquisition_state().get() == MicAcquisitionState::Acquiring
            || (state.mic.listening().get() && state.mic.mute_output().get())
        {
            None
        } else {
            Some(Icon::Mic)
        }
    });
    // Right-button is a generic "more options" affordance — the listen
    // mode is determined by HFR/Mode settings elsewhere, so duplicating
    // it here was confusing.
    let listen_right_value = Signal::derive(|| "\u{2026}".to_string()); // …

    let listen_left_click = Callback::new(move |_: web_sys::MouseEvent| {
        if state.mic.strategy().get_untracked() == MicStrategy::None {
            return;
        }
        let st = state;
        wasm_bindgen_futures::spawn_local(async move {
            microphone::toggle_listen(&st).await;
        });
    });
    let listen_toggle_menu = Callback::new(move |()| {
        toggle_panel(&state, LayerPanel::ListenMode);
    });

    view! {
        <ComboButton
            left_label="Mic"
            left_value=listen_left_value
            left_icon=listen_left_icon
            left_click=listen_left_click
            left_class=listen_left_class
            right_value=listen_right_value
            right_class=listen_right_class
            is_open=listen_is_open
            toggle_menu=listen_toggle_menu
            left_title="Toggle live listening (L)"
            right_title="Listen settings"
            menu_direction="above"
            panel_align="right"
            panel_style="min-width: 220px;"
        >
            // ── Listen output (speaker on / warm-up mute) ──
            // Affects live monitoring only — the spectrogram keeps updating
            // either way. Useful for noise-floor learning before unmuting.
            <div class="layer-panel-title">"Listen output"</div>
            <div style="display: flex; gap: 2px; padding: 0 6px 4px;">
                <button class=move || layer_opt_class(!state.mic.mute_output().get())
                    on:click=move |_| state.mic.mute_output().set(false)
                    title="Play processed audio through speakers"
                >"On"</button>
                <button class=move || layer_opt_class(state.mic.mute_output().get())
                    on:click=move |_| state.mic.mute_output().set(true)
                    title="Mute speaker output (mic warm-up). Spectrogram still updates."
                >"Mute (warm-up)"</button>
            </div>

            // ── Buffer size (PS/PV only) ──
            <Show when=move || matches!(state.playback.mode().get(), PlaybackMode::PitchShift | PlaybackMode::PhaseVocoder)>
                <hr />
                <div class="layer-panel-title">"PS/PV Buffer"</div>
                <div style="display: flex; gap: 2px; padding: 0 6px 4px;">
                    <button class=move || layer_opt_class(state.mic.listen_context_samples().get() == 4096)
                        on:click=move |_| state.mic.listen_context_samples().set(4096)
                        title="4096 samples \u{2014} minimum context (more artifacts, lowest latency)"
                    >"4K"</button>
                    <button class=move || layer_opt_class(state.mic.listen_context_samples().get() == 8192)
                        on:click=move |_| state.mic.listen_context_samples().set(8192)
                        title="8192 samples"
                    >"8K"</button>
                    <button class=move || layer_opt_class(state.mic.listen_context_samples().get() == 16384)
                        on:click=move |_| state.mic.listen_context_samples().set(16384)
                        title="16384 samples (default)"
                    >"16K"</button>
                    <button class=move || layer_opt_class(state.mic.listen_context_samples().get() == 32768)
                        on:click=move |_| state.mic.listen_context_samples().set(32768)
                        title="32768 samples (smoother, more CPU)"
                    >"32K"</button>
                    <button class=move || layer_opt_class(state.mic.listen_context_samples().get() == 65536)
                        on:click=move |_| state.mic.listen_context_samples().set(65536)
                        title="65536 samples (smoothest, most CPU)"
                    >"64K"</button>
                </div>
            </Show>

            <hr />
            <div class="layer-panel-hint" style="padding: 4px 8px; font-size: 11px; opacity: 0.65;">
                "Mode, frequency range, and bandpass live in the HFR / Mode buttons."
            </div>
        </ComboButton>
    }
}
