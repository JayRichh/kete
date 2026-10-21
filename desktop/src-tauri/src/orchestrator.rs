//! The orchestrator from the window: start one, pin or unpin a session,
//! and read a session's transcript for the review sheet. The provisioning
//! requests are protocol 54; a 53 daemon drops the whole connection on a
//! request it cannot decode, so each is refused here, before it is sent,
//! when the daemon is older.

use crate::daemon::Client;
use crate::sessions::ack;
use crate::{created_id, err, settings, Res};
use nebula_config::Config;
use nebula_core::{AgentId, ClientRequest};
use serde::Deserialize;
use std::sync::Arc;
use tauri::State;

/// The first protocol with `StartOrchestrator`, `SetOrchestrator` and
/// `ClearOrchestrator`.
pub const ORCHESTRATOR_PROTOCOL: u32 = 54;

/// What the window says when the daemon is too old for the orchestrator.
#[cfg(unix)]
pub const RELOAD_HINT: &str =
    "The running kete daemon is older than this window. Run `kete reload`, then try again.";
/// Windows daemons can't restart in place: `kete reload` refuses there.
#[cfg(windows)]
pub const RELOAD_HINT: &str = "The running kete daemon is older than this window. Run `kete kill` (stops all sessions), then reopen kete.";

/// How many transcript lines the review sheet's History tab asks for.
pub const HISTORY_LINES: u32 = 400;

/// Refuse unless the connected daemon speaks the orchestrator requests.
pub fn require_orchestrator(client: &Client) -> Res<()> {
    match client.daemon_protocol() {
        0 => Err("Not connected to the kete daemon.".into()),
        v if v < ORCHESTRATOR_PROTOCOL => Err(RELOAD_HINT.into()),
        _ => Ok(()),
    }
}

/// The Start orchestrator form.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartSpec {
    /// A harness id as the quick prompt offers it (a custom one by id).
    pub kind: String,
    #[serde(default)]
    pub profile: Option<String>,
    /// None takes the Agents tab's model, as a quick prompt launch does.
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub effort: Option<String>,
    /// Run in this project's root worktree; None: a workspace of its own.
    #[serde(default)]
    pub project: Option<String>,
    /// The session's name; None (or blank): the daemon's.
    #[serde(default)]
    pub name: Option<String>,
    /// Replace a running orchestrator (it stays, as a worker).
    #[serde(default)]
    pub replace: bool,
}

