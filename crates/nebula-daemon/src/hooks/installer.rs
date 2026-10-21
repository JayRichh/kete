//! Managed-hook installation into the agent CLI's config:
//! `<worktree>/.claude/settings.local.json` (Claude Code),
//! `~/.codex/hooks.json` (Codex CLI), `<worktree>/.cursor/hooks.json`
//! (Cursor CLI), or `~/.copilot/hooks/kete.json` (Copilot CLI, a file of
//! its own in its own dialect). Pi and OpenCode have no hook file — their managed
//! TypeScript extension and plugin are `pi_extension.rs` and
//! `opencode_plugin.rs`, which share this module's atomic writer.
//!
//! Claude and Codex share one hooks dialect (PascalCase event names, groups
//! of `{"hooks": [{"type": "command", ...}]}`). Cursor speaks its own
//! (verified against cursor-agent 2026.08 + cursor.com/docs/agent/hooks):
//! camelCase event names (`beforeSubmitPrompt`, `stop`, ...), flat
//! `{"command": ...}` entries, a required top-level `"version": 1`, and each
//! hook must print a JSON response (`{"continue": true}`) to stdout or
//! gating events fall back to fail-open error handling.
//!
//! Rules (learned the hard way in mission-control):
//! - MERGE, never replace: user hooks are preserved untouched.
//! - Our groups carry `_nebulaManaged: true` and are stripped + rebuilt on
//!   every spawn, so upgrades never accumulate duplicates. A legacy-signature
//!   check (command contains our endpoint + env var) catches untagged strays.
//! - A corrupt file ABORTS the install — never clobber user data.
//! - Commands are env-guarded, so the hooks are inert when the user runs
//!   `claude`/`codex`/`cursor-agent` outside nebula (no NEBULA_* in env →
//!   exit 0).
//!
//! Codex caveat, and the reason its hooks are the one set that does NOT go
//! in the worktree: codex gates every hook behind a startup "Hooks need
//! review" prompt and records the approval in `~/.codex/config.toml
//! [hooks.state]` under the hook FILE'S PATH. Project-local
//! `.codex/hooks.json` therefore re-prompts in every fresh worktree — and
//! an unanswered prompt means no hooks at all, so status stayed grey and no
//! title was ever injected. Installing into `$CODEX_HOME/hooks.json`
//! (default `~/.codex`) makes that path stable: the user trusts nebula's
//! hooks once and every later worktree is silent. The commands stay
//! env-guarded, so they remain inert in codex sessions outside nebula.

use anyhow::{bail, Context, Result};
use nebula_core::env::AGENT_ID;
#[cfg(unix)]
use nebula_core::env::{API_TOKEN, API_URL};
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

/// The hooks file's name under codex's home and cursor's per-worktree dir.
/// Claude keeps its hooks in its settings file instead.
const HOOKS_FILE: &str = "hooks.json";
/// Claude Code's per-checkout settings: hooks and permission rules both.
const CLAUDE_SETTINGS_FILE: &str = "settings.local.json";
/// The CLIs' config directories: `.claude` and `.cursor` are per-worktree;
/// `.codex` is both the worktree-local one an older nebula wrote (pruned
/// now) and the name under `$HOME` when `$CODEX_HOME` is unset.
const CLAUDE_DIR: &str = ".claude";
const CURSOR_DIR: &str = ".cursor";
const CODEX_DIR: &str = ".codex";
/// How long a hook waits on the daemon. Hooks run inline in the agent's
/// turn, so a hung daemon has to cost seconds, not a stuck session.
#[cfg(unix)]
const HOOK_CURL_TIMEOUT_SECS: u32 = 3;

/// Claude's PreToolUse matcher: a regex over the tool name.
pub(crate) const PRE_TOOL_USE_MATCHER: &str = "^(AskUserQuestion|Agent|Task)$";

/// (hook event, optional matcher)
const CLAUDE_EVENTS: &[(&str, Option<&str>)] = &[
    ("UserPromptSubmit", None),
    ("Stop", None),
    // Fired instead of Stop when the turn ends on an API error: a usage
    // limit Claude waits out, a failed login, an overload. Unmatched, so
    // every error kind reaches the status machine (see status.rs).
    ("StopFailure", None),
    ("SessionStart", None),
    ("PermissionRequest", None),
    // No matcher: Claude filters Notification hooks by notification_type
    // before dispatch, and we need `idle_prompt` ("Claude is waiting for
    // your input") as well as `permission_prompt` — it is the only signal
    // that arrives when a turn ends without a Stop — and the usage-limit
    // wait's `quota_auto_resume_*` and the blocking dialogs' types too.
    // See status.rs.
    ("Notification", None),
    // The question (the row goes red), and the call that starts a
    // subagent: its `description` is the task the overview shows on the
    // subagent's row (`registry::subagents`). Anchored, so `TaskCreate`
    // and its kin stay out.
    ("PreToolUse", Some(PRE_TOOL_USE_MATCHER)),
    // Every tool's end, unmatched. Three signals ride it: the answer to an
    // `AskUserQuestion` (the row leaves red); the approval of a permission
    // prompt, which fires no hook of its own — the gated tool simply runs,
    // and its PostToolUse is the first word that the user said yes, for
    // Edit and WebFetch as much as for Bash (see status.rs); and the
    // session's position — Claude reports its working directory on every
    // payload, and EnterWorktree/ExitWorktree (what "do this in a
    // worktree" actually runs) or a Bash `cd` move it, so the row re-homes
    // seconds after the move instead of at the turn's Stop, which can be
    // many minutes later (see registry::reparent_agent_by_cwd).
    ("PostToolUse", None),
    ("SubagentStart", None),
    ("SubagentStop", None),
];

/// Codex has no Notification hook and no AskUserQuestion tool; its native
/// PermissionRequest covers the waiting-on-user state.
const CODEX_EVENTS: &[(&str, Option<&str>)] = &[
    ("UserPromptSubmit", None),
    ("Stop", None),
    ("SessionStart", None),
    ("PermissionRequest", None),
    ("SubagentStart", None),
    ("SubagentStop", None),
];

/// (cursor hook event, nebula hookEvent query value). Cursor has no
/// PermissionRequest hook and nebula always runs cursor-agent with
/// `--force`; its `AskQuestion` dialog fires no tool hook at all, so that
/// wait is read off the screen instead (`pty::question`).
/// `sessionEnd` is skipped: PTY-exit synthetics already cover agent death.
const CURSOR_EVENTS: &[(&str, &str)] = &[
    ("sessionStart", "SessionStart"),
    ("beforeSubmitPrompt", "UserPromptSubmit"),
    ("stop", "Stop"),
    ("subagentStart", "SubagentStart"),
    ("subagentStop", "SubagentStop"),
];

/// (copilot hook event, nebula hookEvent query value), verified against
/// Copilot CLI 1.0.96 by a probe file logging every payload. Copilot's
/// events are camelCase and its payloads Claude-shaped under other names
/// (`sessionId`, `toolName` — aliased by the receiver). `permissionRequest`
/// fires before a tool that needs asking, and the tool's `postToolUse` is
/// the first word that the user said yes, as on Claude. No subagent pair:
/// a `Stop` gated on subagents nobody announced would hold forever.
/// `sessionEnd` is skipped: PTY-exit synthetics already cover agent death.
const COPILOT_EVENTS: &[(&str, &str)] = &[
    ("sessionStart", "SessionStart"),
    ("userPromptSubmitted", "UserPromptSubmit"),
    ("agentStop", "Stop"),
    ("permissionRequest", "PermissionRequest"),
    ("postToolUse", "PostToolUse"),
];

/// Copilot's user-level hooks directory under its home: every `*.json` in
/// it is loaded at startup, before (and whatever the answer to) the folder
/// trust dialog, so one managed file there serves every worktree.
const COPILOT_HOOKS_DIR: &str = "hooks";
/// The managed file in [`COPILOT_HOOKS_DIR`]: wholly nebula's, rewritten
/// on every spawn, so no merge with the user's own hook files is needed.
const COPILOT_HOOKS_FILE: &str = "kete.json";
/// The same file under Kete's old name. Copilot loads every file in the
/// dir, so it goes when the new one is written, or each event fires twice.
const LEGACY_COPILOT_HOOKS_FILE: &str = "bro.json";
/// `~/.copilot`, Copilot's home when `$COPILOT_HOME` is unset.
const COPILOT_DIR: &str = ".copilot";
/// How long Copilot lets a managed hook run before abandoning it.
const COPILOT_HOOK_TIMEOUT_SECS: u32 = 10;

/// Windows: no `curl` + POSIX `${VAR}` one-liner — the CLI may run its
/// hooks under cmd, PowerShell or Git Bash. The hook is this binary's own
/// `_hook <endpoint> <event>` subcommand instead, which does what the
/// one-liner does: inert without the session env, the stdin payload POSTed
/// to the daemon, the reply on stdout for `UserPromptSubmit` only, and
/// cursor's `{"continue": true}` either way.
#[cfg(windows)]
fn hook_subcommand(endpoint: &str, event: &str) -> String {
    let exe = std::env::current_exe()
        .map(|p| shell_neutral_path(&p.display().to_string()))
        .unwrap_or_else(|_| "kete".into());
    format!("{exe} {HOOK_SUBCOMMAND} {endpoint} {event}")
}

/// `path` spelled so bash, cmd and PowerShell all run it as the command
/// word: `/` separators (bash eats a bare `\`), and the part after the
/// drive quoted (spaces survive, and cmd reads no `/` as a switch) while
/// the drive stays bare (PowerShell takes a line opening with a quote as a
/// string, not a command): `C:"/Program Files/kete/kete.exe"`.
#[cfg(windows)]
fn shell_neutral_path(path: &str) -> String {
    let path = path
        .strip_prefix(r"\\?\")
        .unwrap_or(path)
        .replace('\\', "/");
    let bytes = path.as_bytes();
    if bytes.len() > 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        format!("{}\"{}\"", &path[..2], &path[2..])
    } else {
        format!("\"{path}\"")
    }
}

