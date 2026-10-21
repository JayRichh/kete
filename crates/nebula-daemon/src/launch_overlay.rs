//! A LAUNCH OVERLAY: what a role card (0.60, K3) adds to a Claude session's
//! argv on every spawn, resume and restart.
//!
//! The roles lane builds one per session (`Daemon::launch_overlay`, in
//! `registry/roles.rs`); the argv builder (`registry::agent_spawn_command_with`)
//! applies it, in one fixed order so the cached system-prompt prefix stays
//! stable: resume, the harness's permissions flag, model (the launch's own,
//! else the overlay's), effort (likewise), `--permission-mode`,
//! `--strict-mcp-config`, `--tools <list>`, then the system-prompt flag with
//! the overlay's prompt after Kete's guidance and before the PR or issue
//! rule, then the trailing prompt.
//!
//! `--tools` is variadic: Claude's parser keeps taking values until the next
//! option, so a trailing task straight after the tool list would be read as
//! one more tool. The system-prompt flag therefore always follows it, even
//! when it carries nothing else.
//!
//! An empty overlay (`LaunchOverlay::default()`) changes nothing: the argv is
//! 0.59's byte for byte (`registry::tests::the_0_59_claude_argv_is_pinned`).

use std::path::PathBuf;

/// Claude's flag for the starting permission mode.
pub const PERMISSION_MODE_FLAG: &str = "--permission-mode";
/// Claude's flag for running with only the MCP servers `--mcp-config` names
/// (none, when it names none).
pub const STRICT_MCP_FLAG: &str = "--strict-mcp-config";
/// Claude's flag for the built-in tool set itself (variadic).
pub const TOOLS_FLAG: &str = "--tools";

/// What a role card adds to a launch. Every field is optional; the default
/// adds nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LaunchOverlay {
    /// The role's prompt, joined into the appended system prompt after
    /// Kete's guidance and before the PR or issue rule.
    pub append_prompt: Option<String>,
    /// The session's whole built-in tool set (`--tools Read,Write,Bash`).
    pub tools: Option<Vec<String>>,
    /// `--permission-mode <mode>`: `dontAsk`, `bypassPermissions`, ...
    pub permission_mode: Option<String>,
    /// The session's own settings file: it replaces the shared statusLine
    /// file on `--settings`.
    pub settings_path: Option<PathBuf>,
    /// `--strict-mcp-config`: no MCP servers.
    pub strict_mcp: bool,
    /// The model when the launch named none.
    pub model: Option<String>,
    /// The effort when the launch named none.
    pub effort: Option<String>,
}

impl LaunchOverlay {
    /// Adds nothing to a launch.
    pub fn is_empty(&self) -> bool {
        *self == LaunchOverlay::default()
    }

    /// The overlay's flags that sit between the effort and the system
    /// prompt, in their fixed order: `--permission-mode <mode>`,
    /// `--strict-mcp-config`, `--tools <comma-joined list>`.
    pub fn middle_flags(&self) -> Vec<String> {
        let mut args = Vec::new();
        if let Some(mode) = self.permission_mode.as_deref().filter(|m| !m.is_empty()) {
            args.extend([PERMISSION_MODE_FLAG.to_string(), mode.to_string()]);
        }
        if self.strict_mcp {
            args.push(STRICT_MCP_FLAG.to_string());
        }
        if let Some(tools) = &self.tools {
            args.extend([TOOLS_FLAG.to_string(), tools.join(",")]);
        }
        args
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_is_empty_and_adds_no_flags() {
        assert!(LaunchOverlay::default().is_empty());
        assert!(LaunchOverlay::default().middle_flags().is_empty());
        let one = LaunchOverlay {
            strict_mcp: true,
            ..LaunchOverlay::default()
        };
        assert!(!one.is_empty());
    }

    #[test]
    fn middle_flags_keep_their_order() {
        let overlay = LaunchOverlay {
            tools: Some(vec!["Read".into(), "Write".into(), "Bash".into()]),
            permission_mode: Some("dontAsk".into()),
            strict_mcp: true,
            ..LaunchOverlay::default()
        };
        assert_eq!(
            overlay.middle_flags(),
            [
                "--permission-mode",
                "dontAsk",
                "--strict-mcp-config",
                "--tools",
                "Read,Write,Bash"
            ]
        );
    }
}
