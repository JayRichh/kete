//! The triage strip's window side (K6): file a note and hand it to the
//! desk, and the notes filed lately.
//!
//! The same flow as `kete studio note --triage` (the TUI's
//! `triage_strip::file_and_send`), through the window's own daemon
//! connection: the note is written (`nebula_core::studio::inbox_write`),
//! then one turn goes to the desk `choose` picks, and its one line comes
//! back within 120 s.

use crate::daemon::{Client, Model};
use crate::Res;
use nebula_core::studio::inbox_write::{self as iw, Desk, NewNote, Refused, Target};
use nebula_core::{AgentId, AgentKind, AgentRole, AgentStatus, ClientRequest, EntityId, Project};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::State;

/// The last route line this window got, by project id.
static ROUTES: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

/// How often a new desk's status is looked at while its first turn runs.
const POLL: Duration = Duration::from_millis(200);

fn project(model: &Model, repo: &str) -> Res<Project> {
    model
        .projects
        .iter()
        .find(|p| p.id.0 == repo)
        .cloned()
        .ok_or_else(|| "That repo is gone.".to_string())
}

fn inbox(root: &std::path::Path) -> Result<PathBuf, String> {
    iw::inbox_dir(root, nebula_core::studio::settings::for_repo(root).as_ref())
}

/// Whether the repo's committed roles file defines the desk's card.
async fn desk_defined(root: PathBuf) -> bool {
    crate::editors::blocking(move || {
        nebula_core::roles::load_committed(&root)
            .ok()
            .flatten()
            .is_some_and(|f| f.roles.contains_key(iw::DESK_ROLE))
    })
    .await
    .unwrap_or(false)
}

fn choose(model: &Model, repo: &Project, defined: bool, target: Target) -> Result<Desk, String> {
    let role = |id: &AgentId| model.roles.get(&id.0);
    let card = |id: &AgentId| role(id).map(|r| r.card.clone());
    let context = |id: &AgentId| {
        iw::context_percent(
            model.contexts.get(&id.0),
            role(id).and_then(|r| r.context_budget),
        )
    };
    iw::choose(
        &repo.id,
        &iw::Sessions {
            worktrees: &model.worktrees,
            agents: &model.agents,
            card: &card,
            context: &context,
        },
        defined,
        target,
    )
}

/// File `text` as an inbox note in `repo` and send it to `target` (the
/// triage desk, or the orchestrator).
#[tauri::command]
pub async fn triage_submit(
    client: State<'_, Arc<Client>>,
    repo: String,
    text: String,
    target: Option<String>,
) -> Res<Value> {
    submit(&client, repo, text, Target::parse(target.as_deref())).await
}

