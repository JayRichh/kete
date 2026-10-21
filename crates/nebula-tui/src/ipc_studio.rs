//! The 0.60 studio requests as one-shot clients (protocol 66,
//! `nebula_core::STUDIO_PROTOCOL`): what `kete studio`, `kete usage` and
//! `kete orchestrator renew` send, and what the TUI's studio views ask.
//!
//! Each opens its own connection (spawning the daemon when none runs),
//! sends one request and waits for its reply. A daemon older than 66
//! cannot decode the request and would drop the connection on it, so each
//! is refused here first, with the way out.

use crate::ipc::{connect_or_spawn, Connection};
use anyhow::{bail, Result};
use nebula_core::codec::{read_frame, write_frame};
use nebula_core::studio::wire::{LedgerRow, RenewState, StudioQueueRow};
use nebula_core::studio::StudioSettings;
use nebula_core::{AgentId, ClientRequest, ProjectId, ServerEvent, STUDIO_PROTOCOL};
use std::path::PathBuf;

/// What a client says to a daemon below [`STUDIO_PROTOCOL`].
pub const NEEDS_STUDIO_DAEMON: &str = "needs the 0.60 daemon: run `kete kill` and reopen";

/// Request id for these one-shot clients.
const REQ_ID: u64 = 1;

/// What `kete orchestrator renew` (or the TUI's renew prompt) asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenewRequestArgs {
    /// The orchestrator renewing itself (`NEBULA_AGENT_ID` inside its
    /// session); None from a person's shell or the apps.
    pub caller: Option<AgentId>,
    pub note: PathBuf,
    pub profile: Option<String>,
    pub model: Option<String>,
    pub keep_old: bool,
    pub now: bool,
}

/// Refuse a daemon that cannot decode the 0.60 requests.
pub fn require_studio(daemon_protocol: u32, what: &str) -> Result<()> {
    if daemon_protocol < STUDIO_PROTOCOL {
        bail!("{what} {NEEDS_STUDIO_DAEMON} (the running daemon speaks v{daemon_protocol})");
    }
    Ok(())
}

async fn studio_connection(what: &str) -> Result<Connection> {
    let conn = connect_or_spawn().await?;
    require_studio(conn.protocol_version, what)?;
    Ok(conn)
}

/// Wait for the reply to [`REQ_ID`]: `pick` takes the event that answers
/// it; an `Error` for it is the error.
async fn reply<T>(
    conn: &mut Connection,
    mut pick: impl FnMut(ServerEvent) -> Option<T>,
) -> Result<T> {
    loop {
        match read_frame::<ServerEvent, _>(&mut conn.stream).await? {
            Some(ServerEvent::Error {
                req_id: Some(r),
                message,
            }) if r == REQ_ID => bail!("{message}"),
            Some(ev) => {
                if let Some(answer) = pick(ev) {
                    return Ok(answer);
                }
            }
            None => bail!("{}", crate::ipc::CLOSED_BEFORE_REPLY),
        }
    }
}

/// `SetStudio`: what this machine allows `project`'s studio; None turns
/// it off. The daemon refuses it from inside an agent session.
pub async fn set_studio(project: ProjectId, studio: Option<StudioSettings>) -> Result<()> {
    let mut conn = studio_connection("studio settings").await?;
    write_frame(
        &mut conn.stream,
        &ClientRequest::SetStudio {
            req_id: REQ_ID,
            project,
            studio,
        },
    )
    .await?;
    reply(&mut conn, |ev| match ev {
        ServerEvent::Ack { req_id, .. } if req_id == REQ_ID => Some(()),
        _ => None,
    })
    .await
}

/// `StudioQueue`: what the queue runner would do now, for `project` or
/// every studio repo; with `run`, one tick at once.
pub async fn studio_queue(project: Option<ProjectId>, run: bool) -> Result<Vec<StudioQueueRow>> {
    let mut conn = studio_connection("the studio queue").await?;
    write_frame(
        &mut conn.stream,
        &ClientRequest::StudioQueue {
            req_id: REQ_ID,
            project,
            run,
        },
    )
    .await?;
    reply(&mut conn, |ev| match ev {
        ServerEvent::StudioQueueState { req_id, rows } if req_id == REQ_ID => Some(rows),
        _ => None,
    })
    .await
}

/// `RenewOrchestrator`: the daemon's answer, `Scheduled` when it took it.
pub async fn renew_orchestrator(req: RenewRequestArgs) -> Result<RenewState> {
    let mut conn = studio_connection("orchestrator renewal").await?;
    write_frame(
        &mut conn.stream,
        &ClientRequest::RenewOrchestrator {
            req_id: REQ_ID,
            caller: req.caller,
            note: req.note,
            profile: req.profile,
            model: req.model,
            keep_old: req.keep_old,
            now: req.now,
        },
    )
    .await?;
    reply(&mut conn, |ev| match ev {
        ServerEvent::OrchestratorRenewal {
            req_id: Some(r),
            state,
            ..
        } if r == REQ_ID => Some(state),
        _ => None,
    })
    .await
}

