//! STATE MAP (K10): one graph per studio repo of what exists and who is on
//! it: departments, systems, decisions, lanes, pull requests, files and
//! agents, joined by owns, depends on, touched by and blocked by.
//!
//! Two layers. The repo layer comes from the repo's own extractor, a
//! command named in `.kete/studio.toml` under `[state_map]` ([`extractor`])
//! that prints a [`Layer`] as JSON. The live layer is what Kete knows: the
//! repo's worktrees, open pull requests, sessions and subagents
//! ([`live_layer`]). [`merge`] joins them into a [`StateMap`], which the
//! daemon writes to `<repo>/.kete/state-map.json` ([`write`]) and every
//! client reads back ([`read`]). [`tree`] is the hierarchy both apps draw
//! and [`slice`] the compact text an agent reads, at most
//! [`SLICE_MAX_BYTES`].
//!
//! This file uses std, serde, serde_json and toml only, so its tests run
//! on their own (`tests/state_map.rs`).

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

/// The map's path, relative to the repo root. Kept out of git by the repo.
pub const MAP_PATH: &str = ".kete/state-map.json";
/// A slice is cut to fit this many bytes.
pub const SLICE_MAX_BYTES: usize = 2048;
/// An extractor's output over this many bytes is refused.
pub const MAX_LAYER_BYTES: usize = 4 * 1024 * 1024;
/// A map file over this many bytes is not read.
pub const MAX_MAP_BYTES: u64 = 8 * 1024 * 1024;
/// A rebuild waits at least this long after the last one, so a burst of
/// events is one rebuild.
pub const MIN_GAP_MS: i64 = 5_000;
/// A map older than this is rebuilt with or without an event.
pub const EVERY_MS: i64 = 60_000;
/// A label longer than this many characters is cut in a slice.
const MAX_LABEL_CHARS: usize = 100;

/// One repo's state graph.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateMap {
    /// The repo root, `/`-separated.
    pub repo: String,
    /// When it was built, epoch ms.
    pub built_at: i64,
    #[serde(default)]
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub edges: Vec<Edge>,
    /// Why the repo layer is missing, when it is: no extractor, or what it
    /// said when it failed.
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    Department,
    System,
    Subsystem,
    Decision,
    Lane,
    Pr,
    File,
    Agent,
}

impl NodeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            NodeKind::Department => "department",
            NodeKind::System => "system",
            NodeKind::Subsystem => "subsystem",
            NodeKind::Decision => "decision",
            NodeKind::Lane => "lane",
            NodeKind::Pr => "pr",
            NodeKind::File => "file",
            NodeKind::Agent => "agent",
        }
    }
}

/// One thing in the map.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Node {
    /// The repo's own id where it has one (`D-012`); Kete's live nodes use
    /// `dept:<slug>`, `lane:<branch>`, `pr:<number>` and `agent:<id>`.
    pub id: String,
    pub kind: NodeKind,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub owner: Option<String>,
    /// The last change, epoch ms.
    #[serde(default)]
    pub changed_at: Option<i64>,
    /// Open questions on it.
    #[serde(default)]
    pub questions: u32,
    /// Paths (repo-relative) or URLs.
    #[serde(default)]
    pub links: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    Owns,
    DependsOn,
    TouchedBy,
    BlockedBy,
}

/// `from` owns, depends on, is touched by, or is blocked by `to`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub kind: EdgeKind,
}

/// Nodes and edges from one source: what an extractor prints.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Layer {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

/// The `extractor` argv under `[state_map]` in a studio manifest's text,
/// for example `extractor = ["python", "Tools/Studio/state_map.py"]`.
/// None when there is none, it is empty, or the file does not parse.
pub fn extractor(manifest_text: &str) -> Option<Vec<String>> {
    #[derive(Deserialize)]
    struct File {
        state_map: Option<Section>,
    }
    #[derive(Deserialize)]
    struct Section {
        extractor: Option<Vec<String>>,
    }
    let text = manifest_text
        .strip_prefix('\u{feff}')
        .unwrap_or(manifest_text);
    let argv = toml::from_str::<File>(text).ok()?.state_map?.extractor?;
    (!argv.is_empty() && !argv[0].trim().is_empty()).then_some(argv)
}

