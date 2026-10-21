//! STATE MAP (K10): the daemon's builder of `<repo>/.kete/state-map.json`
//! for every studio repo.
//!
//! One task ([`spawn`]) listens to the registry's events and rebuilds when
//! one that changes the map lands ([`rebuilds`]), at most once every
//! `MIN_GAP_MS`, and every `EVERY_MS` without one (`state_map::rebuild_due`).
//! A rebuild runs off the event path (`spawn_blocking`): it gathers the live
//! layer from the store, the role cards and the subagents, runs the repo's
//! extractor at most once a minute, asks `gh` for open pull requests at
//! most once every [`PR_EVERY`], and writes the merged map. Clients read
//! the file; no protocol message carries it.
//!
//! The extractor is repo code run unattended, so it runs only for a repo
//! whose studio mode is on in this machine's `config.local.json`, never
//! for a committed file alone: no shell, stdin closed, its output capped at
//! `MAX_LAYER_BYTES` and killed past [`EXTRACTOR_TIMEOUT`].

use crate::registry::Daemon;
use nebula_core::state_map::{self, Layer, LiveAgent, LiveInputs, LiveLane, LivePr, LiveSubagent};
use nebula_core::studio::{board, manifest, settings};
use nebula_core::{Project, ServerEvent};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::broadcast::error::RecvError;

/// How often the task looks at whether a rebuild is due.
const LOOK_EVERY: Duration = Duration::from_secs(1);
/// Open pull requests are asked of `gh` at most this often per repo.
pub const PR_EVERY: Duration = Duration::from_secs(300);
/// The repo's extractor runs at most this often.
const EXTRACT_EVERY: Duration = Duration::from_secs(60);
/// An extractor still running after this long is killed.
pub const EXTRACTOR_TIMEOUT: Duration = Duration::from_secs(30);
/// `gh pr list` still running after this long is killed.
const GH_TIMEOUT: Duration = Duration::from_secs(15);
/// The departments file is read up to this many bytes.
const MAX_DEPARTMENTS_BYTES: u64 = 262_144;

/// Whether `event` can change a state map: a session, worktree or project
/// came or went, a status or role changed, a subagent moved.
pub fn rebuilds(event: &ServerEvent) -> bool {
    matches!(
        event,
        ServerEvent::EntityUpserted { .. }
            | ServerEvent::EntityRemoved { .. }
            | ServerEvent::StatusChanged { .. }
            | ServerEvent::SessionExited { .. }
            | ServerEvent::SubagentChanged { .. }
            | ServerEvent::SubagentsRemoved { .. }
            | ServerEvent::SessionRoles { .. }
    )
}

/// What a repo's last rebuild kept for the next: the repo layer and the
/// pull requests, each with when it was read.
#[derive(Default)]
struct Cache {
    layer: Option<(Instant, Result<Layer, String>)>,
    prs: Option<(Instant, Vec<LivePr>)>,
}

/// Starts the builder; it ends with the daemon.
pub fn spawn(daemon: Arc<Daemon>) {
    tokio::spawn(async move {
        let mut events = daemon.events.subscribe();
        let mut look = tokio::time::interval(LOOK_EVERY);
        let mut caches: HashMap<PathBuf, Cache> = HashMap::new();
        let mut built_at: Option<i64> = None;
        let mut dirty = false;
        loop {
            tokio::select! {
                _ = daemon.shutdown.cancelled() => break,
                event = events.recv() => match event {
                    Ok(event) => dirty |= rebuilds(&event),
                    Err(RecvError::Lagged(_)) => dirty = true,
                    Err(RecvError::Closed) => break,
                },
                _ = look.tick() => {
                    let now = nebula_core::clock::now_ms();
                    if !state_map::rebuild_due(now, built_at, dirty) {
                        continue;
                    }
                    dirty = false;
                    built_at = Some(now);
                    let d = daemon.clone();
                    let taken = std::mem::take(&mut caches);
                    match tokio::task::spawn_blocking(move || rebuild_all(&d, taken)).await {
                        Ok(kept) => caches = kept,
                        Err(error) => tracing::warn!(%error, "state map rebuild failed"),
                    }
                }
            }
        }
    });
}

