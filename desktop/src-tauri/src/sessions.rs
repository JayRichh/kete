//! Session management from the window: rename, archive and unarchive,
//! delete, restart, move to another checkout, a follow-up turn, and the
//! terminal verbs (rename, close), and the follow-ups that start a session
//! of their own (Start as new session, Ask aside). Each is one daemon
//! request answered by
//! `Ack`, the same ones the TUI's card menu sends; the window confirms
//! the destructive ones before it calls here.

use crate::daemon::Client;
use crate::{err, Res};
use nebula_core::{AgentId, ClientRequest, EntityId, SessionRef, TerminalId, WorktreeId};
use std::sync::Arc;
use tauri::State;

/// Send the request `build` makes with a fresh req_id and wait for its Ack.
pub async fn ack(
    client: &Client,
    build: impl FnOnce(u64) -> ClientRequest,
) -> Res<Option<EntityId>> {
    let req_id = client.req_id();
    client.rpc(req_id, build(req_id)).await.map_err(err)
}

fn name_of(name: &str, what: &str) -> Res<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(format!("A {what} needs a name."));
    }
    Ok(name.to_string())
}

pub async fn rename_agent_in(client: &Client, agent: String, name: String) -> Res<()> {
    let name = name_of(&name, "session")?;
    ack(client, |req_id| ClientRequest::RenameAgent {
        req_id,
        id: AgentId(agent),
        name,
    })
    .await
    .map(drop)
}

pub async fn archive_agent_in(client: &Client, agent: String) -> Res<()> {
    ack(client, |req_id| ClientRequest::ArchiveAgent {
        req_id,
        id: AgentId(agent),
    })
    .await
    .map(drop)
}

pub async fn unarchive_agent_in(client: &Client, agent: String) -> Res<()> {
    ack(client, |req_id| ClientRequest::UnarchiveAgent {
        req_id,
        id: AgentId(agent),
    })
    .await
    .map(drop)
}

pub async fn delete_agent_in(client: &Client, agent: String) -> Res<()> {
    ack(client, |req_id| ClientRequest::DeleteAgent {
        req_id,
        id: AgentId(agent),
    })
    .await
    .map(drop)
}

pub async fn restart_agent_in(client: &Client, agent: String) -> Res<()> {
    ack(client, |req_id| ClientRequest::RestartAgent {
        req_id,
        id: AgentId(agent),
    })
    .await
    .map(drop)
}

pub async fn move_agent_in(client: &Client, agent: String, worktree: String) -> Res<()> {
    let model = client.snapshot();
    let Some(a) = model.agents.iter().find(|a| a.id.0 == agent) else {
        return Err("That session is gone.".into());
    };
    if a.worktree_id.0 == worktree {
        return Err("The session is already in that worktree.".into());
    }
    if !model.worktrees.iter().any(|w| w.id.0 == worktree) {
        return Err("That worktree is gone.".into());
    }
    ack(client, |req_id| ClientRequest::MoveAgent {
        req_id,
        id: AgentId(agent),
        worktree: WorktreeId(worktree),
    })
    .await
    .map(drop)
}

pub async fn rename_terminal_in(client: &Client, terminal: String, name: String) -> Res<()> {
    let name = name_of(&name, "terminal")?;
    ack(client, |req_id| ClientRequest::RenameTerminal {
        req_id,
        id: TerminalId(terminal),
        name,
    })
    .await
    .map(drop)
}

pub async fn close_terminal_in(client: &Client, terminal: String) -> Res<()> {
    ack(client, |req_id| ClientRequest::CloseTerminal {
        req_id,
        id: TerminalId(terminal),
    })
    .await
    .map(drop)
}

