//! Role cards (K3): `<repo>/.kete/roles.toml`, read from committed
//! history, and the permission profiles and guard they carry (K9).
//!
//! Roles are read with `git show HEAD:<path>` in the project's root
//! worktree, never a working copy: an uncommitted edit changes nothing.

pub mod card;
pub mod guard;
pub mod permissions;

pub use card::{parse, Mcp, Permission, RoleCard, RoleError, RolesFile, ROLES_PATH};

use std::path::Path;
use std::process::{Command, Output, Stdio};

/// The roles file at `root_worktree`'s HEAD, with every `prompt_file`
/// read from the same commit into `prompt`. `Ok(None)`: the commit has no
/// roles file (or the folder is no repo, or has no commits yet).
pub fn load_committed(root_worktree: &Path) -> Result<Option<RolesFile>, String> {
    let Some(text) = show_head(root_worktree, ROLES_PATH)? else {
        return Ok(None);
    };
    let mut file = parse(&text).map_err(|e| e.0)?;
    for card in file.roles.values_mut() {
        let Some(path) = card.prompt_file.clone() else {
            continue;
        };
        let path = path.replace('\\', "/");
        let text = show_head(root_worktree, &path)?.ok_or_else(|| {
            format!(
                "{ROLES_PATH}: role `{}`: prompt_file `{path}` is not in the last commit",
                card.id
            )
        })?;
        card::check_prompt(&text)
            .map_err(|why| format!("{ROLES_PATH}: role `{}`: {path}: {why}", card.id))?;
        card.prompt = Some(text);
    }
    Ok(Some(file))
}

/// The commit `root_worktree` has checked out, for a cache key.
pub fn head(root_worktree: &Path) -> Option<String> {
    let out = git(root_worktree, &["rev-parse", "--verify", "--quiet", "HEAD"])?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// `path` as HEAD holds it. None when HEAD has no such file.
fn show_head(root: &Path, path: &str) -> Result<Option<String>, String> {
    let spec = format!("HEAD:{path}");
    let out = git(root, &["show", &spec])
        .ok_or_else(|| format!("could not run git in {}", root.display()))?;
    if !out.status.success() {
        // Only a file (or a commit) that is not there counts as no file;
        // anything else refuses, so a broken read never drops a role's rules.
        let present = git(root, &["cat-file", "-e", &spec]).is_some_and(|o| o.status.success());
        if !present {
            return Ok(None);
        }
        return Err(format!(
            "could not read {path} at HEAD: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    if out.stdout.len() > card::MAX_ROLES_BYTES {
        return Err(format!("{path} at HEAD is over 64 KiB"));
    }
    String::from_utf8(out.stdout)
        .map(Some)
        .map_err(|_| format!("{path} at HEAD is not valid UTF-8 text"))
}

/// `git -C root` with no optional locks, no prompt and no console window.
fn git(root: &Path, args: &[&str]) -> Option<Output> {
    let mut cmd = Command::new("git");
    cmd.arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null());
    #[cfg(windows)]
    crate::platform::no_window(&mut cmd);
    cmd.output().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git_in(dir: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .output()
            .unwrap()
            .status
            .success();
        assert!(ok, "git {args:?}");
    }

    fn repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        git_in(dir.path(), &["init", "-q"]);
        dir
    }

    fn write(dir: &Path, path: &str, text: &str) {
        let file = dir.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, text).unwrap();
    }

    #[test]
    fn no_repo_no_commit_and_no_file_are_none() {
        let plain = tempfile::tempdir().unwrap();
        assert_eq!(load_committed(plain.path()), Ok(None));
        let dir = repo();
        assert_eq!(load_committed(dir.path()), Ok(None), "no commits yet");
        write(dir.path(), "README.md", "x");
        git_in(dir.path(), &["add", "-A"]);
        git_in(dir.path(), &["commit", "-qm", "one"]);
        assert_eq!(load_committed(dir.path()), Ok(None));
        assert!(head(dir.path()).is_some());
    }

    #[test]
    fn roles_and_prompt_files_come_from_head_not_the_working_copy() {
        let dir = repo();
        write(
            dir.path(),
            ROLES_PATH,
            "version = 1\n[roles.runner]\nprompt_file = \".kete/roles/runner.md\"\n",
        );
        write(dir.path(), ".kete/roles/runner.md", "Run the checks.\n");
        git_in(dir.path(), &["add", "-A"]);
        git_in(dir.path(), &["commit", "-qm", "roles"]);
        // Uncommitted edits: a new role and a new prompt.
        write(
            dir.path(),
            ROLES_PATH,
            "version = 1\n[roles.runner]\nprompt_file = \".kete/roles/runner.md\"\n[roles.extra]\n",
        );
        write(dir.path(), ".kete/roles/runner.md", "Do anything.\n");
        let file = load_committed(dir.path()).unwrap().unwrap();
        assert_eq!(file.roles.len(), 1, "the uncommitted role is not there");
        assert_eq!(
            file.roles["runner"].prompt.as_deref(),
            Some("Run the checks.\n")
        );
    }

    #[test]
    fn a_missing_or_oversized_prompt_file_refuses() {
        let dir = repo();
        write(
            dir.path(),
            ROLES_PATH,
            "version = 1\n[roles.runner]\nprompt_file = \"gone.md\"\n",
        );
        git_in(dir.path(), &["add", "-A"]);
        git_in(dir.path(), &["commit", "-qm", "roles"]);
        assert!(load_committed(dir.path())
            .unwrap_err()
            .contains("not in the last commit"));
        write(
            dir.path(),
            "gone.md",
            &"x".repeat(card::MAX_PROMPT_BYTES + 1),
        );
        git_in(dir.path(), &["add", "-A"]);
        git_in(dir.path(), &["commit", "-qm", "prompt"]);
        assert!(load_committed(dir.path()).unwrap_err().contains("bytes"));
    }

    #[test]
    fn a_bad_committed_file_refuses() {
        let dir = repo();
        write(dir.path(), ROLES_PATH, "version = 2\n");
        git_in(dir.path(), &["add", "-A"]);
        git_in(dir.path(), &["commit", "-qm", "roles"]);
        assert!(load_committed(dir.path())
            .unwrap_err()
            .contains("version 2"));
    }
}