/// Rebuilds every studio repo's map, keeping each repo's cache. A repo
/// whose studio mode went off drops out.
fn rebuild_all(daemon: &Daemon, mut caches: HashMap<PathBuf, Cache>) -> HashMap<PathBuf, Cache> {
    let entries = settings::load_all();
    let Ok((projects, ..)) = daemon.store.load_tree() else {
        return caches;
    };
    let mut kept = HashMap::new();
    for key in entries.keys() {
        let key = key.to_string_lossy();
        let Some(project) = projects
            .iter()
            .find(|p| settings::same_repo(&settings::repo_key(&p.repo_path), &key))
        else {
            continue;
        };
        let root = project.repo_path.clone();
        let mut cache = caches.remove(&root).unwrap_or_default();
        let map = rebuild(daemon, project, &mut cache, &mut Sources::Real);
        if let Err(error) = exclude(&root) {
            tracing::warn!(%error, repo = %root.display(), "could not keep the state map out of git");
        }
        if let Err(error) = state_map::write(&root, &map) {
            tracing::warn!(%error, repo = %root.display(), "could not write the state map");
        }
        kept.insert(root, cache);
    }
    kept
}

/// The map's line in `info/exclude`, its temp file included.
const EXCLUDE_LINE: &str = ".kete/state-map.json*";

/// Keeps the map out of `git status` through the checkout's
/// `.git/info/exclude`, as Kete does for its other per-checkout files
/// (`hooks::installer`). A folder without a `.git` directory is left alone.
fn exclude(root: &Path) -> std::io::Result<()> {
    let info = root.join(".git").join("info");
    if !root.join(".git").is_dir() {
        return Ok(());
    }
    let path = info.join("exclude");
    let mut text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e),
    };
    if text.lines().any(|l| l.trim() == EXCLUDE_LINE) {
        return Ok(());
    }
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(EXCLUDE_LINE);
    text.push('\n');
    std::fs::create_dir_all(&info)?;
    std::fs::write(&path, text)
}

/// Where a rebuild's slow inputs come from: the real extractor and `gh`,
/// or a test's answers.
enum Sources {
    Real,
    #[cfg(test)]
    Fixed {
        layer: Result<Layer, String>,
        prs: Vec<LivePr>,
        /// How often each source was asked.
        layers_asked: usize,
        prs_asked: usize,
    },
}

impl Sources {
    fn layer(&mut self, root: &Path) -> Result<Layer, String> {
        match self {
            Sources::Real => extract(root),
            #[cfg(test)]
            Sources::Fixed {
                layer,
                layers_asked,
                ..
            } => {
                *layers_asked += 1;
                layer.clone()
            }
        }
    }

    fn prs(&mut self, root: &Path) -> Vec<LivePr> {
        match self {
            Sources::Real => open_prs(root),
            #[cfg(test)]
            Sources::Fixed { prs, prs_asked, .. } => {
                *prs_asked += 1;
                prs.clone()
            }
        }
    }
}

/// One repo's map, from the cache where it is fresh.
fn rebuild(
    daemon: &Daemon,
    project: &Project,
    cache: &mut Cache,
    sources: &mut Sources,
) -> state_map::StateMap {
    let root = &project.repo_path;
    if cache
        .layer
        .as_ref()
        .is_none_or(|(at, _)| at.elapsed() >= EXTRACT_EVERY)
    {
        cache.layer = Some((Instant::now(), sources.layer(root)));
    }
    if cache
        .prs
        .as_ref()
        .is_none_or(|(at, _)| at.elapsed() >= PR_EVERY)
    {
        cache.prs = Some((Instant::now(), sources.prs(root)));
    }
    let prs = cache
        .prs
        .as_ref()
        .map(|(_, p)| p.clone())
        .unwrap_or_default();
    let live = state_map::live_layer(&live_inputs(daemon, project, prs));
    let (layer, note) = match cache.layer.as_ref().map(|(_, l)| l.clone()) {
        Some(Ok(layer)) => (layer, None),
        Some(Err(why)) => (Layer::default(), Some(why)),
        None => (Layer::default(), None),
    };
    let mut map = state_map::merge(
        &settings::repo_key(root),
        nebula_core::clock::now_ms(),
        layer,
        live,
    );
    map.note = note;
    map
}

