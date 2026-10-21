//! Drawing the GAUNTLET VIEW overlay (K5).
//!
//! Placeholder from the 0.60 foundation lane: an empty state naming the
//! page. The gauntlet lane owns this file; the signature is fixed.

use crate::app::{App, Overlay};
use ratatui::layout::Rect;
use ratatui::Frame;

/// Draw the gauntlet overlay over `area`.
pub fn draw(f: &mut Frame, app: &mut App, area: Rect) {
    let rect = super::empty_state(f, app, area, " Gauntlet ", "Not in this build yet.");
    if let Some(Overlay::Gauntlet(view)) = &mut app.modals.overlay {
        view.area = rect;
    }
}