/// The hidden subcommand [`hook_subcommand`] runs, and what marks a
/// command as nebula's when an older install left it untagged.
#[cfg(windows)]
pub const HOOK_SUBCOMMAND: &str = "_hook";

#[cfg(windows)]
fn hook_command(endpoint: &str, event: &str) -> String {
    hook_subcommand(endpoint, event)
}

#[cfg(windows)]
fn cursor_hook_command(event: &str) -> String {
    hook_subcommand("cursor", event)
}

/// Copilot runs the `powershell` command of a hook on Windows: the same
/// subcommand, which keeps its stdout empty for this route.
#[cfg(windows)]
fn copilot_hook_command(event: &str) -> String {
    hook_subcommand("copilot", event)
}

/// One Claude hook in EXEC FORM (Claude Code 2.1.139+): `command` is this
/// binary, spawned directly with `args` as its argument vector — no Git
/// Bash, cmd or PowerShell in between, so no shell process per hook, no
/// path quoting to survive three shells, and nothing for a machine without
/// Git for Windows to fail on (2.1.299 blocks a guard hook that fails).
/// Codex, Cursor and Copilot keep their shell form: their exec form is
/// unverified.
#[cfg(windows)]
fn claude_exec_hook(event: &str) -> Value {
    let exe = std::env::current_exe()
        .map(|p| {
            let path = p.display().to_string();
            path.strip_prefix(r"\\?\").unwrap_or(&path).to_string()
        })
        .unwrap_or_else(|_| "kete".into());
    json!({
        "type": "command",
        "command": exe,
        "args": [HOOK_SUBCOMMAND, "claude", event],
    })
}

/// A hook command that is nebula's: the curl one-liner's signature, the
/// `_hook` subcommand in shell form, or (a Claude hook in exec form) an
/// argument vector opening with `_hook`.
#[cfg(windows)]
fn is_nebula_hook(hook: &Value) -> bool {
    let shell_form = hook
        .get("command")
        .and_then(Value::as_str)
        .is_some_and(|c| {
            (c.contains("/api/hooks/") && c.contains(AGENT_ID))
                || c.contains(&format!(" {HOOK_SUBCOMMAND} "))
        });
    let exec_form = hook
        .get("args")
        .and_then(Value::as_array)
        .and_then(|args| args.first())
        .and_then(Value::as_str)
        == Some(HOOK_SUBCOMMAND);
    shell_form || exec_form
}

/// The shell test every hook opens with. Outside nebula (a bare `claude`
/// in the same checkout) the session env is absent and the hook must be
/// inert — exit before curl ever runs.
#[cfg(unix)]
fn env_guard() -> String {
    format!("[ -z \"${AGENT_ID}\" ] || [ -z \"${API_URL}\" ]")
}

/// The POST itself, identical for every dialect: bearer auth, the hook's
/// stdin payload passed straight through, and the agent id plus event
/// name on the query string so the daemon needs nothing else to route it.
#[cfg(unix)]
fn hook_curl(endpoint: &str, event: &str) -> String {
    format!(
        "curl -sS -m {HOOK_CURL_TIMEOUT_SECS} -X POST -H \"Authorization: Bearer ${API_TOKEN}\" \
         -H \"Content-Type: application/json\" --data-binary @- \
         \"${API_URL}/api/hooks/{endpoint}?agentId=${AGENT_ID}&hookEvent={event}\""
    )
}

#[cfg(unix)]
fn hook_command(endpoint: &str, event: &str) -> String {
    // UserPromptSubmit passes the daemon's response body through to stdout:
    // Claude Code (and Codex, same dialect) add a hook's stdout to the
    // model's context, which is how the session auto-title instruction
    // reaches the agent. The daemon keeps that body empty except when an
    // instruction is due. Every other event stays fully silent.
    let silence = if event == "UserPromptSubmit" {
        "2>/dev/null"
    } else {
        ">/dev/null 2>&1"
    };
    format!(
        "if {}; then exit 0; fi; {} {silence} || true",
        env_guard(),
        hook_curl(endpoint, event)
    )
}

/// Permission rules letting Claude Code run nebula's own commands without a
/// permission prompt: the auto-title `nebula rename`, and the `nebula
/// worktree` relocation, `nebula spawn` sibling and `nebula open` file tabs
/// its appended system prompt tells it to use (codex/cursor run with their
/// skip-permissions flags). The same commands as PowerShell rules too,
/// for Claude's PowerShell tool — what a Windows machine without Git Bash
/// runs commands with — in the documented `PowerShell(cmd *)` form.
/// 0.60 adds the read-only studio commands the orchestrator's guidance
/// teaches (`kete studio status`, `queue`, `note`, `kete usage`, `kete
/// profile list` and `pick`), `kete orchestrator renew` and `kete state`
/// (the state map, read-only); never the
/// ones that change what a studio may do (`kete studio autonomy`,
/// `enable`, `disable`, `exclude`, `include`).
const CLAUDE_ALLOW_RULES: &[&str] = &[
    "Bash(kete rename:*)",
    "Bash(kete worktree:*)",
    "Bash(kete spawn:*)",
    "Bash(kete open:*)",
    "Bash(kete sessions:*)",
    "Bash(kete read:*)",
    "Bash(kete ask:*)",
    "Bash(kete studio status:*)",
    "Bash(kete studio queue:*)",
    "Bash(kete studio note:*)",
    "Bash(kete usage:*)",
    "Bash(kete profile list:*)",
    "Bash(kete profile pick:*)",
    "Bash(kete orchestrator renew:*)",
    "Bash(kete state:*)",
    "Bash(bro rename:*)",
    "Bash(bro worktree:*)",
    "Bash(bro spawn:*)",
    "Bash(bro open:*)",
    "Bash(bro sessions:*)",
    "Bash(bro read:*)",
    "Bash(bro ask:*)",
    "Bash(bro studio status:*)",
    "Bash(bro studio queue:*)",
    "Bash(bro studio note:*)",
    "Bash(bro usage:*)",
    "Bash(bro profile list:*)",
    "Bash(bro profile pick:*)",
    "Bash(bro orchestrator renew:*)",
    "Bash(bro state:*)",
    "PowerShell(kete rename *)",
    "PowerShell(kete worktree *)",
    "PowerShell(kete spawn *)",
    "PowerShell(kete open *)",
    "PowerShell(kete sessions *)",
    "PowerShell(kete read *)",
    "PowerShell(kete ask *)",
    "PowerShell(kete studio status *)",
    "PowerShell(kete studio queue *)",
    "PowerShell(kete studio note *)",
    "PowerShell(kete usage *)",
    "PowerShell(kete profile list *)",
    "PowerShell(kete profile pick *)",
    "PowerShell(kete orchestrator renew *)",
    "PowerShell(kete state *)",
    "PowerShell(bro rename *)",
    "PowerShell(bro worktree *)",
    "PowerShell(bro spawn *)",
    "PowerShell(bro open *)",
    "PowerShell(bro sessions *)",
    "PowerShell(bro read *)",
    "PowerShell(bro ask *)",
    "PowerShell(bro studio status *)",
    "PowerShell(bro studio queue *)",
    "PowerShell(bro studio note *)",
    "PowerShell(bro usage *)",
    "PowerShell(bro profile list *)",
    "PowerShell(bro profile pick *)",
    "PowerShell(bro orchestrator renew *)",
    "PowerShell(bro state *)",
];

/// Cursor variant: the payload arrives on stdin like Claude's, but cursor
/// expects a JSON response on stdout — `{"continue": true}` keeps gating
/// events (beforeSubmitPrompt) flowing and is ignored by the rest.
#[cfg(unix)]
fn cursor_hook_command(event: &str) -> String {
    format!(
        "if {}; then printf '{{\"continue\": true}}\\n'; exit 0; fi; \
         {} >/dev/null 2>&1 || true; printf '{{\"continue\": true}}\\n'",
        env_guard(),
        hook_curl("cursor", event)
    )
}

/// Copilot variant: fully silent — Copilot reads nothing back from these
/// events, so no reply body has anywhere to go.
#[cfg(unix)]
fn copilot_hook_command(event: &str) -> String {
    format!(
        "if {}; then exit 0; fi; {} >/dev/null 2>&1 || true",
        env_guard(),
        hook_curl("copilot", event)
    )
}

/// A hook command that is nebula's: the curl one-liner's signature.
#[cfg(unix)]
fn is_nebula_hook(hook: &Value) -> bool {
    hook.get("command")
        .and_then(Value::as_str)
        .is_some_and(|c| c.contains("/api/hooks/") && c.contains(AGENT_ID))
}

fn is_nebula_group(group: &Value) -> bool {
    if group.get("_nebulaManaged").and_then(Value::as_bool) == Some(true) {
        return true;
    }
    // Legacy/untagged detection by command signature — nested Claude/Codex
    // shape and flat Cursor shape both.
    if is_nebula_hook(group) {
        return true;
    }
    group
        .get("hooks")
        .and_then(Value::as_array)
        .map(|hooks| hooks.iter().any(is_nebula_hook))
        .unwrap_or(false)
}

/// The hook a managed group runs for `event` on `endpoint`: on Windows a
/// Claude hook goes in exec form ([`claude_exec_hook`]); every other one,
/// and every hook elsewhere, is a shell command.
fn managed_hook(endpoint: &str, event: &str) -> Value {
    #[cfg(windows)]
    if endpoint == "claude" {
        return claude_exec_hook(event);
    }
    json!({ "type": "command", "command": hook_command(endpoint, event) })
}

fn managed_group(endpoint: &str, event: &str, matcher: Option<&str>) -> Value {
    let mut group = Map::new();
    if let Some(m) = matcher {
        group.insert("matcher".into(), json!(m));
    }
    group.insert("hooks".into(), json!([managed_hook(endpoint, event)]));
    group.insert("_nebulaManaged".into(), json!(true));
    Value::Object(group)
}

/// Merge nebula's managed hooks for Claude Code into
/// `<cwd>/.claude/settings.local.json`, plus the permission rule that lets
/// the auto-title `nebula rename` run unprompted, and attribution off: no
/// co-author trailer on its commits, no footer on its PRs. The file is
/// kept out of `git status` through the repository's shared
/// `info/exclude`: Kete writes it before Claude ever runs, so Claude's own
/// ignore of it may not be there yet, and a fresh worktree read as one
/// changed file (a Review card with nothing done) that an agent's
/// `git add -A` then committed.
pub fn install_claude_hooks(cwd: &Path) -> Result<()> {
    install_managed_hooks(
        &cwd.join(CLAUDE_DIR),
        CLAUDE_SETTINGS_FILE,
        "claude",
        CLAUDE_EVENTS,
        CLAUDE_ALLOW_RULES,
        claude_attribution_off,
    )?;
    // Not a git checkout (a bare folder): nothing to keep it out of.
    match git_common_dir(cwd) {
        Ok(common) => ensure_excluded(&common, CLAUDE_SETTINGS_EXCLUDE),
        Err(_) => Ok(()),
    }
}

/// [`CLAUDE_SETTINGS_FILE`] as a line of `info/exclude`.
const CLAUDE_SETTINGS_EXCLUDE: &str = ".claude/settings.local.json";

/// The hidden subcommand kete's Claude statusLine runs (`kete
/// _statusline`), and what marks a `statusLine` as kete's.
pub const STATUSLINE_SUBCOMMAND: &str = "_statusline";

/// The statusLine command: this binary's own `_statusline`, spelled for
/// whichever shell Claude runs it under.
fn statusline_command() -> String {
    let exe = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "kete".into());
    #[cfg(windows)]
    let exe = shell_neutral_path(&exe);
    #[cfg(unix)]
    let exe = nebula_core::shell::single_quote(&exe);
    format!("{exe} {STATUSLINE_SUBCOMMAND}")
}