/// The live half for `project`: its departments, worktrees, live sessions
/// with their roles, running subagents, and `prs`.
fn live_inputs(daemon: &Daemon, project: &Project, prs: Vec<LivePr>) -> LiveInputs {
    let Ok((_, worktrees, agents, _)) = daemon.store.load_tree() else {
        return LiveInputs::default();
    };
    let worktrees: Vec<_> = worktrees
        .into_iter()
        .filter(|w| w.project_id == project.id)
        .collect();
    let roles: HashMap<_, _> = daemon
        .session_roles()
        .into_iter()
        .map(|r| (r.agent.clone(), r))
        .collect();
    let agents: Vec<LiveAgent> = agents
        .iter()
        .filter(|a| !a.archived)
        .filter_map(|a| {
            let w = worktrees.iter().find(|w| w.id == a.worktree_id)?;
            let role = roles.get(&a.id);
            Some(LiveAgent {
                id: a.id.0.clone(),
                name: a.name.clone(),
                status: word(&a.status),
                branch: w.branch.clone(),
                changed_at: a.status_changed_at,
                role: role.map(|r| r.label.clone()),
                department: role.and_then(|r| r.department.clone()),
            })
        })
        .collect();
    let subagents = daemon
        .subagent_rows()
        .into_iter()
        .filter(|s| s.ended_at.is_none() && agents.iter().any(|a| a.id == s.parent_agent.0))
        .map(|s| LiveSubagent {
            id: s.id.clone(),
            parent: s.parent_agent.0.clone(),
            description: s.description.clone(),
            status: word(&s.status),
            changed_at: s.started_at,
        })
        .collect();
    LiveInputs {
        departments: departments(&project.repo_path),
        lanes: worktrees
            .iter()
            .map(|w| LiveLane {
                branch: w.branch.clone(),
                path: settings::repo_key(&w.path),
            })
            .collect(),
        agents,
        subagents,
        prs,
    }
}

/// A unit enum's serde name (`running`, `needs_feedback`).
fn word<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// The manifest's departments table, as `(slug, label)`.
fn departments(root: &Path) -> Vec<(String, String)> {
    let Some(rel) = manifest::load(root)
        .ok()
        .flatten()
        .and_then(|m| m.departments)
    else {
        return Vec::new();
    };
    let path = root.join(rel);
    if !std::fs::metadata(&path).is_ok_and(|m| m.len() <= MAX_DEPARTMENTS_BYTES) {
        return Vec::new();
    }
    std::fs::read_to_string(&path)
        .map(|text| {
            board::departments(&text)
                .into_iter()
                .map(|d| (d.slug, d.label))
                .collect()
        })
        .unwrap_or_default()
}

/// The repo layer: the extractor `.kete/studio.toml` names, run in the
/// repo root. Err says why there is none, in a line.
fn extract(root: &Path) -> Result<Layer, String> {
    let text = std::fs::read_to_string(root.join(manifest::MANIFEST_PATH))
        .map_err(|_| "no .kete/studio.toml".to_string())?;
    let argv = state_map::extractor(&text)
        .ok_or_else(|| "no extractor under [state_map] in .kete/studio.toml".to_string())?;
    let mut cmd = Command::new(&argv[0]);
    cmd.args(&argv[1..]).current_dir(root);
    let out = run_capped(cmd, EXTRACTOR_TIMEOUT, state_map::MAX_LAYER_BYTES)
        .map_err(|why| format!("the extractor {why}"))?;
    state_map::parse_layer(&String::from_utf8_lossy(&out))
}

