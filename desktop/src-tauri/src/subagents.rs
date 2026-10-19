//! The board's cleanup of finished subagents: one `ClearEndedSubagents`,
//! answered by `Ack`, whose `SubagentsRemoved` the model folds in
//! (`daemon.rs`). The request is protocol 63; an older daemon drops the
//! whole connection on one it cannot decode, so it is refused here,
//! before it is sent.

use crate::daemon::Client;
use crate::orchestrator::RELOAD_HINT;
use crate::sessions::ack;
use crate::Res;
use nebula_core::{AgentId, ClientRequest, SUBAGENT_CLEANUP_PROTOCOL};
use std::sync::Arc;
use tauri::State;

/// Drop the ended subagents of session `parent`, or of every session for
/// None. Running ones stay.
pub async fn clear_finished_subagents_in(client: &Client, parent: Option<String>) -> Res<()> {
    match client.daemon_protocol() {
        0 => return Err("Not connected to the kete daemon.".into()),
        v if v < SUBAGENT_CLEANUP_PROTOCOL => return Err(RELOAD_HINT.into()),
        _ => {}
    }
    let parent = parent.map(AgentId);
    ack(client, |req_id| ClientRequest::ClearEndedSubagents {
        req_id,
        parent,
    })
    .await
    .map(drop)
}

#[tauri::command]
pub async fn clear_finished_subagents(
    client: State<'_, Arc<Client>>,
    parent: Option<String>,
) -> Res<()> {
    clear_finished_subagents_in(&client, parent).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sessions::testkit::{ok, run};

    #[tokio::test]
    async fn a_clear_is_sent_only_to_a_daemon_that_takes_it() {
        let (client, mut rx) = Client::connected_for_test();
        client.set_model(|m| m.protocol_version = SUBAGENT_CLEANUP_PROTOCOL - 1);
        let r = clear_finished_subagents_in(&client, None).await;
        assert_eq!(r.unwrap_err(), RELOAD_HINT);
        assert!(rx.try_recv().is_err(), "nothing went out");

        client.set_model(|m| m.protocol_version = SUBAGENT_CLEANUP_PROTOCOL);
        let (r, seen) = run(
            &client,
            &mut rx,
            clear_finished_subagents_in(&client, Some("a1".into())),
            ok,
        )
        .await;
        r.unwrap();
        assert!(matches!(
            &seen[0],
            ClientRequest::ClearEndedSubagents { parent: Some(p), .. } if p.0 == "a1"
        ));
    }
}
