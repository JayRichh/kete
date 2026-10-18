//! The window's worktree and branch git: what a worktree delete would
//! throw away, the settings a session delete reads, and the BRANCH
//! SWITCHER, which moves a repo's ROOT CHECKOUT onto another branch as
//! the terminal app's `c` does (`nebula-tui`'s `branch_switch`, ported in
//! part: stash, bring along, discard, create; no commit-and-switch, and a
//! checkout git abandons part-way is reported rather than undone).
//!
//! The daemon is not asked: its worktree sync notices a root `HEAD` that
//! moved and renames the row. Agents run git in the same repository while
//! a switch runs, so nothing here trusts a position: a stash entry is
//! found by commit and message, and a discard carries the fingerprints of
//! the changes the window showed, so it never throws away an edit it did
//! not show.

use crate::daemon::Client;
use crate::git::{self, git_command, git_raw, output_within};
use crate::{worktree_path, Res};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, UNIX_EPOCH};
use tauri::State;

/// How long a background fetch may run.
const FETCH_TIMEOUT: Duration = Duration::from_secs(120);
/// `git for-each-ref` record: NUL between fields, RS after each record.
const REF_FORMAT: &str = "--format=%(refname)%00%(symref)%00%(HEAD)%00%(worktreepath)%00%(committerdate:unix)%00%(contents:subject)%1e";

// ---- settings and delete state ----

/// The config rows a session delete reads, fresh from config.json (the
/// terminal app may have changed them since the window started).
#[derive(Serialize)]
pub struct WorktreeSettings {
    /// Deleting a worktree's last session or terminal takes the worktree
    /// too, without asking.
    delete_empty_worktree: bool,
}

#[tauri::command]
pub fn worktree_settings() -> WorktreeSettings {
    let cfg = nebula_config::Config::load();
    WorktreeSettings {
        delete_empty_worktree: cfg.delete_empty_worktree,
    }
}

/// Uncommitted, untracked and unpushed work in a checkout, for the
/// worktree delete confirm.
#[tauri::command]
pub async fn checkout_state(
    client: State<'_, Arc<Client>>,
    worktree: String,
) -> Res<git::CheckoutState> {
    let root = worktree_path(&client, &worktree)?;
    tauri::async_runtime::spawn_blocking(move || git::checkout_state(&root))
        .await
        .map_err(|e| e.to_string())?
}

/// Each named worktree's [`git::git_stamp`]: a fingerprint read off file
/// metadata alone, cheap enough to poll every couple of seconds. The
/// window reads a checkout's [`worktree_git`] again only when its stamp
/// moves. A worktree that is gone, or not a checkout, is left out.
#[tauri::command]
pub async fn git_stamps(
    client: State<'_, Arc<Client>>,
    worktrees: Vec<String>,
) -> Res<std::collections::BTreeMap<String, String>> {
    let snap = client.snapshot();
    let roots: Vec<(String, PathBuf)> = worktrees
        .into_iter()
        .filter_map(|id| {
            let w = snap.worktrees.iter().find(|w| w.id.0 == id)?;
            Some((id, w.path.clone()))
        })
        .collect();
    tauri::async_runtime::spawn_blocking(move || {
        roots
            .into_iter()
            .filter_map(|(id, root)| git::git_stamp(&root).map(|s| (id, s)))
            .collect()
    })
    .await
    .map_err(|e| e.to_string())
}

/// How many checkouts one [`worktree_git`] call reads at once.
const GIT_READERS: usize = 4;

