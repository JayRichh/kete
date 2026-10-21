use super::*;
use crate::hooks::HookEnv;
use crate::registry::tests::EnvGuard;
use crate::store::Store;
use nebula_core::{AgentStatus, Entity, ProjectId};
use std::collections::HashSet;

fn daemon() -> Arc<Daemon> {
    daemon_at("/tmp/demo")
}

/// A daemon over an in-memory store with project `p` and its root
/// worktree `w` at `path`.
fn daemon_at(path: &str) -> Arc<Daemon> {
    let daemon = Daemon::new(
        Arc::new(Store::open_in_memory().unwrap()),
        HookEnv {
            port: 0,
            token: String::new(),
        },
    );
    daemon
        .store
        .insert_project(&Project {
            id: ProjectId("p".into()),
            name: "demo".into(),
            repo_path: path.into(),
            sort_order: 0,
        })
        .unwrap();
    daemon
        .store
        .insert_worktree(&Worktree {
            id: WorktreeId("w".into()),
            project_id: ProjectId("p".into()),
            path: path.into(),
            branch: "main".into(),
            is_main: true,
            sort_order: 0,
        })
        .unwrap();
    daemon
}

fn agent(id: &str, role: AgentRole) -> Agent {
    Agent {
        parent: None,
        profile: None,
        id: AgentId(id.into()),
        worktree_id: WorktreeId("w".into()),
        name: id.into(),
        status: AgentStatus::Fresh,
        archived: false,
        archived_at: 0,
        unseen: false,
        status_changed_at: 0,
        kind: AgentKind::Claude,
        custom_harness: None,
        model: None,
        effort: None,
        session_id: None,
        cloud_session_id: None,
        issue_url: None,
        role,
        sort_order: 0,
        alive: false,
        recent_prompts: Vec::new(),
    }
}

/// What stands in for an agent CLI in tests that spawn one.
fn stand_in_cli() -> EnvGuard {
    #[cfg(unix)]
    let program = "/bin/cat";
    #[cfg(windows)]
    let program = "cmd.exe";
    EnvGuard::set(nebula_core::env::AGENT_CMD, program)
}

#[test]
fn only_orchestrator_role_can_use_privileged_list() {
    let daemon = daemon();
    daemon
        .store
        .insert_agent(&agent("worker", AgentRole::Worker))
        .unwrap();
    let err = daemon
        .orchestrator_tree(&AgentId("worker".into()))
        .unwrap_err();
    assert!(err.to_string().contains("only available"));

    daemon
        .store
        .insert_agent(&agent("orchestrator", AgentRole::Orchestrator))
        .unwrap();
    let (_, _, agents, _, _) = daemon
        .orchestrator_tree(&AgentId("orchestrator".into()))
        .unwrap();
    assert_eq!(agents.len(), 2);
    assert!(agents
        .iter()
        .any(|a| a.id.0 == "orchestrator" && a.role == AgentRole::Orchestrator));
}

/// Every privileged verb refuses a worker caller, an archived orchestrator
/// and a caller with no row, before it touches anything.
#[tokio::test]
async fn every_orchestrator_verb_is_gated_on_the_persisted_role() {
    let daemon = daemon();
    daemon
        .store
        .insert_agent(&agent("worker", AgentRole::Worker))
        .unwrap();
    daemon
        .store
        .insert_agent(&agent("target", AgentRole::Worker))
        .unwrap();
    let mut gone = agent("gone", AgentRole::Orchestrator);
    gone.archived = true;
    daemon.store.insert_agent(&gone).unwrap();
    let target = SessionRef::Agent(AgentId("target".into()));

    for (caller, why) in [
        ("worker", "only available"),
        ("gone", "archived"),
        ("missing", "not found"),
    ] {
        let caller = AgentId(caller.into());
        let mut errors = vec![
            ("list", daemon.orchestrator_tree(&caller).err()),
            ("sessions", daemon.orchestrator_sessions(&caller).err()),
            ("read", daemon.orchestrator_read(&caller, &target, 64).err()),
            (
                "read-session",
                daemon
                    .orchestrator_read_session(&caller, "target", 10)
                    .err(),
            ),
            (
                "review",
                daemon
                    .orchestrator_open_review(&caller, vec![target.clone()], Vec::new())
                    .err(),
            ),
        ];
        errors.push((
            "send",
            daemon
                .orchestrator_send(&caller, &AgentId("target".into()), "hi")
                .await
                .err(),
        ));
        errors.push((
            "tell",
            daemon
                .orchestrator_tell(&caller, "target", "hi", Duration::from_millis(10), false)
                .await
                .err(),
        ));
        errors.push((
            "spawn",
            daemon
                .orchestrator_spawn(
                    &caller,
                    OrchestratorSpawnSpec {
                        project: ProjectId("p".into()),
                        worktree: None,
                        branch: None,
                        base: None,
                        name: String::new(),
                        kind: AgentKind::Claude,
                        custom_harness: None,
                        model: None,
                        effort: None,
                        profile: None,
                        starting_prompt: "do it".into(),
                        role_card: None,
                    },
                )
                .await
                .err(),
        ));
        for (verb, err) in errors {
            let err = err.unwrap_or_else(|| panic!("{verb} let {caller} through"));
            assert!(err.to_string().contains(why), "{verb} from {caller}: {err}");
        }
    }
    let (_, _, agents, _) = daemon.store.load_tree().unwrap();
    assert_eq!(agents.len(), 3, "a refused spawn created nothing");
}