/// `UsageLedger`: the ledger's rows from `since` to `until` (local dates,
/// `YYYY-MM-DD`, inclusive), for one project or all.
pub async fn usage_ledger(
    since: Option<String>,
    until: Option<String>,
    project: Option<ProjectId>,
) -> Result<Vec<LedgerRow>> {
    let mut conn = studio_connection("the usage ledger").await?;
    write_frame(
        &mut conn.stream,
        &ClientRequest::UsageLedger {
            req_id: REQ_ID,
            since,
            until,
            project,
        },
    )
    .await?;
    reply(&mut conn, |ev| match ev {
        ServerEvent::UsageLedgerRows { req_id, rows } if req_id == REQ_ID => Some(rows),
        _ => None,
    })
    .await
}

/// Which projects are studio repos (a `.kete/studio.toml` in the root
/// checkout, or a `studio` entry in `config.local.json`), for the menus'
/// state rules. Read off the key path: [`StudioRepos::get`] answers from
/// what it last read and refreshes on a thread of its own once that is
/// [`StudioRepos::FRESH`] old, so a menu never waits on the disk.
#[derive(Debug, Default)]
pub struct StudioRepos {
    inner: std::sync::Arc<std::sync::Mutex<StudioReposState>>,
}

#[derive(Debug, Default)]
struct StudioReposState {
    ids: std::collections::BTreeSet<ProjectId>,
    read_at: Option<std::time::Instant>,
    reading: bool,
}

impl Clone for StudioRepos {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl StudioRepos {
    /// How long a reading stands before the next `get` refreshes it.
    pub const FRESH: std::time::Duration = std::time::Duration::from_secs(10);

    /// The studio repos among `projects` as last read; starts a refresh
    /// in the background when the reading is stale.
    pub fn get(&self, projects: &[nebula_core::Project]) -> std::collections::BTreeSet<ProjectId> {
        let Ok(mut state) = self.inner.lock() else {
            return Default::default();
        };
        let stale = state.read_at.is_none_or(|at| at.elapsed() >= Self::FRESH);
        if stale && !state.reading && !cfg!(test) {
            state.reading = true;
            let inner = self.inner.clone();
            let roots: Vec<(ProjectId, PathBuf)> = projects
                .iter()
                .map(|p| (p.id.clone(), p.repo_path.clone()))
                .collect();
            std::thread::spawn(move || {
                let ids = studio_ids(&roots);
                if let Ok(mut state) = inner.lock() {
                    state.ids = ids;
                    state.read_at = Some(std::time::Instant::now());
                    state.reading = false;
                }
            });
        }
        state.ids.clone()
    }

    /// Set the reading by hand (tests, and a view that just read it).
    pub fn set(&self, ids: impl IntoIterator<Item = ProjectId>) {
        if let Ok(mut state) = self.inner.lock() {
            state.ids = ids.into_iter().collect();
            state.read_at = Some(std::time::Instant::now());
        }
    }
}

/// The projects whose root has a manifest or a `studio` entry in this
/// machine's `config.local.json`.
pub fn studio_ids(roots: &[(ProjectId, PathBuf)]) -> std::collections::BTreeSet<ProjectId> {
    studio_ids_in(roots, &nebula_core::studio::settings::load_all())
}

/// [`studio_ids`] against the studio entries given.
pub fn studio_ids_in(
    roots: &[(ProjectId, PathBuf)],
    entries: &std::collections::BTreeMap<PathBuf, StudioSettings>,
) -> std::collections::BTreeSet<ProjectId> {
    roots
        .iter()
        .filter(|(_, root)| {
            let key = nebula_core::studio::settings::repo_key(root);
            root.join(nebula_core::studio::manifest::MANIFEST_PATH)
                .is_file()
                || entries.keys().any(|k| {
                    nebula_core::studio::settings::same_repo(
                        &nebula_core::studio::settings::repo_key(k),
                        &key,
                    )
                })
        })
        .map(|(id, _)| id.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_daemon_below_66_is_refused_with_the_way_out() {
        let err = require_studio(65, "the studio queue")
            .unwrap_err()
            .to_string();
        assert!(err.contains("needs the 0.60 daemon"), "{err}");
        assert!(err.contains("kete kill"), "{err}");
        assert!(err.contains("v65"), "{err}");
        assert!(require_studio(STUDIO_PROTOCOL, "x").is_ok());
    }

    #[test]
    fn studio_repos_answer_from_their_last_reading() {
        let repos = StudioRepos::default();
        assert!(repos.get(&[]).is_empty());
        repos.set([ProjectId("p".into())]);
        assert!(repos.get(&[]).contains(&ProjectId("p".into())));
    }

    #[test]
    fn a_manifest_makes_a_studio_repo() {
        let dir = tempfile::tempdir().unwrap();
        let with = dir.path().join("with");
        let without = dir.path().join("without");
        std::fs::create_dir_all(with.join(".kete")).unwrap();
        std::fs::create_dir_all(&without).unwrap();
        std::fs::write(with.join(".kete").join("studio.toml"), "version = 1\n").unwrap();
        let roots = [
            (ProjectId("a".into()), with),
            (ProjectId("b".into()), without.clone()),
        ];
        let ids = studio_ids_in(&roots, &Default::default());
        assert!(ids.contains(&ProjectId("a".into())));
        assert!(!ids.contains(&ProjectId("b".into())));
        // A studio entry for its root, under another spelling, makes one too.
        let spelled = PathBuf::from(format!("{}/", without.display()));
        let entries = [(spelled, StudioSettings::default())].into_iter().collect();
        assert!(studio_ids_in(&roots, &entries).contains(&ProjectId("b".into())));
    }
}