/// Each named worktree's [`git::WorktreeGit`], for the overview's worktree
/// rows and the Worktrees view. A worktree that is gone, or whose git
/// can't be read, is left out rather than failing the rest; the window
/// keeps what it had for it.
#[tauri::command]
pub async fn worktree_git(
    client: State<'_, Arc<Client>>,
    worktrees: Vec<String>,
) -> Res<std::collections::BTreeMap<String, git::WorktreeGit>> {
    let snap = client.snapshot();
    let roots: Vec<(String, PathBuf, String)> = worktrees
        .into_iter()
        .filter_map(|id| {
            let w = snap.worktrees.iter().find(|w| w.id.0 == id)?;
            Some((id, w.path.clone(), w.branch.clone()))
        })
        .collect();
    let recorded = snap.bases.clone();
    let mut bases = client.base_refs();
    let (out, bases) = tauri::async_runtime::spawn_blocking(move || {
        // Each checkout's base ref, kept between polls: resolved (two git
        // calls) only for a branch new to it, the setting read only then.
        let mut setting: Option<String> = None;
        let roots: Vec<_> = roots
            .into_iter()
            .map(|(id, root, branch)| {
                let base = bases.get_with(
                    &root,
                    &branch,
                    || {
                        setting
                            .get_or_insert_with(|| {
                                nebula_config::Config::load().worktree_base_branch
                            })
                            .clone()
                    },
                    recorded.get(&id),
                );
                (id, root, base)
            })
            .collect();
        let mut out = std::collections::BTreeMap::new();
        for chunk in roots.chunks(GIT_READERS) {
            std::thread::scope(|s| {
                let reads: Vec<_> = chunk
                    .iter()
                    .map(|(id, root, base)| {
                        s.spawn(move || {
                            // One `rev-list --count` beside the status read.
                            let read = git::worktree_git(root).map(|mut g| {
                                let standing = base.standing(root);
                                g.ahead_of_base = standing.ahead_of_base;
                                g.on_default_branch = standing.on_default_branch;
                                g
                            });
                            (id.clone(), read)
                        })
                    })
                    .collect();
                for r in reads {
                    if let Ok((id, Ok(state))) = r.join() {
                        out.insert(id, state);
                    }
                }
            });
        }
        (out, bases)
    })
    .await
    .map_err(|e| e.to_string())?;
    client.keep_base_refs(bases);
    // The board's Review and Ship lanes read these too, so it costs no
    // git of its own.
    client.set_worktree_changes(out.iter().map(|(id, g)| (id.clone(), g.card_facts())));
    Ok(out)
}

/// Open a worktree's folder in the system's file manager.
#[tauri::command]
pub fn worktree_reveal(
    app: tauri::AppHandle,
    client: State<'_, Arc<Client>>,
    worktree: String,
) -> Res<()> {
    use tauri_plugin_opener::OpenerExt;
    let root = worktree_path(&client, &worktree)?;
    if !root.is_dir() {
        return Err(format!("{} is not there any more", root.display()));
    }
    app.opener()
        .open_path(root.display().to_string(), None::<&str>)
        .map_err(|e| e.to_string())
}

// ---- git ----

/// One row: a local branch, or a remote-tracking branch nothing local
/// shadows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Branch {
    /// `feature-x`, or `origin/feature-x` for a remote branch.
    pub name: String,
    pub remote: bool,
    /// The branch the root checkout is on.
    pub current: bool,
    /// The other checkout a local branch is checked out in: git keeps a
    /// branch in one checkout at a time, so the window refuses it.
    pub checked_out_at: Option<String>,
    /// Committer date of the tip, unix seconds.
    pub committed: i64,
    pub subject: String,
}

/// What a switch lands on: a listed branch, or a new name.
#[derive(Debug, Clone, Deserialize)]
pub struct Target {
    pub name: String,
    #[serde(default)]
    pub remote: bool,
}

impl Target {
    /// The local branch a switch lands on: a remote branch's name without
    /// its remote (`origin/feature-x` → `feature-x`).
    fn local_name(&self) -> &str {
        if self.remote {
            self.name
                .split_once('/')
                .map_or(&self.name, |(_, rest)| rest)
        } else {
            &self.name
        }
    }
}

