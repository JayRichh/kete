//! The DELETE CHECK: what a worktree's confirm says before it takes the
//! checkout off disk — the sessions and terminals that go down with it,
//! its uncommitted changes, the commits nobody pushed — and the two-step
//! way a checkout with uncommitted work is deleted at all.
//!
//! A worktree delete is never forced on the first answer. `Enter` on the
//! confirm sends the delete unforced, which the DAEMON refuses for a
//! checkout with uncommitted changes before it stops anything in it
//! (`UNCOMMITTED_REFUSAL`); git refuses the rest (submodules, a race).
//! Throwing work away takes a second dialog whose only yes is `f`, a key
//! the first never takes — `Enter` and `y` there keep the checkout — so a
//! doubled answer or a key repeat can never chain into a forced delete.
//! The second dialog comes up two ways: from
//! the first one, when the check already found changes, or from the
//! refusal itself, for a delete sent before the check landed or from a
//! confirm that runs none (a card's delete folding its worktree in, the
//! **Delete emptied worktree** setting).
//!
//! The check is client-side git, like the BRANCH SWITCHER's change count:
//! `git status` and a `rev-list` on the blocking pool (`view_jobs`),
//! landing in the confirm in place. A check that fails or is still out
//! never reads as clean. Processes working in the checkout from outside
//! nebula are not looked for: that sweep needs the DAEMON.

use crate::app::{App, ConfirmDialog, Overlay, PendingAction};
use nebula_core::WorktreeId;
use std::path::Path;

/// What the check read in a checkout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Findings {
    /// Uncommitted changes, untracked files included.
    pub changes: usize,
    /// Commits on HEAD its upstream lacks — or, with no upstream, that no
    /// remote has. None when there is no remote to compare with.
    pub unpushed: Option<usize>,
}

/// Where the check stands, as the confirm shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Check {
    Checking,
    Found(Findings),
    Failed(String),
}

/// The words the DAEMON refuses an unforced delete with
/// (`nebula_daemon::registry::UNCOMMITTED_REFUSAL`, kept in step by hand:
/// the TUI does not link the daemon).
const UNCOMMITTED_REFUSAL: &str = "has uncommitted changes";

/// Names a line spells out before it says `+N more`.
const NAMED: usize = 3;

/// Why a [`read_with`] git run gave no output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunError {
    /// It ran past the read's deadline and was killed.
    TimedOut,
    Failed(String),
}

/// One git run for [`read_with`]: `git -C <path> <args>`.
pub type GitRun<'a> = &'a dyn Fn(&Path, &[&str]) -> Result<std::process::Output, RunError>;

/// Read `path`: blocking git, so off the loop wherever there is one.
pub fn read(path: &Path) -> Result<Findings, String> {
    let run = |p: &Path, args: &[&str]| crate::git_diff::run_git(p, args).map_err(RunError::Failed);
    read_with(path, &run).map_err(|e| match e {
        RunError::TimedOut => "git took too long".to_string(),
        RunError::Failed(why) => why,
    })
}

/// [`read`] with every git call bounded by one shared `timeout`: the
/// MOVE CHECK's read, which a hung git must not hold open.
pub fn read_within(path: &Path, timeout: std::time::Duration) -> Result<Findings, RunError> {
    let deadline = std::time::Instant::now() + timeout;
    let run = move |p: &Path, args: &[&str]| {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        if left.is_zero() {
            return Err(RunError::TimedOut);
        }
        let mut cmd = crate::git_diff::git_command(p);
        cmd.args(args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        kete_ado::git::output_within(cmd, left, "git").map_err(|why| {
            if std::time::Instant::now() >= deadline {
                RunError::TimedOut
            } else {
                RunError::Failed(why)
            }
        })
    };
    read_with(path, &run)
}

/// [`read`] through `run`: the changes `git status -uall` lists (each
/// untracked file its own), and the commits HEAD has that its upstream
/// lacks, or with no upstream that no remote has.
pub fn read_with(path: &Path, run: GitRun<'_>) -> Result<Findings, RunError> {
    if !path.is_dir() {
        return Err(RunError::Failed(format!("no folder at {}", path.display())));
    }
    let status = run(path, &["status", "--porcelain=v1", "-z", "-uall"])?;
    if !status.status.success() {
        let stderr = String::from_utf8_lossy(&status.stderr);
        return Err(RunError::Failed(format!(
            "git status failed: {}",
            stderr.trim()
        )));
    }
    let changes = crate::git_diff::parse_status_z(&status.stdout).len();
    Ok(Findings {
        changes,
        unpushed: unpushed(path, run)?,
    })
}

fn count(path: &Path, args: &[&str], run: GitRun<'_>) -> Result<Option<usize>, RunError> {
    let out = match run(path, args) {
        Ok(out) => out,
        Err(RunError::TimedOut) => return Err(RunError::TimedOut),
        Err(RunError::Failed(_)) => return Ok(None),
    };
    if !out.status.success() {
        return Ok(None);
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().parse().ok())
}

fn unpushed(path: &Path, run: GitRun<'_>) -> Result<Option<usize>, RunError> {
    if let Some(n) = count(path, &["rev-list", "--count", "@{upstream}..HEAD"], run)? {
        return Ok(Some(n));
    }
    // No upstream: the commits no remote has. With no remote at all the
    // count would be the whole history, which says nothing.
    let remotes = match run(path, &["remote"]) {
        Ok(out) => out,
        Err(RunError::TimedOut) => return Err(RunError::TimedOut),
        Err(RunError::Failed(_)) => return Ok(None),
    };
    if String::from_utf8_lossy(&remotes.stdout).trim().is_empty() {
        return Ok(None);
    }
    count(
        path,
        &["rev-list", "--count", "HEAD", "--not", "--remotes"],
        run,
    )
}

/// What runs in a checkout: its live sessions' and terminals' names.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Live {
    pub sessions: Vec<String>,
    pub terminals: Vec<String>,
}

