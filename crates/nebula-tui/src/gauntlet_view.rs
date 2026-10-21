//! The GAUNTLET VIEW overlay (K5): every verdict by kind, newest first,
//! with its evidence and the Studio app link.
//!
//! Placeholder from the 0.60 foundation lane: it opens on an empty state.
//! The gauntlet lane owns this file and `ui/gauntlet_view.rs`; the
//! signatures below are fixed (the verb dispatch and the overlay plumbing
//! call them).

use crate::app::{App, Overlay};
use crossterm::event::{KeyCode, KeyEvent, MouseEvent};
use nebula_core::{ClientRequest, ProjectId};
use ratatui::layout::Rect;

/// The gauntlet overlay's state. The gauntlet lane adds its fields.
#[derive(Debug, Clone, Default)]
pub struct GauntletView {
    /// The repo it shows; None: the first studio repo.
    pub repo: Option<ProjectId>,
    /// The verdict it opened on, as (kind, id).
    pub item: Option<(String, String)>,
    /// Where it last drew, for outside clicks.
    pub area: Rect,
}

/// Open the gauntlet view, on `repo` when the verb came from one.
pub fn open(app: &mut App, repo: Option<ProjectId>) {
    app.modals.overlay = Some(Overlay::Gauntlet(GauntletView {
        repo,
        ..GauntletView::default()
    }));
    app.chrome.dirty = true;
}

/// Open the gauntlet view on one verdict (the board's verdict cell).
pub fn open_on(app: &mut App, repo: Option<ProjectId>, kind: &str, id: &str) {
    app.modals.overlay = Some(Overlay::Gauntlet(GauntletView {
        repo,
        item: Some((kind.to_string(), id.to_string())),
        ..GauntletView::default()
    }));
    app.chrome.dirty = true;
}

/// Keys over the view: Esc or `q` closes it.
pub fn handle_key(app: &mut App, key: KeyEvent, out: &mut Vec<ClientRequest>) {
    let _ = out;
    if matches!(key.code, KeyCode::Esc | KeyCode::Char('q')) {
        app.modals.overlay = None;
        app.chrome.dirty = true;
    }
}

/// The pointer over the view; true when it took the event.
pub fn handle_mouse(app: &mut App, mouse: MouseEvent, out: &mut Vec<ClientRequest>) -> bool {
    let _ = (app, mouse, out);
    true
}