/// An extractor's stdout as a layer: `{"nodes": [...], "edges": [...]}`.
/// A node or edge this build can't read (an unknown kind, a missing id) is
/// left out, not fatal; output that is not that object is an error.
pub fn parse_layer(text: &str) -> Result<Layer, String> {
    if text.len() > MAX_LAYER_BYTES {
        return Err(format!(
            "the extractor printed over {} MiB",
            MAX_LAYER_BYTES >> 20
        ));
    }
    let value: Value = serde_json::from_str(text.trim_start_matches('\u{feff}'))
        .map_err(|e| format!("the extractor did not print JSON: {e}"))?;
    let Value::Object(obj) = value else {
        return Err("the extractor printed JSON that is not an object".into());
    };
    fn each<T: serde::de::DeserializeOwned>(v: Option<&Value>) -> Vec<T> {
        v.and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| serde_json::from_value(item.clone()).ok())
                    .collect()
            })
            .unwrap_or_default()
    }
    Ok(Layer {
        nodes: each::<Node>(obj.get("nodes"))
            .into_iter()
            .filter(|n| !n.id.trim().is_empty())
            .collect(),
        edges: each(obj.get("edges")),
    })
}

/// A worktree of the repo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveLane {
    pub branch: String,
    /// `/`-separated.
    pub path: String,
}

/// A session in one of the repo's worktrees.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveAgent {
    pub id: String,
    pub name: String,
    /// `running`, `finished`, `needs_feedback` and so on.
    pub status: String,
    /// The branch of the worktree it runs in.
    pub branch: String,
    pub changed_at: i64,
    /// Its role card's label, when it has one.
    pub role: Option<String>,
    /// Its role's department slug.
    pub department: Option<String>,
}

/// A subagent of a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveSubagent {
    pub id: String,
    pub parent: String,
    pub description: String,
    pub status: String,
    pub changed_at: i64,
}

/// An open pull request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LivePr {
    pub number: u64,
    pub title: String,
    /// Its head branch.
    #[serde(rename = "headRefName")]
    pub head: String,
    #[serde(default)]
    pub url: String,
    #[serde(default, rename = "isDraft")]
    pub draft: bool,
}

/// What Kete knows about a repo right now.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LiveInputs {
    /// `(slug, label)`, from the manifest's departments table.
    pub departments: Vec<(String, String)>,
    pub lanes: Vec<LiveLane>,
    pub agents: Vec<LiveAgent>,
    pub subagents: Vec<LiveSubagent>,
    pub prs: Vec<LivePr>,
}

fn node(id: String, kind: NodeKind, label: String, status: &str) -> Node {
    Node {
        id,
        kind,
        label,
        status: status.into(),
        owner: None,
        changed_at: None,
        questions: 0,
        links: Vec::new(),
    }
}

fn edge(from: &str, to: &str, kind: EdgeKind) -> Edge {
    Edge {
        from: from.into(),
        to: to.into(),
        kind,
    }
}

