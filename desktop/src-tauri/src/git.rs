//! A worktree's changes, for the diff view, and its pull request, for the
//! kanban's Ship lane. Both shell out (`git`, `gh`) with no console
//! window, as the daemon's own helpers do.
//!
//! Diffs are pinned to commits: a range diff runs from the merge-base of
//! its two ends, and every blob is read at a named side. A side that cannot
//! be read is an error or an honest [`Blob`] state, never empty text.

use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// Files above this are shown as too large rather than diffed.
const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct DiffFile {
    pub path: String,
    /// Where a rename came from.
    pub from: Option<String>,
    /// `M`odified, `A`dded, `D`eleted, `R`enamed, `U`ntracked, `C`onflicted.
    pub status: char,
    pub added: Option<u64>,
    pub removed: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiffPair {
    pub original: String,
    pub modified: String,
    /// Why there is no text to compare (binary, too large), if so.
    pub note: Option<String>,
}

fn command(program: &str, cwd: &Path) -> Command {
    let mut cmd = Command::new(program);
    cmd.current_dir(cwd)
        .stdin(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    nebula_core::platform::no_window(&mut cmd);
    cmd
}

/// Runs git and hands back the whole output, success or not. Only failing
/// to start git at all is an error here.
pub(crate) fn git_raw(cwd: &Path, args: &[&str]) -> Result<Output, String> {
    git_command(cwd, args)
        .output()
        .map_err(|e| format!("could not run git: {e}"))
}

pub(crate) fn git_command(cwd: &Path, args: &[&str]) -> Command {
    let mut cmd = command("git", cwd);
    cmd.args(["-c", "core.quotepath=off"])
        .args(args)
        // No window to answer a credential prompt in: fail, don't hang.
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "never")
        // Paths are file names, never globs or pathspec magic.
        .env("GIT_LITERAL_PATHSPECS", "1")
        .stderr(Stdio::piped());
    cmd
}

pub(crate) fn git(cwd: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let out = git_raw(cwd, args)?;
    if !out.status.success() {
        return Err(git_failure(args, &out));
    }
    Ok(out.stdout)
}

/// How long a fetch may take before it is killed.
const FETCH_TIMEOUT: Duration = Duration::from_secs(60);

/// `git fetch <args>`, killed after [`FETCH_TIMEOUT`].
fn git_fetch(cwd: &Path, args: &[&str]) -> Result<(), String> {
    let mut full = vec!["fetch", "--quiet", "--no-tags"];
    full.extend_from_slice(args);
    let mut cmd = git_command(cwd, &full);
    cmd.stdout(Stdio::null());
    let out = output_within(cmd, FETCH_TIMEOUT, "git fetch")?;
    if !out.status.success() {
        return Err(git_failure(&full, &out));
    }
    Ok(())
}

pub(crate) use kete_ado::git::output_within;

/// `git <verb> failed: <first stderr line>`.
fn git_failure(args: &[&str], out: &Output) -> String {
    let verb = args.first().unwrap_or(&"");
    let stderr = String::from_utf8_lossy(&out.stderr);
    match stderr.lines().map(str::trim).find(|l| !l.is_empty()) {
        Some(line) => format!("git {verb} failed: {line}"),
        None => format!("git {verb} failed"),
    }
}

/// Whether git exits cleanly, for the `--verify` and `-e` style probes.
fn git_ok(cwd: &Path, args: &[&str]) -> Result<bool, String> {
    Ok(git_raw(cwd, args)?.status.success())
}

fn git_text(cwd: &Path, args: &[&str]) -> Result<String, String> {
    let out = git(cwd, args)?;
    Ok(String::from_utf8_lossy(&out).trim().to_string())
}

/// One entry of `git status --porcelain=v1 -z`: git's two status letters
/// (`??` untracked), the path from the repo's root, and where a rename or
/// copy came from.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct StatusEntry {
    pub x: char,
    pub y: char,
    pub path: String,
    pub from: Option<String>,
}

/// `git status` in the checkout at `root`, every untracked file listed
/// (not only its folder), limited to `pathspec` (literal, from `root`)
/// when given. Paths are from the repo's root, as porcelain v1 always
/// gives them.
pub(crate) fn status_entries(
    root: &Path,
    pathspec: Option<&str>,
) -> Result<Vec<StatusEntry>, String> {
    let mut args = vec!["status", "--porcelain=v1", "-z", "--untracked-files=all"];
    if let Some(p) = pathspec {
        args.extend(["--", p]);
    }
    Ok(parse_status(&git(root, &args)?))
}

fn parse_status(raw: &[u8]) -> Vec<StatusEntry> {
    let mut out = Vec::new();
    let mut parts = raw.split(|b| *b == 0).filter(|p| !p.is_empty());
    while let Some(entry) = parts.next() {
        if entry.len() < 4 {
            continue;
        }
        let (x, y) = (entry[0] as char, entry[1] as char);
        let path = String::from_utf8_lossy(&entry[3..]).into_owned();
        let renamed = x == 'R' || y == 'R' || x == 'C';
        let from = if renamed {
            parts
                .next()
                .map(|p| String::from_utf8_lossy(p).into_owned())
        } else {
            None
        };
        out.push(StatusEntry { x, y, path, from });
    }
    out
}