#[tokio::test]
async fn the_orchestrator_cannot_send_to_itself_and_waits_or_refuses_a_busy_target() {
    let daemon = daemon();
    daemon
        .store
        .insert_agent(&agent("orch", AgentRole::Orchestrator))
        .unwrap();
    let mut busy = agent("busy-one", AgentRole::Worker);
    busy.status = AgentStatus::Running;
    daemon.store.insert_agent(&busy).unwrap();
    let orch = AgentId("orch".into());

    let err = daemon
        .orchestrator_tell(&orch, "orch", "hi", Duration::ZERO, true)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("itself"), "{err}");

    // By fuzzy name, no wait: refused while it runs.
    let err = daemon
        .orchestrator_tell(&orch, "busy", "hi", Duration::ZERO, false)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("running"), "{err}");

    // Waiting: times out while it stays busy.
    let err = daemon
        .orchestrator_tell(&orch, "busy", "hi", Duration::from_millis(50), true)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("timed out"), "{err}");
}

#[test]
fn launch_rule_leads_with_the_guidance_for_the_orchestrator_only() {
    assert_eq!(launch_rule(AgentRole::Worker, None), None);
    assert_eq!(
        launch_rule(AgentRole::Worker, Some("rule".into())).as_deref(),
        Some("rule")
    );
    assert_eq!(
        launch_rule(AgentRole::Orchestrator, None).as_deref(),
        Some(CLAUDE_ORCHESTRATOR_GUIDANCE)
    );
    let both = launch_rule(AgentRole::Orchestrator, Some("rule".into())).unwrap();
    assert!(both.starts_with(CLAUDE_ORCHESTRATOR_GUIDANCE) && both.ends_with("rule"));
}

#[test]
fn the_guidance_covers_attention_order_the_verbs_and_the_ado_hook() {
    for needle in [
        "needs_feedback",
        "running",
        "unread",
        "kete orchestrator list",
        "kete orchestrator read",
        "kete orchestrator send",
        "kete orchestrator spawn",
        "--worktree",
        "kete orchestrator review",
        "kete sessions",
        "kete ask",
        "subagents",
        "Tell, then ask",
        "Azure DevOps",
        "--project P tasks",
        "--worktree",
        "kete ado start-task",
        "kete ado prs",
        "kete ado pr <id>",
        "kete ado pr create",
        "kete ado vote",
        "kete ado reply",
        "az login",
    ] {
        assert!(
            CLAUDE_ORCHESTRATOR_GUIDANCE.contains(needle),
            "guidance lacks {needle}"
        );
    }
}

#[test]
fn sessions_come_back_in_attention_order_with_role_parent_and_state() {
    let daemon = daemon();
    let mut rows = Vec::new();
    for (id, status, unseen, at) in [
        ("rest", AgentStatus::Finished, false, 90),
        ("unread", AgentStatus::Finished, true, 10),
        ("run-old", AgentStatus::Running, false, 5),
        ("ask", AgentStatus::NeedsFeedback, false, 1),
        ("run-new", AgentStatus::Running, false, 50),
    ] {
        let mut a = agent(id, AgentRole::Worker);
        a.status = status;
        a.unseen = unseen;
        a.status_changed_at = at;
        rows.push(a);
    }
    rows[0].parent = Some(AgentId("orch".into()));
    rows.push(agent("orch", AgentRole::Orchestrator));
    for a in &rows {
        daemon.store.insert_agent(a).unwrap();
    }
    let sessions = daemon
        .orchestrator_sessions(&AgentId("orch".into()))
        .unwrap();
    let order: Vec<&str> = sessions.iter().map(|s| s.summary.id.as_str()).collect();
    assert_eq!(
        order,
        ["ask", "run-new", "run-old", "unread", "rest", "orch"]
    );
    assert_eq!(sessions[3].state, "unread");
    assert_eq!(sessions[0].state, "needs_feedback");
    assert_eq!(sessions[4].parent, Some(AgentId("orch".into())));
    assert_eq!(sessions[5].role, AgentRole::Orchestrator);
    assert!(sessions
        .iter()
        .enumerate()
        .all(|(i, s)| s.attention == i as u32));
}