/// The repo's open pull requests from `gh`; none when it can't say.
fn open_prs(root: &Path) -> Vec<LivePr> {
    let mut cmd = Command::new("gh");
    cmd.args([
        "pr",
        "list",
        "--state",
        "open",
        "--limit",
        "100",
        "--json",
        "number,title,headRefName,url,isDraft",
    ])
    .current_dir(root)
    .env("GH_PROMPT_DISABLED", "1");
    run_capped(cmd, GH_TIMEOUT, 1 << 20)
        .ok()
        .and_then(|out| serde_json::from_slice(&out).ok())
        .unwrap_or_default()
}

/// `cmd`'s stdout once it exits 0: no shell, stdin closed, stderr dropped.
/// Refused when it runs past `timeout` (and killed) or prints over `max`
/// bytes.
fn run_capped(mut cmd: Command, timeout: Duration, max: usize) -> Result<Vec<u8>, String> {
    use std::io::Read;
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    nebula_core::platform::no_window(&mut cmd);
    let mut child = cmd.spawn().map_err(|e| format!("could not start: {e}"))?;
    let mut stdout = child.stdout.take().ok_or("has no output")?;
    let reader = std::thread::spawn(move || {
        let mut out = Vec::new();
        let _ = (&mut stdout).take(max as u64 + 1).read_to_end(&mut out);
        // The rest is drained, so a child printing past the cap exits
        // instead of blocking on a full pipe until the timeout.
        let _ = std::io::copy(&mut stdout, &mut std::io::sink());
        out
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("ran past {} s", timeout.as_secs()));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => return Err(e.to_string()),
        }
    };
    // shortcut: a grandchild the extractor leaves holding stdout keeps this
    // join (and so the rebuild) waiting until it exits; bound the join if
    // an extractor ever does that.
    let out = reader.join().map_err(|_| "output was lost".to_string())?;
    if out.len() > max {
        return Err(format!("printed over {max} bytes"));
    }
    if !status.success() {
        return Err(format!("exited with {status}"));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::HookEnv;
    use crate::store::Store;
    use nebula_core::state_map::{EdgeKind, Node, NodeKind};
    use nebula_core::{
        Agent, AgentId, AgentKind, AgentRole, AgentStatus, ProjectId, Worktree, WorktreeId,
    };

    fn daemon_with_repo(repo: &Path) -> (Arc<Daemon>, Project) {
        let daemon = Daemon::new(
            Arc::new(Store::open_in_memory().unwrap()),
            HookEnv {
                port: 0,
                token: String::new(),
            },
        );
        let project = Project {
            id: ProjectId("p".into()),
            name: "demo".into(),
            repo_path: repo.to_path_buf(),
            sort_order: 0,
        };
        daemon.store.insert_project(&project).unwrap();
        for (id, branch, is_main) in [("w", "main", true), ("w2", "lane-a", false)] {
            daemon
                .store
                .insert_worktree(&Worktree {
                    id: WorktreeId(id.into()),
                    project_id: ProjectId("p".into()),
                    path: repo.join(id),
                    branch: branch.into(),
                    is_main,
                    sort_order: 0,
                })
                .unwrap();
        }
        daemon
            .store
            .insert_agent(&Agent {
                parent: None,
                profile: None,
                id: AgentId("a1".into()),
                worktree_id: WorktreeId("w2".into()),
                name: "runner".into(),
                status: AgentStatus::NeedsFeedback,
                archived: false,
                archived_at: 0,
                unseen: false,
                status_changed_at: 42,
                kind: AgentKind::Claude,
                custom_harness: None,
                model: None,
                effort: None,
                session_id: None,
                cloud_session_id: None,
                issue_url: None,
                role: AgentRole::default(),
                sort_order: 0,
                alive: false,
                recent_prompts: Vec::new(),
            })
            .unwrap();
        (daemon, project)
    }

    fn decision() -> Layer {
        Layer {
            nodes: vec![Node {
                id: "D-1".into(),
                kind: NodeKind::Decision,
                label: "Pacing".into(),
                status: "open".into(),
                owner: None,
                changed_at: None,
                questions: 1,
                links: Vec::new(),
            }],
            edges: vec![state_map::Edge {
                from: "D-1".into(),
                to: "lane:lane-a".into(),
                kind: EdgeKind::BlockedBy,
            }],
        }
    }

    #[test]
    fn only_events_that_move_the_map_rebuild_it() {
        assert!(rebuilds(&ServerEvent::EntityRemoved {
            id: nebula_core::EntityId::Agent(AgentId("a".into())),
        }));
        assert!(!rebuilds(&ServerEvent::Ack {
            req_id: 1,
            created: None
        }));
    }

    #[test]
    fn a_rebuild_merges_the_extractor_with_the_live_layer() {
        let repo = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(repo.path().join(".kete")).unwrap();
        std::fs::write(
            repo.path().join("DEPARTMENTS.md"),
            "| Department |\n|---|\n| Art |\n",
        )
        .unwrap();
        std::fs::write(
            repo.path().join(manifest::MANIFEST_PATH),
            "version = 1\ndepartments = \"DEPARTMENTS.md\"\n",
        )
        .unwrap();
        let (daemon, project) = daemon_with_repo(repo.path());
        let mut sources = Sources::Fixed {
            layer: Ok(decision()),
            prs: vec![LivePr {
                number: 7,
                title: "Pacing pass".into(),
                head: "lane-a".into(),
                url: String::new(),
                draft: false,
            }],
            layers_asked: 0,
            prs_asked: 0,
        };
        let mut cache = Cache::default();
        let map = rebuild(&daemon, &project, &mut cache, &mut sources);
        let ids: Vec<&str> = map.nodes.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "dept:art",
                "D-1",
                "lane:lane-a",
                "lane:main",
                "pr:7",
                "agent:a1"
            ]
        );
        let lane = map.nodes.iter().find(|n| n.id == "lane:lane-a").unwrap();
        assert_eq!(lane.status, "active");
        let agent = map.nodes.iter().find(|n| n.id == "agent:a1").unwrap();
        assert_eq!(agent.status, "needs_feedback");
        assert_eq!(agent.changed_at, Some(42));
        assert!(map
            .edges
            .iter()
            .any(|e| e.from == "D-1" && e.kind == EdgeKind::BlockedBy));
        assert_eq!(map.note, None);

        // A second rebuild inside a minute asks neither source again.
        let again = rebuild(&daemon, &project, &mut cache, &mut sources);
        assert_eq!(again.nodes, map.nodes);
        assert_eq!(asked(&sources), (1, 1));
    }

    /// (layer, pull requests) asks so far.
    fn asked(sources: &Sources) -> (usize, usize) {
        match sources {
            Sources::Fixed {
                layers_asked,
                prs_asked,
                ..
            } => (*layers_asked, *prs_asked),
            Sources::Real => unreachable!(),
        }
    }

    #[test]
    fn the_extractor_is_re_read_each_minute_and_gh_every_five() {
        let repo = tempfile::tempdir().unwrap();
        let (daemon, project) = daemon_with_repo(repo.path());
        let mut sources = Sources::Fixed {
            layer: Ok(Layer::default()),
            prs: Vec::new(),
            layers_asked: 0,
            prs_asked: 0,
        };
        let mut cache = Cache::default();
        rebuild(&daemon, &project, &mut cache, &mut sources);
        assert_eq!(asked(&sources), (1, 1));
        // Each cached read made `layer` and `prs` seconds ago, then a rebuild.
        let mut at = |layer: u64, prs: u64| {
            let ago = |secs| {
                Instant::now()
                    .checked_sub(Duration::from_secs(secs))
                    .unwrap()
            };
            cache.layer.as_mut().unwrap().0 = ago(layer);
            cache.prs.as_mut().unwrap().0 = ago(prs);
            rebuild(&daemon, &project, &mut cache, &mut sources);
            asked(&sources)
        };
        assert_eq!(at(61, 61), (2, 1), "past a minute: the extractor, not gh");
        assert_eq!(at(61, 299), (3, 1), "gh waits out its five minutes");
        assert_eq!(at(61, 301), (4, 2), "past five minutes: gh once more");
    }

    #[test]
    fn a_failed_extractor_leaves_the_live_layer_and_says_why() {
        let repo = tempfile::tempdir().unwrap();
        let (daemon, project) = daemon_with_repo(repo.path());
        let mut sources = Sources::Fixed {
            layer: Err("the extractor exited with exit code: 1".into()),
            prs: Vec::new(),
            layers_asked: 0,
            prs_asked: 0,
        };
        let map = rebuild(&daemon, &project, &mut Cache::default(), &mut sources);
        assert_eq!(
            map.note.as_deref(),
            Some("the extractor exited with exit code: 1")
        );
        assert_eq!(map.nodes.len(), 3, "two lanes and the agent");
    }

    #[test]
    fn without_a_manifest_extractor_nothing_runs() {
        let repo = tempfile::tempdir().unwrap();
        assert_eq!(extract(repo.path()), Err("no .kete/studio.toml".into()));
        std::fs::create_dir_all(repo.path().join(".kete")).unwrap();
        std::fs::write(repo.path().join(manifest::MANIFEST_PATH), "version = 1\n").unwrap();
        assert!(extract(repo.path()).unwrap_err().contains("no extractor"));
    }

    #[test]
    fn the_map_is_excluded_from_git_once() {
        let repo = tempfile::tempdir().unwrap();
        exclude(repo.path()).unwrap();
        assert!(
            !repo.path().join(".git").exists(),
            "not a checkout: untouched"
        );
        std::fs::create_dir_all(repo.path().join(".git/info")).unwrap();
        std::fs::write(repo.path().join(".git/info/exclude"), "# local\ntarget").unwrap();
        exclude(repo.path()).unwrap();
        exclude(repo.path()).unwrap();
        assert_eq!(
            std::fs::read_to_string(repo.path().join(".git/info/exclude")).unwrap(),
            "# local\ntarget\n.kete/state-map.json*\n"
        );
    }

    #[test]
    fn a_command_is_capped_and_timed_out() {
        let echo = |text: &str| {
            let mut cmd = if cfg!(windows) {
                let mut c = Command::new("cmd");
                c.args(["/C", "echo", text]);
                c
            } else {
                let mut c = Command::new("echo");
                c.arg(text);
                c
            };
            cmd.current_dir(std::env::temp_dir());
            cmd
        };
        let out = run_capped(echo("{}"), Duration::from_secs(10), 64).unwrap();
        assert_eq!(String::from_utf8_lossy(&out).trim(), "{}");
        assert!(
            run_capped(echo(&"x".repeat(100)), Duration::from_secs(10), 16)
                .unwrap_err()
                .contains("over 16 bytes")
        );
        // Past the pipe buffer: refused for its size, not left to time out.
        let big = tempfile::tempdir().unwrap();
        let file = big.path().join("big.txt");
        std::fs::write(&file, "x".repeat(256 * 1024)).unwrap();
        let mut cat = if cfg!(windows) {
            let mut c = Command::new("cmd");
            c.args(["/C", "type"]).arg(&file);
            c
        } else {
            let mut c = Command::new("cat");
            c.arg(&file);
            c
        };
        cat.current_dir(big.path());
        assert!(run_capped(cat, Duration::from_secs(5), 1024)
            .unwrap_err()
            .contains("over 1024 bytes"));
        assert!(run_capped(
            Command::new("kete-no-such-program"),
            Duration::from_secs(1),
            16
        )
        .unwrap_err()
        .starts_with("could not start"));
        let mut slow = if cfg!(windows) {
            let mut c = Command::new("ping");
            c.args(["-n", "4", "127.0.0.1"]);
            c
        } else {
            let mut c = Command::new("sleep");
            c.arg("3");
            c
        };
        slow.current_dir(std::env::temp_dir());
        assert_eq!(
            run_capped(slow, Duration::from_secs(1), 1 << 16),
            Err("ran past 1 s".into())
        );
    }
}