/// `git for-each-ref` output in [`REF_FORMAT`] as rows: the current branch
/// first, the other local branches next, then the remote branches with no
/// local branch of the same name. git's newest-first order holds within
/// each group; `origin/HEAD` is dropped.
pub fn parse_refs(out: &str) -> Vec<Branch> {
    let mut local: Vec<Branch> = Vec::new();
    let mut remote: Vec<Branch> = Vec::new();
    for record in out.split('\x1e') {
        let record = record.trim_start_matches(['\n', '\r']);
        let mut fields = record.split('\0');
        let (Some(refname), Some(symref), Some(head), Some(worktree), Some(date)) = (
            fields.next(),
            fields.next(),
            fields.next(),
            fields.next(),
            fields.next(),
        ) else {
            continue;
        };
        let committed = date.trim().parse().unwrap_or(0);
        let subject = fields.next().unwrap_or("").trim().to_string();
        if let Some(name) = refname.strip_prefix("refs/heads/") {
            let current = head == "*";
            local.push(Branch {
                name: name.to_string(),
                remote: false,
                current,
                checked_out_at: (!current && !worktree.is_empty()).then(|| worktree.to_string()),
                committed,
                subject,
            });
        } else if let Some(name) = refname.strip_prefix("refs/remotes/") {
            if !symref.is_empty() || !name.contains('/') {
                continue;
            }
            remote.push(Branch {
                name: name.to_string(),
                remote: true,
                current: false,
                checked_out_at: None,
                committed,
                subject,
            });
        }
    }
    let local_name = |b: &Branch| {
        b.name
            .split_once('/')
            .map_or(String::new(), |(_, r)| r.to_string())
    };
    remote.retain(|r| !local.iter().any(|l| l.name == local_name(r)));
    local.sort_by_key(|b| !b.current);
    local.extend(remote);
    local
}

/// git's complaint as one line: the first `error:` or `fatal:` it printed,
/// without the prefix, else the first thing it said at all.
fn git_error(stderr: &str) -> String {
    let lines: Vec<&str> = stderr
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let line = lines
        .iter()
        .find(|l| l.starts_with("error: ") || l.starts_with("fatal: "))
        .or(lines.first())
        .copied()
        .unwrap_or("git failed");
    line.strip_prefix("error: ")
        .or_else(|| line.strip_prefix("fatal: "))
        .unwrap_or(line)
        .to_string()
}

/// `git <args>` in `root`: stdout, or git's one-line complaint. Without
/// the diff's literal-pathspec setting: `git stash push
/// --include-untracked` under it saves untracked files but leaves them on
/// disk, and the switch would then carry them anyway.
fn run(root: &Path, args: &[&str]) -> Result<String, String> {
    let out = git_command(root, args)
        .env_remove("GIT_LITERAL_PATHSPECS")
        .output()
        .map_err(|e| format!("could not run git: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(git_error(&String::from_utf8_lossy(&out.stderr)))
    }
}

/// Every branch the root checkout could switch to.
pub fn list_branches(root: &Path) -> Result<Vec<Branch>, String> {
    run(
        root,
        &[
            "for-each-ref",
            "--sort=-committerdate",
            REF_FORMAT,
            "refs/heads",
            "refs/remotes",
        ],
    )
    .map(|out| parse_refs(&out))
}

/// One uncommitted change: git's two status letters and the path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Change {
    pub xy: String,
    pub path: String,
}

impl Change {
    fn untracked(&self) -> bool {
        self.xy == "??"
    }

    fn unmerged(&self) -> bool {
        self.xy.contains('U') || self.xy == "AA" || self.xy == "DD"
    }
}

/// `git status --porcelain=v1 -z` output as changes. A rename or copy
/// carries its old path as one more entry, which is skipped.
pub fn parse_status(raw: &[u8]) -> Vec<Change> {
    let mut out = Vec::new();
    let mut entries = raw.split(|b| *b == 0);
    while let Some(e) = entries.next() {
        if e.len() < 4 {
            continue;
        }
        let xy = String::from_utf8_lossy(&e[..2]).into_owned();
        if xy == "!!" {
            continue;
        }
        if e[0] == b'R' || e[0] == b'C' {
            entries.next();
        }
        out.push(Change {
            xy,
            path: String::from_utf8_lossy(&e[3..]).into_owned(),
        });
    }
    out
}

