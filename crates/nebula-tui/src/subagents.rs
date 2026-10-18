//! Claude's in-session SUBAGENTS (the Task / Agent tool) and the SESSION
//! TREE the desktop's overview draws from them: sessions nested under the
//! session that spawned them (`Agent::parent`), subagents under the
//! session they run inside. The daemon reports subagents in the snapshot
//! and as `ServerEvent::SubagentChanged`; [`Subagents`] mirrors that book.
//! The tree is pure here (`build_forest`, `flatten`), so its rules are
//! tested without a screen; `ui::subagents_view` draws the card chip, the
//! pane header's chip and the SUBAGENTS OVERLAY, and
//! `event_loop::subagents` takes their keys and clicks.
//!
//! The ordering and the open-by-default depth are the desktop's
//! (`desktop/src/lib/overview/tree.ts`), so the two windows nest the same
//! sessions the same way.

use nebula_core::{Agent, AgentId, AgentStatus, ProjectId, SubagentStatus, SubagentSummary};
use ratatui::layout::Rect;
use std::collections::{HashMap, HashSet};

/// The daemon's subagents, by the session they run inside, oldest first.
#[derive(Debug, Clone)]
pub struct Subagents {
    by_parent: HashMap<AgentId, Vec<SubagentSummary>>,
    /// The `hide_done_subagents` SETTING: ended subagents fold into a
    /// FINISHED GROUP under their session instead of showing one by one.
    pub hide_done: bool,
}

impl Default for Subagents {
    fn default() -> Self {
        Self {
            by_parent: HashMap::new(),
            hide_done: true,
        }
    }
}

impl Subagents {
    /// One `SubagentsRemoved`: the daemon dropped these rows of `parent`
    /// (a clear, its retention sweep, the session restarting).
    pub fn remove(&mut self, parent: &AgentId, ids: &[String]) {
        if let Some(list) = self.by_parent.get_mut(parent) {
            list.retain(|s| !ids.contains(&s.id));
            if list.is_empty() {
                self.by_parent.remove(parent);
            }
        }
    }

    /// Ended subagents of `parent`: what the FINISHED GROUP holds and a
    /// clear would drop.
    pub fn ended(&self, parent: &AgentId) -> usize {
        self.of(parent)
            .iter()
            .filter(|s| s.status != SubagentStatus::Running)
            .count()
    }

    /// Ended subagents across every session.
    pub fn ended_total(&self) -> usize {
        self.by_parent.keys().map(|p| self.ended(p)).sum()
    }

    /// A snapshot's whole list: replaces everything known.
    pub fn replace(&mut self, all: Vec<SubagentSummary>) {
        self.by_parent.clear();
        for sub in all {
            self.upsert(sub);
        }
    }

    /// One `SubagentChanged`: the whole row, keyed by `(parent_agent, id)`.
    /// Ended ones past [`nebula_core::SUBAGENTS_ENDED_KEPT`] per session
    /// go oldest first, as they do in the daemon's book.
    pub fn upsert(&mut self, sub: SubagentSummary) {
        let list = self.by_parent.entry(sub.parent_agent.clone()).or_default();
        match list.iter_mut().find(|s| s.id == sub.id) {
            Some(slot) => *slot = sub,
            None => list.push(sub),
        }
        list.sort_by_key(|s| s.started_at);
        let mut ended: Vec<(i64, String)> = list
            .iter()
            .filter(|s| s.status != SubagentStatus::Running)
            .map(|s| (s.ended_at.unwrap_or(s.started_at), s.id.clone()))
            .collect();
        if ended.len() > nebula_core::SUBAGENTS_ENDED_KEPT {
            ended.sort();
            let over = ended.len() - nebula_core::SUBAGENTS_ENDED_KEPT;
            let gone: HashSet<String> = ended.into_iter().take(over).map(|(_, id)| id).collect();
            list.retain(|s| !gone.contains(&s.id));
        }
    }
    /// The subagents of `parent`, oldest first.
    pub fn of(&self, parent: &AgentId) -> &[SubagentSummary] {
        self.by_parent.get(parent).map_or(&[], Vec::as_slice)
    }

