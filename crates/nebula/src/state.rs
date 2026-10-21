//! `kete state` (K10): a studio repo's state map, from the file the daemon
//! writes (`nebula_core::state_map::cli_text` holds the four outputs).

use anyhow::{bail, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn run(repo: Option<PathBuf>, slice: Option<String>, json: bool) -> Result<()> {
    let dir = match repo {
        Some(dir) => dir,
        None => std::env::current_dir()?,
    };
    let root = main_checkout(&dir);
    match nebula_core::state_map::cli_text(
        &root,
        slice.as_deref(),
        json,
        nebula_core::clock::now_ms(),
    ) {
        Ok(text) => {
            print!("{text}");
            Ok(())
        }
        Err(why) => bail!("{why}"),
    }
}

/// The checkout the daemon writes the map in: the main one, found from any
/// worktree of the repo through git's common dir. `dir` itself when git
/// cannot say.
fn main_checkout(dir: &Path) -> PathBuf {
    let mut cmd = Command::new("git");
    cmd.arg("-C")
        .arg(dir)
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"]);
    #[cfg(windows)]
    nebula_core::platform::no_window(&mut cmd);
    cmd.output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| PathBuf::from(String::from_utf8_lossy(&out.stdout).trim()))
        .and_then(|common| common_to_checkout(&common))
        .unwrap_or_else(|| dir.to_path_buf())
}

/// `<repo>/.git` is the main checkout's own git dir; a bare repo has none.
fn common_to_checkout(common: &Path) -> Option<PathBuf> {
    (common.file_name()? == ".git").then(|| common.parent().map(Path::to_path_buf))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_main_checkout_is_the_common_dirs_parent() {
        assert_eq!(
            common_to_checkout(Path::new("C:/Code/game/.git")),
            Some(PathBuf::from("C:/Code/game"))
        );
        assert_eq!(common_to_checkout(Path::new("C:/Code/game.git")), None);
    }

    #[test]
    fn a_folder_outside_git_is_read_as_given() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(main_checkout(dir.path()), dir.path());
    }
}
