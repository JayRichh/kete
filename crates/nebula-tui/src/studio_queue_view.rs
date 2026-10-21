//! The studio queue's TUI side (K2): the board header's autonomy and
//! runner line, and the autonomy control.
//!
//! Placeholder from the 0.60 foundation lane. The queue lane owns this
//! file; the signatures below are fixed (the board and the verb dispatch
//! call them).

use crate::app::App;
use crossterm::event::KeyEvent;
use nebula_core::ProjectId;
use ratatui::text::Line;

/// The board header's line for `repo`: its autonomy and what the runner
/// is doing ("idle note at 14:05", "holding: you typed 40 s ago").
pub fn header_line(app: &App, repo: Option<&ProjectId>) -> Line<'static> {
    let _ = (app, repo);
    Line::default()
}

/// Open the autonomy control for `repo` (the `studio_autonomy` verb).
pub fn open_autonomy(app: &mut App, repo: Option<ProjectId>) {
    let _ = repo;
    app.chrome.flash = Some("Studio autonomy: not in this build yet".into());
    app.chrome.dirty = true;
}

/// A key while the autonomy control is up; true when it took it.
pub fn handle_key(app: &mut App, key: KeyEvent) -> bool {
    let _ = (app, key);
    false
}
