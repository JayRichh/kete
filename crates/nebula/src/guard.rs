//! `kete _guard --session <id>` (K9): the PreToolUse hook a role session's
//! settings file runs on every shell command. Exit 2 with the reason on
//! stderr refuses the command; 0 lets it run.
//!
//! Placeholder from the 0.60 foundation lane: it lets every command run
//! (exit 0), so a placeholder never blocks a shell command. The roles
//! lane owns this file.

/// Check the hook payload on stdin for `session`; the process exit code.
pub fn run(session: String) -> i32 {
    let _ = session;
    0
}
