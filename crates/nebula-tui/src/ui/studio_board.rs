//! Drawing the STUDIO BOARD overlay (K4).
//!
//! Placeholder from the 0.60 foundation lane: an empty state naming the
//! page. The board lane owns this file; the signature is fixed.

use crate::app::{App, Overlay};
use ratatui::layout::Rect;
use ratatui::Frame;

/// Draw the board overlay over `area`.
pub fn draw(f: &mut Frame, app: &mut App, area: Rect) {
    let rect = super::empty_state(f, app, area, " Studio board ", "Not in this build yet.");
    if let Some(Overlay::StudioBoard(view)) = &mut app.modals.overlay {
        view.area = rect;
    }
}