#[test]
fn descent_walks_parents_up_to_a_session_root() {
    let parents: HashMap<u32, u32> = [(10, 1), (20, 10), (30, 20), (40, 1), (50, 50)]
        .into_iter()
        .collect();
    let roots: HashSet<u32> = [10].into_iter().collect();
    assert!(descends_from(&parents, 30, &roots), "a grandchild");
    assert!(descends_from(&parents, 10, &roots), "the root itself");
    assert!(!descends_from(&parents, 40, &roots), "a sibling tree");
    assert!(
        !descends_from(&parents, 50, &roots),
        "a self-parented pid ends"
    );
    assert!(!descends_from(&parents, 99, &roots), "an unknown pid");
}

/// Pinning demotes the previous holder, archived or live, broadcasts both
/// rows, and refuses rows that cannot run the orchestrator.
#[test]
fn set_orchestrator_demotes_the_previous_holder_and_broadcasts_both() {
    let daemon = daemon();
    daemon
        .store
        .insert_agent(&agent("old", AgentRole::Orchestrator))
        .unwrap();
    daemon
        .store
        .insert_agent(&agent("new", AgentRole::Worker))
        .unwrap();
    let mut rx = daemon.events.subscribe();
    daemon.set_orchestrator(&AgentId("new".into())).unwrap();
    let mut upserted = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        if let ServerEvent::EntityUpserted {
            entity: Entity::Agent(a),
        } = ev
        {
            upserted.push((a.id.0, a.role.as_str()));
        }
    }
    upserted.sort();
    assert_eq!(
        upserted,
        [
            ("new".to_string(), "orchestrator"),
            ("old".to_string(), "worker")
        ]
    );
    let err = daemon
        .orchestrator_tree(&AgentId("old".into()))
        .unwrap_err();
    assert!(err.to_string().contains("only available"), "{err}");

    let mut archived = agent("archived", AgentRole::Worker);
    archived.archived = true;
    daemon.store.insert_agent(&archived).unwrap();
    assert!(daemon.set_orchestrator(&archived.id).is_err());
    let mut cloud = agent("cloud", AgentRole::Worker);
    cloud.cloud_session_id = Some("cse_1".into());
    daemon.store.insert_agent(&cloud).unwrap();
    assert!(daemon.set_orchestrator(&cloud.id).is_err());
    assert_eq!(daemon.store.orchestrator().unwrap().unwrap().id.0, "new");

    daemon.clear_orchestrator().unwrap();
    assert!(daemon.store.orchestrator().unwrap().is_none());
}

#[tokio::test]
async fn a_second_start_is_refused_while_an_orchestrator_is_live() {
    let daemon = daemon();
    daemon
        .store
        .insert_agent(&agent("orch", AgentRole::Orchestrator))
        .unwrap();
    let err = daemon
        .start_orchestrator(StartOrchestratorSpec {
            project: Some(ProjectId("p".into())),
            ..Default::default()
        })
        .await
        .unwrap_err();
    assert!(err.to_string().contains("already running"), "{err}");
    assert_eq!(daemon.store.orchestrator().unwrap().unwrap().id.0, "orch");
}

/// `replace` starts a new orchestrator in the named project's root
/// worktree, demotes the old one (whose row stays, a worker now), and the
/// new row carries the role from its first spawn.
#[tokio::test]
async fn replace_starts_a_new_orchestrator_and_demotes_the_old() {
    let _cli = stand_in_cli();
    let dir = tempfile::tempdir().unwrap();
    let daemon = daemon_at(dir.path().to_str().unwrap());
    daemon
        .store
        .insert_agent(&agent("orch", AgentRole::Orchestrator))
        .unwrap();
    let created = daemon
        .start_orchestrator(StartOrchestratorSpec {
            project: Some(ProjectId("p".into())),
            replace: true,
            ..Default::default()
        })
        .await
        .unwrap();
    let EntityId::Agent(id) = created else {
        panic!("not an agent: {created:?}");
    };
    let pinned = daemon.store.orchestrator().unwrap().unwrap();
    assert_eq!(pinned.id, id);
    assert_eq!(pinned.name, DEFAULT_ORCHESTRATOR_NAME);
    assert_eq!(pinned.worktree_id, WorktreeId("w".into()));
    let old = daemon
        .store
        .get_agent(&AgentId("orch".into()))
        .unwrap()
        .unwrap();
    assert_eq!(old.role, AgentRole::Worker);
    daemon.kill_session(&SessionRef::Agent(id));
}