async fn submit(client: &Client, repo: String, text: String, target: Target) -> Res<Value> {
    let model = client.snapshot();
    let project = project(&model, &repo)?;
    let root = project.repo_path.clone();
    let note = NewNote {
        text,
        ..NewNote::default()
    };
    let written = crate::editors::blocking(move || {
        let dir = inbox(&root).map_err(Err)?;
        let now = nebula_core::clock::now_ms() / 1000;
        iw::write(&dir, &note, now).map_err(|e| match e {
            Refused::Duplicate(d) => Ok(d),
            Refused::Failed(why) => Err(why),
        })
    })
    .await?;
    let written = match written {
        Ok(w) => w,
        Err(Ok(d)) => {
            return Ok(json!({ "kind": "duplicate", "path": d.existing, "reason": d.reason }))
        }
        Err(Err(why)) => return Err(why),
    };
    let mut sent = json!({
        "kind": "filed", "id": written.id, "path": written.path,
        "to": null, "route": null, "busy": false, "unsent": null,
    });
    if client.daemon_protocol() < nebula_core::STUDIO_PROTOCOL {
        sent["unsent"] = "the running daemon is older than 0.60: run `kete kill` and reopen".into();
        return Ok(sent);
    }
    let defined = desk_defined(project.repo_path.clone()).await;
    let desk = match choose(&client.snapshot(), &project, defined, target) {
        Ok(desk) => desk,
        Err(why) => {
            sent["unsent"] = why.into();
            return Ok(sent);
        }
    };
    let (agent, turn) = match desk {
        Desk::Live(id) => {
            sent["to"] = "desk".into();
            let turn = client.ask(&id.0, &iw::desk_prompt(&written.path)).await;
            (id, turn.map(Some))
        }
        Desk::Orchestrator(id) => {
            sent["to"] = "orchestrator".into();
            let turn = client
                .ask(&id.0, &iw::orchestrator_prompt(&written.path))
                .await;
            (id, turn.map(Some))
        }
        Desk::Start(worktree) => {
            sent["to"] = "desk".into();
            let req_id = client.req_id();
            let created = client
                .rpc(
                    req_id,
                    ClientRequest::CreateAgent {
                        req_id,
                        worktree,
                        name: iw::DESK_NAME.into(),
                        kind: AgentKind::Claude,
                        custom_harness: None,
                        model: None,
                        effort: None,
                        auto_title: false,
                        cloud_prompt: None,
                        starting_prompt: Some(iw::desk_prompt(&written.path)),
                        issue_url: None,
                        role: AgentRole::Worker,
                        profile: None,
                        role_card: Some(iw::DESK_ROLE.into()),
                    },
                )
                .await;
            let id = match created {
                Ok(Some(EntityId::Agent(id))) => id,
                Ok(_) => {
                    sent["unsent"] = "the daemon created no desk".into();
                    return Ok(sent);
                }
                Err(e) => {
                    sent["unsent"] = e.to_string().into();
                    return Ok(sent);
                }
            };
            let done = first_turn(client, &id).await;
            (id, Ok(done.then(String::new)))
        }
    };
    match turn {
        Ok(Some(answer)) => {
            let read = client.read_session(&agent.0, 40).await.ok();
            let route = read
                .as_deref()
                .and_then(iw::reply_line)
                .or_else(|| iw::reply_line(&answer));
            if let Some(line) = &route {
                remember(&repo, line);
            }
            sent["route"] = route.into();
        }
        Ok(None) => sent["busy"] = true.into(),
        Err(e) if e.to_string().contains("timed out") || e.to_string().contains("in time") => {
            sent["busy"] = true.into()
        }
        Err(e) => sent["unsent"] = e.to_string().into(),
    }
    Ok(sent)
}

/// Whether a desk just created ends its first turn within the wait: its
/// row leaves Fresh and Running for an idle status.
async fn first_turn(client: &Client, id: &AgentId) -> bool {
    let deadline = tokio::time::Instant::now() + iw::REPLY_WAIT;
    while tokio::time::Instant::now() < deadline {
        let status = client
            .snapshot()
            .agents
            .iter()
            .find(|a| &a.id == id)
            .map(|a| a.status);
        if turn_over(status) {
            return true;
        }
        tokio::time::sleep(POLL).await;
    }
    false
}

/// A new desk's first turn is over once its row says it stopped.
fn turn_over(status: Option<AgentStatus>) -> bool {
    matches!(
        status,
        Some(AgentStatus::Finished | AgentStatus::Terminated | AgentStatus::Disconnected)
    )
}

fn remember(repo: &str, line: &str) {
    let mut routes = ROUTES.lock().unwrap_or_else(|e| e.into_inner());
    routes
        .get_or_insert_with(HashMap::new)
        .insert(repo.to_string(), line.to_string());
}

fn last_route(repo: &str) -> Option<String> {
    let routes = ROUTES.lock().unwrap_or_else(|e| e.into_inner());
    routes.as_ref().and_then(|r| r.get(repo).cloned())
}

/// The route line and the last notes filed in `repo`.
#[tauri::command]
pub async fn triage_tail(client: State<'_, Arc<Client>>, repo: String) -> Res<Value> {
    let model = client.snapshot();
    let project = project(&model, &repo)?;
    let defined = desk_defined(project.repo_path.clone()).await;
    let root = project.repo_path.clone();
    let (dir, notes) = crate::editors::blocking(move || match inbox(&root) {
        Ok(dir) => {
            let notes = iw::latest(&dir, 3);
            (Ok(dir), notes)
        }
        Err(why) => (Err(why), Vec::new()),
    })
    .await?;
    Ok(tail(
        &model,
        &project,
        dir,
        notes,
        defined,
        last_route(&repo),
    ))
}