/// The live layer. A lane owns the agents in its worktree and the pull
/// request from its branch; an agent owns its subagents; a department is
/// touched by the agents whose role belongs to it. A lane is `active`
/// while one of its agents runs or waits on you, else `idle`.
pub fn live_layer(live: &LiveInputs) -> Layer {
    let mut out = Layer::default();
    for (slug, label) in &live.departments {
        out.nodes.push(node(
            format!("dept:{slug}"),
            NodeKind::Department,
            label.clone(),
            "",
        ));
    }
    for lane in &live.lanes {
        let busy = live
            .agents
            .iter()
            .any(|a| a.branch == lane.branch && matches!(&*a.status, "running" | "needs_feedback"));
        let mut n = node(
            format!("lane:{}", lane.branch),
            NodeKind::Lane,
            lane.branch.clone(),
            if busy { "active" } else { "idle" },
        );
        n.links.push(lane.path.clone());
        out.nodes.push(n);
    }
    for agent in &live.agents {
        let id = format!("agent:{}", agent.id);
        let mut n = node(
            id.clone(),
            NodeKind::Agent,
            agent.name.clone(),
            &agent.status,
        );
        n.owner = agent.role.clone();
        n.changed_at = Some(agent.changed_at);
        out.nodes.push(n);
        out.edges
            .push(edge(&format!("lane:{}", agent.branch), &id, EdgeKind::Owns));
        if let Some(dept) = &agent.department {
            out.edges
                .push(edge(&format!("dept:{dept}"), &id, EdgeKind::TouchedBy));
        }
    }
    for sub in &live.subagents {
        let parent = format!("agent:{}", sub.parent);
        let id = format!("{parent}/{}", sub.id);
        let mut n = node(
            id.clone(),
            NodeKind::Agent,
            sub.description.clone(),
            &sub.status,
        );
        n.changed_at = Some(sub.changed_at);
        out.nodes.push(n);
        out.edges.push(edge(&parent, &id, EdgeKind::Owns));
    }
    for pr in &live.prs {
        let id = format!("pr:{}", pr.number);
        let mut n = node(
            id.clone(),
            NodeKind::Pr,
            format!("#{} {}", pr.number, pr.title),
            if pr.draft { "draft" } else { "open" },
        );
        if !pr.url.is_empty() {
            n.links.push(pr.url.clone());
        }
        out.nodes.push(n);
        out.edges
            .push(edge(&format!("lane:{}", pr.head), &id, EdgeKind::Owns));
    }
    out
}

/// Joins the repo layer and the live layer. A node in both keeps the
/// repo's label and kind and takes the live status, owner and last change
/// where the live side has one; links are the union. Nodes are sorted by
/// kind, then id. An edge whose end is not a node, an edge to itself, and
/// a repeat are dropped.
pub fn merge(repo: &str, built_at: i64, from_repo: Layer, live: Layer) -> StateMap {
    let mut nodes: BTreeMap<String, Node> = BTreeMap::new();
    for n in from_repo.nodes {
        nodes.entry(n.id.clone()).or_insert(n);
    }
    for n in live.nodes {
        match nodes.get_mut(&n.id) {
            None => {
                nodes.insert(n.id.clone(), n);
            }
            Some(have) => {
                if !n.status.is_empty() {
                    have.status = n.status;
                }
                if n.owner.is_some() {
                    have.owner = n.owner;
                }
                have.changed_at = have.changed_at.max(n.changed_at);
                if have.label.is_empty() {
                    have.label = n.label;
                }
                for link in n.links {
                    if !have.links.contains(&link) {
                        have.links.push(link);
                    }
                }
            }
        }
    }
    let mut seen = BTreeSet::new();
    let edges = from_repo
        .edges
        .into_iter()
        .chain(live.edges)
        .filter(|e| e.from != e.to && nodes.contains_key(&e.from) && nodes.contains_key(&e.to))
        .filter(|e| seen.insert(e.clone()))
        .collect();
    let mut nodes: Vec<Node> = nodes.into_values().collect();
    nodes.sort_by(|a, b| (a.kind, &a.id).cmp(&(b.kind, &b.id)));
    StateMap {
        repo: repo.into(),
        built_at,
        nodes,
        edges,
        note: None,
    }
}

