//! `kete _statusline`: the statusLine command kete installs into a Claude
//! session's `.claude/settings.local.json` (`hooks::installer`).
//!
//! Claude pipes its statusLine JSON (documented: rate limits, context
//! window, transcript path) to it on every redraw. It hands that JSON to
//! the daemon (`/api/usage/claude`), which keeps the account's plan usage
//! and the session's context, then prints what the session's status line
//! should show:
//! - the user's own statusLine, when their other settings files define one
//!   (the project's `.claude/settings.json`, then the user settings in
//!   `$CLAUDE_CONFIG_DIR` or `~/.claude`), run with the same stdin;
//! - otherwise a compact `ctx 34% · 5h 21% · wk 3%`.
//!
//! Outside a kete session (no `NEBULA_*` variables) nothing is sent, and
//! the line still prints. Every failure is silent and the exit status is
//! always 0: a status line must never fault the session.

use nebula_core::env;
use serde_json::Value;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// How long the user's own statusLine may run.
const USER_LINE_TIMEOUT: Duration = Duration::from_secs(5);

pub fn run_statusline() -> anyhow::Result<()> {
    let mut payload = Vec::new();
    let _ = std::io::stdin().read_to_end(&mut payload);
    if let Some(agent) = env::non_empty(env::AGENT_ID) {
        if crate::hook::is_query_safe(&agent) {
            let _ = crate::hook::post_to(&format!("/api/usage/claude?agentId={agent}"), &payload);
        }
    }
    let json: Value = serde_json::from_slice(&payload).unwrap_or(Value::Null);
    // The user's own line when it prints one; kete's compact line when
    // they have none, or theirs fails or overruns.
    let line = user_statusline(&json)
        .and_then(|command| run_user_line(&command, &payload))
        .unwrap_or_else(|| compact_line(&json));
    let mut out = std::io::stdout().lock();
    let _ = out.write_all(line.as_bytes());
    let _ = out.flush();
    Ok(())
}

/// `ctx 34% · 5h 21% · resets 9:50 pm · wk 3%`, each part only when the JSON has it.
pub fn compact_line(json: &Value) -> String {
    compact_line_at(
        json,
        nebula_core::clock::now_secs() as i64,
        nebula_core::usage_window::local,
    )
}

/// [`compact_line`] as of `now` in the zone `tz`: a window that reports its
/// reset says it in local time, `5h 87% · resets 9:50 pm`.
pub fn compact_line_at(json: &Value, now: i64, tz: impl Fn(i64) -> i32 + Copy) -> String {
    let r = nebula_core::usage::claude_statusline(json);
    let mut parts = Vec::new();
    if let Some(p) = r.context_percent {
        parts.push(format!("ctx {}", nebula_core::usage::pct_text(Some(p))));
    }
    for w in &r.windows {
        let name = match w.kind {
            nebula_core::usage::WindowKind::ModelWeekly => {
                format!("{} wk", w.label.as_deref().unwrap_or("model"))
            }
            kind => kind.short().to_string(),
        };
        // A statusLine reading is as of this call.
        let mut w = w.clone();
        if w.read_at <= 0 {
            w.read_at = now;
        }
        let label = nebula_core::usage_window::format(&w, now, tz);
        parts.push(format!("{name} {}", label.compact));
    }
    parts.join(" \u{b7} ")
}

/// The statusLine command of a settings file, unless it is kete's own.
fn command_in(path: &Path) -> Option<String> {
    let v = nebula_core::settings::read_object(path).ok()??;
    let line = v.get("statusLine")?;
    if line
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("command")
        != "command"
    {
        return None;
    }
    let cmd = line.get("command")?.as_str()?.trim();
    (!cmd.is_empty() && !cmd.contains(" _statusline")).then(|| cmd.to_string())
}

/// The user's own statusLine. Kete's comes in through `--settings`, which
/// outranks every settings file, so the one Claude would otherwise have
/// drawn is looked up in Claude's own order: the project's local
/// settings, its shared settings, then the user's. Only the project
/// directory Claude names: a `cwd` deeper in the tree is not where its
/// settings live.
fn user_statusline(json: &Value) -> Option<String> {
    let project = project_dir(json);
    let user_dir = env::non_empty(env::CLAUDE_CONFIG_DIR)
        .map(PathBuf::from)
        .or_else(|| env::home_dir().map(|h| h.join(".claude")));
    statusline_from(project.as_deref(), user_dir.as_deref())
}

/// `workspace.project_dir`, and nothing else.
fn project_dir(json: &Value) -> Option<PathBuf> {
    json.pointer("/workspace/project_dir")
        .and_then(Value::as_str)
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
}

