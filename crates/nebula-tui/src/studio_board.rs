//! The STUDIO BOARD overlay (K4): a studio repo's departments, one row
//! each, with its queue, lead, runners, last verdict and spend.
//!
//! Placeholder from the 0.60 foundation lane: it opens on an empty state.
//! The board lane owns this file and `ui/studio_board.rs`; the signatures
//! below are fixed (the verb dispatch and the overlay plumbing call them).

use crate::app::{App, Overlay};
use crossterm::event::{KeyCode, KeyEvent, MouseEvent};
use nebula_core::{ClientRequest, ProjectId};
use ratatui::layout::Rect;

/// The board overlay's state. The board lane adds its fields.
#[derive(Debug, Clone, Default)]
pub struct BoardView {
    /// The repo it shows; None: the first studio repo.
    pub repo: Option<ProjectId>,
    /// Where it last drew, for outside clicks.
    pub area: Rect,
}

/// Open the board, on `repo` when the verb came from one.
pub fn open(app: &mut App, repo: Option<ProjectId>) {
    app.modals.overlay = Some(Overlay::StudioBoard(BoardView {
        repo,
        ..BoardView::default()
    }));
    app.chrome.dirty = true;
}

/// Keys over the board: Esc or `q` closes it.
pub fn handle_key(app: &mut App, key: KeyEvent, out: &mut Vec<ClientRequest>) {
    let _ = out;
    if matches!(key.code, KeyCode::Esc | KeyCode::Char('q')) {
        app.modals.overlay = None;
        app.chrome.dirty = true;
    }
}

/// The pointer over the board; true when it took the event.
pub fn handle_mouse(app: &mut App, mouse: MouseEvent, out: &mut Vec<ClientRequest>) -> bool {
    let _ = (app, mouse, out);
    true
}