/// Two starts at once: provisioning is serialized, so exactly one wins and
/// the other sees it and is refused.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_starts_leave_exactly_one_orchestrator() {
    let _cli = stand_in_cli();
    let dir = tempfile::tempdir().unwrap();
    let daemon = daemon_at(dir.path().to_str().unwrap());
    let spec = || StartOrchestratorSpec {
        project: Some(ProjectId("p".into())),
        ..Default::default()
    };
    let (a, b) = tokio::join!(
        daemon.start_orchestrator(spec()),
        daemon.start_orchestrator(spec())
    );
    let wins: Vec<_> = [&a, &b].into_iter().filter(|r| r.is_ok()).collect();
    assert_eq!(wins.len(), 1, "{a:?} / {b:?}");
    let lost = [a, b].into_iter().find_map(|r| r.err()).unwrap();
    assert!(lost.to_string().contains("already running"), "{lost}");
    let pinned = daemon.store.orchestrator().unwrap().unwrap();
    daemon.kill_session(&SessionRef::Agent(pinned.id));
}

/// A data dir inside a git repo (a dotfiles checkout of the home dir, say):
/// the workspace is still a repo of its own, never the enclosing one, and
/// later starts keep finding it.
#[tokio::test]
async fn a_data_dir_inside_a_repo_still_gets_its_own_workspace_repo() {
    let _cli = stand_in_cli();
    let outer = tempfile::tempdir().unwrap();
    let ok = std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(outer.path())
        .status()
        .unwrap()
        .success();
    assert!(ok);
    let daemon = daemon();
    let home = outer.path().join("data").join("orchestrator");
    daemon.set_orchestrator_home(home.clone());
    for replace in [false, true] {
        let created = daemon
            .start_orchestrator(StartOrchestratorSpec {
                replace,
                ..Default::default()
            })
            .await
            .unwrap();
        if let EntityId::Agent(id) = created {
            daemon.kill_session(&SessionRef::Agent(id));
        }
    }
    assert!(home.join(".git").exists(), "the workspace is its own repo");
    let (projects, _, _, _) = daemon.store.load_tree().unwrap();
    let workspace: Vec<_> = projects
        .iter()
        .filter(|p| p.name == DEFAULT_ORCHESTRATOR_NAME)
        .collect();
    assert_eq!(workspace.len(), 1, "{projects:#?}");
    assert_eq!(
        std::fs::canonicalize(&workspace[0].repo_path).unwrap(),
        std::fs::canonicalize(&home).unwrap(),
        "never the enclosing repo"
    );
}

/// Started with no project, the orchestrator gets a workspace of its own
/// under the data dir: registered once as the `orchestrator` project and
/// reused by the next start, never one of the user's repos.
#[tokio::test]
async fn without_a_project_the_orchestrator_gets_its_own_workspace() {
    let _cli = stand_in_cli();
    let data = tempfile::tempdir().unwrap();
    let daemon = daemon();
    let home = data.path().join("orchestrator");
    daemon.set_orchestrator_home(home.clone());
    let first = daemon
        .start_orchestrator(StartOrchestratorSpec::default())
        .await
        .unwrap();
    let EntityId::Agent(first) = first else {
        panic!("not an agent");
    };
    assert!(home.join(".git").exists(), "the workspace is a git repo");
    let (projects, worktrees, _, _) = daemon.store.load_tree().unwrap();
    let project = projects
        .iter()
        .find(|p| p.name == DEFAULT_ORCHESTRATOR_NAME)
        .expect("the workspace project");
    let agent = daemon.store.get_agent(&first).unwrap().unwrap();
    let wt = worktrees
        .iter()
        .find(|w| w.id == agent.worktree_id)
        .unwrap();
    assert_eq!(wt.project_id, project.id);
    daemon.kill_session(&SessionRef::Agent(first.clone()));

    let second = daemon
        .start_orchestrator(StartOrchestratorSpec {
            replace: true,
            ..Default::default()
        })
        .await
        .unwrap();
    let (projects, _, _, _) = daemon.store.load_tree().unwrap();
    assert_eq!(
        projects
            .iter()
            .filter(|p| p.name == DEFAULT_ORCHESTRATOR_NAME)
            .count(),
        1,
        "the workspace is reused"
    );
    if let EntityId::Agent(id) = second {
        daemon.kill_session(&SessionRef::Agent(id));
    }
}
