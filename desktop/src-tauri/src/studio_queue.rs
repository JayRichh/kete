//! The studio queue's window side (K2): what the runner would send, and
//! the autonomy control's writes (`SetStudio`).
//!
//! The daemon refuses `SetStudio` from inside an agent session; the window
//! is a person's, so it passes. The window edits the inbox and the
//! autonomy only: the exclusions are `kete studio exclude`'s, so a write
//! with an empty list keeps the ones on disk.

use crate::daemon::Client;
use crate::Res;
use nebula_core::studio::wire::StudioQueueRow;
use nebula_core::studio::{settings, StudioSettings};
use nebula_core::{ClientRequest, ProjectId, ServerEvent};
use std::sync::Arc;
use tauri::State;

/// What the queue runner would do now, for `repo` (a project id) or every
/// studio repo; with `run`, one tick at once.
#[tauri::command]
pub async fn studio_queue(
    client: State<'_, Arc<Client>>,
    repo: Option<String>,
    run: bool,
) -> Res<Vec<StudioQueueRow>> {
    let req_id = client.req_id();
    let req = ClientRequest::StudioQueue {
        req_id,
        project: repo.map(ProjectId),
        run,
    };
    match client
        .rpc_reply(req_id, req)
        .await
        .map_err(|e| e.to_string())?
    {
        ServerEvent::StudioQueueState { rows, .. } => Ok(rows),
        _ => Err("the daemon answered something else".into()),
    }
}

/// Set (None: remove) what this machine allows `repo`'s studio.
#[tauri::command]
pub async fn studio_set(
    client: State<'_, Arc<Client>>,
    repo: String,
    studio: Option<StudioSettings>,
) -> Res<()> {
    if client.daemon_protocol() < nebula_core::STUDIO_PROTOCOL {
        return Err("needs the 0.60 daemon: run `kete kill` and reopen".into());
    }
    let root = client
        .model
        .lock()
        .map_err(|_| "the model is unavailable".to_string())?
        .projects
        .iter()
        .find(|p| p.id.0 == repo)
        .map(|p| p.repo_path.clone())
        .ok_or_else(|| format!("no project {repo}"))?;
    let studio = match studio {
        Some(s) => {
            let current = crate::editors::blocking(move || settings::for_repo(&root)).await?;
            Some(keep_exclusions(s, current.as_ref()))
        }
        None => None,
    };
    let req_id = client.req_id();
    client
        .rpc(
            req_id,
            ClientRequest::SetStudio {
                req_id,
                project: ProjectId(repo),
                studio,
            },
        )
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// `wanted`, with `current`'s exclusions when it names none.
fn keep_exclusions(mut wanted: StudioSettings, current: Option<&StudioSettings>) -> StudioSettings {
    if wanted.queue_exclude.is_empty() {
        if let Some(current) = current {
            wanted.queue_exclude = current.queue_exclude.clone();
        }
    }
    wanted
}

#[cfg(test)]
mod tests {
    use super::*;
    use nebula_core::studio::Autonomy;

    #[test]
    fn an_autonomy_change_keeps_the_exclusions() {
        let current = StudioSettings {
            studio_inbox: "notes/inbox".into(),
            autonomy: Autonomy::A1,
            queue_exclude: vec!["paused".into()],
        };
        let wanted = StudioSettings {
            studio_inbox: "notes/inbox".into(),
            autonomy: Autonomy::A2,
            queue_exclude: Vec::new(),
        };
        let out = keep_exclusions(wanted, Some(&current));
        assert_eq!(out.autonomy, Autonomy::A2);
        assert_eq!(out.queue_exclude, ["paused"]);
        // A list it names replaces the old one; no entry yet keeps none.
        let named = StudioSettings {
            queue_exclude: vec!["x".into()],
            ..current.clone()
        };
        assert_eq!(keep_exclusions(named, Some(&current)).queue_exclude, ["x"]);
        assert!(keep_exclusions(current.clone(), None).queue_exclude == ["paused"]);
    }
}