/// Whether the daemon should rebuild now: never built, an event since the
/// last build and [`MIN_GAP_MS`] gone, or [`EVERY_MS`] gone.
pub fn rebuild_due(now: i64, last_built: Option<i64>, dirty: bool) -> bool {
    match last_built {
        None => true,
        Some(at) => now - at >= EVERY_MS || (dirty && now - at >= MIN_GAP_MS),
    }
}

/// One row of the hierarchy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeRow {
    /// Index into `StateMap::nodes`.
    pub node: usize,
    pub depth: usize,
    pub has_children: bool,
}

/// The hierarchy by owns edges: roots are the nodes nothing owns, each
/// followed by what it owns, in node order. A node owned twice shows under
/// its first owner; one only reachable through a cycle shows as a root.
/// Only nodes `open` says yes to show their children.
pub fn tree(map: &StateMap, open: &dyn Fn(&str) -> bool) -> Vec<TreeRow> {
    let index: HashMap<&str, usize> = map
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.id.as_str(), i))
        .collect();
    let mut children = vec![Vec::new(); map.nodes.len()];
    let mut owned = vec![false; map.nodes.len()];
    for e in map.edges.iter().filter(|e| e.kind == EdgeKind::Owns) {
        if let (Some(&from), Some(&to)) = (index.get(&*e.from), index.get(&*e.to)) {
            if !children[from].contains(&to) {
                children[from].push(to);
                owned[to] = true;
            }
        }
    }
    for list in &mut children {
        list.sort_unstable();
    }
    let mut placed = vec![false; map.nodes.len()];
    let mut rows = Vec::new();
    let roots: Vec<usize> = (0..map.nodes.len()).filter(|&i| !owned[i]).collect();
    let rest = 0..map.nodes.len();
    for root in roots.into_iter().chain(rest) {
        if placed[root] {
            continue;
        }
        // Depth first, the stack holding (node, depth).
        let mut stack = vec![(root, 0)];
        while let Some((i, depth)) = stack.pop() {
            if placed[i] {
                continue;
            }
            placed[i] = true;
            let kids: Vec<usize> = children[i]
                .iter()
                .copied()
                .filter(|&c| !placed[c])
                .collect();
            rows.push(TreeRow {
                node: i,
                depth,
                has_children: !kids.is_empty(),
            });
            stack.extend(kids.into_iter().rev().map(|c| (c, depth + 1)));
        }
    }
    // Rows are depth first, so a closed node's descendants are the rows
    // after it that sit deeper.
    let mut closed_at: Option<usize> = None;
    rows.retain(|row| {
        if closed_at.is_some_and(|d| row.depth > d) {
            return false;
        }
        closed_at = (row.has_children && !open(&map.nodes[row.node].id)).then_some(row.depth);
        true
    });
    rows
}

/// A node as one line: `kind id: label (status)`.
pub fn line(n: &Node) -> String {
    let mut out = format!("{} {}", n.kind.as_str(), n.id);
    if !n.label.is_empty() && n.label != n.id {
        out.push_str(": ");
        out.push_str(&cut(&n.label, MAX_LABEL_CHARS));
    }
    if !n.status.is_empty() {
        out.push_str(&format!(" ({})", n.status));
    }
    if n.questions > 0 {
        out.push_str(&format!(", {}", plural(n.questions, "open question")));
    }
    out
}

/// The whole hierarchy as indented text, for `kete state`.
pub fn tree_text(map: &StateMap) -> String {
    let mut out = String::new();
    for row in tree(map, &|_| true) {
        out.push_str(&"  ".repeat(row.depth));
        out.push_str(&line(&map.nodes[row.node]));
        out.push('\n');
    }
    out
}

