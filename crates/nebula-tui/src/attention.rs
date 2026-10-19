//! The ATTENTION STRIP: across every project, how many sessions need you,
//! failed, are running and finished unread — the desktop's
//! `AttentionStrip.svelte`, counted the same way (`repos.ts` `tally` over
//! the SESSION TREE, so a running subagent counts as running and opens
//! the session it runs inside). Each count is a button in the LAUNCHER
//! VIEW's header (`ui::attention_view`): a press opens the next session in
//! that state, round and round.

use crate::app::App;
use crate::subagents::{build_forest, walk_all, Tone};
use nebula_core::{Agent, AgentId};
use std::collections::HashMap;

/// The strip's counts, in the order it draws them. `Failed` only shows
/// when something failed.
pub const KINDS: [Tone; 4] = [Tone::Needs, Tone::Failed, Tone::Running, Tone::Unread];

/// The word after a count: the desktop's.
pub fn word(tone: Tone, n: usize) -> &'static str {
    match tone {
        Tone::Needs if n == 1 => "needs you",
        Tone::Needs => "need you",
        other => other.word(),
    }
}

/// Every live (unarchived) session and subagent the bench's scope admits
/// (`LauncherState::scope`), by tone, as the sessions a press opens — in
/// project order, each project's tree depth first.
pub fn tally(app: &App) -> HashMap<Tone, Vec<AgentId>> {
    let scope = &crate::launcher::effective_scope(app);
    let mut out: HashMap<Tone, Vec<AgentId>> = HashMap::new();
    for p in &app.tree.projects {
        let agents: Vec<&Agent> = app
            .tree
            .agents
            .iter()
            .filter(|a| !a.archived && scope.admits(a, Some(&p.id)))
            .filter(|a| {
                app.tree
                    .worktrees
                    .iter()
                    .any(|w| w.id == a.worktree_id && w.project_id == p.id)
            })
            .collect();
        // Every ended subagent counts on its own, folded or not.
        let forest = build_forest(&agents, &app.desk.subagents, false);
        for n in walk_all(&forest) {
            out.entry(n.tone).or_default().push(n.opens.clone());
        }
    }
    out
}

/// Where each count's round-robin stands.
#[derive(Debug, Clone, Default)]
pub struct Turns {
    at: HashMap<Tone, usize>,
}

impl Turns {
    /// The next session in `tone`'s list, round and round; None when the
    /// list is empty.
    pub fn next(&mut self, tone: Tone, list: &[AgentId]) -> Option<AgentId> {
        if list.is_empty() {
            return None;
        }
        let i = self.at.get(&tone).map_or(0, |i| (i + 1) % list.len());
        self.at.insert(tone, i);
        list.get(i).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_count_walks_its_sessions_round_and_round() {
        let list = vec![AgentId("a".into()), AgentId("b".into())];
        let mut turns = Turns::default();
        let walk: Vec<String> = (0..3)
            .filter_map(|_| turns.next(Tone::Needs, &list))
            .map(|id| id.0)
            .collect();
        assert_eq!(walk, vec!["a", "b", "a"]);
        assert_eq!(turns.next(Tone::Running, &[]), None);
    }

    #[test]
    fn the_words_are_the_desktops() {
        assert_eq!(word(Tone::Needs, 1), "needs you");
        assert_eq!(word(Tone::Needs, 2), "need you");
        assert_eq!(word(Tone::Unread, 2), "unread");
    }
}