    /// `(running, all)` of `parent`'s subagents.
    pub fn counts(&self, parent: &AgentId) -> (usize, usize) {
        let subs = self.of(parent);
        let running = subs
            .iter()
            .filter(|s| s.status == SubagentStatus::Running)
            .count();
        (running, subs.len())
    }
}

/// A row's state in the desktop's words (`status.ts` `Tone`): what the
/// ATTENTION STRIP counts and what colors a SESSION TREE row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tone {
    Needs,
    Failed,
    Unread,
    Running,
    Done,
    Stopped,
    Idle,
}

impl Tone {
    /// The word a row goes by beside its dot.
    pub fn word(self) -> &'static str {
        match self {
            Tone::Needs => "needs you",
            Tone::Failed => "failed",
            Tone::Unread => "unread",
            Tone::Running => "running",
            Tone::Done => "done",
            Tone::Stopped => "stopped",
            Tone::Idle => "idle",
        }
    }
}

/// A session's tone: the desktop's `toneOf`. An archived one wants
/// nothing and reads as stopped.
pub fn agent_tone(a: &Agent) -> Tone {
    if a.archived {
        return Tone::Stopped;
    }
    match a.status {
        AgentStatus::NeedsFeedback => Tone::Needs,
        AgentStatus::Running => Tone::Running,
        AgentStatus::Finished if a.unseen => Tone::Unread,
        AgentStatus::Finished => Tone::Done,
        AgentStatus::Terminated | AgentStatus::Disconnected if a.unseen => Tone::Unread,
        AgentStatus::Terminated | AgentStatus::Disconnected => Tone::Stopped,
        AgentStatus::Fresh => Tone::Idle,
    }
}

/// A subagent's tone: the desktop's `subagentTone`.
pub fn sub_tone(s: &SubagentSummary) -> Tone {
    match s.status {
        SubagentStatus::Running => Tone::Running,
        SubagentStatus::Done => Tone::Done,
        SubagentStatus::Failed => Tone::Failed,
        SubagentStatus::EndedUnknown => Tone::Stopped,
    }
}

/// The word a subagent's row goes by: `closed` for one still open when
/// its session ended or restarted, which says more than `stopped`.
pub fn sub_word(s: &SubagentSummary) -> &'static str {
    match s.status {
        SubagentStatus::Running => "running",
        SubagentStatus::Done => "done",
        SubagentStatus::Failed => "failed",
        SubagentStatus::EndedUnknown => "closed",
    }
}

/// One node of the SESSION TREE.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    /// Unique across the forest: `a:<id>` for a session, `s:<parent>:<id>`
    /// for a subagent (subagent ids repeat across sessions).
    pub key: String,
    pub depth: usize,
    pub tone: Tone,
    /// A session's name; a subagent's task, else its type.
    pub label: String,
    /// The dim words after it: a subagent's type (the indent and the
    /// lighter name already say "subagent"), a session's harness.
    pub meta: String,
    /// Epoch ms the row last moved: what its "3m" reads.
    pub when: i64,
    /// The session Enter on the row opens: its own, or for a subagent
    /// the session it runs inside.
    pub opens: AgentId,
    /// Set on a session row.
    pub session: bool,
    /// The word beside the dot: the tone's, or a subagent's own
    /// ([`sub_word`]), or the FINISHED GROUP's.
    pub word: &'static str,
    /// Starts folded whatever its depth: the FINISHED GROUP.
    pub folded: bool,
    pub children: Vec<Node>,
}

/// Children deeper than this start folded (the desktop's `OPEN_DEPTH`).
pub const OPEN_DEPTH: usize = 2;