fn tail(
    model: &Model,
    project: &Project,
    dir: Result<PathBuf, String>,
    notes: Vec<nebula_core::studio::inbox::InboxNote>,
    defined: bool,
    route: Option<String>,
) -> Value {
    let root = model
        .worktrees
        .iter()
        .find(|w| w.project_id == project.id && w.is_main)
        .map(|w| w.id.clone());
    let desk_runs = model.agents.iter().any(|a| {
        !a.archived
            && Some(&a.worktree_id) == root.as_ref()
            && model
                .roles
                .get(&a.id.0)
                .is_some_and(|r| r.card == iw::DESK_ROLE)
    });
    let orchestrator = model
        .agents
        .iter()
        .any(|a| a.role.is_orchestrator() && !a.archived);
    let (inbox, error) = match dir {
        Ok(dir) => (Some(dir), None),
        Err(why) => (None, Some(why)),
    };
    json!({
        "repo": project.id.0,
        "name": project.name,
        "inbox": inbox,
        "error": error,
        "desk": defined || desk_runs,
        "orchestrator": orchestrator,
        "route": route,
        "notes": notes.iter().map(|n| json!({
            "id": n.id, "title": n.title, "path": n.path,
            "status": n.status.as_str(), "to": n.to, "at": n.at,
        })).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use nebula_core::{ProjectId, Worktree, WorktreeId};

    fn model_with(repo_path: &std::path::Path) -> (Model, Project) {
        let project = Project {
            id: ProjectId("p1".into()),
            name: "game".into(),
            repo_path: repo_path.to_path_buf(),
            sort_order: 0,
        };
        let model = Model {
            projects: vec![project.clone()],
            worktrees: vec![Worktree {
                id: WorktreeId("w1".into()),
                project_id: project.id.clone(),
                path: repo_path.to_path_buf(),
                branch: "main".into(),
                is_main: true,
                sort_order: 0,
            }],
            ..Model::default()
        };
        (model, project)
    }

    #[test]
    fn a_first_turn_ends_on_a_stopped_row() {
        assert!(!turn_over(None));
        assert!(!turn_over(Some(AgentStatus::Fresh)));
        assert!(!turn_over(Some(AgentStatus::Running)));
        assert!(turn_over(Some(AgentStatus::Finished)));
    }

    #[test]
    fn the_tail_lists_the_newest_notes_and_who_can_take_one() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".kete")).unwrap();
        std::fs::write(
            dir.path().join(".kete/studio.toml"),
            "version = 1\ninbox = \"notes/inbox\"\n",
        )
        .unwrap();
        let (model, project) = model_with(dir.path());
        let inbox = inbox(dir.path()).unwrap();
        for (i, text) in ["one", "two", "three", "four"].iter().enumerate() {
            let note = NewNote {
                text: (*text).into(),
                ..NewNote::default()
            };
            iw::write(&inbox, &note, 1_792_632_120 + i as i64 * 60).unwrap();
        }
        let notes = iw::latest(&inbox, 3);
        let v = tail(&model, &project, Ok(inbox), notes, true, Some("art".into()));
        let titles: Vec<&str> = v["notes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n["title"].as_str().unwrap())
            .collect();
        assert_eq!(titles, ["four", "three", "two"]);
        assert_eq!(v["desk"], true);
        assert_eq!(v["orchestrator"], false);
        assert_eq!(v["route"], "art");
        assert_eq!(v["error"], Value::Null);
        let none = tail(
            &model,
            &project,
            Err("no inbox".into()),
            Vec::new(),
            false,
            None,
        );
        assert_eq!(
            (none["error"].as_str(), none["desk"].as_bool()),
            (Some("no inbox"), Some(false))
        );
    }

    #[tokio::test]
    async fn a_note_with_no_one_to_take_it_is_filed_unsent() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".kete")).unwrap();
        std::fs::write(
            dir.path().join(".kete/studio.toml"),
            "version = 1\ninbox = \"notes/inbox\"\n",
        )
        .unwrap();
        let (client, _rx) = Client::connected_for_test();
        let (model, _) = model_with(dir.path());
        client.set_model(|m| {
            m.projects = model.projects.clone();
            m.worktrees = model.worktrees.clone();
            m.protocol_version = nebula_core::STUDIO_PROTOCOL;
        });
        let sent = submit(&client, "p1".into(), "storm sky".into(), Target::Desk)
            .await
            .unwrap();
        assert_eq!(sent["kind"], "filed");
        assert_eq!(sent["to"], Value::Null);
        assert!(
            sent["unsent"].as_str().unwrap().contains("no triage desk"),
            "{sent}"
        );
        let again = submit(&client, "p1".into(), "storm sky".into(), Target::Desk)
            .await
            .unwrap();
        assert_eq!(again["kind"], "duplicate", "{again}");
    }
}