/// A turn typed at a session from outside its pane, as the TUI's
/// follow-up box sends it: the text (bracketed paste when it has a line
/// break, so the CLI takes it as one message), then Enter on its own.
pub fn follow_up_in(client: &Client, agent: String, text: String) -> Res<()> {
    let text = text.trim_end();
    if text.trim().is_empty() {
        return Err("The follow-up is empty.".into());
    }
    let model = client.snapshot();
    let Some(a) = model.agents.iter().find(|a| a.id.0 == agent) else {
        return Err("That session is gone.".into());
    };
    if !a.alive {
        return Err(format!(
            "{} is not running: open or restart it first, then send the follow-up.",
            a.name
        ));
    }
    let session = SessionRef::Agent(AgentId(agent));
    client
        .send(ClientRequest::Input {
            session: session.clone(),
            data: turn_bytes(text),
        })
        .map_err(err)?;
    client
        .send(ClientRequest::Input {
            session,
            data: b"\r".to_vec(),
        })
        .map_err(err)
}

/// The bytes a turn's text goes down the PTY as.
pub fn turn_bytes(text: &str) -> Vec<u8> {
    if text.contains('\n') {
        let mut out = b"\x1b[200~".to_vec();
        out.extend_from_slice(text.as_bytes());
        out.extend_from_slice(b"\x1b[201~");
        out
    } else {
        text.as_bytes().to_vec()
    }
}

/// Start `prompt` as a session of its own beside `agent`: the daemon's
/// `SpawnSiblingAgent`, as `kete spawn` sends it from inside a session. The
/// new session runs in the same checkout on the same harness, model,
/// effort and account, and nests under `agent`. Returns its id, for the
/// window to open.
pub async fn spawn_follow_up_in(client: &Client, agent: String, prompt: String) -> Res<String> {
    let prompt = prompt.trim();
    if prompt.is_empty() {
        return Err("The follow-up is empty.".into());
    }
    if prompt.len() > nebula_core::MAX_CLOUD_PROMPT_BYTES {
        return Err(format!(
            "The follow-up is too long (max {} KiB).",
            nebula_core::MAX_CLOUD_PROMPT_BYTES / 1024
        ));
    }
    let model = client.snapshot();
    let Some(a) = model.agents.iter().find(|a| a.id.0 == agent) else {
        return Err("That session is gone.".into());
    };
    if a.archived {
        return Err(format!(
            "{} is archived: unarchive it to start a session from it.",
            a.name
        ));
    }
    let starting_prompt = prompt.to_string();
    match ack(client, |req_id| ClientRequest::SpawnSiblingAgent {
        req_id,
        id: AgentId(agent),
        kind: None,
        starting_prompt,
        worktree: None,
        base: None,
        profile: None,
    })
    .await?
    {
        Some(EntityId::Agent(id)) => Ok(id.0),
        _ => Err("The daemon started no session.".into()),
    }
}

/// ASK ASIDE: `question` about `agent`, answered by a session of its own
/// beside it (`nebula_core::follow_up::aside_prompt`).
pub async fn ask_aside_in(client: &Client, agent: String, question: String) -> Res<String> {
    if question.trim().is_empty() {
        return Err("The question is empty.".into());
    }
    let name = client
        .snapshot()
        .agents
        .iter()
        .find(|a| a.id.0 == agent)
        .map(|a| a.name.clone())
        .ok_or_else(|| "That session is gone.".to_string())?;
    let prompt = nebula_core::follow_up::aside_prompt(&name, &question);
    spawn_follow_up_in(client, agent, prompt).await
}

#[tauri::command]
pub async fn spawn_follow_up(
    client: State<'_, Arc<Client>>,
    agent: String,
    prompt: String,
) -> Res<String> {
    spawn_follow_up_in(&client, agent, prompt).await
}

#[tauri::command]
pub async fn ask_aside(
    client: State<'_, Arc<Client>>,
    agent: String,
    question: String,
) -> Res<String> {
    ask_aside_in(&client, agent, question).await
}

#[tauri::command]
pub async fn rename_agent(client: State<'_, Arc<Client>>, agent: String, name: String) -> Res<()> {
    rename_agent_in(&client, agent, name).await
}

#[tauri::command]
pub async fn archive_agent(client: State<'_, Arc<Client>>, agent: String) -> Res<()> {
    archive_agent_in(&client, agent).await
}