/// What an agent reads about `id`: the node, then each node an edge joins
/// it to, grouped by how, cut to [`SLICE_MAX_BYTES`]. None when the map
/// has no such node.
pub fn slice(map: &StateMap, id: &str, now: i64) -> Option<String> {
    let n = map.nodes.iter().find(|n| n.id == id)?;
    let by_id: HashMap<&str, &Node> = map.nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let mut lines = vec![
        format!(
            "State map of {}, built {}.",
            map.repo,
            ago(now, map.built_at)
        ),
        line(n),
    ];
    if let Some(owner) = &n.owner {
        lines.push(format!("Owner: {owner}."));
    }
    if let Some(at) = n.changed_at {
        lines.push(format!("Changed {}.", ago(now, at)));
    }
    if !n.links.is_empty() {
        lines.push(format!("Links: {}.", n.links.join(", ")));
    }
    // (heading, edge kind, true when `id` is the edge's `to`)
    const GROUPS: [(&str, EdgeKind, bool); 8] = [
        ("Owned by", EdgeKind::Owns, true),
        ("Owns", EdgeKind::Owns, false),
        ("Depends on", EdgeKind::DependsOn, false),
        ("Needed by", EdgeKind::DependsOn, true),
        ("Blocked by", EdgeKind::BlockedBy, false),
        ("Blocks", EdgeKind::BlockedBy, true),
        ("Touched by", EdgeKind::TouchedBy, false),
        ("Touches", EdgeKind::TouchedBy, true),
    ];
    for (heading, kind, incoming) in GROUPS {
        let others: Vec<&Node> = map
            .edges
            .iter()
            .filter(|e| e.kind == kind)
            .filter_map(|e| match incoming {
                true if e.to == id => by_id.get(&*e.from).copied(),
                false if e.from == id => by_id.get(&*e.to).copied(),
                _ => None,
            })
            .collect();
        if others.is_empty() {
            continue;
        }
        lines.push(format!("{heading}:"));
        lines.extend(others.into_iter().map(|o| format!("- {}", line(o))));
    }
    Some(fit(lines, id))
}

/// `lines` joined, as many as fit [`SLICE_MAX_BYTES`] with room for the
/// line that says how many were left out.
fn fit(lines: Vec<String>, id: &str) -> String {
    let tail_room = 120 + id.len();
    let total = lines.len();
    let mut out = String::new();
    for (i, l) in lines.iter().enumerate() {
        let l = cut(l, SLICE_MAX_BYTES / 2);
        let last = i + 1 == total;
        let room = if last {
            SLICE_MAX_BYTES
        } else {
            SLICE_MAX_BYTES - tail_room
        };
        if out.len() + l.len() + 1 > room {
            out.push_str(&format!(
                "And {} more. `kete state --slice {id} --json` has them all.\n",
                plural((total - i) as u32, "line")
            ));
            return out;
        }
        out.push_str(&l);
        out.push('\n');
    }
    out
}

/// `text` cut to at most `max` characters, ending in `...` when cut.
fn cut(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.into();
    }
    let mut out: String = text.chars().take(max.saturating_sub(3)).collect();
    out.push_str("...");
    out
}

fn plural(n: u32, word: &str) -> String {
    if n == 1 {
        format!("1 {word}")
    } else {
        format!("{n} {word}s")
    }
}

/// How long ago `at` was: `just now`, `4 min ago`, `3 h ago`, `2 days ago`.
pub fn ago(now: i64, at: i64) -> String {
    let secs = (now - at).max(0) / 1000;
    match secs {
        0..=59 => "just now".into(),
        60..=3599 => format!("{} min ago", secs / 60),
        3600..=86_399 => format!("{} h ago", secs / 3600),
        _ => format!("{} ago", plural((secs / 86_400) as u32, "day")),
    }
}

/// The edges of `id`, for `kete state --slice <id> --json`: the node and
/// every node an edge joins it to, with those edges.
pub fn neighbourhood(map: &StateMap, id: &str) -> Option<StateMap> {
    map.nodes.iter().find(|n| n.id == id)?;
    let edges: Vec<Edge> = map
        .edges
        .iter()
        .filter(|e| e.from == id || e.to == id)
        .cloned()
        .collect();
    let keep: BTreeSet<&str> = edges
        .iter()
        .flat_map(|e| [e.from.as_str(), e.to.as_str()])
        .chain([id])
        .collect();
    Some(StateMap {
        nodes: map
            .nodes
            .iter()
            .filter(|n| keep.contains(n.id.as_str()))
            .cloned()
            .collect(),
        edges,
        ..map.clone()
    })
}

