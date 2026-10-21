//! The usage ledger's CSV export from the TUI (K7, the `usage_csv` verb).
//!
//! Placeholder from the 0.60 foundation lane. The ledger lane owns this
//! file; the signature is fixed (the verb dispatch calls it).

use crate::app::App;

/// Write `<data dir>/exports/usage-<date>.csv` and flash its path.
pub fn run(app: &mut App) {
    app.chrome.flash = Some("Export usage CSV: not in this build yet".into());
    app.chrome.dirty = true;
}