/// The checkout's uncommitted changes. An untracked directory that is a
/// repository of its own (a worktree nested in the root, like
/// `.claude/worktrees/x`) is not one: no switch moves it, a stash skips it.
pub fn changes(root: &Path) -> Result<Vec<Change>, String> {
    let out = git_raw(root, &["status", "--porcelain=v1", "-z"])?;
    if !out.status.success() {
        return Err(git_error(&String::from_utf8_lossy(&out.stderr)));
    }
    let mut files = parse_status(&out.stdout);
    files.retain(|f| {
        !(f.untracked() && f.path.ends_with('/') && root.join(&f.path).join(".git").exists())
    });
    Ok(files)
}

/// Each change as the window showed it: status, path, size and mtime, so
/// another edit to a file already listed still reads as a change.
fn fingerprints(root: &Path, files: &[Change]) -> Vec<String> {
    files
        .iter()
        .map(|f| {
            let stamp = std::fs::symlink_metadata(root.join(&f.path))
                .map(|m| {
                    let modified = m
                        .modified()
                        .ok()
                        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                        .map_or(0, |d| d.as_nanos());
                    format!("{} {modified}", m.len())
                })
                .unwrap_or_else(|_| "-".into());
            format!("{} {} {stamp}", f.xy, f.path)
        })
        .collect()
}

fn fingerprint_now(root: &Path) -> Vec<String> {
    changes(root)
        .map(|files| fingerprints(root, &files))
        .unwrap_or_default()
}

/// The operation git is in the middle of, if any: no switch then.
fn in_progress(root: &Path) -> Option<&'static str> {
    const MARKS: [(&str, &str); 5] = [
        ("MERGE_HEAD", "a merge"),
        ("rebase-merge", "a rebase"),
        ("rebase-apply", "a rebase"),
        ("CHERRY_PICK_HEAD", "a cherry-pick"),
        ("REVERT_HEAD", "a revert"),
    ];
    let mut args = vec!["rev-parse"];
    for (mark, _) in MARKS {
        args.extend(["--git-path", mark]);
    }
    let out = run(root, &args).ok()?;
    out.lines()
        .zip(MARKS)
        .find(|(path, _)| root.join(path.trim()).exists())
        .map(|(_, (_, what))| what)
}

/// The branch HEAD is on; None when it is detached.
fn head_branch(root: &Path) -> Option<String> {
    run(root, &["symbolic-ref", "-q", "--short", "HEAD"])
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// The stash stack, top first, as (commit, subject).
fn stash_entries(root: &Path) -> Vec<(String, String)> {
    run(root, &["stash", "list", "--format=%H%x00%s"])
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.split_once('\0'))
        .map(|(c, s)| (c.to_string(), s.to_string()))
        .collect()
}

/// Stash the checkout's changes under `message` and return the entry's
/// commit, found as the new entry carrying the message (other checkouts
/// push onto the same stack). None when there was nothing to save.
fn stash_push(root: &Path, message: &str) -> Result<Option<String>, String> {
    let before: HashSet<String> = stash_entries(root).into_iter().map(|(c, _)| c).collect();
    run(
        root,
        &[
            "stash",
            "push",
            "--quiet",
            "--include-untracked",
            "-m",
            message,
        ],
    )?;
    Ok(stash_entries(root)
        .into_iter()
        .find(|(c, s)| !before.contains(c) && s.ends_with(message))
        .map(|(c, _)| c))
}

/// Apply the stash entry `commit` back and drop it; false when it would
/// not apply, which leaves the entry. The drop names a position, so the
/// position is checked against the commit right before.
fn stash_restore(root: &Path, commit: &str) -> bool {
    let applied = run(root, &["stash", "apply", "--quiet", "--index", commit]).is_ok()
        || run(root, &["stash", "apply", "--quiet", commit]).is_ok();
    if applied {
        if let Some(i) = stash_entries(root).iter().position(|(c, _)| c == commit) {
            let entry = format!("stash@{{{i}}}");
            if run(root, &["rev-parse", &entry]).is_ok_and(|c| c.trim() == commit) {
                let _ = run(root, &["stash", "drop", "--quiet", &entry]);
            }
        }
    }
    applied
}

