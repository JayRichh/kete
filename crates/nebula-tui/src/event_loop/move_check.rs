//! The MOVE CHECK: before a session moves to another checkout, what stays
//! behind in the one it leaves. Uncommitted files and unpushed commits
//! belong to the checkout, not the session, so a move leaves them there.
//! A checkout holding either says so and asks; a clean one moves at once
//! (or, for a drag with **Confirm drag move** on, asks the usual question).
//!
//! The read is the DELETE CHECK's (`delete_check::read`), on the blocking
//! pool, landing through [`land`]. A read that fails moves without the
//! report: nothing is lost by a move, the work stays where it was. The
//! desktop builds the same line (`desktop/src/lib/move.ts`).

use super::launcher;
use crate::app::{App, ConfirmDialog, Overlay, PendingAction};
use crate::delete_check::Findings;
use nebula_core::{AgentId, ClientRequest, WorktreeId};

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// `3 changed files and 2 unpushed commits stay in feature-x.`, or None
/// for a clean checkout.
pub(crate) fn report(found: &Findings, branch: &str) -> Option<String> {
    let unpushed = found.unpushed.unwrap_or(0);
    let mut parts = Vec::new();
    if found.changes > 0 {
        parts.push(plural(found.changes, "changed file", "changed files"));
    }
    if unpushed > 0 {
        parts.push(plural(unpushed, "unpushed commit", "unpushed commits"));
    }
    if parts.is_empty() {
        return None;
    }
    let verb = if found.changes + unpushed == 1 {
        "stays"
    } else {
        "stay"
    };
    Some(format!("{} {verb} in {branch}.", parts.join(" and ")))
}

/// The checkout session `id` runs in: its path and branch.
fn home(app: &App, id: &AgentId) -> Option<(std::path::PathBuf, String)> {
    let agent = app.tree.agents.iter().find(|a| &a.id == id)?;
    let w = app
        .tree
        .worktrees
        .iter()
        .find(|w| w.id == agent.worktree_id)?;
    Some((w.path.clone(), w.branch.clone()))
}

/// Move session `id` to `worktree` once its checkout has been read. `ask`:
/// a drag with **Confirm drag move** on, which asks even when clean.
pub(super) fn start(
    app: &mut App,
    id: AgentId,
    worktree: WorktreeId,
    ask: bool,
    out: &mut Vec<ClientRequest>,
) {
    if launcher::move_plan(app, &id, &worktree).is_none() {
        return;
    }
    let Some((path, _)) = home(app, &id) else {
        return launcher::move_agent(app, id, worktree, out);
    };
    match app.jobs.view_jobs.clone() {
        Some(jobs) => jobs.run(move || {
            Some(crate::view_jobs::Answer::MoveCheck {
                agent: id,
                worktree,
                ask,
                result: crate::delete_check::read(&path),
            })
        }),
        None => {
            let result = crate::delete_check::read(&path);
            land(app, id, worktree, ask, result, out);
        }
    }
}

/// The read is back: ask with the report, ask the usual question, or move.
pub(super) fn land(
    app: &mut App,
    id: AgentId,
    worktree: WorktreeId,
    ask: bool,
    result: Result<Findings, String>,
    out: &mut Vec<ClientRequest>,
) {
    let Some((name, dest, _)) = launcher::move_plan(app, &id, &worktree) else {
        return;
    };
    let branch = home(app, &id).map(|(_, b)| b).unwrap_or_default();
    match result.ok().and_then(|f| report(&f, &branch)) {
        Some(line) => {
            app.modals.overlay = Some(Overlay::Confirm(ConfirmDialog {
                title: "Move session".into(),
                message: format!("{line} Move '{name}' to {dest} anyway?"),
                action: PendingAction::MoveAgent { id, worktree },
                area: ratatui::layout::Rect::default(),
            }));
            app.chrome.dirty = true;
        }
        None if ask => launcher::confirm_move(app, id, worktree),
        None => launcher::move_agent(app, id, worktree, out),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(changes: usize, unpushed: Option<usize>) -> Findings {
        Findings { changes, unpushed }
    }

    #[test]
    fn a_clean_checkout_has_nothing_to_report() {
        assert_eq!(report(&found(0, Some(0)), "main"), None);
        assert_eq!(report(&found(0, None), "main"), None);
    }

    #[test]
    fn the_report_counts_files_and_commits_and_names_the_branch() {
        assert_eq!(
            report(&found(3, Some(2)), "feature-x").as_deref(),
            Some("3 changed files and 2 unpushed commits stay in feature-x.")
        );
        assert_eq!(
            report(&found(1, None), "feature-x").as_deref(),
            Some("1 changed file stays in feature-x.")
        );
        assert_eq!(
            report(&found(0, Some(1)), "b").as_deref(),
            Some("1 unpushed commit stays in b.")
        );
    }
}