#[tauri::command]
pub async fn unarchive_agent(client: State<'_, Arc<Client>>, agent: String) -> Res<()> {
    unarchive_agent_in(&client, agent).await
}

#[tauri::command]
pub async fn delete_agent(client: State<'_, Arc<Client>>, agent: String) -> Res<()> {
    delete_agent_in(&client, agent).await
}

#[tauri::command]
pub async fn restart_agent(client: State<'_, Arc<Client>>, agent: String) -> Res<()> {
    restart_agent_in(&client, agent).await
}

#[tauri::command]
pub async fn move_agent(
    client: State<'_, Arc<Client>>,
    agent: String,
    worktree: String,
) -> Res<()> {
    move_agent_in(&client, agent, worktree).await
}

#[tauri::command]
pub async fn rename_terminal(
    client: State<'_, Arc<Client>>,
    terminal: String,
    name: String,
) -> Res<()> {
    rename_terminal_in(&client, terminal, name).await
}

#[tauri::command]
pub async fn close_terminal(client: State<'_, Arc<Client>>, terminal: String) -> Res<()> {
    close_terminal_in(&client, terminal).await
}

#[tauri::command]
pub fn follow_up(client: State<'_, Arc<Client>>, agent: String, text: String) -> Res<()> {
    follow_up_in(&client, agent, text)
}

#[cfg(test)]
pub(crate) mod testkit {
    //! A stand-in daemon: run a command against a test client, take the
    //! one request it sends, and answer it.
    use crate::daemon::Client;
    use nebula_core::{ClientRequest, EntityId, ServerEvent};
    use std::future::Future;
    use std::sync::Arc;
    use tokio::sync::mpsc::UnboundedReceiver;

    /// The req_id a request carries, for the reply.
    pub fn req_id(req: &ClientRequest) -> u64 {
        let v = serde_json::to_value(req).unwrap();
        v.as_object()
            .and_then(|o| o.values().next())
            .and_then(|inner| inner.get("req_id"))
            .and_then(|r| r.as_u64())
            .unwrap_or_else(|| panic!("no req_id on {req:?}"))
    }

    /// Run `call` to completion while answering each request it sends
    /// with `reply` (Ack with `created`, or an Error with the message).
    /// Returns the call's result and every request seen, in order.
    pub async fn run<T>(
        client: &Arc<Client>,
        rx: &mut UnboundedReceiver<ClientRequest>,
        call: impl Future<Output = T>,
        reply: impl Fn(&ClientRequest) -> Result<Option<EntityId>, String>,
    ) -> (T, Vec<ClientRequest>) {
        let mut seen = Vec::new();
        tokio::pin!(call);
        loop {
            tokio::select! {
                out = &mut call => {
                    while let Ok(req) = rx.try_recv() {
                        seen.push(req);
                    }
                    return (out, seen);
                }
                Some(req) = rx.recv() => {
                    let has_id = serde_json::to_value(&req).unwrap().as_object()
                        .and_then(|o| o.values().next())
                        .is_some_and(|inner| inner.get("req_id").is_some());
                    if has_id {
                        let id = req_id(&req);
                        client.answer(match reply(&req) {
                            Ok(created) => ServerEvent::Ack { req_id: id, created },
                            Err(message) => ServerEvent::Error { req_id: Some(id), message },
                        });
                    }
                    seen.push(req);
                }
            }
        }
    }