/// The index file's bytes, saved before a discard unstages everything, to
/// put back exactly if the switch then fails.
struct SavedIndex {
    path: PathBuf,
    bytes: Vec<u8>,
}

fn save_index(root: &Path) -> Option<SavedIndex> {
    let path = root.join(
        run(root, &["rev-parse", "--git-path", "index"])
            .ok()?
            .trim(),
    );
    let bytes = std::fs::read(&path).ok()?;
    Some(SavedIndex { path, bytes })
}

/// Write the saved index back as git writes one, through `index.lock`.
fn restore_index(saved: &SavedIndex) -> bool {
    let mut lock = saved.path.clone().into_os_string();
    lock.push(".lock");
    let lock = PathBuf::from(lock);
    let Ok(mut file) = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&lock)
    else {
        return false;
    };
    let written = file.write_all(&saved.bytes).and_then(|_| file.sync_all());
    drop(file);
    if written.is_ok() && std::fs::rename(&lock, &saved.path).is_ok() {
        return true;
    }
    let _ = std::fs::remove_file(&lock);
    false
}

/// How uncommitted changes travel with a switch.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Carry {
    /// Expect a clean checkout: if it isn't, stop and ask.
    Ask,
    /// `git stash push --include-untracked`, then switch; a refused switch
    /// applies the entry straight back.
    Stash,
    /// A plain `git switch`: changes that don't collide come along.
    Bring,
    /// Unstage everything, then `git switch --discard-changes`. `keys` are
    /// the fingerprints of the changes the window showed; a checkout that
    /// no longer matches them is not touched, and the window asks again.
    Discard { keys: Vec<String> },
    /// `git switch --create`: a new branch off HEAD; every change comes.
    Create,
}

/// What a switch came to.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Outcome {
    /// On `branch`; `note` says what happened to the changes, or what git
    /// complained of once it had switched.
    Switched {
        branch: String,
        note: Option<String>,
    },
    /// The checkout has these changes and needs an answer about them;
    /// nothing was touched. `keys` fingerprint them.
    Dirty {
        files: Vec<Change>,
        keys: Vec<String>,
    },
    /// Nothing moved.
    Failed { message: String },
    /// The switch failed and the checkout is no longer as it was shown.
    Stopped { message: String },
}

fn failed(message: impl Into<String>) -> Outcome {
    Outcome::Failed {
        message: message.into(),
    }
}

