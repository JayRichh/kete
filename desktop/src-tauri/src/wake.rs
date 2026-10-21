//! WHAKAOHO (wake-up) in the window: the view's overview, a schedule's
//! save, and Wake now.
//!
//! The overview and Wake now are `kete wake status --json` and `kete wake
//! <account> --force --json`, run as the TUI runs them: the decisions, the
//! runner (with its print fallback) and the log live in one place, the CLI,
//! and the window never works them out a second time. A save is
//! `nebula_core::wake::task::save` in this process: the schedule into
//! `config.local.json` and the scheduled task rewritten to run the same
//! `kete` the window starts its daemon with.

use crate::Res;
use nebula_core::wake::{self, task, WakeSchedule};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

/// How long the overview may take (it asks the daemon for usual hours).
const STATUS_TIMEOUT: Duration = Duration::from_secs(30);
/// How long a Wake now may take: the daemon's session gets two minutes,
/// and a print-mode fallback another two.
const WAKE_TIMEOUT: Duration = Duration::from_secs(300);

fn kete() -> Res<PathBuf> {
    crate::daemon::find_kete()
        .ok_or_else(|| "Can't find kete: install it beside the app or on PATH.".to_string())
}

/// `kete <args>` with no window, its stdout parsed as JSON. A run that
/// exits non-zero still answers with what it printed (Wake now prints the
/// failed report); only no JSON at all is an error.
async fn run_json(kete: &Path, args: &[&str], timeout: Duration) -> Res<serde_json::Value> {
    let mut cmd = tokio::process::Command::new(kete);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    {
        // CREATE_NO_WINDOW: the CLI is a console program.
        cmd.creation_flags(0x0800_0000);
    }
    let out = tokio::time::timeout(timeout, cmd.output())
        .await
        .map_err(|_| format!("kete {} took too long", args.join(" ")))?
        .map_err(|e| format!("couldn't run kete: {e}"))?;
    serde_json::from_slice(&out.stdout).map_err(|_| {
        let said = String::from_utf8_lossy(&out.stderr);
        let said = said.trim().trim_start_matches("Error: ");
        if said.is_empty() {
            format!("kete {} printed nothing to read", args.join(" "))
        } else {
            said.to_string()
        }
    })
}

/// Every Claude account with its schedule, next run, usual hours, the
/// suggested hour, each anchor's preview and the last decisions.
#[tauri::command]
pub async fn wake_overview() -> Res<serde_json::Value> {
    run_json(&kete()?, &["wake", "status", "--json"], STATUS_TIMEOUT).await
}

/// Wake `account` now, whatever its schedule says: the report as `kete
/// wake --json` prints it (a one-item list).
#[tauri::command]
pub async fn wake_now(account: String) -> Res<serde_json::Value> {
    let account = account.trim().to_string();
    if account.is_empty() || account.starts_with('-') {
        return Err("No account named.".into());
    }
    run_json(
        &kete()?,
        &["wake", &account, "--force", "--json"],
        WAKE_TIMEOUT,
    )
    .await
}

/// Is `key` a Claude account among `all`: `@claude`, or a profile for
/// Claude?
fn claude_account(all: &[nebula_core::profiles::Profile], key: &str) -> Res<()> {
    if key == "@claude" {
        return Ok(());
    }
    nebula_core::profiles::resolve(all, "claude", Some(key))?
        .map(drop)
        .ok_or_else(|| format!("{key} isn't a Claude account."))
}

/// Save `account`'s schedule and rewrite the scheduled task; the line
/// that says what the task now does.
#[tauri::command]
pub async fn wake_set(account: String, schedule: WakeSchedule) -> Res<String> {
    let account = account.trim().to_string();
    claude_account(&nebula_core::profiles::load(), &account)?;
    if schedule.anchor_min >= wake::DAY_MIN || !schedule.anchor_min.is_multiple_of(wake::STEP_MIN) {
        return Err("The hour moves in 15-minute steps.".into());
    }
    let exe = kete()?;
    let exe = exe.to_string_lossy();
    let exe = exe.trim_start_matches(r"\\?\").to_string();
    tokio::task::spawn_blocking(move || {
        task::save(&account, Some(&schedule), &exe)?;
        Ok(task::status_line(&wake::load(), Some(&exe)))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_claude_accounts_take_a_schedule() {
        let work = nebula_core::profiles::Profile {
            name: "work".into(),
            harness: "claude".into(),
            env: Default::default(),
            default: false,
            use_default: false,
            disabled: false,
            reserved: false,
        };
        let codex = nebula_core::profiles::Profile {
            name: "cx".into(),
            harness: "codex".into(),
            ..work.clone()
        };
        let all = [work, codex];
        assert!(claude_account(&all, "@claude").is_ok());
        assert!(claude_account(&all, "work").is_ok());
        assert!(claude_account(&all, "cx").is_err(), "a Codex account");
        assert!(claude_account(&all, "@codex").is_err());
        assert!(claude_account(&all, "nobody").is_err());
    }
}
