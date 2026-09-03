//! Inline stroke-SVG icon set.
//!
//! One consistent set of 24x24 line icons (Lucide-style geometry, drawn by
//! hand) replacing the emoji / platform-variable glyphs that used to sit in
//! buttons. Icons are `1em` square and use `currentColor`, so the parent's
//! `font-size` and `color` size and tint them exactly like text.

use leptos::prelude::*;

/// Which icon to draw. Names describe the picture, not the action, so the
/// same icon can serve several buttons.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Icon {
    /// Panel with the left pane marked (sidebar toggle).
    SidebarLeft,
    /// Panel with the right pane marked (info panel toggle).
    SidebarRight,
    Mic,
    /// Simple bat silhouette (wings spread, facing viewer).
    Bat,
    /// Filled play triangle.
    Play,
    /// Filled record circle.
    Record,
    /// Filled pause bars.
    Pause,
    /// Gear.
    Settings,
    /// Open hand (pan tool).
    Hand,
    /// Dashed rectangle (marquee select tool).
    Select,
    /// Picture frame (previews / thumbnails).
    Image,
    Trash,
    /// Arrow into a tray (save / download).
    Download,
    /// Three horizontal lines.
    Menu,
    /// Grid (spectrogram overview).
    Grid,
    /// Sine wave (waveform overview).
    Wave,
    ArrowLeft,
    ArrowRight,
    Undo,
    Redo,
    /// Four corner brackets (expand to full range).
    Fullscreen,
    /// Plain window rectangle (snap to current view).
    Window,
    /// Small right-pointing chevron (disclosure / collapsed section).
    ChevronRight,
}

impl Icon {
    /// SVG body (children of the `<svg>` element) for this icon.
    /// Everything inherits `stroke="currentColor" fill="none"` from the
    /// root; the few filled shapes opt in with `fill="currentColor"`.
    fn body(self) -> &'static str {
        match self {
            Icon::SidebarLeft => {
                r#"<rect x="3" y="3" width="18" height="18" rx="2"/><path d="M9 3v18"/><path d="M6 8v8" stroke-width="2.5"/>"#
            }
            Icon::SidebarRight => {
                r#"<rect x="3" y="3" width="18" height="18" rx="2"/><path d="M15 3v18"/><path d="M18 8v8" stroke-width="2.5"/>"#
            }
            Icon::Mic => {
                r#"<path d="M12 2a3 3 0 0 0-3 3v7a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3z"/><path d="M19 10v2a7 7 0 0 1-14 0v-2"/><path d="M12 19v3"/>"#
            }
            Icon::Bat => {
                r#"<path d="M10 3.5l1 3h2l1-3"/><path d="M12 6.5c-1.5 1.5-3 2.5-5 2.5L2 13c2.5 0 3.5 2 4 4.5 1.5-1.5 3-1 4 1 .5-1.5 1-2.5 2-3 1 .5 1.5 1.5 2 3 1-2 2.5-2.5 4-1 .5-2.5 1.5-4.5 4-4.5l-5-4c-2 0-3.5-1-5-2.5z"/>"#
            }
            Icon::Play => r#"<path d="M7 4l13 8-13 8z" fill="currentColor"/>"#,
            Icon::Record => r#"<circle cx="12" cy="12" r="7" fill="currentColor"/>"#,
            Icon::Pause => {
                r#"<rect x="6" y="4" width="4" height="16" rx="1" fill="currentColor"/><rect x="14" y="4" width="4" height="16" rx="1" fill="currentColor"/>"#
            }
            Icon::Settings => {
                r#"<path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z"/><circle cx="12" cy="12" r="3"/>"#
            }
            Icon::Hand => {
                r#"<path d="M18 11V6a2 2 0 0 0-4 0v1"/><path d="M14 10V4a2 2 0 0 0-4 0v2"/><path d="M10 10.5V6a2 2 0 0 0-4 0v8"/><path d="M18 8a2 2 0 1 1 4 0v6a8 8 0 0 1-8 8h-2c-2.8 0-4.5-.86-5.99-2.34l-3.6-3.6a2 2 0 0 1 2.83-2.82L7 15"/>"#
            }
            Icon::Select => {
                r#"<rect x="4" y="4" width="16" height="16" rx="1" stroke-dasharray="3 2.5"/>"#
            }
            Icon::Image => {
                r#"<rect x="3" y="3" width="18" height="18" rx="2"/><circle cx="9" cy="9" r="1.75"/><path d="M21 15l-4.5-4.5L6 21"/>"#
            }
            Icon::Trash => {
                r#"<path d="M3 6h18"/><path d="M8 6V4h8v2"/><path d="M19 6l-1 14H6L5 6"/><path d="M10 11v6"/><path d="M14 11v6"/>"#
            }
            Icon::Download => {
                r#"<path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><path d="M7 10l5 5 5-5"/><path d="M12 15V3"/>"#
            }
            Icon::Menu => r#"<path d="M4 6h16"/><path d="M4 12h16"/><path d="M4 18h16"/>"#,
            Icon::Grid => {
                r#"<rect x="3" y="3" width="18" height="18" rx="2"/><path d="M3 9h18"/><path d="M3 15h18"/><path d="M9 3v18"/><path d="M15 3v18"/>"#
            }
            Icon::Wave => r#"<path d="M2 12c2-6 3-6 5 0s3 6 5 0 3-6 5 0 3 6 5 0"/>"#,
            Icon::ArrowLeft => r#"<path d="M19 12H5"/><path d="M12 5l-7 7 7 7"/>"#,
            Icon::ArrowRight => r#"<path d="M5 12h14"/><path d="M12 5l7 7-7 7"/>"#,
            Icon::Undo => r#"<path d="M3 7v6h6"/><path d="M21 17a9 9 0 0 0-15-6.7L3 13"/>"#,
            Icon::Redo => r#"<path d="M21 7v6h-6"/><path d="M3 17a9 9 0 0 1 15-6.7L21 13"/>"#,
            Icon::Fullscreen => {
                r#"<path d="M8 3H5a2 2 0 0 0-2 2v3"/><path d="M21 8V5a2 2 0 0 0-2-2h-3"/><path d="M3 16v3a2 2 0 0 0 2 2h3"/><path d="M16 21h3a2 2 0 0 0 2-2v-3"/>"#
            }
            Icon::Window => r#"<rect x="3" y="6" width="18" height="12" rx="2"/>"#,
            Icon::ChevronRight => r#"<path d="M9 6l6 6-6 6"/>"#,
        }
    }
}

/// Render an inline SVG icon. Sized by the parent's `font-size` (`1em`),
/// tinted by `color`. Decorative: hidden from assistive tech, so keep the
/// `title=` on the surrounding button.
#[component]
pub fn Icon(kind: Icon, #[prop(default = "")] class: &'static str) -> impl IntoView {
    let class = if class.is_empty() {
        "icon".to_string()
    } else {
        format!("icon {class}")
    };
    view! {
        <svg
            class=class
            viewBox="0 0 24 24"
            width="1em"
            height="1em"
            fill="none"
            stroke="currentColor"
            stroke-width="1.75"
            stroke-linecap="round"
            stroke-linejoin="round"
            aria-hidden="true"
            inner_html=kind.body()
        ></svg>
    }
}