/// Every changed path in the checkout: staged, unstaged and untracked, each
/// once, with line counts against HEAD where git has them.
pub fn changed_files(root: &Path) -> Result<Vec<DiffFile>, String> {
    let mut files: Vec<DiffFile> = status_entries(root, None)?
        .into_iter()
        .map(|e| {
            let status = match (e.x, e.y) {
                ('?', '?') => 'U',
                ('U', _) | (_, 'U') | ('A', 'A') | ('D', 'D') => 'C',
                _ if e.from.is_some() => 'R',
                ('A', _) => 'A',
                ('D', _) | (_, 'D') => 'D',
                _ => 'M',
            };
            DiffFile {
                path: e.path,
                from: e.from,
                status,
                added: None,
                removed: None,
            }
        })
        .collect();
    // Line counts for tracked changes; a repo with no commit yet has none.
    if let Ok(numstat) = git(root, &["diff", "HEAD", "--numstat", "-z", "-M"]) {
        apply_numstat(&mut files, &numstat);
    }
    count_untracked(root, &mut files);
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

/// Every path the checkout differs in from commit `base` (the merge-base
/// `nebula_core::review_base::resolve` found): committed since, staged,
/// unstaged and untracked, each once, with line counts against `base`.
pub fn files_since(root: &Path, base: &str) -> Result<Vec<DiffFile>, String> {
    let mut files: Vec<DiffFile> = nebula_core::review_base::files_since(root, base)?
        .into_iter()
        .map(|c| DiffFile {
            path: c.path,
            from: c.from,
            status: c.status,
            added: None,
            removed: None,
        })
        .collect();
    if let Ok(numstat) = git(
        root,
        &["diff", base, "--numstat", "-z", "-M", "--no-ext-diff", "--"],
    ) {
        apply_numstat(&mut files, &numstat);
    }
    count_untracked(root, &mut files);
    Ok(files)
}

/// An untracked file's lines all count as added.
fn count_untracked(root: &Path, files: &mut [DiffFile]) {
    for f in files.iter_mut().filter(|f| f.status == 'U') {
        if let Ok(text) = std::fs::read(root.join(&f.path)) {
            if !is_binary(&text) {
                f.added = Some(text.split(|b| *b == b'\n').count() as u64);
                f.removed = Some(0);
            }
        }
    }
}

/// `git diff --numstat -z`: `add\tdel\tpath\0`, or for a rename
/// `add\tdel\t\0from\0to\0`. Binary files count `-`.
fn apply_numstat(files: &mut [DiffFile], raw: &[u8]) {
    let mut parts = raw.split(|b| *b == 0);
    while let Some(head) = parts.next() {
        if head.is_empty() {
            continue;
        }
        let text = String::from_utf8_lossy(head);
        let mut cols = text.splitn(3, '\t');
        let (Some(a), Some(d), Some(path)) = (cols.next(), cols.next(), cols.next()) else {
            continue;
        };
        let path = if path.is_empty() {
            let _from = parts.next();
            parts
                .next()
                .map(|p| String::from_utf8_lossy(p).into_owned())
                .unwrap_or_default()
        } else {
            path.to_string()
        };
        if let Some(f) = files.iter_mut().find(|f| f.path == path) {
            f.added = a.parse().ok();
            f.removed = d.parse().ok();
        }
    }
}

fn is_binary(bytes: &[u8]) -> bool {
    bytes.iter().take(8000).any(|b| *b == 0)
}

/// HEAD's side and the disk's side of one changed file. A side that should
/// exist and cannot be read is an error, not an empty editor.
pub fn file_pair(root: &Path, file: &DiffFile) -> Result<DiffPair, String> {
    let head_path = file.from.as_deref().unwrap_or(&file.path);
    let original = if matches!(file.status, 'A' | 'U') {
        Blob::Text(String::new())
    } else {
        match blob(root, &DiffSide::Commit("HEAD".into()), head_path)? {
            // Both sides added it: a conflict with no HEAD copy.
            Blob::Missing if file.status == 'C' => Blob::Text(String::new()),
            Blob::Missing => return Err(format!("{head_path} is missing at HEAD")),
            b => b,
        }
    };
    let modified = if file.status == 'D' {
        Blob::Text(String::new())
    } else {
        match blob(root, &DiffSide::Worktree, &file.path)? {
            // Staged as added or renamed then deleted on disk (`AD`, `RD`),
            // or a delete conflict.
            Blob::Missing if matches!(file.status, 'A' | 'R' | 'C') => Blob::Text(String::new()),
            Blob::Missing => return Err(format!("{} is missing on disk", file.path)),
            b => b,
        }
    };
    let note = |note: String| DiffPair {
        original: String::new(),
        modified: String::new(),
        note: Some(note),
    };
    match (original, modified) {
        (Blob::TooLarge(n), _) | (_, Blob::TooLarge(n)) => {
            Ok(note(format!("{} KB, too large to diff here", n / 1024)))
        }
        (Blob::Binary, _) | (_, Blob::Binary) => Ok(note("Binary file".into())),
        (Blob::Text(original), Blob::Text(modified)) => Ok(DiffPair {
            original,
            modified,
            note: None,
        }),
        // Missing was turned into an error above.
        _ => Err("unreadable diff side".into()),
    }
}

/// Where a blob is read from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "rev", rename_all = "lowercase")]
pub enum DiffSide {
    /// The file on disk.
    Worktree,
    /// The staged copy (stage 0).
    Index,
    /// A commit, by SHA or any rev git resolves to one.
    Commit(String),
}

/// One side of a diff, honestly: text, or why there is none.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", content = "value", rename_all = "camelCase")]
pub enum Blob {
    Text(String),
    /// The path does not exist on that side.
    Missing,
    /// Bytes on that side, over the cap.
    TooLarge(u64),
    /// A NUL byte in the first 8000 bytes, or not UTF-8.
    Binary,
}

impl Blob {
    fn from_bytes(bytes: Vec<u8>) -> Blob {
        if is_binary(&bytes) {
            return Blob::Binary;
        }
        match String::from_utf8(bytes) {
            Ok(text) => Blob::Text(text),
            Err(_) => Blob::Binary,
        }
    }
}

/// A repo-relative path, with forward slashes, that stays inside the repo.
fn repo_path(path: &str) -> Result<String, String> {
    let clean = path.replace('\\', "/");
    let p = Path::new(&clean);
    let escapes = p.components().any(|c| {
        matches!(
            c,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    });
    if clean.is_empty() || escapes {
        return Err(format!("{path} is not a path inside the repo"));
    }
    Ok(clean.trim_start_matches("./").to_string())
}

/// The content of `path` at `side`, capped at 2 MB.
pub fn blob(root: &Path, side: &DiffSide, path: &str) -> Result<Blob, String> {
    let path = repo_path(path)?;
    // Only "no such entry" is Missing; any git failure surfaces as an error.
    let (kind, object) = match side {
        DiffSide::Worktree => return worktree_blob(&root.join(&path)),
        DiffSide::Index => {
            // `<mode> <object> <stage>\t<path>\0`, stage 0 only.
            let raw = git(root, &["ls-files", "--stage", "-z", "--", &path])?;
            let entry = raw
                .split(|b| *b == 0)
                .filter_map(|e| parse_entry(e, &path))
                .find(|meta| meta.len() >= 3 && meta[2] == "0");
            match entry {
                Some(meta) => ("blob".to_string(), meta[1].clone()),
                None => return Ok(Blob::Missing),
            }
        }
        DiffSide::Commit(rev) => {
            // A commit's tree, or a tree itself: a session's snapshots are
            // trees (`nebula_core::session_snapshot`).
            let tree = resolve_tree(root, rev)?;
            // `<mode> <type> <object>\t<path>\0`.
            let raw = git(root, &["ls-tree", "-z", &tree, "--", &path])?;
            match raw.split(|b| *b == 0).find_map(|e| parse_entry(e, &path)) {
                Some(meta) if meta.len() >= 3 => (meta[1].clone(), meta[2].clone()),
                _ => return Ok(Blob::Missing),
            }
        }
    };
    if kind != "blob" {
        return Err(format!("{path} is a {kind} there, not a file"));
    }
    let size: u64 = git_text(root, &["cat-file", "-s", &object])?
        .parse()
        .map_err(|_| "git cat-file gave no size".to_string())?;
    if size > MAX_FILE_BYTES {
        return Ok(Blob::TooLarge(size));
    }
    Ok(Blob::from_bytes(git(root, &["cat-file", "blob", &object])?))
}

/// One `<fields>\t<path>` entry of `ls-files -s -z` or `ls-tree -z`, if it
/// is exactly `path`.
fn parse_entry(entry: &[u8], path: &str) -> Option<Vec<String>> {
    let text = std::str::from_utf8(entry).ok()?;
    let (meta, name) = text.split_once('\t')?;
    (name == path).then(|| meta.split_whitespace().map(str::to_string).collect())
}

fn worktree_blob(file: &Path) -> Result<Blob, String> {
    let meta = match std::fs::metadata(file) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Blob::Missing),
        Err(e) => return Err(format!("could not read {}: {e}", file.display())),
    };
    if meta.is_dir() {
        return Err(format!("{} is a folder, not a file", file.display()));
    }
    if meta.len() > MAX_FILE_BYTES {
        return Ok(Blob::TooLarge(meta.len()));
    }
    let bytes =
        std::fs::read(file).map_err(|e| format!("could not read {}: {e}", file.display()))?;
    Ok(Blob::from_bytes(bytes))
}

/// The commit `rev` names, as a full SHA.
/// `rev` as a tree: a commit's, or a tree object named directly.
pub fn resolve_tree(root: &Path, rev: &str) -> Result<String, String> {
    let tree = format!("{rev}^{{tree}}");
    git_text(root, &["rev-parse", "--verify", "--quiet", &tree])
        .map_err(|_| format!("{rev} is not in this clone; fetch it first"))
}

pub fn resolve_commit(root: &Path, rev: &str) -> Result<String, String> {
    let commit = format!("{rev}^{{commit}}");
    git_text(root, &["rev-parse", "--verify", "--quiet", &commit])
        .map_err(|_| format!("{rev} is not in this clone; fetch it first"))
}

/// Where `base` and `tip` forked, as a SHA. The range diff runs from here,
/// so changes landed on `base` since the fork do not show as reverts.
#[allow(dead_code)] // Named in the plan; kept for callers without SHAs.
pub fn merge_base(root: &Path, base: &str, tip: &str) -> Result<String, String> {
    let base = resolve_commit(root, base)?;
    let tip = resolve_commit(root, tip)?;
    merge_base_of(root, &base, &tip)
}

fn merge_base_of(root: &Path, base_sha: &str, tip_sha: &str) -> Result<String, String> {
    git_text(root, &["merge-base", base_sha, tip_sha])
        .map_err(|_| "these commits share no history in this clone; fetch more of it".to_string())
}

/// The files `tip` changes since it forked from `base`.
#[allow(dead_code)] // Named in the plan; `diff_range` uses `diff_files_between`.
pub fn diff_files_range(root: &Path, base: &str, tip: &str) -> Result<Vec<DiffFile>, String> {
    let to = resolve_commit(root, tip)?;
    let from = merge_base_of(root, &resolve_commit(root, base)?, &to)?;
    diff_files_between(root, &from, &to)
}

