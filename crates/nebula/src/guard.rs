//! `kete _guard --session <id>` (K9): the PreToolUse hook a role session's
//! settings file runs on every shell command. Exit 2 with the reason on
//! stderr refuses the command; 0 lets it run.
//!
//! It never contacts the daemon, so a hung daemon cannot let a push
//! through. The level is the repo's live one from the `studio` map in
//! `config.local.json`, else the one the session's settings file was
//! written with; a session outside any studio repo has none.

use nebula_core::roles::{self, guard, permissions};
use nebula_core::studio::autonomy::Autonomy;
use std::io::Read;
use std::path::Path;

/// Check the hook payload on stdin for `session`; the process exit code.
pub fn run(session: String) -> i32 {
    let mut text = String::new();
    // Bounded: a payload is one tool call.
    if std::io::stdin()
        .take(4 * 1024 * 1024)
        .read_to_string(&mut text)
        .is_err()
    {
        return refuse("kete guard: could not read the hook payload");
    }
    let Ok(payload) = serde_json::from_str::<serde_json::Value>(&text) else {
        return refuse("kete guard: the hook payload is not JSON");
    };
    let Some(command) = guard::command_of(&payload) else {
        return 0;
    };
    match guard::check(command, autonomy_for(&session)) {
        Ok(()) => 0,
        Err(reason) => refuse(&reason),
    }
}

fn refuse(reason: &str) -> i32 {
    eprintln!("{reason}");
    2
}

/// The level `session`'s commands are checked at.
fn autonomy_for(session: &str) -> Autonomy {
    // Kete writes the id; anything that could leave the sessions folder
    // has no settings file, and is checked as no studio session.
    let safe = !session.is_empty()
        && session
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'));
    let settings = safe
        .then(|| {
            std::fs::read_to_string(roles::session_dir(session).join(roles::SETTINGS_FILE)).ok()
        })
        .flatten()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok());
    let env = settings.as_ref().and_then(|v| v.get("env"));
    let repo = env
        .and_then(|e| e.get(permissions::ENV_STUDIO_REPO))
        .and_then(|v| v.as_str());
    let written = env
        .and_then(|e| e.get(permissions::ENV_STUDIO_AUTONOMY))
        .and_then(|v| v.as_str())
        .and_then(Autonomy::parse);
    let live = repo
        .and_then(|r| nebula_core::studio::settings::for_repo(Path::new(r)))
        .map(|s| s.autonomy);
    live.or(written).unwrap_or(Autonomy::A3)
}