fn some(s: Option<String>) -> Option<String> {
    s.map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

/// Start the orchestrator in the daemon's own workspace; its agent id.
pub async fn start_orchestrator_in(client: &Client, cfg: &Config, spec: StartSpec) -> Res<String> {
    require_orchestrator(client)?;
    let launch = settings::launch_args(cfg, spec.kind.trim(), some(spec.model), some(spec.effort))?;
    let profile = some(spec.profile);
    let created = ack(client, |req_id| ClientRequest::StartOrchestrator {
        req_id,
        kind: launch.kind,
        custom_harness: launch.custom,
        model: launch.model,
        effort: launch.effort,
        profile,
        project: some(spec.project).map(nebula_core::ProjectId),
        name: some(spec.name),
        replace: spec.replace,
        role_card: None,
    })
    .await?;
    Ok(created_id(created))
}

/// Pin `agent` as the orchestrator (whoever held it becomes a worker).
pub async fn set_orchestrator_in(client: &Client, agent: String) -> Res<()> {
    require_orchestrator(client)?;
    if !client.snapshot().agents.iter().any(|a| a.id.0 == agent) {
        return Err("That session is gone.".into());
    }
    ack(client, |req_id| ClientRequest::SetOrchestrator {
        req_id,
        agent: AgentId(agent),
    })
    .await
    .map(drop)
}

/// Unpin the orchestrator; it keeps running as a worker.
pub async fn clear_orchestrator_in(client: &Client) -> Res<()> {
    require_orchestrator(client)?;
    ack(client, |req_id| ClientRequest::ClearOrchestrator { req_id })
        .await
        .map(drop)
}

#[tauri::command]
pub async fn start_orchestrator(client: State<'_, Arc<Client>>, spec: StartSpec) -> Res<String> {
    start_orchestrator_in(&client, &Config::load(), spec).await
}

#[tauri::command]
pub async fn set_orchestrator(client: State<'_, Arc<Client>>, agent: String) -> Res<()> {
    set_orchestrator_in(&client, agent).await
}

#[tauri::command]
pub async fn clear_orchestrator(client: State<'_, Arc<Client>>) -> Res<()> {
    clear_orchestrator_in(&client).await
}

/// A session's transcript (else its terminal tail), for the review
/// sheet's History tab. `ReadSession` is older than 54: no guard.
#[tauri::command]
pub async fn read_session(client: State<'_, Arc<Client>>, agent: String) -> Res<String> {
    client
        .read_session(&agent, HISTORY_LINES)
        .await
        .map_err(err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sessions::testkit::{self, ok};
    use nebula_core::{AgentKind, EntityId};

    fn connected(
        protocol: u32,
    ) -> (
        Arc<Client>,
        tokio::sync::mpsc::UnboundedReceiver<ClientRequest>,
    ) {
        let (client, rx) = Client::connected_for_test();
        client.set_model(|m| m.protocol_version = protocol);
        (client, rx)
    }

    #[tokio::test]
    async fn a_53_daemon_is_never_sent_a_provisioning_request() {
        let (client, mut rx) = connected(53);
        let cfg = Config::default();
        let start = start_orchestrator_in(
            &client,
            &cfg,
            StartSpec {
                kind: "claude".into(),
                ..Default::default()
            },
        )
        .await;
        assert_eq!(start.unwrap_err(), RELOAD_HINT);
        assert_eq!(
            clear_orchestrator_in(&client).await.unwrap_err(),
            RELOAD_HINT
        );
        assert_eq!(
            set_orchestrator_in(&client, "a".into()).await.unwrap_err(),
            RELOAD_HINT
        );
        assert!(rx.try_recv().is_err(), "nothing may reach a 53 daemon");
    }

    #[tokio::test]
    async fn start_sends_the_harness_with_the_agents_tab_defaults() {
        let (client, mut rx) = connected(54);
        let cfg = Config {
            claude_model: "opus".into(),
            ..Default::default()
        };
        let (out, seen) = testkit::run(
            &client,
            &mut rx,
            start_orchestrator_in(
                &client,
                &cfg,
                StartSpec {
                    kind: "claude".into(),
                    profile: Some(" work ".into()),
                    project: Some("p1".into()),
                    name: Some("  ".into()),
                    replace: true,
                    ..Default::default()
                },
            ),
            |_| Ok(Some(EntityId::Agent(AgentId("orch".into())))),
        )
        .await;
        assert_eq!(out.unwrap(), "orch");
        let [ClientRequest::StartOrchestrator {
            kind,
            model,
            profile,
            project,
            name,
            replace,
            ..
        }] = seen.as_slice()
        else {
            panic!("expected one StartOrchestrator: {seen:?}");
        };
        assert_eq!(*kind, AgentKind::Claude);
        assert_eq!(model.as_deref(), Some("opus"));
        assert_eq!(profile.as_deref(), Some("work"));
        assert_eq!(project.as_ref().map(|p| p.0.as_str()), Some("p1"));
        assert_eq!(*name, None, "a blank name is the daemon's");
        assert!(*replace);
    }

    #[tokio::test]
    async fn pin_and_unpin_send_their_requests() {
        let (client, mut rx) = connected(54);
        client.set_model(|m| m.agents.push(bare_agent("a")));
        let (out, seen) = testkit::run(
            &client,
            &mut rx,
            set_orchestrator_in(&client, "a".into()),
            ok,
        )
        .await;
        out.unwrap();
        assert!(matches!(
            seen.as_slice(),
            [ClientRequest::SetOrchestrator { agent, .. }] if agent.0 == "a"
        ));
        let (out, seen) = testkit::run(&client, &mut rx, clear_orchestrator_in(&client), ok).await;
        out.unwrap();
        assert!(matches!(
            seen.as_slice(),
            [ClientRequest::ClearOrchestrator { .. }]
        ));
        assert_eq!(
            set_orchestrator_in(&client, "gone".into())
                .await
                .unwrap_err(),
            "That session is gone."
        );
    }

    fn bare_agent(id: &str) -> nebula_core::Agent {
        nebula_core::Agent {
            id: AgentId(id.into()),
            worktree_id: nebula_core::WorktreeId("w".into()),
            name: id.into(),
            status: nebula_core::AgentStatus::Fresh,
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
            role: nebula_core::AgentRole::Worker,
            sort_order: 0,
            alive: true,
            recent_prompts: Vec::new(),
            profile: None,
            parent: None,
        }
    }
}