/// Move the checkout at `root` onto `target`, carrying its changes as
/// `carry` says. Nothing runs mid-merge or with conflicts, and a switch
/// git refuses puts the stash back and the discard's unstaging back.
pub fn switch(root: &Path, target: &Target, carry: &Carry) -> Outcome {
    let to = target.local_name().to_string();
    if to.trim().is_empty() {
        return failed("type a branch name");
    }
    if let Some(what) = in_progress(root) {
        return failed(format!(
            "the checkout is in the middle of {what}: finish or abort it first"
        ));
    }
    let files = match changes(root) {
        Ok(files) => files,
        Err(e) => return failed(e),
    };
    if let Some(file) = files.iter().find(|f| f.unmerged()) {
        return failed(format!(
            "{} has unresolved conflicts: resolve them first",
            file.path
        ));
    }
    let head = head_branch(root);
    let from = head.clone().unwrap_or_else(|| "HEAD".into());

    let mut note: Option<String> = None;
    let mut stash: Option<String> = None;
    let mut saved_index: Option<SavedIndex> = None;
    let mut args = vec!["switch", "--quiet"];
    match carry {
        Carry::Ask => {
            if !files.is_empty() {
                let keys = fingerprints(root, &files);
                return Outcome::Dirty { files, keys };
            }
        }
        Carry::Bring => {}
        Carry::Create => {
            // `origin/x` as a local name would shadow the remote branch.
            let remotes = run(root, &["remote"]).unwrap_or_default();
            if let Some(remote) = remotes
                .lines()
                .map(str::trim)
                .find(|r| !r.is_empty() && target.name.starts_with(&format!("{r}/")))
            {
                return failed(format!(
                    "a branch named {} would be mistaken for {remote}'s: pick another name",
                    target.name
                ));
            }
        }
        Carry::Stash => {
            let message = format!("kete: {from} before switching to {to}");
            match stash_push(root, &message) {
                Ok(entry) => {
                    if entry.is_some() {
                        note = Some(format!("changes stashed as \"{message}\""));
                    }
                    stash = entry;
                }
                Err(e) => return failed(format!("stash failed: {e}")),
            }
        }
        Carry::Discard { keys: expected } => {
            let keys = fingerprints(root, &files);
            if &keys != expected {
                return Outcome::Dirty { files, keys };
            }
            saved_index = save_index(root);
            if let Err(e) = run(root, &["reset", "--quiet"]) {
                return failed(format!("unstaging failed: {e}"));
            }
            args.push("--discard-changes");
        }
    }
    if target.remote {
        args.push("--track");
    }
    if *carry == Carry::Create {
        args.push("--create");
    }
    args.push(&target.name);

    let before = fingerprint_now(root);
    let error = match run(root, &args) {
        Ok(_) => return Outcome::Switched { branch: to, note },
        Err(e) => e,
    };
    // A failing post-checkout hook exits non-zero after HEAD has moved:
    // that switch happened, and the changes must not be put back.
    if head_branch(root).as_deref() == Some(to.as_str()) && head.as_deref() != Some(to.as_str()) {
        let warning = format!("git complained after switching: {error}");
        return Outcome::Switched {
            branch: to,
            note: Some(match note {
                Some(n) => format!("{n} \u{b7} {warning}"),
                None => warning,
            }),
        };
    }
    let mut error = if error.contains("would be overwritten") {
        format!("files in the checkout collide with {to}: stash them instead")
    } else {
        error
    };
    let changed = fingerprint_now(root) != before;
    if changed {
        error = format!("{error}: git stopped part-way and left files from {to} in the checkout; check git status");
    }
    if let Some(saved) = &saved_index {
        restore_index(saved);
    }
    if let Some(entry) = &stash {
        if !stash_restore(root, entry) {
            return Outcome::Stopped {
                message: format!("{error} (your changes are still in the stash)"),
            };
        }
    }
    if changed {
        return Outcome::Stopped { message: error };
    }
    failed(error)
}

/// `git fetch --all`, stopped past [`FETCH_TIMEOUT`]. True when it worked.
pub fn fetch(root: &Path) -> bool {
    let mut cmd = git_command(root, &["fetch", "--all", "--quiet"]);
    cmd.stdout(std::process::Stdio::null());
    output_within(cmd, FETCH_TIMEOUT, "git fetch").is_ok_and(|o| o.status.success())
}

// ---- commands ----

/// Root checkouts with a switch running: one at a time, since two gits on
/// one index trip over `index.lock`.
#[derive(Default)]
pub struct Switching(Mutex<HashSet<String>>);

/// The root checkout of `worktree`'s repo, or why it can't switch.
fn root_of(client: &Client, worktree: &str) -> Res<PathBuf> {
    let model = client.snapshot();
    let w = model
        .worktrees
        .iter()
        .find(|w| w.id.0 == worktree)
        .ok_or_else(|| "that worktree is gone".to_string())?;
    if !w.is_main {
        return Err("Only the repo's root checkout switches branches.".into());
    }
    worktree_path(client, worktree)
}

/// What the switcher lists: the branches, and how many uncommitted
/// changes the checkout has (None when git could not say).
#[derive(Serialize)]
pub struct Listing {
    branches: Vec<Branch>,
    changes: Option<usize>,
}