fn is_kete_statusline(value: &Value) -> bool {
    value
        .get("command")
        .and_then(Value::as_str)
        .is_some_and(|c| c.contains(&format!(" {STATUSLINE_SUBCOMMAND}")))
}

/// The kete-owned settings file a Claude session gets through
/// `--settings`: only kete's `statusLine`. It lives in the data dir, so no
/// repository file carries it, and a plain `claude` run outside kete
/// keeps Claude's own footer.
pub const STATUSLINE_SETTINGS_FILE: &str = "claude-statusline.json";

/// Write [`STATUSLINE_SETTINGS_FILE`] into `dir` with this binary's
/// current path, unless it already says exactly that. Returns its path.
pub fn write_statusline_settings(dir: &Path) -> Result<PathBuf> {
    let text = serde_json::to_string_pretty(&json!({
        "statusLine": { "type": "command", "command": statusline_command(), "padding": 0 }
    }))?;
    install_unless_unchanged(dir, STATUSLINE_SETTINGS_FILE, &text)?;
    Ok(dir.join(STATUSLINE_SETTINGS_FILE))
}

/// One-release migration: a 0.45 dev build wrote kete's `statusLine` into
/// `<cwd>/.claude/settings.local.json`. Take it out again when it is
/// kete's own (`… _statusline`); a statusLine of the user's is never
/// touched. Writes only when something changed.
pub fn strip_kete_statusline(cwd: &Path) -> Result<()> {
    let dir = cwd.join(CLAUDE_DIR);
    let path = dir.join(CLAUDE_SETTINGS_FILE);
    if !path.exists() {
        return Ok(());
    }
    let mut root = load_hooks_root(&path)?;
    let root_obj = root_object_mut(&mut root, &path)?;
    if !root_obj.get("statusLine").is_some_and(is_kete_statusline) {
        return Ok(());
    }
    root_obj.remove("statusLine");
    write_hooks_root(&dir, CLAUDE_SETTINGS_FILE, &root)
}

/// Claude Code's two attribution switches, forced off on every spawn
/// whatever the file said: the legacy boolean and the newer per-surface
/// text (an empty string writes nothing).
fn claude_attribution_off(root_obj: &mut Map<String, Value>) {
    root_obj.insert("includeCoAuthoredBy".into(), json!(false));
    root_obj.insert("attribution".into(), json!({ "commit": "", "pr": "" }));
}

/// Copilot's per-checkout settings, relative to the worktree root.
const COPILOT_WORKTREE_SETTINGS_DIR: &str = ".github/copilot";
const COPILOT_WORKTREE_SETTINGS_FILE: &str = "settings.local.json";
/// The same path as a line of `info/exclude`.
const COPILOT_WORKTREE_SETTINGS_EXCLUDE: &str = ".github/copilot/settings.local.json";

/// Turn Copilot's co-author trailer off in `<cwd>/.github/copilot/
/// settings.local.json` (merged: anything else the user set stays), and
/// keep that file out of `git status` through the repository's shared
/// `info/exclude`, so it never rides an agent's commit.
pub fn install_copilot_worktree_settings(cwd: &Path) -> Result<()> {
    let dir = cwd.join(COPILOT_WORKTREE_SETTINGS_DIR);
    let path = dir.join(COPILOT_WORKTREE_SETTINGS_FILE);
    let mut root = load_hooks_root(&path)?;
    let root_obj = root_object_mut(&mut root, &path)?;
    if root_obj.get("includeCoAuthoredBy") != Some(&json!(false)) {
        root_obj.insert("includeCoAuthoredBy".into(), json!(false));
        write_hooks_root(&dir, COPILOT_WORKTREE_SETTINGS_FILE, &root)?;
    }
    ensure_excluded(&git_common_dir(cwd)?, COPILOT_WORKTREE_SETTINGS_EXCLUDE)
}

/// The repository's shared git dir for the checkout at `cwd`, read off the
/// filesystem: `.git` itself in a main checkout, else the `gitdir:` a
/// linked worktree's `.git` file names, followed through its `commondir`.
fn git_common_dir(cwd: &Path) -> Result<PathBuf> {
    let dot_git = cwd.join(".git");
    if dot_git.is_dir() {
        return Ok(dot_git);
    }
    let text =
        std::fs::read_to_string(&dot_git).with_context(|| format!("read {}", dot_git.display()))?;
    let Some(gitdir) = text.trim().strip_prefix("gitdir:") else {
        bail!("{} names no gitdir", dot_git.display());
    };
    let gitdir = cwd.join(gitdir.trim());
    match std::fs::read_to_string(gitdir.join("commondir")) {
        Ok(common) => Ok(gitdir.join(common.trim())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(gitdir),
        Err(e) => Err(e).with_context(|| format!("read {}", gitdir.join("commondir").display())),
    }
}

/// Add `line` to `<common_dir>/info/exclude` unless it is already there.
fn ensure_excluded(common_dir: &Path, line: &str) -> Result<()> {
    let info = common_dir.join("info");
    let path = info.join("exclude");
    let existing = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e).with_context(|| format!("read {}", path.display())),
    };
    if existing.lines().any(|l| l.trim() == line) {
        return Ok(());
    }
    let mut text = existing;
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(line);
    text.push('\n');
    write_text_atomic(&info, "exclude", &text)
}

/// Idempotently add one entry to `permissions.allow`, preserving everything
/// the user put there. Same abort-don't-clobber policy as the hook merge.
fn ensure_permission_allow(
    root_obj: &mut Map<String, Value>,
    entry: &str,
    path: &Path,
) -> Result<()> {
    let perms = root_obj.entry("permissions").or_insert_with(|| json!({}));
    let perms_obj = object_mut(perms, "\"permissions\"", path)?;
    let allow = perms_obj.entry("allow").or_insert_with(|| json!([]));
    let allow_arr = array_mut(allow, "permissions.allow", path)?;
    if !allow_arr.iter().any(|v| v.as_str() == Some(entry)) {
        allow_arr.push(json!(entry));
    }
    Ok(())
}

/// The abort-don't-clobber gate on a config file's root: anything but an
/// object is not a config nebula understands, so it is left exactly as
/// found and the install fails instead.
fn root_object_mut<'a>(root: &'a mut Value, path: &Path) -> Result<&'a mut Map<String, Value>> {
    match root.as_object_mut() {
        Some(obj) => Ok(obj),
        None => bail!(
            "{} is not a JSON object: refusing to modify it",
            path.display()
        ),
    }
}

/// `v` as a mutable object, or the same refusal naming the field (`what`,
/// spelled as the message should show it) inside `path`.
fn object_mut<'a>(v: &'a mut Value, what: &str, path: &Path) -> Result<&'a mut Map<String, Value>> {
    match v.as_object_mut() {
        Some(obj) => Ok(obj),
        None => bail!(
            "{what} in {} is not an object: refusing to modify it",
            path.display()
        ),
    }
}

/// Array twin of [`object_mut`].
fn array_mut<'a>(v: &'a mut Value, what: &str, path: &Path) -> Result<&'a mut Vec<Value>> {
    match v.as_array_mut() {
        Some(arr) => Ok(arr),
        None => bail!(
            "{what} in {} is not an array: refusing to modify it",
            path.display()
        ),
    }
}

/// Codex's home (`$CODEX_HOME`, else `~/.codex`) — where its hooks live so
/// one trust approval covers every worktree. See the module header.
pub fn codex_home() -> PathBuf {
    nebula_core::env::non_empty(nebula_core::env::CODEX_HOME)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            nebula_core::env::home_dir()
                .unwrap_or_default()
                .join(CODEX_DIR)
        })
}

