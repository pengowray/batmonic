// Phone "Hear" sheet — the Hearing Bar's controls as a bottom sheet.
//
// On narrow viewports the Hearing Bar (HFR · Range · modes · PASS · Gain ·
// NR · Notch) scrolls sideways and clips, and its abbreviations have no room
// to explain themselves. On mobile the bar is replaced by one chip showing
// the current playback mode; tapping it opens this sheet, which lists every
// mode with its full name and a one-line consequence, then stacks the
// existing band / output / bandpass / gain / NR / notch combos underneath so
// nothing is lost.
//
// The sheet is always mounted (hidden by CSS when closed) because
// `ModeRadioGroup` owns effects that keep filter settings in sync — they
// must run even while the sheet is closed.
//
// Open state is its own store field (`panels.hear_sheet_open`) rather than a
// `LayerPanel` variant: the combos inside the sheet open their own layer
// panels, and sharing the single `layer_panel_open` slot would close the
// sheet the moment one of them opened.

use crate::state::store_fields::*;
use leptos::portal::Portal;
use leptos::prelude::*;

use crate::components::hearing_bar::{BandHfrCell, BandpassCombo, GainCombo};
use crate::components::hfr_button::RangeButton;
use crate::components::mode_button::{ModeBucket, ModeRadioGroup};
use crate::components::noise_combos::{NotchCombo, NrCombo};
use crate::components::output_range_button::OutputRangeCombo;
use crate::state::AppState;

fn close(state: &AppState) {
    state.panels.hear_sheet_open().set(false);
    state.panels.layer_panel_open().set(None);
}

/// The chip that stands in for the Hearing Bar on phones. Shows the mode
/// currently in effect; tapping toggles the sheet.
#[component]
pub fn HearChip() -> impl IntoView {
    let state = expect_context::<AppState>();
    let bucket = Signal::derive(move || {
        let on = state.viewmode.hfr_enabled().get();
        if on {
            ModeBucket::from_mode(state.playback.mode().get())
        } else {
            ModeBucket::Normal
        }
    });
    let open = move || state.panels.hear_sheet_open().get();
    view! {
        <button
            class=move || if open() { "layer-btn hear-chip open" } else { "layer-btn hear-chip" }
            title="Playback mode, band, gain and filters"
            on:click=move |ev: web_sys::MouseEvent| {
                ev.stop_propagation();
                state.panels.hear_sheet_open().update(|o| *o = !*o);
            }
        >
            <span class="layer-btn-category">"Hear"</span>
            <span class="layer-btn-value">
                {move || bucket.get().name()}
                <span class="hear-chip-abbr">{move || bucket.get().label()}</span>
            </span>
        </button>
    }
}

#[component]
pub fn HearSheet() -> impl IntoView {
    let state = expect_context::<AppState>();
    let open = move || state.panels.hear_sheet_open().get();
    view! {
        <Portal>
            <div
                class=move || if open() { "hear-sheet-backdrop open" } else { "hear-sheet-backdrop" }
                on:click=move |_| close(&state)
            ></div>
            <div
                class=move || if open() { "hear-sheet open" } else { "hear-sheet" }
                on:click=|ev: web_sys::MouseEvent| ev.stop_propagation()
                on:touchstart=|ev: web_sys::TouchEvent| ev.stop_propagation()
            >
                <div class="hear-sheet-handle" on:click=move |_| close(&state)></div>
                <div class="hear-sheet-title">
                    <span>"Hear"</span>
                    <span class="hear-sheet-hint">"How the recording is played back"</span>
                </div>

                <div class="hear-sheet-sec">"Playback mode"</div>
                <ModeRadioGroup list_layout=true/>

                <div class="hear-sheet-sec">"Frequency band"</div>
                <div class="hear-sheet-row">
                    <span class="hear-sheet-key">"Band"</span>
                    <div class="hear-sheet-controls">
                        <BandHfrCell/>
                        <RangeButton/>
                    </div>
                </div>
                <div class="hear-sheet-row">
                    <span class="hear-sheet-key">"Output range"</span>
                    <div class="hear-sheet-controls"><OutputRangeCombo/></div>
                </div>
                <div class="hear-sheet-row">
                    <span class="hear-sheet-key">"Bandpass"</span>
                    <div class="hear-sheet-controls"><BandpassCombo/></div>
                </div>

                <div class="hear-sheet-sec">"Level and filters"</div>
                <div class="hear-sheet-row">
                    <span class="hear-sheet-key">"Gain"</span>
                    <div class="hear-sheet-controls"><GainCombo/></div>
                </div>
                <div class="hear-sheet-row">
                    <span class="hear-sheet-key">"Noise reduction"</span>
                    <div class="hear-sheet-controls"><NrCombo/></div>
                </div>
                <div class="hear-sheet-row">
                    <span class="hear-sheet-key">"Notch filter"</span>
                    <div class="hear-sheet-controls"><NotchCombo/></div>
                </div>
            </div>
        </Portal>
    }
}