#[tauri::command]
pub async fn branch_list(client: State<'_, Arc<Client>>, worktree: String) -> Res<Listing> {
    let root = root_of(&client, &worktree)?;
    tauri::async_runtime::spawn_blocking(move || {
        Ok(Listing {
            branches: list_branches(&root)?,
            changes: changes(&root).ok().map(|c| c.len()),
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// A background `git fetch --all` for the switcher; the window asks at
/// most once a minute per checkout and lists again when it lands.
#[tauri::command]
pub async fn branch_fetch(client: State<'_, Arc<Client>>, worktree: String) -> Res<bool> {
    let root = root_of(&client, &worktree)?;
    tauri::async_runtime::spawn_blocking(move || fetch(&root))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn branch_switch(
    client: State<'_, Arc<Client>>,
    switching: State<'_, Switching>,
    worktree: String,
    target: Target,
    carry: Carry,
) -> Res<Outcome> {
    let root = root_of(&client, &worktree)?;
    if !switching
        .0
        .lock()
        .expect("switching lock")
        .insert(worktree.clone())
    {
        return Err("A switch is already running in this checkout.".into());
    }
    let out = tauri::async_runtime::spawn_blocking(move || switch(&root, &target, &carry)).await;
    switching
        .0
        .lock()
        .expect("switching lock")
        .remove(&worktree);
    out.map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::tests::Repo;

    fn target(name: &str) -> Target {
        Target {
            name: name.into(),
            remote: false,
        }
    }

    fn rec(refname: &str, symref: &str, head: &str, wt: &str, date: i64, subject: &str) -> String {
        format!("{refname}\0{symref}\0{head}\0{wt}\0{date}\0{subject}\x1e\n")
    }

    #[test]
    fn refs_put_current_first_and_drop_shadowed_remotes() {
        let out = [
            rec("refs/heads/topic", "", "", "C:/wt/topic", 30, "topic work"),
            rec("refs/heads/main", "", "*", "C:/repo", 20, "base"),
            rec(
                "refs/remotes/origin/HEAD",
                "refs/remotes/origin/main",
                "",
                "",
                20,
                "base",
            ),
            rec("refs/remotes/origin/main", "", "", "", 20, "base"),
            rec(
                "refs/remotes/origin/fresh",
                "",
                "",
                "",
                40,
                "from a colleague",
            ),
        ]
        .concat();
        let rows = parse_refs(&out);
        let names: Vec<&str> = rows.iter().map(|b| b.name.as_str()).collect();
        assert_eq!(names, ["main", "topic", "origin/fresh"]);
        assert!(rows[0].current && rows[0].checked_out_at.is_none());
        assert_eq!(rows[1].checked_out_at.as_deref(), Some("C:/wt/topic"));
        assert!(rows[2].remote);
        assert_eq!(rows[2].subject, "from a colleague");
    }

    #[test]
    fn status_parse_skips_a_renames_old_path() {
        let raw = b"R  new.rs\0old.rs\0 M a.rs\0?? dir/\0UU c.rs\0";
        let files = parse_status(raw);
        let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, ["new.rs", "a.rs", "dir/", "c.rs"]);
        assert!(files[2].untracked());
        assert!(files[3].unmerged());
    }

    /// A repo on `main` with a `topic` branch that changes `a.txt`.
    fn two_branches() -> Repo {
        let repo = Repo::new("switch");
        repo.write("a.txt", b"one\n");
        repo.commit("one");
        repo.git(&["branch", "topic"]);
        repo.git(&["checkout", "-q", "topic"]);
        repo.write("a.txt", b"topic\n");
        repo.commit("topic");
        repo.git(&["checkout", "-q", "main"]);
        repo
    }

    #[test]
    fn a_clean_switch_moves_head() {
        let repo = two_branches();
        let out = switch(&repo.0, &target("topic"), &Carry::Ask);
        assert_eq!(
            out,
            Outcome::Switched {
                branch: "topic".into(),
                note: None
            }
        );
        assert_eq!(repo.git(&["symbolic-ref", "--short", "HEAD"]), "topic");
    }

    #[test]
    fn changes_ask_first_then_stash_carries_them_off() {
        let repo = two_branches();
        repo.write("b.txt", b"mine\n");
        let Outcome::Dirty { files, .. } = switch(&repo.0, &target("topic"), &Carry::Ask) else {
            panic!("expected a question");
        };
        assert_eq!(files[0].path, "b.txt");
        let out = switch(&repo.0, &target("topic"), &Carry::Stash);
        assert!(
            matches!(out, Outcome::Switched { ref note, .. } if note.as_deref().is_some_and(|n| n.contains("stashed")))
        );
        assert!(!repo.0.join("b.txt").exists());
        assert!(repo
            .git(&["stash", "list"])
            .contains("kete: main before switching to topic"));
    }

    #[test]
    fn a_colliding_bring_is_refused_and_leaves_the_edit() {
        let repo = two_branches();
        repo.write("a.txt", b"edited\n");
        let out = switch(&repo.0, &target("topic"), &Carry::Bring);
        assert!(
            matches!(out, Outcome::Failed { ref message } if message.contains("collide")),
            "{out:?}"
        );
        assert_eq!(
            std::fs::read_to_string(repo.0.join("a.txt")).unwrap(),
            "edited\n"
        );
        assert_eq!(repo.git(&["symbolic-ref", "--short", "HEAD"]), "main");
    }

    #[test]
    fn discard_needs_the_changes_it_was_shown() {
        let repo = two_branches();
        repo.write("a.txt", b"edited\n");
        let Outcome::Dirty { keys, .. } = switch(&repo.0, &target("topic"), &Carry::Ask) else {
            panic!("expected a question");
        };
        // Another edit lands after the prompt: nothing is thrown away.
        repo.write("c.txt", b"agent\n");
        let again = switch(&repo.0, &target("topic"), &Carry::Discard { keys });
        let Outcome::Dirty { keys, .. } = again else {
            panic!("expected the question again, got {again:?}");
        };
        let out = switch(&repo.0, &target("topic"), &Carry::Discard { keys });
        assert!(matches!(out, Outcome::Switched { .. }), "{out:?}");
        assert_eq!(
            std::fs::read_to_string(repo.0.join("a.txt")).unwrap(),
            "topic\n"
        );
        // Untracked files stay.
        assert!(repo.0.join("c.txt").exists());
    }

    #[test]
    fn create_makes_the_branch_and_refuses_a_remote_lookalike() {
        let repo = two_branches();
        repo.git(&["remote", "add", "origin", "https://example.invalid/x.git"]);
        let out = switch(&repo.0, &target("origin/new"), &Carry::Create);
        assert!(matches!(out, Outcome::Failed { ref message } if message.contains("mistaken")));
        let out = switch(&repo.0, &target("fresh"), &Carry::Create);
        assert!(matches!(out, Outcome::Switched { ref branch, .. } if branch == "fresh"));
        assert_eq!(repo.git(&["symbolic-ref", "--short", "HEAD"]), "fresh");
    }

    #[test]
    fn a_remote_branch_switches_to_a_tracking_local() {
        let origin = two_branches();
        let clone = Repo::new("clone-switch");
        clone.git(&["remote", "add", "origin", &crate::git::tests::url(&origin)]);
        clone.git(&["fetch", "-q", "origin"]);
        clone.git(&["checkout", "-q", "-b", "main", "origin/main"]);
        let rows = list_branches(&clone.0).unwrap();
        let topic = rows
            .iter()
            .find(|b| b.name == "origin/topic")
            .expect("listed");
        assert!(topic.remote);
        let out = switch(
            &clone.0,
            &Target {
                name: "origin/topic".into(),
                remote: true,
            },
            &Carry::Ask,
        );
        assert!(
            matches!(out, Outcome::Switched { ref branch, .. } if branch == "topic"),
            "{out:?}"
        );
        assert_eq!(
            clone.git(&["rev-parse", "--abbrev-ref", "topic@{upstream}"]),
            "origin/topic"
        );
    }
}