    pub fn ok(_: &ClientRequest) -> Result<Option<EntityId>, String> {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::testkit::{ok, run};
    use super::*;
    use nebula_core::{Agent, ProjectId, TerminalTab, Worktree};

    pub(crate) fn agent(id: &str, worktree: &str, alive: bool) -> Agent {
        let mut a: Agent = serde_json::from_value(serde_json::json!({
            "id": id, "worktree_id": worktree, "name": id, "status": "finished",
            "archived": false, "unseen": false, "status_changed_at": 0, "kind": "claude",
            "sort_order": 0, "alive": alive
        }))
        .unwrap();
        a.alive = alive;
        a
    }

    fn worktree(id: &str) -> Worktree {
        Worktree {
            id: WorktreeId(id.into()),
            project_id: ProjectId("p".into()),
            path: std::env::temp_dir().join(id),
            branch: id.into(),
            is_main: false,
            sort_order: 0,
        }
    }

    #[tokio::test]
    async fn each_session_verb_sends_its_request_and_waits_for_the_ack() {
        let (client, mut rx) = Client::connected_for_test();
        let (r, seen) = run(
            &client,
            &mut rx,
            rename_agent_in(&client, "a1".into(), "  fix it ".into()),
            ok,
        )
        .await;
        r.unwrap();
        assert!(
            matches!(&seen[0], ClientRequest::RenameAgent { id, name, .. } if id.0 == "a1" && name == "fix it")
        );

        let (r, seen) = run(&client, &mut rx, archive_agent_in(&client, "a1".into()), ok).await;
        r.unwrap();
        assert!(matches!(&seen[0], ClientRequest::ArchiveAgent { id, .. } if id.0 == "a1"));

        let (r, seen) = run(
            &client,
            &mut rx,
            unarchive_agent_in(&client, "a1".into()),
            ok,
        )
        .await;
        r.unwrap();
        assert!(matches!(&seen[0], ClientRequest::UnarchiveAgent { id, .. } if id.0 == "a1"));

        let (r, seen) = run(&client, &mut rx, delete_agent_in(&client, "a1".into()), ok).await;
        r.unwrap();
        assert!(matches!(&seen[0], ClientRequest::DeleteAgent { id, .. } if id.0 == "a1"));

        let (r, seen) = run(&client, &mut rx, restart_agent_in(&client, "a1".into()), ok).await;
        r.unwrap();
        assert!(matches!(&seen[0], ClientRequest::RestartAgent { id, .. } if id.0 == "a1"));

        let (r, seen) = run(
            &client,
            &mut rx,
            rename_terminal_in(&client, "t1".into(), "logs".into()),
            ok,
        )
        .await;
        r.unwrap();
        assert!(
            matches!(&seen[0], ClientRequest::RenameTerminal { id, name, .. } if id.0 == "t1" && name == "logs")
        );

        let (r, seen) = run(
            &client,
            &mut rx,
            close_terminal_in(&client, "t1".into()),
            ok,
        )
        .await;
        r.unwrap();
        assert!(matches!(&seen[0], ClientRequest::CloseTerminal { id, .. } if id.0 == "t1"));
    }

    #[tokio::test]
    async fn a_daemon_error_comes_back_as_the_commands_error() {
        let (client, mut rx) = Client::connected_for_test();
        let (r, _) = run(
            &client,
            &mut rx,
            delete_agent_in(&client, "a1".into()),
            |_| Err("no such agent".into()),
        )
        .await;
        assert_eq!(r.unwrap_err(), "no such agent");
    }

    #[tokio::test]
    async fn an_empty_name_is_refused_before_anything_is_sent() {
        let (client, mut rx) = Client::connected_for_test();
        let (r, seen) = run(
            &client,
            &mut rx,
            rename_agent_in(&client, "a1".into(), "  ".into()),
            ok,
        )
        .await;
        assert!(r.is_err());
        assert!(seen.is_empty());
        let (r, seen) = run(
            &client,
            &mut rx,
            rename_terminal_in(&client, "t1".into(), "".into()),
            ok,
        )
        .await;
        assert!(r.is_err());
        assert!(seen.is_empty());
    }

    #[tokio::test]
    async fn a_move_goes_to_another_worktree_that_exists() {
        let (client, mut rx) = Client::connected_for_test();
        client.set_model(|m| {
            m.agents = vec![agent("a1", "w1", true)];
            m.worktrees = vec![worktree("w1"), worktree("w2")];
        });
        let (r, seen) = run(
            &client,
            &mut rx,
            move_agent_in(&client, "a1".into(), "w2".into()),
            ok,
        )
        .await;
        r.unwrap();
        assert!(
            matches!(&seen[0], ClientRequest::MoveAgent { id, worktree, .. } if id.0 == "a1" && worktree.0 == "w2")
        );
        for (to, why) in [("w1", "same"), ("w9", "gone")] {
            let (r, seen) = run(
                &client,
                &mut rx,
                move_agent_in(&client, "a1".into(), to.into()),
                ok,
            )
            .await;
            assert!(r.is_err(), "{why}");
            assert!(seen.is_empty(), "{why}");
        }
    }

    #[tokio::test]
    async fn a_follow_up_types_the_text_then_enter_into_a_live_session() {
        let (client, mut rx) = Client::connected_for_test();
        client.set_model(|m| {
            m.agents = vec![agent("a1", "w1", true), agent("dead", "w1", false)];
            m.terminals = Vec::<TerminalTab>::new();
        });
        follow_up_in(&client, "a1".into(), "run the tests".into()).unwrap();
        let first = rx.try_recv().unwrap();
        let second = rx.try_recv().unwrap();
        assert!(matches!(first, ClientRequest::Input { ref data, .. } if data == b"run the tests"));
        assert!(matches!(second, ClientRequest::Input { ref data, .. } if data == b"\r"));

        follow_up_in(&client, "a1".into(), "one\ntwo\n".into()).unwrap();
        let first = rx.try_recv().unwrap();
        assert!(
            matches!(first, ClientRequest::Input { ref data, .. } if data == b"\x1b[200~one\ntwo\x1b[201~")
        );
        let _ = rx.try_recv();

        assert!(follow_up_in(&client, "dead".into(), "hi".into())
            .unwrap_err()
            .contains("restart"));
        assert!(follow_up_in(&client, "a1".into(), "  ".into()).is_err());
        assert!(
            rx.try_recv().is_err(),
            "nothing sent for a refused follow-up"
        );
    }

    #[tokio::test]
    async fn a_follow_up_session_is_spawned_beside_the_session_and_its_id_returned() {
        let (client, mut rx) = Client::connected_for_test();
        let mut archived = agent("old", "w1", false);
        archived.archived = true;
        client.set_model(|m| m.agents = vec![agent("a1", "w1", true), archived]);
        let created = |_: &ClientRequest| Ok(Some(EntityId::Agent(AgentId("a2".into()))));
        let (r, seen) = run(
            &client,
            &mut rx,
            spawn_follow_up_in(&client, "a1".into(), "  write the tests \n".into()),
            created,
        )
        .await;
        assert_eq!(r.unwrap(), "a2");
        assert!(matches!(
            &seen[0],
            ClientRequest::SpawnSiblingAgent {
                id,
                kind: None,
                starting_prompt,
                worktree: None,
                base: None,
                profile: None,
                ..
            } if id.0 == "a1" && starting_prompt == "write the tests"
        ));

        let (r, seen) = run(
            &client,
            &mut rx,
            ask_aside_in(&client, "a1".into(), "why two refreshes?".into()),
            created,
        )
        .await;
        assert_eq!(r.unwrap(), "a2");
        let ClientRequest::SpawnSiblingAgent {
            starting_prompt, ..
        } = &seen[0]
        else {
            panic!("not a spawn: {:?}", seen[0]);
        };
        assert_eq!(
            starting_prompt,
            &nebula_core::follow_up::aside_prompt("a1", "why two refreshes?")
        );

        for (id, text, why) in [
            ("a1", "   ", "empty"),
            ("old", "go", "archived"),
            ("gone", "go", "gone"),
        ] {
            let (r, seen) = run(
                &client,
                &mut rx,
                spawn_follow_up_in(&client, id.into(), text.into()),
                created,
            )
            .await;
            assert!(r.is_err(), "{why}");
            assert!(seen.is_empty(), "{why}");
        }
        let (r, _) = run(
            &client,
            &mut rx,
            spawn_follow_up_in(&client, "a1".into(), "go".into()),
            |_| Err("agent is archived".into()),
        )
        .await;
        assert_eq!(r.unwrap_err(), "agent is archived");
    }
}