impl Live {
    pub fn total(&self) -> usize {
        self.sessions.len() + self.terminals.len()
    }
}

pub fn live_in(app: &App, id: &WorktreeId) -> Live {
    Live {
        sessions: app
            .tree
            .agents
            .iter()
            .filter(|a| &a.worktree_id == id && !a.archived)
            .map(|a| a.name.clone())
            .collect(),
        terminals: app
            .tree
            .terminals
            .iter()
            .filter(|t| &t.worktree_id == id)
            .map(|t| t.name.clone())
            .collect(),
    }
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// `a, b, c +2 more`.
fn names(all: &[String]) -> String {
    let mut out = all
        .iter()
        .take(NAMED)
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    if all.len() > NAMED {
        out.push_str(&format!(" +{} more", all.len() - NAMED));
    }
    out
}

/// The line saying what a delete stops.
pub fn live_line(live: &Live) -> String {
    let mut parts = Vec::new();
    if !live.sessions.is_empty() {
        parts.push(format!(
            "{} ({})",
            plural(live.sessions.len(), "session", "sessions"),
            names(&live.sessions)
        ));
    }
    if !live.terminals.is_empty() {
        parts.push(format!(
            "{} ({})",
            plural(live.terminals.len(), "terminal", "terminals"),
            names(&live.terminals)
        ));
    }
    if parts.is_empty() {
        "Nothing is running in it.".into()
    } else {
        format!("Stops {}.", parts.join(" and "))
    }
}

/// `1 uncommitted change` / `3 uncommitted changes` — the BRANCH
/// SWITCHER's words.
fn changes_text(n: usize) -> String {
    plural(n, "uncommitted change", "uncommitted changes")
}

/// The first confirm's message.
pub fn confirm_message(branch: &str, live: &Live, check: &Check) -> String {
    let mut lines = vec![
        format!("Delete worktree '{branch}' from disk?"),
        live_line(live),
    ];
    match check {
        Check::Checking => lines.push("Checking it for uncommitted changes…".into()),
        Check::Failed(why) => lines.push(format!(
            "Could not check it ({why}): git keeps it if anything is uncommitted."
        )),
        Check::Found(found) => {
            if found.changes > 0 {
                lines.push(format!(
                    "{}: Enter asks again before throwing them away.",
                    changes_text(found.changes)
                ));
            } else {
                lines.push("Nothing uncommitted.".into());
            }
            if let Some(n) = found.unpushed.filter(|n| *n > 0) {
                lines.push(format!(
                    "{} not pushed (the branch keeps them).",
                    plural(n, "commit", "commits")
                ));
            }
        }
    }
    lines.join("\n")
}

/// The first confirm for worktree `id`, as `check` leaves it: `Enter`
/// sends the unforced delete — or, once the check has found uncommitted
/// changes, opens the second confirm.
pub fn worktree_confirm(app: &App, id: &WorktreeId, check: &Check) -> Option<ConfirmDialog> {
    let w = app.tree.worktrees.iter().find(|w| &w.id == id)?;
    let action = match check {
        Check::Found(found) if found.changes > 0 => PendingAction::AskForceDeleteWorktree {
            worktree: id.clone(),
            why: format!("'{}' has {}", w.branch, changes_text(found.changes)),
        },
        _ => PendingAction::DeleteWorktree(id.clone()),
    };
    Some(ConfirmDialog {
        title: "Delete worktree".into(),
        message: confirm_message(&w.branch, &live_in(app, id), check),
        action,
        area: ratatui::layout::Rect::default(),
    })
}

/// The second confirm: the forced delete, whose only yes is `f`. `why`
/// is the first line — what the check found, or the refusal's own words.
pub fn force_confirm(branch: &str, id: &WorktreeId, why: &str) -> ConfirmDialog {
    let why = why.trim().trim_end_matches('.');
    let mut chars = why.chars();
    let why: String = chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default();
    ConfirmDialog {
        title: "Throw away uncommitted work".into(),
        message: format!(
            "{why}.\nDelete '{branch}' anyway, throwing away everything not committed?\nThis cannot be undone."
        ),
        action: PendingAction::ForceDeleteWorktree(id.clone()),
        area: ratatui::layout::Rect::default(),
    }
}

/// Is `message` a refusal a forced delete would get past — the DAEMON's
/// own for uncommitted changes, or git's for a checkout it will only
/// remove with `--force`?
pub fn is_refusal(message: &str) -> bool {
    message.contains(UNCOMMITTED_REFUSAL)
        || message.contains("--force")
        || message.contains("modified or untracked files")
        || message.contains("containing submodules")
}

/// Start the check for `id`'s confirm: on the blocking pool with the
/// answer landing through [`land`], or inline where no loop runs (unit
/// tests), as the worktree views do.
pub fn start(app: &mut App, id: &WorktreeId) {
    let Some(path) = app
        .tree
        .worktrees
        .iter()
        .find(|w| &w.id == id)
        .map(|w| w.path.clone())
    else {
        return;
    };
    match app.jobs.view_jobs.clone() {
        Some(jobs) => {
            let worktree = id.clone();
            jobs.run(move || {
                Some(crate::view_jobs::Answer::DeleteCheck {
                    worktree,
                    result: read(&path),
                })
            });
        }
        None => land(app, id.clone(), read(&path)),
    }
}

/// The worktree the open confirm is the first DELETE CHECK confirm of.
fn confirm_target(app: &App) -> Option<WorktreeId> {
    match &app.modals.overlay {
        Some(Overlay::Confirm(c)) => match &c.action {
            PendingAction::DeleteWorktree(id)
            | PendingAction::AskForceDeleteWorktree { worktree: id, .. } => Some(id.clone()),
            _ => None,
        },
        _ => None,
    }
}

/// A check's answer: the confirm still open on that worktree is rebuilt
/// around it; one closed since drops it.
pub fn land(app: &mut App, worktree: WorktreeId, result: Result<Findings, String>) {
    if confirm_target(app).as_ref() != Some(&worktree) {
        return;
    }
    let check = match result {
        Ok(found) => Check::Found(found),
        Err(why) => Check::Failed(why),
    };
    if let Some(dialog) = worktree_confirm(app, &worktree, &check) {
        app.modals.overlay = Some(Overlay::Confirm(dialog));
        app.chrome.dirty = true;
    }
}

#[cfg(test)]
mod tests {
    fn repo() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(tmp.path())
            .args(["init", "-q"])
            .output()
            .unwrap();
        assert!(out.status.success());
        tmp
    }

    /// A deadline that has passed runs no git and reads as timed out.
    #[test]
    fn a_bounded_read_past_its_deadline_times_out() {
        let tmp = repo();
        assert_eq!(
            read_within(tmp.path(), std::time::Duration::ZERO),
            Err(RunError::TimedOut)
        );
    }

    /// A git that times out partway stops the read as timed out, never as a
    /// clean checkout; one that fails on the commit count leaves it unknown.
    #[test]
    fn a_git_that_times_out_partway_is_a_timeout_not_a_clean_read() {
        let tmp = repo();
        let ok = |stdout: &str| {
            let real = std::process::Command::new("git")
                .arg("--version")
                .output()
                .unwrap();
            std::process::Output {
                status: real.status,
                stdout: stdout.as_bytes().to_vec(),
                stderr: Vec::new(),
            }
        };
        let status_then_hang = |_: &Path, args: &[&str]| match args[0] {
            "status" => Ok(ok("?? a.txt ")),
            _ => Err(RunError::TimedOut),
        };
        assert_eq!(
            read_with(tmp.path(), &status_then_hang),
            Err(RunError::TimedOut)
        );
        let status_then_fail = |_: &Path, args: &[&str]| match args[0] {
            "status" => Ok(ok("?? a.txt ?? b.txt ")),
            _ => Err(RunError::Failed("no git".into())),
        };
        assert_eq!(
            read_with(tmp.path(), &status_then_fail),
            Ok(Findings {
                changes: 2,
                unpushed: None
            })
        );
    }

    use super::*;

    fn live(sessions: &[&str], terminals: &[&str]) -> Live {
        Live {
            sessions: sessions.iter().map(|s| s.to_string()).collect(),
            terminals: terminals.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn the_live_line_names_what_stops() {
        assert_eq!(live_line(&Live::default()), "Nothing is running in it.");
        assert_eq!(
            live_line(&live(&["fix-auth"], &[])),
            "Stops 1 session (fix-auth)."
        );
        assert_eq!(
            live_line(&live(&["a", "b", "c", "d", "e"], &["zsh"])),
            "Stops 5 sessions (a, b, c +2 more) and 1 terminal (zsh)."
        );
    }

    #[test]
    fn the_message_never_reads_clean_until_the_check_says_so() {
        let l = live(&["a"], &[]);
        let checking = confirm_message("feat", &l, &Check::Checking);
        assert!(checking.contains("Checking it"), "{checking}");
        assert!(!checking.contains("Nothing uncommitted"));
        let failed = confirm_message("feat", &l, &Check::Failed("no folder".into()));
        assert!(
            failed.contains("Could not check it (no folder)"),
            "{failed}"
        );
        assert!(!failed.contains("Nothing uncommitted"));
        let clean = confirm_message(
            "feat",
            &l,
            &Check::Found(Findings {
                changes: 0,
                unpushed: Some(0),
            }),
        );
        assert_eq!(
            clean,
            "Delete worktree 'feat' from disk?\nStops 1 session (a).\nNothing uncommitted."
        );
        let dirty = confirm_message(
            "feat",
            &Live::default(),
            &Check::Found(Findings {
                changes: 3,
                unpushed: Some(2),
            }),
        );
        assert_eq!(
            dirty,
            "Delete worktree 'feat' from disk?\nNothing is running in it.\n\
             3 uncommitted changes: Enter asks again before throwing them away.\n\
             2 commits not pushed (the branch keeps them)."
        );
    }

    #[test]
    fn refusals_a_force_gets_past_are_recognised() {
        assert!(is_refusal(
            "worktree 'feat' has uncommitted changes (2): kept, nothing in it stopped"
        ));
        assert!(is_refusal(
            "fatal: '/x/feat' contains modified or untracked files, use --force to delete it"
        ));
        assert!(!is_refusal("worktree not found"));
    }

    /// A real checkout: the change count and the unpushed count come from
    /// git, and a folder that is gone fails rather than reading clean.
    #[test]
    fn read_counts_changes_and_unpushed_commits() {
        let git = |dir: &Path, args: &[&str]| {
            let out = std::process::Command::new("git")
                .arg("-C")
                .arg(dir)
                .args(args)
                .env("GIT_AUTHOR_NAME", "t")
                .env("GIT_AUTHOR_EMAIL", "t@example.com")
                .env("GIT_COMMITTER_NAME", "t")
                .env("GIT_COMMITTER_EMAIL", "t@example.com")
                .output()
                .unwrap();
            assert!(out.status.success(), "{args:?}: {out:?}");
        };
        let tmp = tempfile::tempdir().unwrap();
        let origin = tmp.path().join("origin");
        std::fs::create_dir(&origin).unwrap();
        git(&origin, &["init", "-b", "main"]);
        git(&origin, &["commit", "--allow-empty", "-m", "init"]);
        let repo = tmp.path().join("repo");
        git(
            tmp.path(),
            &["clone", &origin.to_string_lossy(), &repo.to_string_lossy()],
        );
        assert_eq!(
            read(&repo),
            Ok(Findings {
                changes: 0,
                unpushed: Some(0)
            })
        );
        git(&repo, &["commit", "--allow-empty", "-m", "local"]);
        std::fs::write(repo.join("a.txt"), "x").unwrap();
        std::fs::write(repo.join("b.txt"), "y").unwrap();
        assert_eq!(
            read(&repo),
            Ok(Findings {
                changes: 2,
                unpushed: Some(1)
            })
        );
        assert!(read(&tmp.path().join("gone")).is_err());
    }
}