/// Merge nebula's managed hooks for Codex into `<codex_home>/hooks.json`.
pub fn install_codex_hooks(codex_home: &Path) -> Result<()> {
    install_managed_hooks(codex_home, HOOKS_FILE, "codex", CODEX_EVENTS, &[], |_| {})
}

/// Drop the per-worktree `.codex/hooks.json` groups an older nebula wrote:
/// left in place they are a second, never-trusted copy of the same hooks
/// that codex would re-prompt for. Foreign groups (mission-control's) stay;
/// a file left holding nothing at all is removed, and so is a `.codex`
/// directory that existed only for it.
pub fn prune_codex_worktree_hooks(cwd: &Path) -> Result<()> {
    let dir = cwd.join(CODEX_DIR);
    let path = dir.join(HOOKS_FILE);
    if !path.exists() {
        return Ok(());
    }
    let mut root = load_hooks_root(&path)?;
    let Some(hooks_obj) = root
        .as_object_mut()
        .and_then(|o| o.get_mut("hooks"))
        .and_then(Value::as_object_mut)
    else {
        return Ok(());
    };
    let before = hooks_obj.len();
    purge_nebula_groups(hooks_obj);
    let emptied = hooks_obj.is_empty();
    if emptied && before > 0 && root.as_object().map(|o| o.len()) == Some(1) {
        std::fs::remove_file(&path).with_context(|| format!("remove {}", path.display()))?;
        // Only ours to remove, and only while it holds nothing else.
        let _ = std::fs::remove_dir(&dir);
        return Ok(());
    }
    write_hooks_root(&dir, HOOKS_FILE, &root)
}

/// Strip nebula's groups from under EVERY event key of a loaded hooks
/// object (not just the ones about to be reinstalled: stale keys from an
/// older install shape must go, and events nebula may drop in the future
/// must not linger), then drop the keys that emptied. User groups are left
/// exactly as found.
fn purge_nebula_groups(hooks_obj: &mut Map<String, Value>) {
    for (_, groups) in hooks_obj.iter_mut() {
        if let Some(arr) = groups.as_array_mut() {
            arr.retain(|g| !is_nebula_group(g));
        }
    }
    hooks_obj.retain(|_, groups| groups.as_array().map(|a| !a.is_empty()).unwrap_or(true));
}

/// Merge nebula's managed hooks for Cursor into `<cwd>/.cursor/hooks.json`,
/// in Cursor's own dialect. Also migrates away the Claude-shaped groups an
/// older nebula wrote there (events Cursor never fires — the original
/// "cursor status never updates" bug).
pub fn install_cursor_hooks(cwd: &Path) -> Result<()> {
    let dir = cwd.join(CURSOR_DIR);
    let path = dir.join(HOOKS_FILE);
    let mut root = load_hooks_root(&path)?;

    let root_obj = root_object_mut(&mut root, &path)?;
    // Cursor requires a top-level version; never overwrite an existing one.
    root_obj.entry("version").or_insert(json!(1));
    let hooks = root_obj.entry("hooks").or_insert_with(|| json!({}));
    let hooks_obj = object_mut(hooks, "\"hooks\"", &path)?;

    // The stale PascalCase keys from the old Claude-shaped install go here.
    purge_nebula_groups(hooks_obj);

    for (cursor_event, nebula_event) in CURSOR_EVENTS {
        let groups = hooks_obj
            .entry(cursor_event.to_string())
            .or_insert_with(|| json!([]));
        let groups_arr = array_mut(groups, &format!("hooks.{cursor_event}"), &path)?;
        groups_arr.push(json!({
            "command": cursor_hook_command(nebula_event),
            "_nebulaManaged": true,
        }));
    }

    write_hooks_root(&dir, HOOKS_FILE, &root)
}

/// Copilot's home (`$COPILOT_HOME`, else `~/.copilot`) — where its
/// user-level hooks directory lives.
pub fn copilot_home() -> PathBuf {
    nebula_core::env::non_empty(nebula_core::env::COPILOT_HOME)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            nebula_core::env::home_dir()
                .unwrap_or_default()
                .join(COPILOT_DIR)
        })
}

/// Write nebula's managed hooks for Copilot into
/// `<copilot_home>/hooks/kete.json`, in Copilot's own dialect: a top-level
/// `"version": 1`, camelCase event keys, and per entry a `bash` and a
/// `powershell` command (Copilot picks the one for its platform) with a
/// timeout. The file is nebula's alone, so it is written whole — and left
/// alone when it already says exactly this.
pub fn install_copilot_hooks(copilot_home: &Path) -> Result<()> {
    let mut hooks = Map::new();
    for (copilot_event, nebula_event) in COPILOT_EVENTS {
        let command = copilot_hook_command(nebula_event);
        hooks.insert(
            copilot_event.to_string(),
            json!([{
                "type": "command",
                "bash": command,
                "powershell": command,
                "timeoutSec": COPILOT_HOOK_TIMEOUT_SECS,
            }]),
        );
    }
    let root = json!({ "version": 1, "hooks": hooks });
    let dir = copilot_home.join(COPILOT_HOOKS_DIR);
    install_unless_unchanged(
        &dir,
        COPILOT_HOOKS_FILE,
        &serde_json::to_string_pretty(&root)?,
    )?;
    match std::fs::remove_file(dir.join(LEGACY_COPILOT_HOOKS_FILE)) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            Err(e).context("remove the old Copilot hooks file")
        }
        _ => Ok(()),
    }
}

fn load_hooks_root(path: &Path) -> Result<Value> {
    if !path.exists() {
        return Ok(json!({}));
    }
    let text = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    if text.trim().is_empty() {
        return Ok(json!({}));
    }
    match serde_json::from_str(&text) {
        Ok(v) => Ok(v),
        Err(e) => bail!(
            "{} is not valid JSON ({e}): refusing to modify it; fix or remove the file",
            path.display()
        ),
    }
}

fn write_hooks_root(dir: &Path, file_name: &str, root: &Value) -> Result<()> {
    write_text_atomic(dir, file_name, &serde_json::to_string_pretty(root)?)
}

/// Write the managed file `name` holding `source` into `dir`, unless it
/// already holds exactly that — an unchanged mtime keeps the CLI's own
/// bookkeeping of the directory (pi's extension cache, OpenCode's plugin
/// scan) quiet.
pub(super) fn install_unless_unchanged(dir: &Path, name: &str, source: &str) -> Result<()> {
    if let Ok(existing) = std::fs::read_to_string(dir.join(name)) {
        if existing == source {
            return Ok(());
        }
    }
    write_text_atomic(dir, name, source)
}

pub(super) fn write_text_atomic(dir: &Path, file_name: &str, text: &str) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    // Atomic write: tmp + rename.
    let tmp = dir.join(format!(".{file_name}.nebula-tmp"));
    let path = dir.join(file_name);
    std::fs::write(&tmp, text).with_context(|| format!("write {}", tmp.display()))?;
    std::fs::rename(&tmp, &path).with_context(|| format!("rename into {}", path.display()))?;
    Ok(())
}

/// Load `<dir>/<file_name>`, strip-and-rebuild nebula's groups for `events`
/// in the Claude/Codex dialect, make sure each of `allow_rules` is in
/// `permissions.allow`, and write it back. Claude's settings file carries
/// hooks and permissions both, so the one pass serves it and codex's
/// hooks-only file alike (codex passes no rules). `settings` sets any
/// further top-level keys in the same pass (Claude's attribution switches).
fn install_managed_hooks(
    dir: &Path,
    file_name: &str,
    endpoint: &str,
    events: &[(&str, Option<&str>)],
    allow_rules: &[&str],
    settings: impl FnOnce(&mut Map<String, Value>),
) -> Result<()> {
    let path = dir.join(file_name);
    let mut root = load_hooks_root(&path)?;

    let root_obj = root_object_mut(&mut root, &path)?;
    merge_managed_hooks(root_obj, endpoint, events, &path)?;
    for rule in allow_rules {
        ensure_permission_allow(root_obj, rule, &path)?;
    }
    settings(root_obj);
    write_hooks_root(dir, file_name, &root)
}

/// Strip-and-rebuild nebula's groups under each event key of a loaded
/// Claude/Codex-dialect config, leaving user groups untouched.
fn merge_managed_hooks(
    root_obj: &mut Map<String, Value>,
    endpoint: &str,
    events: &[(&str, Option<&str>)],
    path: &Path,
) -> Result<()> {
    let hooks = root_obj.entry("hooks").or_insert_with(|| json!({}));
    let hooks_obj = object_mut(hooks, "\"hooks\"", path)?;

    // One event may carry several managed groups (one per matcher), so the
    // strip-and-rebuild happens once per event name — stripping again for
    // a second matcher would delete the group the first one just added.
    let mut stripped: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for (event, matcher) in events {
        let groups = hooks_obj
            .entry(event.to_string())
            .or_insert_with(|| json!([]));
        let groups_arr = array_mut(groups, &format!("hooks.{event}"), path)?;
        if stripped.insert(event) {
            groups_arr.retain(|g| !is_nebula_group(g));
        }
        groups_arr.push(managed_group(endpoint, event, *matcher));
    }
    Ok(())
}

/// Cursor can't receive daemon-injected context (its hooks are gating-only,
/// answering with their own JSON), so the auto-title instruction ships as a
/// managed always-on project rule instead. The file is wholly nebula-owned
/// (namespaced filename) and rewritten on every spawn. Firing outside
/// nebula or on later prompts is harmless: the rule env-guards itself, and
/// the daemon accepts at most one auto-title per session.
pub fn install_cursor_title_rule(cwd: &Path) -> Result<()> {
    let dir = cwd.join(CURSOR_DIR).join("rules");
    write_text_atomic(&dir, "nebula-title.mdc", &cursor_title_rule())
}