/// `agents` nested by `parent`, each session's subagents under it — the
/// desktop's `buildForest`. Sessions in sort order (then by when they
/// last moved), subagents oldest first; a parent outside `agents`, or a
/// parent chain that loops, leaves the session a root. With `hide_done`
/// a session's ended subagents go under one FINISHED GROUP node, `3
/// finished`, which starts folded; its running ones stay one by one.
pub fn build_forest(agents: &[&Agent], subs: &Subagents, hide_done: bool) -> Vec<Node> {
    let by_id: HashMap<&AgentId, &Agent> = agents.iter().map(|a| (&a.id, *a)).collect();
    let attachable = |a: &Agent| -> bool {
        let mut seen: HashSet<&AgentId> = HashSet::from([&a.id]);
        let mut p = a.parent.as_ref();
        while let Some(id) = p {
            if !seen.insert(id) {
                return false;
            }
            match by_id.get(id) {
                Some(up) => p = up.parent.as_ref(),
                None => return true,
            }
        }
        true
    };
    let mut ordered: Vec<&Agent> = agents.to_vec();
    ordered.sort_by_key(|a| (a.sort_order, a.status_changed_at));
    // Who hangs under whom, decided first; the nodes are built bottom-up
    // from it.
    let mut kids: HashMap<AgentId, Vec<&Agent>> = HashMap::new();
    let mut roots: Vec<&Agent> = Vec::new();
    for a in &ordered {
        match a.parent.as_ref() {
            Some(p) if p != &a.id && by_id.contains_key(p) && attachable(a) => {
                kids.entry(p.clone()).or_default().push(a)
            }
            _ => roots.push(a),
        }
    }
    fn sub_node(s: &SubagentSummary, depth: usize, opens: &AgentId) -> Node {
        Node {
            key: format!("s:{}:{}", s.parent_agent.0, s.id),
            depth,
            tone: sub_tone(s),
            label: sub_label(s),
            meta: if s.kind.is_empty() {
                "subagent".into()
            } else {
                s.kind.clone()
            },
            when: s.ended_at.unwrap_or(s.started_at),
            opens: opens.clone(),
            session: false,
            word: sub_word(s),
            folded: false,
            children: Vec::new(),
        }
    }
    fn node(
        a: &Agent,
        depth: usize,
        kids: &HashMap<AgentId, Vec<&Agent>>,
        subs: &Subagents,
        hide_done: bool,
    ) -> Node {
        let mut children: Vec<Node> = kids
            .get(&a.id)
            .map(|k| {
                k.iter()
                    .map(|c| node(c, depth + 1, kids, subs, hide_done))
                    .collect()
            })
            .unwrap_or_default();
        let mut finished = Vec::new();
        for s in subs.of(&a.id) {
            if hide_done && s.status != SubagentStatus::Running {
                finished.push(sub_node(s, depth + 2, &a.id));
            } else {
                children.push(sub_node(s, depth + 1, &a.id));
            }
        }
        if !finished.is_empty() {
            children.push(Node {
                key: format!("f:{}", a.id.0),
                depth: depth + 1,
                tone: Tone::Done,
                label: format!("{} finished", finished.len()),
                meta: "subagents".into(),
                when: finished.iter().map(|n| n.when).max().unwrap_or(0),
                opens: a.id.clone(),
                session: false,
                word: "",
                folded: true,
                children: finished,
            });
        }
        Node {
            key: format!("a:{}", a.id.0),
            depth,
            tone: agent_tone(a),
            label: a.name.clone(),
            meta: a.kind.as_str().to_string(),
            when: a.status_changed_at,
            opens: a.id.clone(),
            session: true,
            word: agent_tone(a).word(),
            folded: false,
            children,
        }
    }
    roots
        .iter()
        .map(|a| node(a, 0, &kids, subs, hide_done))
        .collect()
}

/// What a subagent's row is called: the task it was given, else its type.
pub fn sub_label(s: &SubagentSummary) -> String {
    if !s.description.is_empty() {
        s.description.clone()
    } else if !s.kind.is_empty() {
        s.kind.clone()
    } else {
        "subagent".into()
    }
}

/// One drawn row of the tree.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub node: Node,
    /// Whether its children are showing.
    pub expanded: bool,
    /// Descendants a fold hides, for `+3`.
    pub hidden: usize,
}

impl Row {
    pub fn has_kids(&self) -> bool {
        !self.node.children.is_empty()
    }
}

fn count(n: &Node) -> usize {
    n.children.iter().map(|c| 1 + count(c)).sum()
}

