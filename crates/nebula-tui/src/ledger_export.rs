//! The usage ledger's CSV export from the TUI (K7, the `usage_csv` verb).
//!
//! The key only starts it. The daemon is asked for today's rows, and the
//! file is written, on the blocking pool (`view_jobs`); the footer says
//! where the file went, or why not, when that lands (`Answer::Flash`).

use std::path::{Path, PathBuf};

use crate::app::App;

/// Write `<data dir>/exports/usage-<date>.csv` and flash its path.
pub fn run(app: &mut App) {
    let Some(jobs) = app.jobs.view_jobs.clone() else {
        app.chrome.flash = Some("Usage CSV: not available here".into());
        app.chrome.dirty = true;
        return;
    };
    // Taken here, on the loop: the closure below runs on a blocking thread
    // and needs the runtime to wait on the daemon.
    let runtime = tokio::runtime::Handle::current();
    app.chrome.flash = Some("Writing usage CSV...".into());
    app.chrome.dirty = true;
    jobs.run(move || {
        let said = match runtime.block_on(export()) {
            Ok(path) => format!("Usage CSV written: {}", path.display()),
            Err(why) => format!("Usage CSV: {why}"),
        };
        Some(crate::view_jobs::Answer::Flash(said))
    });
}

/// Today's rows from the daemon, written to [`export_path`].
async fn export() -> Result<PathBuf, String> {
    let day = nebula_core::usage_ledger::local_day(nebula_core::clock::now_secs() as i64);
    let rows = crate::ipc_studio::usage_ledger(Some(day.clone()), Some(day.clone()), None)
        .await
        .map_err(|e| e.to_string())?;
    let path = export_path(&nebula_core::paths::data_dir(), &day);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let mut file = std::fs::File::create(&path).map_err(|e| e.to_string())?;
    nebula_core::usage_ledger::write_csv(&rows, &mut file).map_err(|e| e.to_string())?;
    Ok(path)
}

/// Where the export for `day` (`YYYY-MM-DD`) goes under `data_dir`.
fn export_path(data_dir: &Path, day: &str) -> PathBuf {
    data_dir.join("exports").join(format!("usage-{day}.csv"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_path_is_exports_usage_day_csv() {
        let path = export_path(Path::new("data"), "2026-10-22");
        assert_eq!(
            path,
            Path::new("data")
                .join("exports")
                .join("usage-2026-10-22.csv")
        );
    }
}