/// Same instruction the injectable CLIs get, wrapped in cursor's rule
/// frontmatter and an env guard (a project rule can't be switched off from
/// the daemon's side the way an injection can).
fn cursor_title_rule() -> String {
    format!(
        "---
description: Kete session auto-title (managed by kete: edits are overwritten)
alwaysApply: true
---

This rule applies only when the environment variable NEBULA_AGENT_ID is set
(the session runs inside kete). If it is unset, ignore this rule entirely.

On the first user message of a new conversation:

{}
",
        super::AUTO_TITLE_INSTRUCTION
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exact one-liners that land in users' config files. Pinned
    /// verbatim so the shared curl prelude can be factored without a byte
    /// of the installed command drifting.
    #[cfg(unix)]
    #[test]
    fn hook_commands_are_spelled_exactly() {
        assert_eq!(
            hook_command("claude", "UserPromptSubmit"),
            "if [ -z \"$NEBULA_AGENT_ID\" ] || [ -z \"$NEBULA_API_URL\" ]; then exit 0; fi; \
             curl -sS -m 3 -X POST -H \"Authorization: Bearer $NEBULA_API_TOKEN\" \
             -H \"Content-Type: application/json\" --data-binary @- \
             \"$NEBULA_API_URL/api/hooks/claude?agentId=$NEBULA_AGENT_ID&hookEvent=UserPromptSubmit\" \
             2>/dev/null || true"
        );
        assert_eq!(
            hook_command("codex", "Stop"),
            "if [ -z \"$NEBULA_AGENT_ID\" ] || [ -z \"$NEBULA_API_URL\" ]; then exit 0; fi; \
             curl -sS -m 3 -X POST -H \"Authorization: Bearer $NEBULA_API_TOKEN\" \
             -H \"Content-Type: application/json\" --data-binary @- \
             \"$NEBULA_API_URL/api/hooks/codex?agentId=$NEBULA_AGENT_ID&hookEvent=Stop\" \
             >/dev/null 2>&1 || true"
        );
        assert_eq!(
            cursor_hook_command("Stop"),
            "if [ -z \"$NEBULA_AGENT_ID\" ] || [ -z \"$NEBULA_API_URL\" ]; then \
             printf '{\"continue\": true}\\n'; exit 0; fi; \
             curl -sS -m 3 -X POST -H \"Authorization: Bearer $NEBULA_API_TOKEN\" \
             -H \"Content-Type: application/json\" --data-binary @- \
             \"$NEBULA_API_URL/api/hooks/cursor?agentId=$NEBULA_AGENT_ID&hookEvent=Stop\" \
             >/dev/null 2>&1 || true; printf '{\"continue\": true}\\n'"
        );
    }

    /// Whether `cmd` posts `event` to the daemon's `endpoint`: the curl
    /// URL on unix, the `_hook` subcommand's arguments on Windows.
    fn routes(cmd: &str, endpoint: &str, event: &str) -> bool {
        #[cfg(unix)]
        {
            cmd.contains(&format!("/api/hooks/{endpoint}?"))
                && cmd.contains(&format!("hookEvent={event}"))
        }
        #[cfg(windows)]
        {
            cmd.ends_with(&format!(" {HOOK_SUBCOMMAND} {endpoint} {event}"))
        }
    }

    /// Whether a Claude-dialect `hook` object posts `event` to `endpoint`:
    /// [`routes`] on its command, or — a Claude hook on Windows, in exec
    /// form — this binary's path, bare, run with `_hook <endpoint> <event>`.
    fn routes_hook(hook: &Value, endpoint: &str, event: &str) -> bool {
        #[cfg(windows)]
        if let Some(args) = hook.get("args") {
            let exe = std::env::current_exe().unwrap().display().to_string();
            let exe = exe.strip_prefix(r"\\?\").unwrap_or(&exe).to_string();
            return hook["command"] == json!(exe)
                && *args == json!([HOOK_SUBCOMMAND, endpoint, event]);
        }
        hook["command"]
            .as_str()
            .is_some_and(|cmd| routes(cmd, endpoint, event))
    }

    fn read_json(dir: &Path, rel: &str) -> Value {
        let text = std::fs::read_to_string(dir.join(rel)).unwrap();
        serde_json::from_str(&text).unwrap()
    }

    #[test]
    fn the_statusline_settings_file_is_kete_s_own_and_rewritten_only_on_change() {
        let tmp = tempfile::tempdir().unwrap();
        let path = write_statusline_settings(tmp.path()).unwrap();
        assert_eq!(path, tmp.path().join(STATUSLINE_SETTINGS_FILE));
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let cmd = v["statusLine"]["command"].as_str().unwrap();
        assert!(cmd.ends_with(" _statusline"), "{cmd}");
        assert_eq!(v["statusLine"]["type"], "command");
        assert_eq!(
            v.as_object().unwrap().len(),
            1,
            "nothing but the statusLine"
        );
        let stamp = std::fs::metadata(&path).unwrap().modified().unwrap();
        write_statusline_settings(tmp.path()).unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(), stamp);
    }

    #[test]
    fn the_migration_strips_only_kete_s_old_statusline() {
        let tmp = tempfile::tempdir().unwrap();
        strip_kete_statusline(tmp.path()).unwrap();
        assert!(
            !tmp.path().join(".claude").exists(),
            "no file, nothing made"
        );
        install_claude_hooks(tmp.path()).unwrap();
        let path = tmp.path().join(".claude").join("settings.local.json");
        let mut settings = read_settings(tmp.path());
        settings["statusLine"] =
            json!({"type": "command", "command": "C:\"/k/kete.exe\" _statusline", "padding": 0});
        std::fs::write(&path, settings.to_string()).unwrap();
        strip_kete_statusline(tmp.path()).unwrap();
        let after = read_settings(tmp.path());
        assert!(after.get("statusLine").is_none());
        assert!(after["hooks"]["Stop"].is_array(), "hooks kept");
        let mut settings = after;
        settings["statusLine"] = json!({"type": "command", "command": "my-line"});
        std::fs::write(&path, settings.to_string()).unwrap();
        strip_kete_statusline(tmp.path()).unwrap();
        assert_eq!(
            read_settings(tmp.path())["statusLine"]["command"],
            "my-line"
        );
    }

    fn read_settings(dir: &Path) -> Value {
        read_json(dir, ".claude/settings.local.json")
    }

    #[test]
    fn installs_into_missing_file() {
        let tmp = tempfile::tempdir().unwrap();
        install_claude_hooks(tmp.path()).unwrap();
        let settings = read_settings(tmp.path());
        let stop = &settings["hooks"]["Stop"];
        assert_eq!(stop.as_array().unwrap().len(), 1);
        assert_eq!(stop[0]["_nebulaManaged"], json!(true));
        assert!(routes_hook(&stop[0]["hooks"][0], "claude", "Stop"));
        // Notification carries no matcher: idle_prompt has to reach us too.
        let notification = &settings["hooks"]["Notification"];
        assert!(notification[0].get("matcher").is_none());
        let pre = &settings["hooks"]["PreToolUse"];
        assert_eq!(pre[0]["matcher"], json!(PRE_TOOL_USE_MATCHER));
        // StopFailure, unmatched: every error kind ends a turn somehow.
        let failure = &settings["hooks"]["StopFailure"];
        assert_eq!(failure.as_array().unwrap().len(), 1);
        assert!(failure[0].get("matcher").is_none());
        assert!(routes_hook(
            &failure[0]["hooks"][0],
            "claude",
            "StopFailure"
        ));
    }

    /// The tool names Claude's anchored PreToolUse matcher lets through,
    /// read off the regex's one alternation.
    fn pre_tool_use_tools(matcher: &str) -> Vec<&str> {
        let inner = matcher
            .strip_prefix("^(")
            .and_then(|m| m.strip_suffix(")$"))
            .expect("an anchored alternation");
        inner.split('|').collect()
    }

    /// The Agent (and older Task) call that starts a subagent has to
    /// reach the daemon, or every subagent's description stays empty. A
    /// reinstall swaps an older nebula's question-only group for it.
    #[test]
    fn pre_tool_use_matches_the_subagent_tools_beside_the_question() {
        let tools = pre_tool_use_tools(PRE_TOOL_USE_MATCHER);
        for tool in ["AskUserQuestion", "Agent", "Task"] {
            assert!(tools.contains(&tool), "{tool} is not matched: {tools:?}");
        }
        assert!(!tools.contains(&"TaskCreate"), "anchored, so kin stay out");

        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join(".claude");
        std::fs::create_dir_all(&dir).unwrap();
        let mut old = managed_group("claude", "PreToolUse", Some("AskUserQuestion"));
        old["_nebulaManaged"] = json!(true);
        std::fs::write(
            dir.join("settings.local.json"),
            serde_json::to_string(&json!({ "hooks": { "PreToolUse": [old] } })).unwrap(),
        )
        .unwrap();
        install_claude_hooks(tmp.path()).unwrap();
        let settings = read_settings(tmp.path());
        let groups = settings["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(groups.len(), 1, "{groups:?}");
        let matcher = groups[0]["matcher"].as_str().unwrap();
        assert!(pre_tool_use_tools(matcher).contains(&"Agent"));
        assert!(routes_hook(&groups[0]["hooks"][0], "claude", "PreToolUse"));
    }

    #[test]
    fn post_tool_use_is_one_unmatched_group_across_reinstalls() {
        // PostToolUse carries no matcher: a permission prompt's approval is
        // only ever visible as the gated tool's completion, whatever the
        // tool, and the AskUserQuestion answer and the cwd probe ride the
        // same group. A reinstall (every spawn) must leave exactly one —
        // and must replace the two matched groups an older nebula wrote.
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join(".claude");
        std::fs::create_dir_all(&dir).unwrap();
        let old = |matcher: &str| {
            let mut g = managed_group("claude", "PostToolUse", Some(matcher));
            g["_nebulaManaged"] = json!(true);
            g
        };
        std::fs::write(
            dir.join("settings.local.json"),
            serde_json::to_string(&json!({
                "hooks": {
                    "PostToolUse": [old("AskUserQuestion"), old("Bash|EnterWorktree|ExitWorktree")]
                }
            }))
            .unwrap(),
        )
        .unwrap();
        install_claude_hooks(tmp.path()).unwrap();
        install_claude_hooks(tmp.path()).unwrap();
        let settings = read_settings(tmp.path());
        let groups = settings["hooks"]["PostToolUse"].as_array().unwrap();
        assert_eq!(groups.len(), 1, "{groups:?}");
        assert!(groups[0].get("matcher").is_none());
        assert_eq!(groups[0]["_nebulaManaged"], json!(true));
        assert!(routes_hook(&groups[0]["hooks"][0], "claude", "PostToolUse"));
    }

    /// Windows: the hook is this binary's `_hook` subcommand, the endpoint
    /// and event as its words, and a legacy untagged one is still ours.
    #[cfg(windows)]
    #[test]
    fn hook_commands_run_the_hook_subcommand() {
        let exe = shell_neutral_path(&std::env::current_exe().unwrap().display().to_string());
        assert_eq!(
            shell_neutral_path(r"C:\Program Files\kete\kete.exe"),
            r#"C:"/Program Files/kete/kete.exe""#
        );
        assert_eq!(
            shell_neutral_path(r"\\?\D:\b\kete.exe"),
            r#"D:"/b/kete.exe""#
        );
        assert_eq!(
            shell_neutral_path(r"\\host\share\kete.exe"),
            r#""//host/share/kete.exe""#
        );
        assert_eq!(
            hook_command("claude", "UserPromptSubmit"),
            format!("{exe} _hook claude UserPromptSubmit")
        );
        assert_eq!(
            cursor_hook_command("Stop"),
            format!("{exe} _hook cursor Stop")
        );
        assert!(!hook_command("codex", "Stop").contains('\\'));
        assert!(is_nebula_hook(
            &json!({ "command": hook_command("codex", "Stop") })
        ));
        assert!(!is_nebula_hook(&json!({ "command": "my-own-hook Stop" })));

        // Claude's hook in exec form: the binary's own path, bare — no
        // shell strips quotes from it — and the subcommand as its words.
        let hook = claude_exec_hook("Stop");
        let path = std::env::current_exe().unwrap().display().to_string();
        assert_eq!(
            hook,
            json!({
                "type": "command",
                "command": path.strip_prefix(r"\\?\").unwrap_or(&path),
                "args": ["_hook", "claude", "Stop"],
            })
        );
        assert!(is_nebula_hook(&hook));
        assert!(!is_nebula_hook(
            &json!({ "command": "node", "args": ["lint.js", "_hook"] })
        ));
    }

    /// On Windows the Claude groups go in exec form, replacing what an
    /// older Kete wrote — a tagged shell-form group, an untagged one, an
    /// untagged exec-form one — and a reinstall writes the file
    /// byte for byte the same. The user's own hook stays, and Codex keeps
    /// its shell form.
    #[cfg(windows)]
    #[test]
    fn claude_hooks_move_to_exec_form_and_stay_there() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join(".claude");
        std::fs::create_dir_all(&dir).unwrap();
        let old_shell = |event: &str| {
            json!({ "hooks": [{
                "type": "command",
                "command": format!(r#"C:"/old/kete.exe" _hook claude {event}"#),
            }] })
        };
        let mut tagged = old_shell("Stop");
        tagged["_nebulaManaged"] = json!(true);
        let untagged_exec = json!({ "hooks": [{
            "type": "command",
            "command": r"C:\old\kete.exe",
            "args": ["_hook", "claude", "Notification"],
        }] });
        let mine = json!({ "hooks": [{ "type": "command", "command": "my-own-hook" }] });
        std::fs::write(
            dir.join("settings.local.json"),
            serde_json::to_string(&json!({ "hooks": {
                "Stop": [tagged, mine.clone()],
                "SessionStart": [old_shell("SessionStart")],
                "Notification": [untagged_exec],
            } }))
            .unwrap(),
        )
        .unwrap();
        install_claude_hooks(tmp.path()).unwrap();
        let settings = read_settings(tmp.path());
        for (event, _) in CLAUDE_EVENTS {
            let groups = settings["hooks"][event].as_array().unwrap();
            let ours: Vec<&Value> = groups.iter().filter(|g| is_nebula_group(g)).collect();
            assert_eq!(ours.len(), 1, "{event}: {groups:?}");
            assert!(
                routes_hook(&ours[0]["hooks"][0], "claude", event),
                "{event}: {:?}",
                ours[0]
            );
            assert!(ours[0]["hooks"][0].get("args").is_some(), "{event}");
        }
        assert!(
            settings["hooks"]["Stop"]
                .as_array()
                .unwrap()
                .contains(&mine),
            "the user's hook stays"
        );
        let first = std::fs::read(dir.join("settings.local.json")).unwrap();
        install_claude_hooks(tmp.path()).unwrap();
        assert_eq!(
            std::fs::read(dir.join("settings.local.json")).unwrap(),
            first,
            "a reinstall changes nothing"
        );

        let codex = tempfile::tempdir().unwrap();
        install_codex_hooks(codex.path()).unwrap();
        let hook = &read_json(codex.path(), "hooks.json")["hooks"]["Stop"][0]["hooks"][0];
        assert!(hook.get("args").is_none(), "Codex keeps its shell form");
        assert!(routes(hook["command"].as_str().unwrap(), "codex", "Stop"));
    }

    #[cfg(unix)]
    #[test]
    fn user_prompt_submit_command_pipes_response_to_stdout() {
        // The daemon's UserPromptSubmit response body is the auto-title
        // context injection — that one command must let stdout through
        // (stderr still silenced); every other event stays fully silent.
        let tmp = tempfile::tempdir().unwrap();
        install_claude_hooks(tmp.path()).unwrap();
        let settings = read_settings(tmp.path());
        let submit = settings["hooks"]["UserPromptSubmit"][0]["hooks"][0]["command"]
            .as_str()
            .unwrap();
        assert!(submit.contains("2>/dev/null"), "stderr silenced: {submit}");
        assert!(
            !submit.contains(">/dev/null 2>&1"),
            "stdout must pass through: {submit}"
        );
        let stop = settings["hooks"]["Stop"][0]["hooks"][0]["command"]
            .as_str()
            .unwrap();
        assert!(
            stop.contains(">/dev/null 2>&1"),
            "stop stays silent: {stop}"
        );
    }

    #[test]
    fn claude_install_adds_rename_permission_idempotently() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join(".claude");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("settings.local.json"),
            serde_json::to_string(&json!({
                "permissions": { "allow": ["Bash(ls:*)"], "deny": ["WebFetch"] }
            }))
            .unwrap(),
        )
        .unwrap();

        install_claude_hooks(tmp.path()).unwrap();
        install_claude_hooks(tmp.path()).unwrap();
        let settings = read_settings(tmp.path());
        let allow = settings["permissions"]["allow"].as_array().unwrap();
        assert_eq!(allow[0], json!("Bash(ls:*)"), "user entry preserved first");
        for rule in CLAUDE_ALLOW_RULES {
            assert_eq!(
                allow.iter().filter(|v| v.as_str() == Some(rule)).count(),
                1,
                "exactly one {rule} entry after reinstalls: {allow:?}"
            );
        }
        assert_eq!(settings["permissions"]["deny"][0], json!("WebFetch"));
        // Every Bash rule has its PowerShell twin, for Claude's PowerShell
        // tool on a machine without Git Bash.
        for bash in CLAUDE_ALLOW_RULES
            .iter()
            .filter_map(|r| r.strip_prefix("Bash("))
        {
            let twin = format!("PowerShell({} *)", bash.trim_end_matches(":*)"));
            assert!(CLAUDE_ALLOW_RULES.contains(&twin.as_str()), "{twin}");
        }
    }

    /// Whether a prefix rule (`Bash(x:*)`, `PowerShell(x *)`) lets `cmd`
    /// run: `x` alone, or `x` and its arguments.
    fn allows(rule: &str, cmd: &str) -> bool {
        let prefix = rule
            .strip_prefix("Bash(")
            .and_then(|r| r.strip_suffix(":*)"))
            .or_else(|| {
                rule.strip_prefix("PowerShell(")
                    .and_then(|r| r.strip_suffix(" *)"))
            })
            .expect("a prefix rule");
        cmd == prefix || cmd.starts_with(&format!("{prefix} "))
    }

    /// The 0.60 studio rules let the read-only commands through and never
    /// the ones that change what a studio may do: only a person does that.
    #[test]
    fn studio_rules_allow_reading_and_never_changing_autonomy() {
        for bin in ["kete", "bro"] {
            for cmd in [
                "studio status --json",
                "studio queue --project my-game",
                "studio note \"storm sky\" --project my-game",
                "usage --csv --since 2026-10-21",
                "profile list --json",
                "profile pick --json",
                "orchestrator renew --from handover.md",
                "state",
                "state --slice D-012 --json",
            ] {
                let cmd = format!("{bin} {cmd}");
                assert!(
                    CLAUDE_ALLOW_RULES.iter().any(|r| allows(r, &cmd)),
                    "{cmd} is not allowed"
                );
            }
            for cmd in [
                "studio",
                "studio autonomy my-game A3",
                "studio enable my-game",
                "studio disable my-game",
                "studio exclude my-game x",
                "studio include my-game x",
                "profile reserve jayrich",
                "profile set-project my-game max2",
                "roles accept-bypass max2",
            ] {
                let cmd = format!("{bin} {cmd}");
                assert!(
                    !CLAUDE_ALLOW_RULES.iter().any(|r| allows(r, &cmd)),
                    "{cmd} is allowed"
                );
            }
        }
    }

    /// Codex hooks land in its home dir, not the worktree — that stable
    /// path is what keeps its trust prompt to a single approval.
    #[test]
    fn codex_installs_into_codex_home() {
        let tmp = tempfile::tempdir().unwrap();
        install_codex_hooks(tmp.path()).unwrap();
        let hooks = read_json(tmp.path(), "hooks.json");
        let stop = &hooks["hooks"]["Stop"];
        assert_eq!(stop.as_array().unwrap().len(), 1);
        assert_eq!(stop[0]["_nebulaManaged"], json!(true));
        let cmd = stop[0]["hooks"][0]["command"].as_str().unwrap();
        assert!(routes(cmd, "codex", "Stop"), "codex endpoint: {cmd}");
        // Claude-only events must not leak into the codex file.
        assert!(hooks["hooks"].get("Notification").is_none());
        assert!(hooks["hooks"].get("PreToolUse").is_none());
        assert!(hooks["hooks"].get("PostToolUse").is_none());
        assert_eq!(
            hooks["hooks"]["PermissionRequest"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        // UserPromptSubmit still lets the daemon's response through: that
        // body is the auto-title injection.
        let submit = hooks["hooks"]["UserPromptSubmit"][0]["hooks"][0]["command"]
            .as_str()
            .unwrap();
        assert!(!submit.contains(">/dev/null 2>&1"), "stdout: {submit}");
    }

    #[test]
    fn copilot_installs_its_own_dialect_into_its_hooks_dir() {
        let tmp = tempfile::tempdir().unwrap();
        install_copilot_hooks(tmp.path()).unwrap();
        let hooks = read_json(tmp.path(), "hooks/kete.json");
        assert_eq!(hooks["version"], json!(1));
        let events: Vec<&str> = hooks["hooks"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        for (event, _) in COPILOT_EVENTS {
            assert!(events.contains(event), "{event} missing: {events:?}");
        }
        assert!(hooks["hooks"].get("Stop").is_none(), "camelCase only");
        let stop = &hooks["hooks"]["agentStop"][0];
        assert_eq!(stop["type"], json!("command"));
        let cmd = stop["powershell"].as_str().unwrap();
        assert_eq!(stop["bash"].as_str(), Some(cmd));
        assert!(routes(cmd, "copilot", "Stop"), "copilot endpoint: {cmd}");
        assert!(stop["timeoutSec"].as_u64().is_some());

        // Rewritten whole and only when it changed: a second install
        // leaves the file's bytes as they were.
        let path = tmp.path().join("hooks").join("kete.json");
        let before = std::fs::read_to_string(&path).unwrap();
        install_copilot_hooks(tmp.path()).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before);

        // The file Bro wrote goes, so no event fires twice.
        let legacy = tmp.path().join("hooks").join(LEGACY_COPILOT_HOOKS_FILE);
        std::fs::write(&legacy, &before).unwrap();
        install_copilot_hooks(tmp.path()).unwrap();
        assert!(!legacy.exists());
    }

    #[test]
    fn worktree_settings_turn_attribution_off() {
        // Claude: both switches, forced off over whatever the user had.
        let tmp = tempfile::tempdir().unwrap();
        let claude = tmp.path().join(".claude");
        std::fs::create_dir_all(&claude).unwrap();
        std::fs::write(
            claude.join("settings.local.json"),
            r#"{"includeCoAuthoredBy": true, "attribution": {"commit": "x"}, "model": "opus"}"#,
        )
        .unwrap();
        install_claude_hooks(tmp.path()).unwrap();
        let settings = read_settings(tmp.path());
        assert_eq!(settings["includeCoAuthoredBy"], json!(false));
        assert_eq!(settings["attribution"], json!({ "commit": "", "pr": "" }));
        assert_eq!(settings["model"], json!("opus"));

        // Copilot, in a linked worktree: its settings file, and one exclude
        // line in the repository's shared git dir however often it runs.
        let common = tmp.path().join("repo.git");
        let admin = common.join("worktrees").join("feat");
        std::fs::create_dir_all(&admin).unwrap();
        std::fs::write(admin.join("commondir"), "../..\n").unwrap();
        std::fs::create_dir_all(common.join("info")).unwrap();
        std::fs::write(common.join("info").join("exclude"), "# git ls-files\n*.log").unwrap();
        let wt = tmp.path().join("feat");
        std::fs::create_dir_all(&wt).unwrap();
        std::fs::write(wt.join(".git"), format!("gitdir: {}\n", admin.display())).unwrap();

        install_copilot_worktree_settings(&wt).unwrap();
        install_copilot_worktree_settings(&wt).unwrap();
        assert_eq!(
            read_json(&wt, ".github/copilot/settings.local.json"),
            json!({ "includeCoAuthoredBy": false })
        );
        let exclude = std::fs::read_to_string(common.join("info").join("exclude")).unwrap();
        assert_eq!(
            exclude,
            "# git ls-files\n*.log\n.github/copilot/settings.local.json\n"
        );

        // A main checkout's `.git` directory is the common dir itself.
        let main = tmp.path().join("main");
        std::fs::create_dir_all(main.join(".git")).unwrap();
        install_copilot_worktree_settings(&main).unwrap();
        let exclude = std::fs::read_to_string(main.join(".git/info/exclude")).unwrap();
        assert_eq!(exclude, ".github/copilot/settings.local.json\n");
    }

    /// Claude's settings file stays out of `git status` in a checkout,
    /// once however often it is installed, and a folder that is no git
    /// checkout still gets its hooks.
    #[test]
    fn claude_settings_are_excluded_from_git() {
        let tmp = tempfile::tempdir().unwrap();
        let main = tmp.path().join("main");
        std::fs::create_dir_all(main.join(".git")).unwrap();
        install_claude_hooks(&main).unwrap();
        install_claude_hooks(&main).unwrap();
        let exclude = std::fs::read_to_string(main.join(".git/info/exclude")).unwrap();
        assert_eq!(exclude, ".claude/settings.local.json\n");

        let plain = tmp.path().join("plain");
        std::fs::create_dir_all(&plain).unwrap();
        install_claude_hooks(&plain).unwrap();
        assert!(plain.join(".claude/settings.local.json").exists());
    }

    #[test]
    fn codex_home_prefers_env_over_home() {
        // Serialised with the other env-reading test by running in one test.
        let tmp = tempfile::tempdir().unwrap();
        let saved = std::env::var("CODEX_HOME").ok();
        std::env::set_var("CODEX_HOME", tmp.path());
        assert_eq!(codex_home(), tmp.path());
        std::env::set_var("CODEX_HOME", "");
        assert_eq!(
            codex_home(),
            nebula_core::env::home_dir()
                .unwrap_or_default()
                .join(".codex")
        );
        match saved {
            Some(v) => std::env::set_var("CODEX_HOME", v),
            None => std::env::remove_var("CODEX_HOME"),
        }
    }

    /// Migration off the old per-worktree install: our groups go, foreign
    /// ones stay, and a file left with nothing in it is removed outright.
    #[test]
    fn codex_worktree_hooks_are_pruned() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join(".codex");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("hooks.json"),
            serde_json::to_string(&json!({
                "hooks": {
                    "Stop": [
                        { "_nebulaManaged": true,
                          "hooks": [{ "type": "command",
                            "command": "curl $NEBULA_API_URL/api/hooks/codex?agentId=$NEBULA_AGENT_ID" }] },
                        { "_mcManaged": true,
                          "hooks": [{ "type": "command", "command": "curl $MC_API_URL/api/hooks/codex" }] }
                    ],
                    "UserPromptSubmit": [
                        { "_nebulaManaged": true,
                          "hooks": [{ "type": "command",
                            "command": "curl $NEBULA_API_URL/api/hooks/codex?agentId=$NEBULA_AGENT_ID" }] }
                    ]
                }
            }))
            .unwrap(),
        )
        .unwrap();

        prune_codex_worktree_hooks(tmp.path()).unwrap();
        let hooks = read_json(tmp.path(), ".codex/hooks.json");
        // Ours gone from both keys; the emptied key pruned; foreign kept.
        assert!(hooks["hooks"].get("UserPromptSubmit").is_none());
        let stop = hooks["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stop.len(), 1);
        assert_eq!(stop[0]["_mcManaged"], json!(true));

        // Second worktree: the file was nebula's alone, so nothing is left
        // behind — not the file, not the directory it needed.
        let solo = tempfile::tempdir().unwrap();
        let solo_dir = solo.path().join(".codex");
        std::fs::create_dir_all(&solo_dir).unwrap();
        std::fs::write(
            solo_dir.join("hooks.json"),
            serde_json::to_string(&json!({
                "hooks": { "Stop": [
                    { "_nebulaManaged": true,
                      "hooks": [{ "type": "command",
                        "command": "curl $NEBULA_API_URL/api/hooks/codex?agentId=$NEBULA_AGENT_ID" }] }
                ] }
            }))
            .unwrap(),
        )
        .unwrap();
        prune_codex_worktree_hooks(solo.path()).unwrap();
        assert!(!solo_dir.join("hooks.json").exists());
        assert!(!solo_dir.exists());

        // Nothing to prune is not an error.
        let bare = tempfile::tempdir().unwrap();
        prune_codex_worktree_hooks(bare.path()).unwrap();
    }

    #[test]
    fn cursor_title_rule_is_written_and_rewritten() {
        let tmp = tempfile::tempdir().unwrap();
        install_cursor_title_rule(tmp.path()).unwrap();
        let path = tmp.path().join(".cursor/rules/nebula-title.mdc");
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("alwaysApply: true"));
        assert!(text.contains("kete rename"));
        assert!(text.contains("NEBULA_AGENT_ID"), "must be env-guarded");
        // Wholly nebula-owned: a scribbled-on file is simply replaced.
        std::fs::write(&path, "user scribbles").unwrap();
        install_cursor_title_rule(tmp.path()).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("kete rename"));
    }

    #[test]
    fn preserves_user_hooks_and_settings() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join(".claude");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("settings.local.json"),
            serde_json::to_string(&json!({
                "permissions": { "allow": ["Bash(ls:*)"] },
                "hooks": {
                    "Stop": [
                        { "hooks": [{ "type": "command", "command": "say done" }] }
                    ]
                }
            }))
            .unwrap(),
        )
        .unwrap();

        install_claude_hooks(tmp.path()).unwrap();
        let settings = read_settings(tmp.path());
        assert_eq!(settings["permissions"]["allow"][0], json!("Bash(ls:*)"));
        let stop = settings["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stop.len(), 2, "user group + kete group");
        assert_eq!(stop[0]["hooks"][0]["command"], json!("say done"));
        assert_eq!(stop[1]["_nebulaManaged"], json!(true));
    }

    #[test]
    fn reinstall_does_not_accumulate_duplicates() {
        let tmp = tempfile::tempdir().unwrap();
        install_claude_hooks(tmp.path()).unwrap();
        install_claude_hooks(tmp.path()).unwrap();
        install_claude_hooks(tmp.path()).unwrap();
        let settings = read_settings(tmp.path());
        for (event, _) in CLAUDE_EVENTS {
            // One group per (event, matcher) pair.
            let expected = CLAUDE_EVENTS.iter().filter(|(e, _)| e == event).count();
            assert_eq!(
                settings["hooks"][*event].as_array().unwrap().len(),
                expected,
                "{event} accumulated duplicates"
            );
        }
    }

    #[test]
    fn strips_legacy_untagged_nebula_groups() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join(".claude");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("settings.local.json"),
            serde_json::to_string(&json!({
                "hooks": {
                    "Stop": [
                        // Old nebula install without the marker.
                        { "hooks": [{ "type": "command",
                            "command": "curl $NEBULA_API_URL/api/hooks/claude?agentId=$NEBULA_AGENT_ID" }] }
                    ]
                }
            }))
            .unwrap(),
        )
        .unwrap();
        install_claude_hooks(tmp.path()).unwrap();
        let settings = read_settings(tmp.path());
        assert_eq!(settings["hooks"]["Stop"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn corrupt_file_aborts_without_clobbering() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join(".claude");
        std::fs::create_dir_all(&dir).unwrap();
        let original = "{ this is not json";
        std::fs::write(dir.join("settings.local.json"), original).unwrap();
        assert!(install_claude_hooks(tmp.path()).is_err());
        let after = std::fs::read_to_string(dir.join("settings.local.json")).unwrap();
        assert_eq!(after, original, "corrupt file must be left untouched");
    }

    #[test]
    fn codex_preserves_foreign_managed_groups() {
        // Mission Control also writes _mcManaged groups into hook files.
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(
            dir.join("hooks.json"),
            serde_json::to_string(&json!({
                "hooks": {
                    "Stop": [
                        { "hooks": [{ "type": "command", "command": "curl $MC_API_URL/api/hooks/codex" }],
                          "_mcManaged": true }
                    ]
                }
            }))
            .unwrap(),
        )
        .unwrap();

        install_codex_hooks(tmp.path()).unwrap();
        install_codex_hooks(tmp.path()).unwrap();
        let hooks = read_json(tmp.path(), "hooks.json");
        let stop = hooks["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stop.len(), 2, "foreign managed group + kete group");
        assert_eq!(stop[0]["_mcManaged"], json!(true));
        assert_eq!(stop[1]["_nebulaManaged"], json!(true));
    }

    #[test]
    fn codex_corrupt_file_aborts_without_clobbering() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let original = "not json at all";
        std::fs::write(dir.join("hooks.json"), original).unwrap();
        assert!(install_codex_hooks(dir).is_err());
        // A worktree copy in the same shape is left alone too.
        assert!(prune_codex_worktree_hooks(dir).is_ok());
        let after = std::fs::read_to_string(dir.join("hooks.json")).unwrap();
        assert_eq!(after, original, "corrupt file must be left untouched");
    }

    #[test]
    fn cursor_installs_native_dialect_into_missing_file() {
        let tmp = tempfile::tempdir().unwrap();
        install_cursor_hooks(tmp.path()).unwrap();
        let hooks = read_json(tmp.path(), ".cursor/hooks.json");
        // Cursor requires the top-level version marker.
        assert_eq!(hooks["version"], json!(1));
        // camelCase cursor events, flat command entries.
        let stop = &hooks["hooks"]["stop"];
        assert_eq!(stop.as_array().unwrap().len(), 1);
        assert_eq!(stop[0]["_nebulaManaged"], json!(true));
        let cmd = stop[0]["command"].as_str().unwrap();
        assert!(routes(cmd, "cursor", "Stop"), "cursor endpoint: {cmd}");
        // Gating hooks must answer cursor with a JSON response (on Windows
        // the `_hook` subcommand prints it).
        #[cfg(unix)]
        assert!(
            cmd.contains("{\"continue\": true}"),
            "stdout response: {cmd}"
        );
        let submit = &hooks["hooks"]["beforeSubmitPrompt"][0];
        assert!(routes(
            submit["command"].as_str().unwrap(),
            "cursor",
            "UserPromptSubmit"
        ));
        assert!(routes(
            hooks["hooks"]["sessionStart"][0]["command"]
                .as_str()
                .unwrap(),
            "cursor",
            "SessionStart"
        ));
        assert!(routes(
            hooks["hooks"]["subagentStop"][0]["command"]
                .as_str()
                .unwrap(),
            "cursor",
            "SubagentStop"
        ));
        // Claude-dialect events must not appear — cursor never fires them.
        for key in [
            "Stop",
            "UserPromptSubmit",
            "SessionStart",
            "PermissionRequest",
        ] {
            assert!(hooks["hooks"].get(key).is_none(), "{key} leaked");
        }
    }

    #[test]
    fn cursor_reinstall_does_not_accumulate_duplicates() {
        let tmp = tempfile::tempdir().unwrap();
        install_cursor_hooks(tmp.path()).unwrap();
        install_cursor_hooks(tmp.path()).unwrap();
        let hooks = read_json(tmp.path(), ".cursor/hooks.json");
        for (event, _) in CURSOR_EVENTS {
            assert_eq!(
                hooks["hooks"][*event].as_array().unwrap().len(),
                1,
                "{event} accumulated duplicates"
            );
        }
    }

    #[test]
    fn cursor_migrates_legacy_claude_shaped_groups_and_keeps_foreign() {
        // An older nebula wrote Claude-dialect groups (PascalCase events,
        // nested hooks arrays) that cursor never fires; mission-control
        // writes flat _mcManaged groups into the same file. Migration must
        // remove the former and preserve the latter.
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join(".cursor");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("hooks.json"),
            serde_json::to_string(&json!({
                "version": 1,
                "hooks": {
                    "Stop": [
                        { "_nebulaManaged": true,
                          "hooks": [{ "type": "command",
                            "command": "curl $NEBULA_API_URL/api/hooks/cursor?agentId=$NEBULA_AGENT_ID&hookEvent=Stop" }] }
                    ],
                    "UserPromptSubmit": [
                        { "_nebulaManaged": true,
                          "hooks": [{ "type": "command",
                            "command": "curl $NEBULA_API_URL/api/hooks/cursor?agentId=$NEBULA_AGENT_ID&hookEvent=UserPromptSubmit" }] }
                    ],
                    "stop": [
                        { "command": "curl $MC_API_URL/api/hooks/cursor", "_mcManaged": true }
                    ]
                }
            }))
            .unwrap(),
        )
        .unwrap();

        install_cursor_hooks(tmp.path()).unwrap();
        let hooks = read_json(tmp.path(), ".cursor/hooks.json");
        // Stale Claude-dialect keys are gone entirely (empty arrays pruned).
        assert!(hooks["hooks"].get("Stop").is_none());
        assert!(hooks["hooks"].get("UserPromptSubmit").is_none());
        // Foreign managed group survives ahead of ours.
        let stop = hooks["hooks"]["stop"].as_array().unwrap();
        assert_eq!(stop.len(), 2, "mc group + kete group");
        assert_eq!(stop[0]["_mcManaged"], json!(true));
        assert_eq!(stop[1]["_nebulaManaged"], json!(true));
    }

    #[test]
    fn cursor_corrupt_file_aborts_without_clobbering() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join(".cursor");
        std::fs::create_dir_all(&dir).unwrap();
        let original = "{ not json";
        std::fs::write(dir.join("hooks.json"), original).unwrap();
        assert!(install_cursor_hooks(tmp.path()).is_err());
        let after = std::fs::read_to_string(dir.join("hooks.json")).unwrap();
        assert_eq!(after, original, "corrupt file must be left untouched");
    }

    #[test]
    fn per_kind_installs_do_not_interfere() {
        let tmp = tempfile::tempdir().unwrap();
        let codex_dir = tempfile::tempdir().unwrap();
        install_claude_hooks(tmp.path()).unwrap();
        install_codex_hooks(codex_dir.path()).unwrap();
        install_cursor_hooks(tmp.path()).unwrap();
        let claude = read_settings(tmp.path());
        let codex = read_json(codex_dir.path(), "hooks.json");
        let cursor = read_json(tmp.path(), ".cursor/hooks.json");
        assert!(routes_hook(
            &claude["hooks"]["Stop"][0]["hooks"][0],
            "claude",
            "Stop"
        ));
        assert!(routes(
            codex["hooks"]["Stop"][0]["hooks"][0]["command"]
                .as_str()
                .unwrap(),
            "codex",
            "Stop"
        ));
        assert!(routes(
            cursor["hooks"]["stop"][0]["command"].as_str().unwrap(),
            "cursor",
            "Stop"
        ));
    }
}