fn statusline_from(project: Option<&Path>, user_dir: Option<&Path>) -> Option<String> {
    project
        .and_then(|p| command_in(&p.join(".claude").join("settings.local.json")))
        .or_else(|| project.and_then(|p| command_in(&p.join(".claude").join("settings.json"))))
        .or_else(|| user_dir.and_then(|d| command_in(&d.join("settings.json"))))
}

/// Run the user's statusLine with `payload` on its stdin: its stdout, or
/// None when it fails, prints nothing, or overruns [`USER_LINE_TIMEOUT`].
/// Output is read in chunks with a deadline, so a command that leaves a
/// background child holding stdout open cannot hang the status line: once
/// the command itself exits, what it printed within [`DRAIN_AFTER_EXIT`]
/// is the line. An overrun kills the whole process tree.
fn run_user_line(command: &str, payload: &[u8]) -> Option<String> {
    run_user_line_within(command, payload, USER_LINE_TIMEOUT)
}

/// [`run_user_line`] with its overrun at `timeout`.
fn run_user_line_within(command: &str, payload: &[u8], timeout: Duration) -> Option<String> {
    let mut cmd = shell_command(command);
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let mut child = cmd.spawn().ok()?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(payload);
    }
    let mut stdout = child.stdout.take()?;
    let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match stdout.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });
    let started = Instant::now();
    let mut out = Vec::new();
    let mut exited_at: Option<Instant> = None;
    loop {
        if exited_at.is_none() {
            match child.try_wait() {
                Ok(Some(_)) => exited_at = Some(Instant::now()),
                Ok(None) => {}
                Err(_) => {
                    kill_tree(&mut child);
                    return None;
                }
            }
        }
        if let Some(at) = exited_at {
            if at.elapsed() >= DRAIN_AFTER_EXIT {
                break;
            }
        } else if started.elapsed() >= timeout {
            kill_tree(&mut child);
            return None;
        }
        match rx.recv_timeout(Duration::from_millis(20)) {
            Ok(chunk) => out.extend(chunk),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            // The pipe closed: everything is in.
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                if exited_at.is_some() {
                    break;
                }
                if child.try_wait().ok().flatten().is_some() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
    let text = String::from_utf8(out).ok()?;
    (!text.trim().is_empty()).then_some(text)
}

/// How long output may keep arriving after the command itself exited.
const DRAIN_AFTER_EXIT: Duration = Duration::from_millis(200);

/// Kill `child` and everything it started: its process group on unix,
/// `taskkill /T` on Windows.
fn kill_tree(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        let _ = Command::new("kill")
            .args(["-KILL", "--", &format!("-{}", child.id())])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(windows)]
    {
        let mut kill = Command::new("taskkill");
        kill.args(["/T", "/F", "/PID", &child.id().to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        nebula_core::platform::no_window(&mut kill);
        let _ = kill.status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// The shell Claude itself runs a statusLine under: `sh -c` on unix; Git
/// Bash when it is on PATH on Windows (as Claude prefers), else cmd.
fn shell_command(command: &str) -> Command {
    #[cfg(unix)]
    {
        let mut c = Command::new("sh");
        c.args(["-c", command]);
        c
    }
    #[cfg(windows)]
    {
        let mut c = match git_bash() {
            Some(bash) => {
                let mut c = Command::new(bash);
                c.args(["-c", command]);
                c
            }
            None => {
                use std::os::windows::process::CommandExt;
                let mut c = Command::new("cmd");
                c.arg("/C").raw_arg(command);
                c
            }
        };
        nebula_core::platform::no_window(&mut c);
        c
    }
}

/// Git Bash, as Claude finds it: `CLAUDE_CODE_GIT_BASH_PATH`, a `bash.exe`
/// on PATH that is not WSL's (System32), or the one beside `git.exe`.
#[cfg(windows)]
fn git_bash() -> Option<PathBuf> {
    if let Some(p) = env::non_empty("CLAUDE_CODE_GIT_BASH_PATH").map(PathBuf::from) {
        if p.is_file() {
            return Some(p);
        }
    }
    let path = std::env::var_os("PATH")?;
    let dirs: Vec<PathBuf> = std::env::split_paths(&path).collect();
    let wsl = |p: &Path| {
        p.to_string_lossy()
            .to_ascii_lowercase()
            .contains("\\windows\\system32")
    };
    dirs.iter()
        .map(|d| d.join("bash.exe"))
        .find(|p| p.is_file() && !wsl(p))
        .or_else(|| {
            dirs.iter()
                .map(|d| d.join("git.exe"))
                .find(|p| p.is_file())
                .and_then(|git| {
                    let root = git.parent()?.parent()?;
                    [
                        root.join("bin").join("bash.exe"),
                        root.join("usr").join("bin").join("bash.exe"),
                    ]
                    .into_iter()
                    .find(|p| p.is_file())
                })
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_compact_line_shows_what_the_json_has() {
        let line = compact_line(&json!({
            "context_window": {"used_percentage": 34.2},
            "rate_limits": {
                "five_hour": {"used_percentage": 21},
                "seven_day": {"used_percentage": 3}
            }
        }));
        assert_eq!(line, "ctx 34% \u{b7} 5h 21% \u{b7} wk 3%");
        assert_eq!(compact_line(&json!({})), "");
        // A reset time shows in the local zone (NZDT, +13): 08:50 UTC is 9:50 pm.
        let now = nebula_core::claude_usage::epoch_secs("2026-10-19T07:38:00Z").unwrap();
        let reset = nebula_core::claude_usage::epoch_secs("2026-10-19T08:50:00Z").unwrap();
        let line = compact_line_at(
            &json!({"rate_limits": {"five_hour": {"used_percentage": 87, "resets_at": reset}}}),
            now,
            |_| 13 * 3600,
        );
        assert_eq!(line, "5h 87% · resets 9:50 pm");
        assert_eq!(
            compact_line(&json!({"context_window": {"used_percentage": 8}})),
            "ctx 8%",
            "API billing: no plan windows"
        );
    }

    /// A settings file saved by Notepad (a byte order mark first) still
    /// gives up its statusLine.
    #[test]
    fn a_settings_file_with_a_bom_is_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(
            &path,
            "\u{feff}{\"statusLine\": {\"type\": \"command\", \"command\": \"my-line\"}}",
        )
        .unwrap();
        assert_eq!(command_in(&path).as_deref(), Some("my-line"));
    }

    #[test]
    fn the_user_line_comes_from_the_project_then_the_user_and_never_kete() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("proj");
        let user = dir.path().join("user");
        std::fs::create_dir_all(project.join(".claude")).unwrap();
        std::fs::create_dir_all(&user).unwrap();
        assert_eq!(statusline_from(Some(&project), Some(&user)), None);
        std::fs::write(
            user.join("settings.json"),
            r#"{"statusLine": {"type": "command", "command": "user-line"}}"#,
        )
        .unwrap();
        assert_eq!(
            statusline_from(Some(&project), Some(&user)).as_deref(),
            Some("user-line")
        );
        std::fs::write(
            project.join(".claude").join("settings.json"),
            r#"{"statusLine": {"type": "command", "command": "proj-line"}}"#,
        )
        .unwrap();
        assert_eq!(
            statusline_from(Some(&project), Some(&user)).as_deref(),
            Some("proj-line")
        );
        std::fs::write(
            project.join(".claude").join("settings.json"),
            r#"{"statusLine": {"type": "command", "command": "C:\"/k/kete.exe\" _statusline"}}"#,
        )
        .unwrap();
        assert_eq!(
            statusline_from(Some(&project), Some(&user)).as_deref(),
            Some("user-line"),
            "kete's own never chains to itself"
        );
        // The project's local settings come first (`--settings` outranks
        // them, so this is the line Claude would have drawn).
        std::fs::write(
            project.join(".claude").join("settings.local.json"),
            r#"{"statusLine": {"type": "command", "command": "local-line"}}"#,
        )
        .unwrap();
        assert_eq!(
            statusline_from(Some(&project), Some(&user)).as_deref(),
            Some("local-line")
        );
    }

    #[test]
    fn only_the_project_dir_names_the_project() {
        assert_eq!(
            project_dir(&serde_json::json!({"cwd": "/somewhere/deeper"})),
            None
        );
        assert_eq!(
            project_dir(&serde_json::json!({"cwd": "/p/sub", "workspace": {"project_dir": "/p"}})),
            Some(PathBuf::from("/p"))
        );
    }

    #[test]
    fn a_line_whose_background_child_holds_stdout_still_returns() {
        #[cfg(windows)]
        if git_bash().is_none() {
            return;
        }
        // Bounds measured against the minute the background child holds
        // stdout, not against how fast a loaded machine starts Git Bash:
        // a line that waited on the child would overrun the generous
        // timeout here and come back empty, however long bash took.
        let roomy = Duration::from_secs(30);
        let out = run_user_line_within("(sleep 60 &) ; echo quick", b"", roomy).unwrap_or_default();
        assert!(out.contains("quick"), "{out:?}");
        // A line that never ends is cut off at its timeout, and the caller
        // falls back.
        let started = Instant::now();
        assert_eq!(
            run_user_line_within("sleep 60", b"", Duration::from_secs(1)),
            None
        );
        assert!(started.elapsed() < roomy, "{:?}", started.elapsed());
        assert_eq!(run_user_line("true", b""), None, "an empty line falls back");
    }

    #[test]
    fn a_user_line_gets_the_payload_on_stdin() {
        #[cfg(windows)]
        if git_bash().is_none() {
            return; // no Git Bash here: cmd has no stdin echo worth testing
        }
        let out = run_user_line("cat", b"hello").unwrap_or_default();
        assert!(out.contains("hello"), "{out:?}");
    }
}