/// The rows to draw, depth first — the desktop's `flatten`. A node's
/// children show while they sit at [`OPEN_DEPTH`] or shallower, unless the
/// user opened or closed that node by key.
pub fn flatten(forest: &[Node], open: &HashSet<String>, closed: &HashSet<String>) -> Vec<Row> {
    fn walk(n: &Node, open: &HashSet<String>, closed: &HashSet<String>, out: &mut Vec<Row>) {
        let has = !n.children.is_empty();
        let expanded = has
            && !closed.contains(&n.key)
            && (open.contains(&n.key) || (!n.folded && n.depth < OPEN_DEPTH));
        out.push(Row {
            node: n.clone(),
            expanded,
            hidden: if has && !expanded { count(n) } else { 0 },
        });
        if expanded {
            for c in &n.children {
                walk(c, open, closed, out);
            }
        }
    }
    let mut out = Vec::new();
    for r in forest {
        walk(r, open, closed, &mut out);
    }
    out
}

/// Every node, depth first — the desktop's `walkAll`.
pub fn walk_all(forest: &[Node]) -> Vec<&Node> {
    fn walk<'a>(n: &'a Node, out: &mut Vec<&'a Node>) {
        out.push(n);
        for c in &n.children {
            walk(c, out);
        }
    }
    let mut out = Vec::new();
    for n in forest {
        walk(n, &mut out);
    }
    out
}

/// The SUBAGENTS OVERLAY (`⇧S`, or a click on a card's `⑂` chip): the
/// project's SESSION TREE, the desktop overview's, with each session's
/// subagents nested under it.
#[derive(Debug, Clone, Default)]
pub struct SubagentsView {
    /// The project the tree is of; None is every project.
    pub project: Option<ProjectId>,
    /// Nodes the user opened or closed by hand, by key.
    pub open: HashSet<String>,
    pub closed: HashSet<String>,
    /// The rows as last built ([`SubagentsView::rebuild`]).
    pub rows: Vec<Row>,
    pub cursor: usize,
    /// Whole modal rect and the rows' rect, written back during draw.
    pub area: Rect,
    pub list_area: Rect,
    /// First row drawn, written back during draw.
    pub scroll: usize,
}

impl SubagentsView {
    /// Rebuild the rows from `forest`, keeping the cursor on the node it
    /// was on where that node still shows.
    pub fn rebuild(&mut self, forest: &[Node]) {
        let keep = self.rows.get(self.cursor).map(|r| r.node.key.clone());
        self.rows = flatten(forest, &self.open, &self.closed);
        if let Some(key) = keep {
            if let Some(i) = self.rows.iter().position(|r| r.node.key == key) {
                self.cursor = i;
            }
        }
        self.cursor = self.cursor.min(self.rows.len().saturating_sub(1));
    }

    /// Put the cursor on `key`'s row when it shows.
    pub fn land_on(&mut self, key: &str) {
        if let Some(i) = self.rows.iter().position(|r| r.node.key == key) {
            self.cursor = i;
        }
    }

    pub fn step(&mut self, delta: i64) {
        self.cursor = crate::app::clamp_selection(self.cursor as i64 + delta, self.rows.len());
    }

    /// Open (`true`) or fold the row under the cursor; false when that
    /// changed nothing (no children, or already that way).
    pub fn set_open(&mut self, open: bool) -> bool {
        let Some(row) = self.rows.get(self.cursor) else {
            return false;
        };
        if !row.has_kids() || row.expanded == open {
            return false;
        }
        let key = row.node.key.clone();
        if open {
            self.closed.remove(&key);
            self.open.insert(key);
        } else {
            self.open.remove(&key);
            self.closed.insert(key);
        }
        true
    }