/// `<repo_root>/.kete/state-map.json`. A missing file is `Ok(None)`.
pub fn read(repo_root: &Path) -> Result<Option<StateMap>, String> {
    let path = repo_root.join(MAP_PATH);
    let meta = match std::fs::metadata(&path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("could not read {MAP_PATH}: {e}")),
    };
    if meta.len() > MAX_MAP_BYTES {
        return Err(format!("{MAP_PATH} is over {} MiB", MAX_MAP_BYTES >> 20));
    }
    let text =
        std::fs::read_to_string(&path).map_err(|e| format!("could not read {MAP_PATH}: {e}"))?;
    serde_json::from_str(&text)
        .map(Some)
        .map_err(|e| format!("{MAP_PATH} is not a state map: {e}"))
}

/// Writes `map` to `<repo_root>/.kete/state-map.json`, one line, through a
/// temp file and a rename so a reader never sees half of it.
pub fn write(repo_root: &Path, map: &StateMap) -> std::io::Result<()> {
    let path = repo_root.join(MAP_PATH);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec(map)?)?;
    std::fs::rename(&tmp, &path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(id: &str, kind: NodeKind) -> Node {
        node(id.into(), kind, String::new(), "")
    }

    fn map(nodes: Vec<Node>, edges: Vec<Edge>) -> StateMap {
        merge("C:/Code/game", 0, Layer { nodes, edges }, Layer::default())
    }

    #[test]
    fn the_extractor_is_read_from_the_manifest() {
        let text =
            "version = 1\n[state_map]\nextractor = [\"python\", \"Tools/Studio/state_map.py\"]\n";
        assert_eq!(
            extractor(text),
            Some(vec!["python".into(), "Tools/Studio/state_map.py".into()])
        );
        assert_eq!(extractor("version = 1\n"), None);
        assert_eq!(extractor("[state_map]\nextractor = []\n"), None);
        assert_eq!(extractor("[state_map]\nextractor = [\" \"]\n"), None);
        assert_eq!(extractor("not toml ["), None);
    }

    #[test]
    fn a_layer_keeps_what_it_can_read() {
        let text = r#"{"nodes": [
            {"id": "D-012", "kind": "decision", "label": "Combat pacing", "questions": 2},
            {"id": "X-1", "kind": "spaceship"},
            {"id": "", "kind": "file"},
            {"kind": "file"}
        ], "edges": [
            {"from": "D-012", "to": "SYS-1", "kind": "depends_on"},
            {"from": "D-012", "to": "SYS-1", "kind": "rhymes_with"}
        ]}"#;
        let layer = parse_layer(text).unwrap();
        assert_eq!(layer.nodes.len(), 1);
        assert_eq!(layer.nodes[0].questions, 2);
        assert_eq!(layer.edges.len(), 1);
        assert!(parse_layer("[]").is_err());
        assert!(parse_layer("hello").is_err());
        assert_eq!(parse_layer("{}").unwrap(), Layer::default());
        assert!(parse_layer(&" ".repeat(MAX_LAYER_BYTES + 1)).is_err());
    }

    fn live() -> LiveInputs {
        LiveInputs {
            departments: vec![("art".into(), "Art and 3D".into())],
            lanes: vec![
                LiveLane {
                    branch: "main".into(),
                    path: "C:/Code/game".into(),
                },
                LiveLane {
                    branch: "lane-a".into(),
                    path: "C:/Code/game-a".into(),
                },
            ],
            agents: vec![LiveAgent {
                id: "a1".into(),
                name: "Runner".into(),
                status: "running".into(),
                branch: "lane-a".into(),
                changed_at: 7,
                role: Some("Art runner".into()),
                department: Some("art".into()),
            }],
            subagents: vec![LiveSubagent {
                id: "s1".into(),
                parent: "a1".into(),
                description: "Explore textures".into(),
                status: "running".into(),
                changed_at: 9,
            }],
            prs: vec![LivePr {
                number: 31,
                title: "Textures".into(),
                head: "lane-a".into(),
                url: "https://example.invalid/pr/31".into(),
                draft: true,
            }],
        }
    }

    #[test]
    fn the_live_layer_hangs_agents_and_prs_off_their_lane() {
        let m = merge("r", 0, Layer::default(), live_layer(&live()));
        let ids: Vec<&str> = m.nodes.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "dept:art",
                "lane:lane-a",
                "lane:main",
                "pr:31",
                "agent:a1",
                "agent:a1/s1"
            ]
        );
        let lane_a = &m.nodes[1];
        assert_eq!(lane_a.status, "active");
        assert_eq!(m.nodes[2].status, "idle");
        assert_eq!(m.nodes[3].status, "draft");
        assert_eq!(m.nodes[4].owner.as_deref(), Some("Art runner"));
        assert!(m
            .edges
            .contains(&edge("lane:lane-a", "agent:a1", EdgeKind::Owns)));
        assert!(m
            .edges
            .contains(&edge("lane:lane-a", "pr:31", EdgeKind::Owns)));
        assert!(m
            .edges
            .contains(&edge("agent:a1", "agent:a1/s1", EdgeKind::Owns)));
        assert!(m
            .edges
            .contains(&edge("dept:art", "agent:a1", EdgeKind::TouchedBy)));
    }

    #[test]
    fn merge_keeps_the_repos_words_and_the_live_status() {
        let mut from_repo = n("lane:lane-a", NodeKind::Lane);
        from_repo.label = "Textures lane".into();
        from_repo.status = "planned".into();
        from_repo.questions = 3;
        from_repo.links = vec!["docs/lane-a.md".into()];
        let repo_layer = Layer {
            nodes: vec![from_repo, n("SYS-1", NodeKind::System)],
            edges: vec![
                edge("SYS-1", "lane:lane-a", EdgeKind::Owns),
                edge("SYS-1", "lane:lane-a", EdgeKind::Owns),
                edge("SYS-1", "SYS-1", EdgeKind::DependsOn),
                edge("SYS-1", "gone", EdgeKind::DependsOn),
            ],
        };
        let m = merge("r", 5, repo_layer, live_layer(&live()));
        let lane = m.nodes.iter().find(|n| n.id == "lane:lane-a").unwrap();
        assert_eq!(lane.label, "Textures lane");
        assert_eq!(lane.status, "active");
        assert_eq!(lane.questions, 3);
        assert_eq!(lane.links, ["docs/lane-a.md", "C:/Code/game-a"]);
        let from_sys: Vec<&Edge> = m.edges.iter().filter(|e| e.from == "SYS-1").collect();
        assert_eq!(from_sys, [&edge("SYS-1", "lane:lane-a", EdgeKind::Owns)]);
        assert_eq!(m.built_at, 5);
    }

    #[test]
    fn rebuilds_wait_out_a_burst_and_never_go_stale() {
        assert!(rebuild_due(0, None, false));
        assert!(!rebuild_due(1_000, Some(0), true));
        assert!(rebuild_due(MIN_GAP_MS, Some(0), true));
        assert!(!rebuild_due(EVERY_MS - 1, Some(0), false));
        assert!(rebuild_due(EVERY_MS, Some(0), false));
    }

    #[test]
    fn the_tree_follows_owns_edges_and_survives_cycles() {
        let m = map(
            vec![
                n("A", NodeKind::System),
                n("B", NodeKind::Subsystem),
                n("C", NodeKind::Subsystem),
                n("X", NodeKind::File),
                n("Y", NodeKind::File),
            ],
            vec![
                edge("A", "B", EdgeKind::Owns),
                edge("A", "C", EdgeKind::Owns),
                edge("B", "C", EdgeKind::Owns),
                edge("A", "X", EdgeKind::DependsOn),
                edge("X", "Y", EdgeKind::Owns),
                edge("Y", "X", EdgeKind::Owns),
            ],
        );
        let rows = tree(&m, &|_| true);
        let shown: Vec<(&str, usize)> = rows
            .iter()
            .map(|r| (m.nodes[r.node].id.as_str(), r.depth))
            .collect();
        assert_eq!(shown, [("A", 0), ("B", 1), ("C", 2), ("X", 0), ("Y", 1)]);
        let closed = tree(&m, &|id| id != "A");
        assert_eq!(closed.len(), 3);
        assert!(closed[0].has_children);
        assert_eq!(
            tree_text(&m),
            "system A\n  subsystem B\n    subsystem C\nfile X\n  file Y\n"
        );
    }

    #[test]
    fn a_slice_names_the_node_and_its_neighbours() {
        let mut d = n("D-012", NodeKind::Decision);
        d.label = "Combat pacing".into();
        d.status = "open".into();
        d.questions = 2;
        d.changed_at = Some(0);
        let m = map(
            vec![
                d,
                n("SYS-1", NodeKind::System),
                n("dept:design", NodeKind::Department),
            ],
            vec![
                edge("dept:design", "D-012", EdgeKind::Owns),
                edge("D-012", "SYS-1", EdgeKind::DependsOn),
            ],
        );
        let text = slice(&m, "D-012", 180_000).unwrap();
        assert_eq!(
            text,
            "State map of C:/Code/game, built 3 min ago.\n\
             decision D-012: Combat pacing (open), 2 open questions\n\
             Changed 3 min ago.\n\
             Owned by:\n\
             - department dept:design\n\
             Depends on:\n\
             - system SYS-1\n"
        );
        assert_eq!(slice(&m, "nope", 0), None);
        let near = neighbourhood(&m, "SYS-1").unwrap();
        assert_eq!(near.nodes.len(), 2);
        assert_eq!(near.edges.len(), 1);
    }

    #[test]
    fn a_slice_never_passes_its_cap() {
        let mut nodes = vec![n("hub", NodeKind::System)];
        let mut edges = Vec::new();
        for i in 0..400 {
            let id = format!("file-{i:03}");
            let mut f = n(&id, NodeKind::File);
            f.label = "a long label ".repeat(20);
            nodes.push(f);
            edges.push(edge("hub", &id, EdgeKind::Owns));
        }
        let text = slice(&map(nodes, edges), "hub", 0).unwrap();
        assert!(text.len() <= SLICE_MAX_BYTES, "{} bytes", text.len());
        assert!(text.ends_with("`kete state --slice hub --json` has them all.\n"));
        assert!(text.starts_with("State map of"));
    }

    #[test]
    fn the_map_file_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read(dir.path()), Ok(None));
        let m = merge("r", 1, Layer::default(), live_layer(&live()));
        write(dir.path(), &m).unwrap();
        assert_eq!(read(dir.path()), Ok(Some(m)));
        assert!(!dir.path().join(".kete/state-map.json.tmp").exists());
        std::fs::write(dir.path().join(MAP_PATH), "{").unwrap();
        assert!(read(dir.path()).is_err());
    }

    #[test]
    fn ages_read_plainly() {
        assert_eq!(ago(59_000, 0), "just now");
        assert_eq!(ago(120_000, 0), "2 min ago");
        assert_eq!(ago(7_200_000, 0), "2 h ago");
        assert_eq!(ago(86_400_000, 0), "1 day ago");
        assert_eq!(ago(0, 5_000), "just now");
    }
}
