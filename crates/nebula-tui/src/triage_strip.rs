//! The TRIAGE STRIP (K6): one line at the bottom of the orchestrator view
//! that files an inbox note and hands it to the triage desk.
//!
//! Placeholder from the 0.60 foundation lane: it takes no rows and draws
//! nothing. The triage lane owns this file; the signatures below are
//! fixed (`orchestrator_view::split` and the chip-mode `t` call them).

use crate::app::App;
use crossterm::event::KeyEvent;
use ratatui::layout::Rect;
use ratatui::Frame;

/// Rows the strip takes at the bottom of the orchestrator view.
pub fn rows(app: &App) -> u16 {
    let _ = app;
    0
}

/// Open the triage line (chip mode `t`, the `triage_note` verb).
pub fn open(app: &mut App) {
    app.chrome.flash = Some("Triage note: not in this build yet".into());
    app.chrome.dirty = true;
}

/// A key while the strip has the focus; true when it took it.
pub fn handle_key(app: &mut App, key: KeyEvent) -> bool {
    let _ = (app, key);
    false
}

/// Draw the strip into `area` (the rows [`rows`] asked for).
pub fn draw(f: &mut Frame, app: &mut App, area: Rect) {
    let _ = (f, app, area);
}