    /// The row the cursor's row hangs under, for `←` on a leaf.
    pub fn parent_of_cursor(&self) -> Option<usize> {
        let depth = self.rows.get(self.cursor)?.node.depth;
        (0..self.cursor)
            .rev()
            .find(|&i| self.rows[i].node.depth < depth)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use nebula_core::WorktreeId;

    pub(crate) fn agent(id: &str, parent: Option<&str>, status: AgentStatus, order: i64) -> Agent {
        Agent {
            parent: parent.map(|p| AgentId(p.into())),
            profile: None,
            id: AgentId(id.into()),
            worktree_id: WorktreeId("w1".into()),
            name: id.into(),
            status,
            archived: false,
            archived_at: 0,
            unseen: false,
            kind: nebula_core::AgentKind::Claude,
            custom_harness: None,
            model: None,
            effort: None,
            session_id: None,
            cloud_session_id: None,
            sort_order: order,
            status_changed_at: 0,
            alive: true,
            issue_url: None,
            role: nebula_core::AgentRole::Worker,
            recent_prompts: Vec::new(),
        }
    }
    pub(crate) fn sub(parent: &str, id: &str, status: SubagentStatus, at: i64) -> SubagentSummary {
        SubagentSummary {
            id: id.into(),
            parent_agent: AgentId(parent.into()),
            kind: "Explore".into(),
            description: format!("task {id}"),
            started_at: at,
            ended_at: (status != SubagentStatus::Running).then_some(at + 1),
            status,
        }
    }

    #[test]
    fn a_change_replaces_its_row_and_keys_by_parent_and_id() {
        let mut book = Subagents::default();
        book.upsert(sub("a", "x", SubagentStatus::Running, 1));
        book.upsert(sub("b", "x", SubagentStatus::Running, 2));
        book.upsert(sub("a", "x", SubagentStatus::Done, 1));
        assert_eq!(book.counts(&AgentId("a".into())), (0, 1));
        assert_eq!(book.counts(&AgentId("b".into())), (1, 1));
    }

    #[test]
    fn ended_subagents_past_the_kept_count_go_oldest_first() {
        let mut book = Subagents::default();
        let kept = nebula_core::SUBAGENTS_ENDED_KEPT;
        for i in 0..kept + 3 {
            book.upsert(sub(
                "a",
                &format!("e{i}"),
                SubagentStatus::Done,
                i as i64 * 10,
            ));
        }
        book.upsert(sub("a", "live", SubagentStatus::Running, 0));
        let ids: Vec<&str> = book
            .of(&AgentId("a".into()))
            .iter()
            .map(|s| s.id.as_str())
            .collect();
        assert_eq!(ids.len(), kept + 1);
        assert!(!ids.contains(&"e0") && !ids.contains(&"e2"), "{ids:?}");
        assert!(ids.contains(&"e3") && ids.contains(&"live"), "{ids:?}");
    }

    #[test]
    fn sessions_nest_under_their_parent_and_subagents_under_their_session() {
        let root = agent("root", None, AgentStatus::Running, 0);
        let kid = agent("kid", Some("root"), AgentStatus::NeedsFeedback, 1);
        // A loop leaves both ends roots rather than vanishing.
        let x = agent("x", Some("y"), AgentStatus::Fresh, 2);
        let y = agent("y", Some("x"), AgentStatus::Fresh, 3);
        let mut book = Subagents::default();
        book.upsert(sub("kid", "s1", SubagentStatus::Running, 5));
        let forest = build_forest(&[&root, &kid, &x, &y], &book, false);
        let keys: Vec<(&str, usize)> = walk_all(&forest)
            .iter()
            .map(|n| (n.key.as_str(), n.depth))
            .collect();
        assert_eq!(
            keys,
            vec![
                ("a:root", 0),
                ("a:kid", 1),
                ("s:kid:s1", 2),
                ("a:x", 0),
                ("a:y", 0)
            ]
        );
        let s1 = walk_all(&forest)[2];
        assert_eq!(s1.opens, AgentId("kid".into()));
        assert_eq!(s1.tone, Tone::Running);
        assert_eq!(s1.label, "task s1");
        assert_eq!(s1.meta, "Explore");
    }

    #[test]
    fn deep_rows_start_folded_and_a_hand_fold_sticks() {
        let a = agent("a", None, AgentStatus::Running, 0);
        let b = agent("b", Some("a"), AgentStatus::Running, 1);
        let c = agent("c", Some("b"), AgentStatus::Running, 2);
        let mut book = Subagents::default();
        book.upsert(sub("c", "s", SubagentStatus::Running, 1));
        let forest = build_forest(&[&a, &b, &c], &book, true);
        let rows = flatten(&forest, &HashSet::new(), &HashSet::new());
        // c sits at depth 2: shown, but its own children (depth 3) wait.
        let keys: Vec<&str> = rows.iter().map(|r| r.node.key.as_str()).collect();
        assert_eq!(keys, vec!["a:a", "a:b", "a:c"]);
        assert_eq!(rows[2].hidden, 1);

        let mut view = SubagentsView::default();
        view.rebuild(&forest);
        view.cursor = 2;
        assert!(view.set_open(true));
        view.rebuild(&forest);
        assert_eq!(view.rows.len(), 4);
        view.cursor = 0;
        assert!(view.set_open(false));
        view.rebuild(&forest);
        assert_eq!(view.rows.len(), 1);
        assert_eq!(view.rows[0].hidden, 3);
    }

    /// Every subagent is in the tree; with `hide_done` the ended ones
    /// sit under one folded `N finished` node, the running ones beside it.
    #[test]
    fn ended_subagents_fold_into_a_finished_group() {
        let root = agent("root", None, AgentStatus::Running, 0);
        let mut book = Subagents::default();
        assert!(book.hide_done, "folded by default");
        book.upsert(sub("root", "live", SubagentStatus::Running, 1));
        book.upsert(sub("root", "ok", SubagentStatus::Done, 2));
        book.upsert(sub("root", "lost", SubagentStatus::EndedUnknown, 3));

        let forest = build_forest(&[&root], &book, true);
        let keys: Vec<&str> = walk_all(&forest).iter().map(|n| n.key.as_str()).collect();
        assert_eq!(
            keys,
            vec![
                "a:root",
                "s:root:live",
                "f:root",
                "s:root:ok",
                "s:root:lost"
            ],
            "nothing is left out of the tree"
        );
        let group = walk_all(&forest)[2];
        assert_eq!(group.label, "2 finished");
        assert_eq!(walk_all(&forest)[4].word, "closed");

        let rows = flatten(&forest, &HashSet::new(), &HashSet::new());
        let shown: Vec<&str> = rows.iter().map(|r| r.node.key.as_str()).collect();
        assert_eq!(shown, vec!["a:root", "s:root:live", "f:root"]);
        assert_eq!(rows[2].hidden, 2, "the count badge");

        let open = HashSet::from(["f:root".to_string()]);
        assert_eq!(flatten(&forest, &open, &HashSet::new()).len(), 5);

        let flat = build_forest(&[&root], &book, false);
        assert_eq!(flatten(&flat, &HashSet::new(), &HashSet::new()).len(), 4);
    }

    #[test]
    fn a_removal_drops_only_the_named_rows() {
        let mut book = Subagents::default();
        book.upsert(sub("a", "x", SubagentStatus::Done, 1));
        book.upsert(sub("a", "y", SubagentStatus::Running, 2));
        book.upsert(sub("b", "x", SubagentStatus::Done, 3));
        assert_eq!(book.ended_total(), 2);
        book.remove(&AgentId("a".into()), &["x".to_string()]);
        assert_eq!(book.counts(&AgentId("a".into())), (1, 1));
        assert_eq!(
            book.ended(&AgentId("b".into())),
            1,
            "another session's x stays"
        );
    }

    #[test]
    fn tones_are_the_desktops() {
        let mut a = agent("a", None, AgentStatus::Finished, 0);
        assert_eq!(agent_tone(&a), Tone::Done);
        a.unseen = true;
        assert_eq!(agent_tone(&a), Tone::Unread);
        a.status = AgentStatus::Terminated;
        assert_eq!(agent_tone(&a), Tone::Unread);
        a.unseen = false;
        assert_eq!(agent_tone(&a), Tone::Stopped);
        a.archived = true;
        a.status = AgentStatus::NeedsFeedback;
        assert_eq!(agent_tone(&a), Tone::Stopped);
        assert_eq!(
            sub_tone(&sub("a", "f", SubagentStatus::Failed, 0)),
            Tone::Failed
        );
    }
}
