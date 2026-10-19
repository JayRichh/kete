//! Repos that agents work in without anyone running `kete add`. A Claude
//! background job cuts its subagents' checkouts next to the repos it works
//! on (`C:\Code\app-audio` beside `C:\Code\app`, or under the repo's own
//! `.claude/worktrees`), and a repo nobody registered never reaches the
//! sidebar, however many checkouts and PRs it has. Each registered repo's
//! sibling folders are looked at: a folder that is a linked checkout, or a
//! repo holding linked checkouts, is registered through `add_project`, which
//! roots the project at the main checkout and adopts every checkout git
//! lists. Registered repos already get their checkouts from the stamp tick
//! (`sync_project_worktrees`).
//!
//! A repo the user removed is remembered in `declined-repos.json` and not
//! registered again.

use super::*;
use std::collections::HashSet;

/// Repos registered in one pass: a folder of fifty clones is not an
/// agent's work.
const ADOPTED_PER_PASS: usize = 8;

fn declined_path() -> PathBuf {
    nebula_core::paths::data_dir().join("declined-repos.json")
}

fn key(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_lowercase()
}

fn read_declined(file: &Path) -> HashSet<String> {
    std::fs::read_to_string(file)
        .ok()
        .and_then(|text| serde_json::from_str::<Vec<String>>(&text).ok())
        .unwrap_or_default()
        .into_iter()
        .collect()
}

fn decline_in(file: &Path, repo: &Path) {
    let mut declined = read_declined(file);
    if declined.insert(key(repo)) {
        let mut sorted: Vec<_> = declined.into_iter().collect();
        sorted.sort();
        if let Ok(json) = serde_json::to_string_pretty(&sorted) {
            let _ = std::fs::write(file, json);
        }
    }
}

/// Folders beside the registered repos that show agent work: a `.git` file
/// (a linked checkout) or a `.git` directory with linked checkouts under it.
/// `known` is every registered repo and checkout path, `declined` the repos
/// the user removed (both through [`key`]).
fn candidates(
    repos: &[PathBuf],
    known: &HashSet<String>,
    declined: &HashSet<String>,
) -> Vec<PathBuf> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut out = Vec::new();
    for repo in repos {
        let Some(parent) = repo.parent() else {
            continue;
        };
        let Ok(entries) = std::fs::read_dir(parent) else {
            continue;
        };
        let mut siblings: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        siblings.sort();
        for dir in siblings {
            let name = key(&dir);
            if known.contains(&name) || declined.contains(&name) || !seen.insert(name) {
                continue;
            }
            let dot_git = dir.join(".git");
            let Ok(meta) = std::fs::metadata(&dot_git) else {
                continue;
            };
            let shows_agent_work = meta.is_file()
                || std::fs::read_dir(dot_git.join("worktrees"))
                    .map(|mut d| d.next().is_some())
                    .unwrap_or(false);
            if shows_agent_work {
                out.push(dir);
            }
        }
    }
    out
}

impl Daemon {
    /// Register the repos [`candidates`] finds. Returns how many were added.
    /// A repo `add_project` refuses (already registered through another
    /// checkout, not a repo after all) is skipped; `tried` keeps one failure
    /// from being retried every pass.
    pub async fn adopt_agent_repos(self: &Arc<Self>, tried: &mut HashSet<String>) -> usize {
        let Ok((projects, worktrees, _, _)) = self.store_blocking(|s| s.load_tree()).await else {
            return 0;
        };
        let repos: Vec<PathBuf> = projects.iter().map(|p| p.repo_path.clone()).collect();
        let known: HashSet<String> = projects
            .iter()
            .map(|p| key(&p.repo_path))
            .chain(worktrees.iter().map(|w| key(&w.path)))
            .collect();
        let declined = read_declined(&declined_path());
        let mut added = 0;
        for dir in candidates(&repos, &known, &declined) {
            if added >= ADOPTED_PER_PASS || !tried.insert(key(&dir)) {
                continue;
            }
            match self.add_project(&dir, None, false).await {
                Ok(_) => {
                    tracing::info!(path = %dir.display(), "registered a repo agents work in");
                    added += 1;
                }
                Err(e) => tracing::debug!(path = %dir.display(), error = %e, "not registered"),
            }
        }
        added
    }

    /// Remember a removed project's repo so the scan leaves it be.
    pub(super) fn decline_repo(repo: &Path) {
        // Tests drive remove_project against the real data dir.
        if cfg!(test) {
            return;
        }
        decline_in(&declined_path(), repo);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo_with_worktrees(root: &Path, name: &str) -> PathBuf {
        let dir = root.join(name);
        std::fs::create_dir_all(dir.join(".git/worktrees/w1")).unwrap();
        dir
    }

    #[test]
    fn siblings_showing_agent_work_are_candidates_and_the_rest_are_not() {
        let root = tempfile::tempdir().unwrap();
        let registered = root.path().join("app");
        std::fs::create_dir_all(registered.join(".git")).unwrap();
        let other = repo_with_worktrees(root.path(), "other");
        let linked = root.path().join("app-audio");
        std::fs::create_dir_all(&linked).unwrap();
        std::fs::write(linked.join(".git"), "gitdir: x").unwrap();
        let quiet = root.path().join("quiet");
        std::fs::create_dir_all(quiet.join(".git")).unwrap();
        std::fs::create_dir_all(root.path().join("notes")).unwrap();

        let known = HashSet::from([key(&registered)]);
        let found = candidates(&[registered], &known, &HashSet::new());
        assert_eq!(found, vec![linked, other]);
    }

    #[test]
    fn known_and_declined_folders_are_left_out() {
        let root = tempfile::tempdir().unwrap();
        let registered = root.path().join("app");
        std::fs::create_dir_all(registered.join(".git")).unwrap();
        let a = repo_with_worktrees(root.path(), "a");
        let b = repo_with_worktrees(root.path(), "b");
        let known = HashSet::from([key(&registered), key(&a)]);
        let declined = HashSet::from([key(&b)]);
        assert!(candidates(&[registered], &known, &declined).is_empty());
    }

    #[test]
    fn a_declined_repo_is_remembered_across_reads() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("declined-repos.json");
        decline_in(&file, Path::new("C:\\Code\\Bro\\"));
        decline_in(&file, Path::new("C:/Code/bro"));
        assert_eq!(
            read_declined(&file),
            HashSet::from(["c:/code/bro".to_string()])
        );
    }
}