/// The files changed from commit `from` to commit `to`, both SHAs.
pub fn diff_files_between(root: &Path, from: &str, to: &str) -> Result<Vec<DiffFile>, String> {
    let names = git(
        root,
        &[
            "diff",
            "--name-status",
            "-z",
            "-M",
            "--no-ext-diff",
            from,
            to,
        ],
    )?;
    let mut files = parse_name_status(&names);
    let numstat = git(
        root,
        &["diff", "--numstat", "-z", "-M", "--no-ext-diff", from, to],
    )?;
    apply_numstat(&mut files, &numstat);
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

/// `git diff --name-status -z`: `X\0path\0`, or `R100\0from\0to\0`.
fn parse_name_status(raw: &[u8]) -> Vec<DiffFile> {
    let mut files = Vec::new();
    let mut parts = raw.split(|b| *b == 0).filter(|p| !p.is_empty());
    let text = |p: &[u8]| String::from_utf8_lossy(p).into_owned();
    while let Some(code) = parts.next() {
        let Some(&letter) = code.first() else {
            continue;
        };
        let Some(first) = parts.next() else {
            break;
        };
        let (from, path) = if matches!(letter, b'R' | b'C') {
            let Some(to) = parts.next() else {
                break;
            };
            (Some(text(first)), text(to))
        } else {
            (None, text(first))
        };
        let status = match letter {
            b'A' => 'A',
            b'D' => 'D',
            b'R' | b'C' => 'R',
            b'U' => 'C',
            _ => 'M',
        };
        // A copy keeps its source, so it reads as an add.
        let status = if letter == b'C' { 'A' } else { status };
        files.push(DiffFile {
            path,
            from,
            status,
            added: None,
            removed: None,
        });
    }
    files
}

/// One `@@` hunk of a file's diff, with the file header it needs to be
/// applied on its own.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hunk {
    /// The `@@ -a,b +c,d @@` line.
    pub header: String,
    /// `diff --git`, `---`, `+++` and this hunk: a patch `git apply` takes.
    pub patch: String,
    pub old_start: u32,
    pub new_start: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HunkAction {
    /// Worktree hunk into the index.
    Stage,
    /// Staged hunk back out of the index.
    Unstage,
    /// Worktree hunk reverted on disk.
    Discard,
}

/// The hunks of `path`: staged (index against HEAD) or not (worktree
/// against index). A binary or unchanged file has none.
pub fn hunks(root: &Path, path: &str, staged: bool) -> Result<Vec<Hunk>, String> {
    let path = repo_path(path)?;
    let mut args = vec!["diff", "-U3", "--no-color", "--no-ext-diff"];
    if staged {
        args.push("--cached");
    }
    args.extend(["--", &path]);
    let raw = git(root, &args)?;
    let text = String::from_utf8(raw)
        .map_err(|_| format!("{path} is not UTF-8 text; stage it whole instead"))?;
    Ok(parse_hunks(&text))
}

/// Splits one file's `git diff` output into self-contained hunk patches.
/// Lines keep their own endings, so CRLF content survives the round trip.
fn parse_hunks(diff: &str) -> Vec<Hunk> {
    let mut header = String::new();
    let mut hunks: Vec<Hunk> = Vec::new();
    for line in diff.split_inclusive('\n') {
        if line.starts_with("@@") {
            let (old_start, new_start) = hunk_starts(line);
            hunks.push(Hunk {
                header: line.trim_end().to_string(),
                patch: format!("{header}{line}"),
                old_start,
                new_start,
            });
        } else if let Some(h) = hunks.last_mut() {
            if line.starts_with("diff --git") {
                // A second file: not ours, as the diff is limited to one path.
                break;
            }
            h.patch.push_str(line);
        } else {
            header.push_str(line);
        }
    }
    hunks
}

/// `@@ -12,3 +14,5 @@ fn x` gives (12, 14).
fn hunk_starts(line: &str) -> (u32, u32) {
    let mut old = 0;
    let mut new = 0;
    for word in line.split_whitespace().skip(1) {
        let start = |w: &str| w[1..].split(',').next().and_then(|n| n.parse().ok());
        if word.starts_with('-') {
            old = start(word).unwrap_or(0);
        } else if word.starts_with('+') {
            new = start(word).unwrap_or(0);
            break;
        }
    }
    (old, new)
}

/// Stages, unstages or discards one hunk, through `git apply` on a temp
/// patch file. Stage and discard take unstaged hunks; unstage takes staged.
pub fn apply_hunk(root: &Path, hunk: &Hunk, action: HunkAction) -> Result<(), String> {
    if !hunk.patch.starts_with("diff --git") || !hunk.patch.contains("\n@@") {
        return Err("that hunk has no file header to apply against".into());
    }
    let patch = TempPatch::write(&hunk.patch)?;
    let file = patch.0.to_string_lossy().into_owned();
    let mut args = vec!["apply", "--whitespace=nowarn"];
    match action {
        HunkAction::Stage => args.push("--cached"),
        HunkAction::Unstage => args.extend(["--cached", "-R"]),
        HunkAction::Discard => args.push("-R"),
    }
    args.push(&file);
    git(root, &args).map(|_| ())
}

/// A patch file in the temp dir, removed when dropped.
struct TempPatch(PathBuf);

impl TempPatch {
    fn write(text: &str) -> Result<TempPatch, String> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let name = format!(
            "kete-hunk-{}-{}.patch",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let path = std::env::temp_dir().join(name);
        std::fs::write(&path, text.as_bytes())
            .map_err(|e| format!("could not write the hunk patch: {e}"))?;
        Ok(TempPatch(path))
    }
}

impl Drop for TempPatch {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Brings `refs` up to date from `remote`. Present is not current: every
/// named ref is fetched again with a force refspec, so a moved
/// `refs/pull/{id}/merge` or target branch is picked up.
/// - `refs/...` (such as `refs/pull/12/merge`) lands under the same name.
/// - A full SHA (40 or 64 hex) is fetched by id, only when missing.
/// - A branch (`main`, `origin/main`) lands in `refs/remotes/<remote>/`.
/// - A trailing `^N` / `~N` (`refs/pull/12/merge^2`) is kept for reading
///   and dropped for the fetch.
///
/// A shallow clone where the first two refs share no history is deepened by
/// 200 commits. A failed fetch falls back to a cached copy when there is
/// one; [`prepare_range_at`] reports that as `offline`.
// Named in the plan; the desktop commands use `prepare_range_at` instead.
#[allow(dead_code)]
pub fn ensure_refs(root: &Path, remote: &str, refs: &[&str]) -> Result<(), String> {
    let mut specs = Vec::new();
    for r in refs {
        // A failed fetch with a cached copy is tolerated here.
        refresh_ref(root, remote, r, &mut specs)?;
    }
    if let [a, b, ..] = refs {
        let a = resolve_commit(root, &commit_name(root, remote, a, true)?)?;
        let b = resolve_commit(root, &commit_name(root, remote, b, true)?)?;
        deepen_if_needed(root, remote, &a, &b, &specs)?;
    }
    Ok(())
}

/// A range diff's two ends, pinned to SHAs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RangeRefs {
    /// The merge-base of the target and the tip.
    pub base: String,
    pub tip: String,
    /// A fetch failed and a cached ref was used instead.
    pub offline: bool,
}

/// [`prepare_range_at`] on `origin`, with nothing fetched first.
#[cfg(test)]
pub fn prepare_range(root: &Path, base: &str, tip: &str) -> Result<RangeRefs, String> {
    prepare_range_at(root, "origin", base, tip, &[])
}

/// Refreshes `base` (the target) and `tip` from `remote`, then pins both:
/// `base` becomes the merge-base. The target prefers `<remote>/<branch>`
/// over a local branch of the same name, which may be stale; a tip that is
/// a local branch is read as it is, unpushed work included, and not
/// fetched. A failed fetch falls back to the cached ref and sets
/// `offline`.
///
/// `prefetch` refs (a PR's `refs/pull/{id}/merge`) are fetched first, and
/// their failure is no error: they only bring in commits that `base` and
/// `tip`, given as SHAs, may name.
pub fn prepare_range_at(
    root: &Path,
    remote: &str,
    base: &str,
    tip: &str,
    prefetch: &[&str],
) -> Result<RangeRefs, String> {
    let mut specs = Vec::new();
    for r in prefetch {
        let _ = refresh_ref(root, remote, r, &mut specs);
    }
    let tip_is_local = !tip.starts_with("refs/") && has_commit(root, &format!("refs/heads/{tip}"))?;
    let mut offline = refresh_ref(root, remote, base, &mut specs)?;
    if !tip_is_local {
        offline |= refresh_ref(root, remote, tip, &mut specs)?;
    }
    let base_sha = resolve_commit(root, &commit_name(root, remote, base, true)?)?;
    let tip_sha = resolve_commit(root, &commit_name(root, remote, tip, false)?)?;
    if !offline {
        deepen_if_needed(root, remote, &base_sha, &tip_sha, &specs)?;
    }
    Ok(RangeRefs {
        base: merge_base_of(root, &base_sha, &tip_sha)?,
        tip: tip_sha,
        offline,
    })
}

/// `refs/pull/7/merge^2` gives (`refs/pull/7/merge`, `^2`): the name to
/// fetch, and the walk from it kept for reading. No walk gives an empty
/// suffix.
fn split_walk(r: &str) -> (&str, &str) {
    match r.find(['^', '~']) {
        Some(i)
            if i > 0
                && r[i..]
                    .bytes()
                    .all(|b| matches!(b, b'^' | b'~' | b'0'..=b'9')) =>
        {
            r.split_at(i)
        }
        _ => (r, ""),
    }
}

/// Fetches one ref from `remote`; pushes its refspec onto `specs` for a
/// later deepen. `Ok(true)` when the fetch failed but a cached copy
/// resolves.
fn refresh_ref(
    root: &Path,
    remote: &str,
    r: &str,
    specs: &mut Vec<String>,
) -> Result<bool, String> {
    let r = r.trim();
    if r.is_empty() {
        return Err("an empty ref was asked for".into());
    }
    let (name, _) = split_walk(r);
    if is_full_sha(name) {
        if !has_commit(root, r)? {
            git_fetch(root, &[remote, name])?;
            if !has_commit(root, r)? {
                return Err(format!("{r} is not on {remote}"));
            }
        }
        return Ok(false);
    }
    if is_short_sha(name) && has_commit(root, r)? {
        // An abbreviated SHA this clone already has; it cannot be fetched.
        return Ok(false);
    }
    let spec = if name.starts_with("refs/") {
        format!("+{name}:{name}")
    } else {
        let branch = name.strip_prefix(&format!("{remote}/")).unwrap_or(name);
        format!("+refs/heads/{branch}:refs/remotes/{remote}/{branch}")
    };
    let fetched = git_fetch(root, &[remote, &spec]);
    specs.push(spec);
    let cached = has_commit(root, &commit_name(root, remote, r, true)?)?;
    match fetched {
        Ok(()) if cached => Ok(false),
        Ok(()) => Err(format!("{r} is not on {remote}")),
        Err(_) if cached => Ok(true),
        Err(e) => Err(e),
    }
}

/// In a shallow clone, fetches 200 more commits of `specs` from `remote`
/// when the two SHAs share no history yet.
fn deepen_if_needed(
    root: &Path,
    remote: &str,
    a: &str,
    b: &str,
    specs: &[String],
) -> Result<(), String> {
    let shallow = git_text(root, &["rev-parse", "--is-shallow-repository"])? == "true";
    if !shallow || merge_base_of(root, a, b).is_ok() {
        return Ok(());
    }
    let mut args = vec!["--deepen=200", remote];
    args.extend(specs.iter().map(String::as_str));
    git_fetch(root, &args)
}

fn has_commit(root: &Path, rev: &str) -> Result<bool, String> {
    let commit = format!("{rev}^{{commit}}");
    git_ok(root, &["rev-parse", "--verify", "--quiet", &commit])
}

/// The name to read `rev` by after a refresh. SHAs and `refs/...` are
/// themselves. A branch is `<remote>/<branch>` when `prefer_remote` (a
/// target, where the local copy may be stale) and that exists, else the
/// local branch if it resolves, else `<remote>/<branch>`. A trailing walk
/// (`^2`, `~1`) stays on the end.
pub fn commit_name(
    root: &Path,
    remote: &str,
    rev: &str,
    prefer_remote: bool,
) -> Result<String, String> {
    let (name, walk) = split_walk(rev);
    if name.starts_with("refs/") || is_full_sha(name) || is_short_sha(name) {
        return Ok(rev.to_string());
    }
    let tracking = format!(
        "{remote}/{}{walk}",
        name.strip_prefix(&format!("{remote}/")).unwrap_or(name)
    );
    if prefer_remote && has_commit(root, &tracking)? {
        return Ok(tracking);
    }
    if has_commit(root, rev)? {
        return Ok(rev.to_string());
    }
    Ok(tracking)
}

fn is_hex(s: &str) -> bool {
    s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// A whole SHA-1 or SHA-256 object id: the only kind fetched by id.
fn is_full_sha(s: &str) -> bool {
    matches!(s.len(), 40 | 64) && is_hex(s)
}

fn is_short_sha(s: &str) -> bool {
    (7..40).contains(&s.len()) && is_hex(s)
}

/// How long one `gh` lookup may take before the poll gives up on it.
const GH_TIMEOUT: Duration = Duration::from_secs(30);
/// The fields [`pick_pr`] reads from `gh pr list`.
const PR_FIELDS: &str = "number,url,title,state,isDraft,headRefName,headRefOid,isCrossRepository";

/// `gh <args>` in `root`, stdout on success, under [`GH_TIMEOUT`]. None
/// when there is no `gh`, no network, no sign-in, or it took too long.
fn gh(root: &Path, args: &[&str]) -> Option<Vec<u8>> {
    let mut cmd = command("gh", root);
    cmd.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
    let out = output_within(cmd, GH_TIMEOUT, "gh").ok()?;
    out.status.success().then_some(out.stdout)
}

/// The pull request on `branch`, in whatever state it is in, asked of
/// `gh` in the checkout: a few of the branch's newest, of which
/// [`pick_pr`] takes one. None when there is none, or no `gh`, or no
/// network.
pub fn branch_pr(root: &Path, branch: &str) -> Option<crate::daemon::PrInfo> {
    let out = gh(
        root,
        &[
            "pr", "list", "--state", "all", "--head", branch, "--limit", "5", "--json", PR_FIELDS,
        ],
    )?;
    pick_pr(&out, branch)
}

/// The pull request a branch shows, from `gh pr list --json` output: one
/// whose head is `branch` in this repository (a fork's PR from its own
/// `main` is not the main checkout's), an open one before any other (a
/// branch reopened after an old merge), else the newest. `gh`'s state
/// words become `open`, `merged` or `closed`; one it adds later reads as
/// open, as the terminal app reads it.
pub fn pick_pr(json: &[u8], branch: &str) -> Option<crate::daemon::PrInfo> {
    let list: Vec<serde_json::Value> = serde_json::from_slice(json).ok()?;
    let state = |v: &serde_json::Value| match v.get("state").and_then(|s| s.as_str()) {
        Some("MERGED") => "merged",
        Some("CLOSED") => "closed",
        _ => "open",
    };
    let number = |v: &serde_json::Value| v.get("number").and_then(|n| n.as_u64()).unwrap_or(0);
    let v = list
        .iter()
        .filter(|v| v.get("headRefName").and_then(|h| h.as_str()) == Some(branch))
        .filter(|v| {
            !v.get("isCrossRepository")
                .and_then(|c| c.as_bool())
                .unwrap_or(false)
        })
        .min_by_key(|v| (state(v) != "open", std::cmp::Reverse(number(v))))?;
    Some(crate::daemon::PrInfo {
        number: v.get("number")?.as_u64()?,
        url: v.get("url")?.as_str()?.to_string(),
        title: v.get("title")?.as_str().unwrap_or_default().to_string(),
        draft: v.get("isDraft").and_then(|d| d.as_bool()).unwrap_or(false),
        state: state(v).to_string(),
        head_sha: v
            .get("headRefOid")
            .and_then(|s| s.as_str())
            .unwrap_or_default()
            .to_string(),
    })
}

/// Whether an open pull request targets `branch` (a PR stacked on it).
/// None when `gh` could not say, which a cleanup reads as "yes".
pub fn has_open_pr_onto(root: &Path, branch: &str) -> Option<bool> {
    let out = gh(
        root,
        &[
            "pr", "list", "--state", "open", "--base", branch, "--limit", "1", "--json", "number",
        ],
    )?;
    let list: Vec<serde_json::Value> = serde_json::from_slice(&out).ok()?;
    Some(!list.is_empty())
}

/// The files a commit, checkout, reset, stage, fetch or pack touches, read
/// off the checkout's own git dir and its repository's common dir.
const OWN_STAMP_FILES: [&str; 3] = ["HEAD", "index", "logs/HEAD"];
const COMMON_STAMP_FILES: [&str; 2] = ["packed-refs", "FETCH_HEAD"];

/// The git dir and common dir of the checkout at `root`, without running
/// git: `.git` is the git dir in a main checkout, and in a linked one a
/// file naming it (`gitdir: <path>`), whose `commondir` names the shared one.
pub fn git_dirs(root: &Path) -> Option<(PathBuf, PathBuf)> {
    let dot = root.join(".git");
    let meta = std::fs::metadata(&dot).ok()?;
    let own = if meta.is_dir() {
        dot
    } else {
        let text = std::fs::read_to_string(&dot).ok()?;
        let named = text.trim().strip_prefix("gitdir:")?.trim();
        let p = PathBuf::from(named);
        if p.is_absolute() {
            p
        } else {
            root.join(p)
        }
    };
    let common = match std::fs::read_to_string(own.join("commondir")) {
        Ok(text) => {
            let p = PathBuf::from(text.trim());
            if p.is_absolute() {
                p
            } else {
                own.join(p)
            }
        }
        Err(_) => own.clone(),
    };
    Some((own, common))
}

/// A cheap fingerprint of the checkout's git state: each stamp file's
/// size and modified time, with no git process. It changes when a commit,
/// checkout, reset, stage or fetch lands; an edit to a working-tree file
/// leaves it as it was. None when `root` is not a checkout.
pub fn git_stamp(root: &Path) -> Option<String> {
    let (own, common) = git_dirs(root)?;
    let part = |p: PathBuf| match std::fs::metadata(p) {
        Ok(m) => {
            let at = m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_nanos());
            format!("{}:{at}", m.len())
        }
        Err(_) => "-".to_string(),
    };
    let own_parts = OWN_STAMP_FILES.iter().map(|f| part(own.join(f)));
    let common_parts = COMMON_STAMP_FILES.iter().map(|f| part(common.join(f)));
    Some(own_parts.chain(common_parts).collect::<Vec<_>>().join("|"))
}

/// What deleting a checkout would throw away, for the delete confirm.
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct CheckoutState {
    /// Tracked files changed, staged or not.
    pub changed: u32,
    pub untracked: u32,
    /// Commits on HEAD that no remote-tracking branch holds; None when the
    /// repo has no remote, or HEAD has no commit yet.
    pub unpushed: Option<u32>,
}

/// The checkout's [`CheckoutState`]: one `git status` and, with a remote,
/// one `git rev-list` against every remote-tracking branch (so a branch
/// with no upstream counts the commits only it has).
pub fn checkout_state(root: &Path) -> Result<CheckoutState, String> {
    let raw = git(root, &["status", "--porcelain=v1", "-z"])?;
    let (changed, untracked) = count_status(&raw);
    let unpushed = if git_text(root, &["remote"])?.is_empty() {
        None
    } else {
        git_text(root, &["rev-list", "--count", "HEAD", "--not", "--remotes"])
            .ok()
            .and_then(|n| n.parse().ok())
    };
    Ok(CheckoutState {
        changed,
        untracked,
        unpushed,
    })
}

/// A checkout as the overview's worktree rows show it: its uncommitted
/// work, where it stands against its upstream, and when HEAD last moved.
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct WorktreeGit {
    /// Tracked files changed, staged or not.
    pub changed: u32,
    pub untracked: u32,
    /// The upstream (`origin/feature-x`); None for a branch with none.
    pub upstream: Option<String>,
    /// Commits HEAD has that the upstream lacks, and the other way round;
    /// None with no upstream.
    pub ahead: Option<u32>,
    pub behind: Option<u32>,
    /// Committer date of HEAD, unix seconds; 0 before the first commit.
    pub committed: i64,
    /// Commits on the branch since its base branch's merge-base
    /// (`nebula_core::review_base`); 0 on the base branch or with none.
    pub ahead_of_base: u32,
    /// The checkout is on the base branch itself.
    pub on_default_branch: bool,
}

impl WorktreeGit {
    /// Work no pull request carries yet: changed or untracked files,
    /// commits ahead of the upstream, or commits since the base.
    pub fn has_changes(&self) -> bool {
        self.changed + self.untracked > 0
            || self.ahead.is_some_and(|n| n > 0)
            || self.ahead_of_base > 0
    }

    /// What the board reads off it, the pull request aside.
    pub fn card_facts(&self) -> nebula_core::board::CardFacts {
        nebula_core::board::CardFacts {
            has_changes: self.has_changes(),
            ahead_of_base: self.ahead_of_base,
            on_default_branch: self.on_default_branch,
            pr: None,
        }
    }
}

/// The checkout's [`WorktreeGit`]: one `git status --porcelain=v2
/// --branch` (counts and ahead/behind in one go) and one `git log -1`.
pub fn worktree_git(root: &Path) -> Result<WorktreeGit, String> {
    let raw = git(root, &["status", "--porcelain=v2", "--branch", "-z"])?;
    // An untracked folder that is a repository of its own (a worktree
    // nested in the root, like `.claude/worktrees/x`) is not a change, as
    // the branch switcher's `changes` reads it.
    let mut out = parse_status_v2(&raw, |dir| root.join(dir).join(".git").exists());
    out.committed = git_text(root, &["log", "-1", "--format=%ct"])
        .ok()
        .and_then(|t| t.parse().ok())
        .unwrap_or(0);
    Ok(out)
}

/// `git status --porcelain=v2 --branch -z` output as a [`WorktreeGit`]
/// (all but `committed`). A rename or copy (`2`) carries its old path as
/// one more entry; an untracked folder `nested` says is a repository is
/// skipped.
pub fn parse_status_v2(raw: &[u8], nested: impl Fn(&str) -> bool) -> WorktreeGit {
    let mut out = WorktreeGit::default();
    let mut entries = raw.split(|b| *b == 0);
    while let Some(e) = entries.next() {
        let line = String::from_utf8_lossy(e);
        if let Some(up) = line.strip_prefix("# branch.upstream ") {
            out.upstream = Some(up.trim().to_string());
        } else if let Some(ab) = line.strip_prefix("# branch.ab ") {
            let mut it = ab.split_whitespace();
            out.ahead = it
                .next()
                .and_then(|a| a.trim_start_matches('+').parse().ok());
            out.behind = it
                .next()
                .and_then(|b| b.trim_start_matches('-').parse().ok());
        } else if let Some(path) = line.strip_prefix("? ") {
            if !(path.ends_with('/') && nested(path)) {
                out.untracked += 1;
            }
        } else if line.starts_with(['1', '2', 'u']) {
            out.changed += 1;
            if line.starts_with("2 ") {
                entries.next();
            }
        }
    }
    out
}

/// (tracked changes, untracked files) in `git status --porcelain=v1 -z`
/// output. A rename or copy carries its old path as one more entry.
fn count_status(raw: &[u8]) -> (u32, u32) {
    let (mut changed, mut untracked) = (0, 0);
    let mut entries = raw.split(|b| *b == 0);
    while let Some(e) = entries.next() {
        if e.len() < 3 {
            continue;
        }
        match &e[..2] {
            b"??" => untracked += 1,
            b"!!" => {}
            xy => {
                changed += 1;
                if xy[0] == b'R' || xy[0] == b'C' {
                    entries.next();
                }
            }
        }
    }
    (changed, untracked)
}

/// Whether `gh` is on PATH at all, so the lookups are skipped cleanly.
pub fn have_gh() -> bool {
    let mut cmd = Command::new("gh");
    cmd.arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    nebula_core::platform::no_window(&mut cmd);
    cmd.status().map(|s| s.success()).unwrap_or(false)
}

/// The URL of the remote a repo's web page follows: `origin` when there
/// is one, else the first remote git lists (as the TUI's `G` picks).
pub fn remote_url(root: &Path) -> Result<String, String> {
    let get = |name: &str| {
        git(root, &["remote", "get-url", name])
            .ok()
            .map(|out| String::from_utf8_lossy(&out).trim().to_string())
            .filter(|url| !url.is_empty())
    };
    if let Some(url) = get("origin") {
        return Ok(url);
    }
    let names = git(root, &["remote"])?;
    let names = String::from_utf8_lossy(&names);
    let first = names
        .lines()
        .map(str::trim)
        .find(|n| !n.is_empty())
        .ok_or_else(|| "This repo has no git remote.".to_string())?;
    get(first).ok_or_else(|| format!("No URL is set for the remote {first}."))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn numstat_fills_plain_and_renamed_entries() {
        let mut files = vec![
            DiffFile {
                path: "a.rs".into(),
                from: None,
                status: 'M',
                added: None,
                removed: None,
            },
            DiffFile {
                path: "new.rs".into(),
                from: Some("old.rs".into()),
                status: 'R',
                added: None,
                removed: None,
            },
        ];
        apply_numstat(&mut files, b"3\t1\ta.rs\x002\t0\t\x00old.rs\x00new.rs\x00");
        assert_eq!((files[0].added, files[0].removed), (Some(3), Some(1)));
        assert_eq!((files[1].added, files[1].removed), (Some(2), Some(0)));
    }

    /// A throwaway repo under the temp dir, removed when dropped.
    pub(crate) struct Repo(pub(crate) PathBuf);

    impl Repo {
        pub(crate) fn new(tag: &str) -> Repo {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let dir = std::env::temp_dir().join(format!(
                "kete-git-test-{tag}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let repo = Repo(dir);
            repo.git(&["init", "-q", "-b", "main"]);
            repo.git(&["config", "user.name", "Kete Test"]);
            repo.git(&["config", "user.email", "test@example.invalid"]);
            repo.git(&["config", "core.autocrlf", "false"]);
            repo.git(&["config", "commit.gpgsign", "false"]);
            repo
        }

        pub(crate) fn git(&self, args: &[&str]) -> String {
            let out = git_raw(&self.0, args).unwrap();
            assert!(
                out.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        }

        pub(crate) fn write(&self, path: &str, bytes: &[u8]) {
            let p = self.0.join(path);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, bytes).unwrap();
        }

        fn read(&self, path: &str) -> String {
            std::fs::read_to_string(self.0.join(path)).unwrap()
        }

        pub(crate) fn commit(&self, msg: &str) -> String {
            self.git(&["add", "-A"]);
            self.git(&["commit", "-q", "-m", msg]);
            self.git(&["rev-parse", "HEAD"])
        }

        fn staged(&self, path: &str) -> String {
            self.git(&["show", &format!(":{path}")])
        }
    }

    impl Drop for Repo {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Twenty numbered lines with `eol` endings.
    fn lines(eol: &str) -> String {
        (1..=20).map(|n| format!("line {n}{eol}")).collect()
    }

    /// Lines 2 and 18 changed: two hunks, far enough apart at -U3.
    fn two_edits(eol: &str) -> String {
        lines(eol)
            .replace(&format!("line 2{eol}"), &format!("line two{eol}"))
            .replace(&format!("line 18{eol}"), &format!("line eighteen{eol}"))
    }

    #[test]
    fn a_path_missing_at_a_commit_is_missing_not_empty() {
        let repo = Repo::new("missing");
        repo.write("a.txt", b"hello\n");
        let sha = repo.commit("one");
        let side = DiffSide::Commit(sha);
        assert_eq!(blob(&repo.0, &side, "nope.txt").unwrap(), Blob::Missing);
        // A tree reads the same way: a session's snapshots are trees.
        let tree = resolve_tree(&repo.0, "HEAD").unwrap();
        assert_eq!(
            blob(&repo.0, &DiffSide::Commit(tree), "a.txt").unwrap(),
            Blob::Text("hello\n".into())
        );
        assert_eq!(
            blob(&repo.0, &side, "a.txt").unwrap(),
            Blob::Text("hello\n".into())
        );
        assert_eq!(
            blob(&repo.0, &DiffSide::Worktree, "gone.txt").unwrap(),
            Blob::Missing
        );
        assert_eq!(
            blob(&repo.0, &DiffSide::Index, "gone.txt").unwrap(),
            Blob::Missing
        );
    }

    #[test]
    fn a_commit_not_in_the_clone_is_an_error() {
        let repo = Repo::new("nocommit");
        repo.write("a.txt", b"hello\n");
        repo.commit("one");
        let side = DiffSide::Commit("0123456789abcdef0123456789abcdef01234567".into());
        let e = blob(&repo.0, &side, "a.txt").unwrap_err();
        assert!(e.contains("not in this clone"), "{e}");
    }

    #[test]
    fn files_since_the_base_keep_committed_work_and_count_lines() {
        let repo = Repo::new("since");
        repo.write("a.txt", b"one\n");
        let base = repo.commit("base");
        repo.git(&["checkout", "-q", "-b", "feat"]);
        repo.write("b.txt", b"b1\nb2\n");
        repo.commit("agent work");
        repo.write("a.txt", b"one\ntwo\n");
        repo.write("new.txt", b"x");
        // Against HEAD the committed file is gone; against the base it stays.
        let head: Vec<String> = changed_files(&repo.0)
            .unwrap()
            .into_iter()
            .map(|f| f.path)
            .collect();
        assert_eq!(head, vec!["a.txt", "new.txt"]);
        let since = files_since(&repo.0, &base).unwrap();
        let got: Vec<(&str, char, Option<u64>)> = since
            .iter()
            .map(|f| (f.path.as_str(), f.status, f.added))
            .collect();
        assert_eq!(
            got,
            vec![
                ("a.txt", 'M', Some(1)),
                ("b.txt", 'A', Some(2)),
                ("new.txt", 'U', Some(1)),
            ]
        );
    }

    #[test]
    fn a_path_outside_the_repo_is_refused() {
        let repo = Repo::new("escape");
        assert!(blob(&repo.0, &DiffSide::Worktree, "../x").is_err());
        assert!(blob(&repo.0, &DiffSide::Worktree, "/etc/passwd").is_err());
    }

    #[test]
    fn a_3mb_file_is_too_large_on_every_side() {
        let repo = Repo::new("large");
        let big = vec![b'a'; 3 * 1024 * 1024];
        repo.write("big.txt", &big);
        let sha = repo.commit("big");
        let size = big.len() as u64;
        for side in [DiffSide::Worktree, DiffSide::Index, DiffSide::Commit(sha)] {
            assert_eq!(
                blob(&repo.0, &side, "big.txt").unwrap(),
                Blob::TooLarge(size),
                "{side:?}"
            );
        }
    }

    #[test]
    fn a_nul_byte_is_binary_on_every_side() {
        let repo = Repo::new("binary");
        repo.write("bin.dat", b"abc\0def");
        let sha = repo.commit("bin");
        for side in [DiffSide::Worktree, DiffSide::Index, DiffSide::Commit(sha)] {
            assert_eq!(
                blob(&repo.0, &side, "bin.dat").unwrap(),
                Blob::Binary,
                "{side:?}"
            );
        }
    }

    #[test]
    fn file_pair_errors_instead_of_going_blank() {
        let repo = Repo::new("pair");
        repo.write("a.txt", b"one\n");
        repo.commit("one");
        repo.write("a.txt", b"two\n");
        let ok = DiffFile {
            path: "a.txt".into(),
            from: None,
            status: 'M',
            added: None,
            removed: None,
        };
        let pair = file_pair(&repo.0, &ok).unwrap();
        assert_eq!(
            (pair.original.as_str(), pair.modified.as_str()),
            ("one\n", "two\n")
        );
        let ghost = DiffFile {
            path: "ghost.txt".into(),
            ..ok
        };
        assert!(file_pair(&repo.0, &ghost.clone()).is_err());
        // Added then deleted on disk: an empty side, as before.
        let added_gone = DiffFile {
            status: 'A',
            ..ghost
        };
        assert!(file_pair(&repo.0, &added_gone).is_ok());
    }

    #[test]
    fn staging_hunk_one_leaves_hunk_two_unstaged() {
        for eol in ["\n", "\r\n"] {
            let repo = Repo::new("stage");
            repo.write("f.txt", lines(eol).as_bytes());
            repo.commit("base");
            repo.write("f.txt", two_edits(eol).as_bytes());

            let unstaged = hunks(&repo.0, "f.txt", false).unwrap();
            assert_eq!(unstaged.len(), 2, "{eol:?}");
            assert!(unstaged[0].patch.starts_with("diff --git"));
            assert_eq!(unstaged[0].old_start, 1);
            assert!(unstaged[1].old_start > 10);

            apply_hunk(&repo.0, &unstaged[0], HunkAction::Stage).unwrap();

            let index = repo.staged("f.txt");
            assert!(index.contains("line two"), "{eol:?}");
            assert!(!index.contains("line eighteen"), "{eol:?}");
            let staged = hunks(&repo.0, "f.txt", true).unwrap();
            assert_eq!(staged.len(), 1);
            assert_eq!(staged[0].old_start, 1);
            let left = hunks(&repo.0, "f.txt", false).unwrap();
            assert_eq!(left.len(), 1);
            assert!(left[0].patch.contains("+line eighteen"));
            // The disk is untouched, CRLF and all.
            assert_eq!(repo.read("f.txt"), two_edits(eol));

            apply_hunk(&repo.0, &staged[0], HunkAction::Unstage).unwrap();
            assert!(hunks(&repo.0, "f.txt", true).unwrap().is_empty());
            assert_eq!(repo.staged("f.txt").replace('\r', ""), lines("\n").trim());
        }
    }

    #[test]
    fn discard_reverts_only_that_hunk() {
        for eol in ["\n", "\r\n"] {
            let repo = Repo::new("discard");
            repo.write("f.txt", lines(eol).as_bytes());
            repo.commit("base");
            repo.write("f.txt", two_edits(eol).as_bytes());

            let unstaged = hunks(&repo.0, "f.txt", false).unwrap();
            apply_hunk(&repo.0, &unstaged[1], HunkAction::Discard).unwrap();

            let want = lines(eol).replace(&format!("line 2{eol}"), &format!("line two{eol}"));
            assert_eq!(repo.read("f.txt"), want, "{eol:?}");
            let left = hunks(&repo.0, "f.txt", false).unwrap();
            assert_eq!(left.len(), 1);
            assert!(left[0].patch.contains("+line two"));
        }
    }

    #[test]
    fn a_hunk_without_a_file_header_is_refused() {
        let repo = Repo::new("header");
        let bare = Hunk {
            header: "@@ -1 +1 @@".into(),
            patch: "@@ -1 +1 @@\n-a\n+b\n".into(),
            old_start: 1,
            new_start: 1,
        };
        assert!(apply_hunk(&repo.0, &bare, HunkAction::Stage).is_err());
    }

    #[test]
    fn range_diff_runs_from_the_merge_base_not_the_target_tip() {
        let repo = Repo::new("range");
        repo.write("shared.txt", b"base\n");
        let fork = repo.commit("base");
        repo.git(&["checkout", "-q", "-b", "feature"]);
        repo.write("feature.txt", b"mine\n");
        repo.commit("feature work");
        repo.git(&["checkout", "-q", "main"]);
        repo.write("landed.txt", b"theirs\n");
        repo.write("shared.txt", b"base\nmore\n");
        repo.commit("main moved on");

        assert_eq!(merge_base(&repo.0, "main", "feature").unwrap(), fork);
        let files = diff_files_range(&repo.0, "main", "feature").unwrap();
        let paths: Vec<_> = files.iter().map(|f| (f.path.as_str(), f.status)).collect();
        assert_eq!(paths, vec![("feature.txt", 'A')]);
        assert_eq!((files[0].added, files[0].removed), (Some(1), Some(0)));
    }

    #[test]
    fn range_diff_reports_renames_and_deletes() {
        let repo = Repo::new("rename");
        repo.write("old.txt", lines("\n").as_bytes());
        repo.write("dead.txt", b"x\n");
        let base = repo.commit("base");
        repo.git(&["mv", "old.txt", "new.txt"]);
        repo.git(&["rm", "-q", "dead.txt"]);
        let tip = repo.commit("move");
        let files = diff_files_range(&repo.0, &base, &tip).unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!((files[0].path.as_str(), files[0].status), ("dead.txt", 'D'));
        assert_eq!(files[1].path, "new.txt");
        assert_eq!(files[1].status, 'R');
        assert_eq!(files[1].from.as_deref(), Some("old.txt"));
    }

    /// An origin whose `main` has one commit and whose `refs/pull/7/merge`
    /// points at a `topic` commit on top of it. Returns (origin, pr sha).
    fn origin_with_pr() -> (Repo, String) {
        let origin = Repo::new("origin");
        origin.write("a.txt", b"a\n");
        origin.commit("one");
        origin.git(&["checkout", "-q", "-b", "topic"]);
        origin.write("b.txt", b"b\n");
        let pr = origin.commit("pr");
        origin.git(&["update-ref", "refs/pull/7/merge", &pr]);
        origin.git(&["checkout", "-q", "main"]);
        (origin, pr)
    }

    pub(crate) fn url(repo: &Repo) -> String {
        format!(
            "file:///{}",
            repo.0
                .to_string_lossy()
                .replace('\\', "/")
                .trim_start_matches('/')
        )
    }

    /// A clone that has origin's `main` only.
    fn clone_of(origin: &Repo) -> Repo {
        let clone = Repo::new("clone");
        clone.git(&["remote", "add", "origin", &url(origin)]);
        clone.git(&["fetch", "-q", "origin", "main"]);
        clone
    }

    #[test]
    fn ensure_refs_fetches_a_missing_pull_merge_ref() {
        let (origin, pr) = origin_with_pr();
        let clone = clone_of(&origin);
        assert!(!has_commit(&clone.0, "refs/pull/7/merge").unwrap());

        ensure_refs(&clone.0, "origin", &["main", "refs/pull/7/merge"]).unwrap();
        assert_eq!(clone.git(&["rev-parse", "refs/pull/7/merge"]), pr);
        assert_eq!(
            commit_name(&clone.0, "origin", "main", true).unwrap(),
            "origin/main"
        );
        let files = diff_files_range(&clone.0, "origin/main", "refs/pull/7/merge").unwrap();
        assert_eq!(files.len(), 1);

        let e = ensure_refs(&clone.0, "origin", &["refs/pull/99/merge"]).unwrap_err();
        assert!(e.starts_with("git fetch failed: "), "{e}");
        assert!(e.contains("refs/pull/99/merge"), "{e}");
    }

    #[test]
    fn a_moved_pull_merge_ref_is_refetched_not_reused() {
        let (origin, pr) = origin_with_pr();
        let clone = clone_of(&origin);
        let first = prepare_range(&clone.0, "main", "refs/pull/7/merge").unwrap();
        assert_eq!((first.tip.as_str(), first.offline), (pr.as_str(), false));

        // The PR gets a new commit; its merge ref moves on origin.
        origin.git(&["checkout", "-q", "topic"]);
        origin.write("c.txt", b"c\n");
        let moved = origin.commit("pr again");
        origin.git(&["update-ref", "refs/pull/7/merge", &moved]);

        let second = prepare_range(&clone.0, "main", "refs/pull/7/merge").unwrap();
        assert_eq!(second.tip, moved);
        assert!(!second.offline);
        let files = diff_files_between(&clone.0, &second.base, &second.tip).unwrap();
        assert_eq!(files.len(), 2);
    }

    #[test]
    fn the_target_prefers_origin_over_a_stale_local_branch() {
        let (origin, _) = origin_with_pr();
        let clone = clone_of(&origin);
        let old = clone.git(&["rev-parse", "origin/main"]);
        clone.git(&["branch", "main", "origin/main"]);
        clone.git(&["checkout", "-q", "main"]);
        // main moves on origin; the local main stays behind.
        origin.write("landed.txt", b"x\n");
        let landed = origin.commit("landed");
        origin.git(&["checkout", "-q", "-b", "pr2", &landed]);
        origin.write("d.txt", b"d\n");
        let pr2 = origin.commit("pr2");
        origin.git(&["update-ref", "refs/pull/8/merge", &pr2]);

        let r = prepare_range(&clone.0, "main", "refs/pull/8/merge").unwrap();
        assert_eq!(r.base, landed, "merge-base must come from origin/main");
        assert_ne!(r.base, old);
        let files = diff_files_between(&clone.0, &r.base, &r.tip).unwrap();
        let paths: Vec<_> = files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, vec!["d.txt"]);
    }

    #[test]
    fn a_failed_fetch_uses_the_cached_ref_and_says_offline() {
        let (origin, pr) = origin_with_pr();
        let clone = clone_of(&origin);
        prepare_range(&clone.0, "main", "refs/pull/7/merge").unwrap();
        clone.git(&[
            "remote",
            "set-url",
            "origin",
            "file:///nowhere/kete-no-such-repo",
        ]);

        let r = prepare_range(&clone.0, "main", "refs/pull/7/merge").unwrap();
        assert_eq!(r.tip, pr);
        assert!(r.offline);
        // With nothing cached, the fetch error surfaces.
        let e = prepare_range(&clone.0, "main", "refs/pull/9/merge").unwrap_err();
        assert!(e.starts_with("git fetch failed: "), "{e}");
    }

    #[test]
    fn a_local_tip_branch_is_read_as_is_without_fetching() {
        let (origin, _) = origin_with_pr();
        let clone = clone_of(&origin);
        clone.git(&["checkout", "-q", "-b", "work", "origin/main"]);
        clone.write("w.txt", b"w\n");
        let tip = clone.commit("unpushed");
        let r = prepare_range(&clone.0, "main", "work").unwrap();
        assert_eq!((r.tip.as_str(), r.offline), (tip.as_str(), false));
    }

    #[test]
    fn a_shallow_clone_is_deepened_until_the_merge_base_shows() {
        let origin = Repo::new("deep-origin");
        origin.write("a.txt", b"a\n");
        origin.commit("fork point");
        origin.git(&["checkout", "-q", "-b", "topic"]);
        origin.write("t.txt", b"t\n");
        let pr = origin.commit("pr");
        origin.git(&["update-ref", "refs/pull/3/merge", &pr]);
        origin.git(&["checkout", "-q", "main"]);
        for n in 0..3 {
            origin.write("a.txt", format!("a{n}\n").as_bytes());
            origin.commit("main moves");
        }

        let clone = Repo::new("shallow");
        clone.git(&["remote", "add", "origin", &url(&origin)]);
        clone.git(&[
            "fetch",
            "-q",
            "--depth=1",
            "origin",
            "+refs/heads/main:refs/remotes/origin/main",
        ]);
        clone.git(&[
            "fetch",
            "-q",
            "--depth=1",
            "origin",
            "+refs/pull/3/merge:refs/pull/3/merge",
        ]);
        assert_eq!(clone.git(&["rev-parse", "--is-shallow-repository"]), "true");
        assert!(merge_base(&clone.0, "origin/main", "refs/pull/3/merge").is_err());

        let r = prepare_range(&clone.0, "main", "refs/pull/3/merge").unwrap();
        assert_eq!(r.tip, pr);
        let files = diff_files_between(&clone.0, &r.base, &r.tip).unwrap();
        let paths: Vec<_> = files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, vec!["t.txt"]);
    }

    /// An origin whose `refs/pull/5/merge` is a real merge commit: main
    /// moved on (`landed.txt`) and the PR branch added `p.txt`. Returns
    /// (origin, target sha, source sha).
    fn origin_with_merge_ref() -> (Repo, String, String) {
        let origin = Repo::new("merge-origin");
        origin.write("a.txt", b"a\n");
        origin.commit("one");
        origin.git(&["checkout", "-q", "-b", "topic"]);
        origin.write("p.txt", b"p\n");
        let source = origin.commit("pr work");
        origin.git(&["checkout", "-q", "main"]);
        origin.write("landed.txt", b"l\n");
        let target = origin.commit("main moves");
        origin.git(&["checkout", "-q", "--detach", &target]);
        origin.git(&["merge", "-q", "--no-ff", "-m", "merge pr", &source]);
        let merge = origin.git(&["rev-parse", "HEAD"]);
        origin.git(&["update-ref", "refs/pull/5/merge", &merge]);
        origin.git(&["checkout", "-q", "main"]);
        (origin, target, source)
    }

    #[test]
    fn a_pr_diff_pins_to_its_last_merge_commits_or_the_merge_ref_parents() {
        let (origin, target, source) = origin_with_merge_ref();

        // The SHAs, with the merge ref fetched first to bring them in.
        let clone = clone_of(&origin);
        assert!(!has_commit(&clone.0, &source).unwrap());
        let r =
            prepare_range_at(&clone.0, "origin", &target, &source, &["refs/pull/5/merge"]).unwrap();
        assert_eq!(r.tip, source);
        let files = diff_files_between(&clone.0, &r.base, &r.tip).unwrap();
        let paths: Vec<_> = files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, vec!["p.txt"], "only the PR's own change");

        // No SHAs: the merge ref's two parents, fetched by the ref.
        let fresh = clone_of(&origin);
        let r = prepare_range_at(
            &fresh.0,
            "origin",
            "refs/pull/5/merge^1",
            "refs/pull/5/merge^2",
            &[],
        )
        .unwrap();
        assert_eq!(r.tip, source);
        let files = diff_files_between(&fresh.0, &r.base, &r.tip).unwrap();
        let paths: Vec<_> = files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, vec!["p.txt"]);
    }

    #[test]
    fn a_range_reads_from_the_named_remote() {
        let (origin, pr) = origin_with_pr();
        let clone = Repo::new("upstream-clone");
        clone.git(&["remote", "add", "upstream", &url(&origin)]);
        clone.git(&["fetch", "-q", "upstream", "main"]);
        let r = prepare_range_at(&clone.0, "upstream", "main", "refs/pull/7/merge", &[]).unwrap();
        assert_eq!((r.tip.as_str(), r.offline), (pr.as_str(), false));
        assert_eq!(
            commit_name(&clone.0, "upstream", "main", true).unwrap(),
            "upstream/main"
        );
    }

    #[test]
    fn walks_split_off_the_fetch_name() {
        assert_eq!(
            split_walk("refs/pull/5/merge^2"),
            ("refs/pull/5/merge", "^2")
        );
        assert_eq!(split_walk("main~1"), ("main", "~1"));
        assert_eq!(split_walk("main"), ("main", ""));
        assert_eq!(split_walk("a^b"), ("a^b", ""));
    }

    #[test]
    fn only_whole_shas_are_fetched_by_id() {
        assert!(is_full_sha(&"a".repeat(40)));
        assert!(is_full_sha(&"a".repeat(64)));
        assert!(!is_full_sha(&"a".repeat(12)));
        assert!(!is_full_sha(&"g".repeat(40)));
        assert!(is_short_sha("abc1234"));
    }

    #[test]
    fn a_blob_in_a_folder_reads_at_every_side() {
        let repo = Repo::new("subdir");
        repo.write("src/deep/a.txt", b"deep\n");
        let sha = repo.commit("deep");
        for side in [
            DiffSide::Index,
            DiffSide::Commit(sha.clone()),
            DiffSide::Worktree,
        ] {
            assert_eq!(
                blob(&repo.0, &side, "src/deep/a.txt").unwrap(),
                Blob::Text("deep\n".into()),
                "{side:?}"
            );
        }
        let e = blob(&repo.0, &DiffSide::Commit(sha.clone()), "src").unwrap_err();
        assert!(e.contains("tree"), "{e}");
        // A glob is a file name, not a pattern.
        assert_eq!(
            blob(&repo.0, &DiffSide::Commit(sha), "src/*/a.txt").unwrap(),
            Blob::Missing
        );
    }

    #[test]
    fn a_rename_deleted_on_disk_keeps_an_empty_side() {
        let repo = Repo::new("rd");
        repo.write("old.txt", b"x\n");
        repo.commit("base");
        repo.git(&["mv", "old.txt", "new.txt"]);
        std::fs::remove_file(repo.0.join("new.txt")).unwrap();
        let rd = DiffFile {
            path: "new.txt".into(),
            from: Some("old.txt".into()),
            status: 'R',
            added: None,
            removed: None,
        };
        let pair = file_pair(&repo.0, &rd).unwrap();
        assert_eq!(
            (pair.original.as_str(), pair.modified.as_str()),
            ("x\n", "")
        );
    }

    #[test]
    fn hunk_starts_reads_both_sides() {
        assert_eq!(hunk_starts("@@ -12,3 +14,5 @@ fn x"), (12, 14));
        assert_eq!(hunk_starts("@@ -1 +1 @@"), (1, 1));
        assert_eq!(hunk_starts("@@ -0,0 +1,2 @@"), (0, 1));
    }

    #[test]
    fn pick_pr_prefers_open_then_newest_and_skips_forks() {
        let json = br#"[
            {"number":9,"url":"u9","title":"fork","state":"OPEN","isDraft":false,"headRefName":"main","headRefOid":"f","isCrossRepository":true},
            {"number":5,"url":"u5","title":"old","state":"MERGED","isDraft":false,"headRefName":"main","headRefOid":"a","isCrossRepository":false},
            {"number":7,"url":"u7","title":"newer","state":"CLOSED","isDraft":false,"headRefName":"main","headRefOid":"b","isCrossRepository":false}
        ]"#;
        let pr = pick_pr(json, "main").unwrap();
        assert_eq!(
            (pr.number, pr.state.as_str(), pr.head_sha.as_str()),
            (7, "closed", "b")
        );

        let json = br#"[
            {"number":8,"url":"u8","title":"later merge","state":"MERGED","isDraft":false,"headRefName":"topic","headRefOid":"c","isCrossRepository":false},
            {"number":3,"url":"u3","title":"reopened","state":"OPEN","isDraft":true,"headRefName":"topic","headRefOid":"d","isCrossRepository":false}
        ]"#;
        let pr = pick_pr(json, "topic").unwrap();
        assert_eq!((pr.number, pr.state.as_str(), pr.draft), (3, "open", true));
        assert!(pick_pr(json, "other").is_none());
        assert!(pick_pr(b"[]", "topic").is_none());
    }

    #[test]
    fn count_status_counts_tracked_and_untracked_once() {
        assert_eq!(
            count_status(b"R  new\0old\0 M a\0A  b\0?? c\0?? d/\0"),
            (3, 2)
        );
        assert_eq!(count_status(b""), (0, 0));
    }

    #[test]
    fn checkout_state_reports_changes_and_unpushed_commits() {
        let origin = Repo::new("state-origin");
        origin.write("a.txt", b"a\n");
        origin.commit("one");
        let repo = Repo::new("state");
        // No remote: nothing to say about pushing.
        repo.write("a.txt", b"a\n");
        repo.commit("local");
        assert_eq!(checkout_state(&repo.0).unwrap().unpushed, None);

        let clone = clone_of(&origin);
        clone.git(&["checkout", "-q", "-b", "topic", "origin/main"]);
        assert_eq!(
            checkout_state(&clone.0).unwrap(),
            CheckoutState {
                changed: 0,
                untracked: 0,
                unpushed: Some(0)
            }
        );
        clone.write("b.txt", b"b\n");
        clone.commit("two");
        clone.write("a.txt", b"edited\n");
        clone.write("new.txt", b"n\n");
        assert_eq!(
            checkout_state(&clone.0).unwrap(),
            CheckoutState {
                changed: 1,
                untracked: 1,
                unpushed: Some(1)
            }
        );
    }

    /// The board's `has_changes`: files, or commits the upstream lacks.
    #[test]
    fn changes_are_files_or_unpushed_commits() {
        let g = |changed, untracked, ahead| WorktreeGit {
            changed,
            untracked,
            ahead,
            ..WorktreeGit::default()
        };
        assert!(!g(0, 0, None).has_changes());
        assert!(!g(0, 0, Some(0)).has_changes());
        assert!(g(1, 0, None).has_changes());
        assert!(g(0, 2, Some(0)).has_changes());
        assert!(g(0, 0, Some(3)).has_changes());
        // Committed since the base and never pushed: no upstream to be
        // ahead of, but work all the same.
        let committed = WorktreeGit {
            ahead_of_base: 2,
            ..WorktreeGit::default()
        };
        assert!(committed.has_changes());
        let facts = committed.card_facts();
        assert!(facts.has_changes && facts.ahead_of_base == 2 && !facts.on_default_branch);
    }

    #[test]
    fn status_v2_counts_changes_and_reads_ahead_behind() {
        let raw = b"# branch.oid abc\0# branch.head topic\0# branch.upstream origin/topic\0# branch.ab +2 -3\0\
1 .M N... 100644 100644 100644 a a a.txt\0\
2 R. N... 100644 100644 100644 a a R100 new.txt\0old.txt\0\
u UU N... 100644 100644 100644 100644 a a a c.txt\0\
? n.txt\0? m.txt\0! ignored.txt\0";
        assert_eq!(
            parse_status_v2(raw, |_| false),
            WorktreeGit {
                changed: 3,
                untracked: 2,
                upstream: Some("origin/topic".into()),
                ahead: Some(2),
                behind: Some(3),
                committed: 0,
                ..WorktreeGit::default()
            }
        );
        let none = parse_status_v2(b"# branch.oid (initial)\0# branch.head main\0", |_| false);
        assert_eq!(
            parse_status_v2(b"? sub/\0? dir/\0", |d| d == "sub/").untracked,
            1,
            "a nested repository is not an untracked change"
        );
        assert_eq!(none, WorktreeGit::default());
    }

    #[test]
    fn worktree_git_reads_a_real_checkout() {
        let origin = Repo::new("wtgit-origin");
        origin.write("a.txt", b"a\n");
        origin.commit("one");
        let clone = clone_of(&origin);
        clone.git(&["checkout", "-q", "-b", "topic", "origin/main"]);
        let clean = worktree_git(&clone.0).unwrap();
        assert_eq!((clean.changed, clean.untracked), (0, 0));
        assert_eq!(clean.upstream.as_deref(), Some("origin/main"));
        assert_eq!((clean.ahead, clean.behind), (Some(0), Some(0)));
        assert!(clean.committed > 0);
        clone.write("b.txt", b"b\n");
        clone.commit("two");
        clone.write("a.txt", b"edited\n");
        clone.write("new.txt", b"n\n");
        let busy = worktree_git(&clone.0).unwrap();
        assert_eq!((busy.changed, busy.untracked), (1, 1));
        assert_eq!((busy.ahead, busy.behind), (Some(1), Some(0)));
        let gone = std::env::temp_dir().join("kete-git-test-no-such-worktree");
        assert!(worktree_git(&gone).is_err());
    }

    #[test]
    fn checkout_state_of_a_missing_folder_is_an_error() {
        let gone = std::env::temp_dir().join("kete-git-test-no-such-checkout");
        assert!(checkout_state(&gone).is_err());
    }

    #[test]
    fn a_main_checkout_stamp_moves_on_commit_and_stage_but_not_on_an_edit() {
        let repo = Repo::new("stamp");
        repo.write("a.txt", b"a\n");
        repo.commit("one");
        let (own, common) = git_dirs(&repo.0).unwrap();
        assert_eq!(own, repo.0.join(".git"));
        assert_eq!(common, own);
        let first = git_stamp(&repo.0).unwrap();
        assert_eq!(git_stamp(&repo.0).unwrap(), first, "nothing changed");
        // A working-tree edit touches no git file.
        repo.write("a.txt", b"edited\n");
        assert_eq!(git_stamp(&repo.0).unwrap(), first);
        repo.git(&["add", "a.txt"]);
        let staged = git_stamp(&repo.0).unwrap();
        assert_ne!(staged, first, "staging rewrites the index");
        repo.git(&["commit", "-q", "-m", "two"]);
        assert_ne!(
            git_stamp(&repo.0).unwrap(),
            staged,
            "a commit appends to logs/HEAD"
        );
    }

    #[test]
    fn a_linked_checkout_reads_its_own_git_dir_and_the_shared_common_dir() {
        let repo = Repo::new("stamp-main");
        repo.write("a.txt", b"a\n");
        repo.commit("one");
        let linked = repo.0.with_file_name(format!(
            "{}-linked",
            repo.0.file_name().unwrap().to_string_lossy()
        ));
        let _ = std::fs::remove_dir_all(&linked);
        repo.git(&[
            "worktree",
            "add",
            "-q",
            "-b",
            "topic",
            &linked.to_string_lossy(),
        ]);
        let (own, common) = git_dirs(&linked).unwrap();
        assert!(
            std::fs::canonicalize(&own)
                .unwrap()
                .starts_with(std::fs::canonicalize(repo.0.join(".git/worktrees")).unwrap()),
            "a linked checkout has its own git dir under the common one"
        );
        assert_eq!(
            std::fs::canonicalize(&common).unwrap(),
            std::fs::canonicalize(repo.0.join(".git")).unwrap()
        );
        let before = git_stamp(&linked).unwrap();
        let main_before = git_stamp(&repo.0).unwrap();
        std::fs::write(linked.join("b.txt"), b"b\n").unwrap();
        git_raw(&linked, &["add", "b.txt"]).unwrap();
        git_raw(&linked, &["commit", "-q", "-m", "two"]).unwrap();
        assert_ne!(git_stamp(&linked).unwrap(), before);
        assert_eq!(
            git_stamp(&repo.0).unwrap(),
            main_before,
            "the main checkout's own files are untouched"
        );
        let _ = std::fs::remove_dir_all(&linked);
    }

    #[test]
    fn a_folder_that_is_no_checkout_has_no_stamp() {
        let gone = std::env::temp_dir().join("kete-git-test-no-such-stamp");
        assert_eq!(git_stamp(&gone), None);
    }
}
