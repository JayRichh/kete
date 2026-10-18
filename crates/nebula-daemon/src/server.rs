//! Unix-socket (Windows: named-pipe) server: accept loop and per-client
//! request handling. The
//! PTY plane of a connection (attach replay, forwarding) lives in `attach`.

use crate::attach::{self, PaneSize};
use crate::pr_scope::CreatePrAgentSpec;
use crate::registry::{CreateAgentSpec, Daemon};
use anyhow::{Context, Result};
use nebula_core::codec::{read_frame, write_frame};
use nebula_core::{ClientRequest, ServerEvent, SessionRef, PROTOCOL_VERSION};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, BufWriter};
#[cfg(unix)]
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{broadcast, mpsc};

/// The most of a ring one `TailOutput` copies, whatever the client asked:
/// the answer is built on the request loop, ahead of the next Input frame.
const TAIL_MAX_BYTES: u32 = 64 * 1024;

/// The Windows accept loop: one connected pipe instance per client (see
/// `platform::PipeListener`), served exactly as a socket connection is.
#[cfg(windows)]
pub async fn accept_loop(daemon: Arc<Daemon>, mut listener: crate::platform::PipeListener) {
    loop {
        tokio::select! {
            _ = daemon.shutdown.cancelled() => break,
            accepted = listener.accept() => match accepted {
                Ok(pipe) => {
                    let daemon = daemon.clone();
                    tokio::spawn(async move {
                        let peer = crate::platform::client_pid(&pipe);
                        let (read_half, write_half) = tokio::io::split(pipe);
                        if let Err(e) = serve_client(daemon, read_half, write_half, peer).await {
                            tracing::debug!(error = %e, "client connection ended with error");
                        }
                    });
                }
                Err(e) => {
                    tracing::warn!(error = %e, "accept failed");
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
            },
        }
    }
}

#[cfg(unix)]
pub async fn accept_loop(daemon: Arc<Daemon>, listener: UnixListener) {
    loop {
        tokio::select! {
            _ = daemon.shutdown.cancelled() => break,
            accepted = listener.accept() => match accepted {
                Ok((stream, _)) => {
                    let daemon = daemon.clone();
                    tokio::spawn(async move {
                        if let Err(e) = handle_client(daemon, stream).await {
                            tracing::debug!(error = %e, "client connection ended with error");
                        }
                    });
                }
                Err(e) => {
                    tracing::warn!(error = %e, "accept failed");
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
            },
        }
    }
}

#[cfg(unix)]
async fn handle_client(daemon: Arc<Daemon>, stream: UnixStream) -> Result<()> {
    let peer = stream
        .peer_cred()
        .ok()
        .and_then(|c| c.pid())
        .and_then(|pid| u32::try_from(pid).ok());
    let (read_half, write_half) = stream.into_split();
    serve_client(daemon, read_half, write_half, peer).await
}

/// One client connection, over whichever halves the platform's stream
/// splits into. `peer` is the client process's pid when the OS reports it:
/// what the orchestrator provisioning requests are checked against.
async fn serve_client<R, W>(
    daemon: Arc<Daemon>,
    read_half: R,
    write_half: W,
    peer: Option<u32>,
) -> Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin + Send + 'static,
{
    let mut reader = tokio::io::BufReader::new(read_half);

    // Single writer task; everything else sends frames through this channel
    // so PTY forwards and RPC replies never interleave mid-frame.
    let (out_tx, mut out_rx) = mpsc::channel::<ServerEvent>(256);
    let writer_task = tokio::spawn(async move {
        let mut w = BufWriter::new(write_half);
        while let Some(ev) = out_rx.recv().await {
            if write_frame(&mut w, &ev).await.is_err() {
                break;
            }
        }
        let _ = w.shutdown().await;
    });

    let mut client = ClientConnection::new(daemon, out_tx);
    client.peer_pid = peer;
    let result: Result<()> = async {
        while let Some(req) = read_frame::<ClientRequest, _>(&mut reader).await? {
            if !client.handle_request(req).await? {
                break;
            }
        }
        Ok(())
    }
    .await;

    client.cleanup().await;
    drop(client);
    let _ = writer_task.await;
    result
}

struct ClientConnection {
    daemon: Arc<Daemon>,
    out_tx: mpsc::Sender<ServerEvent>,
    attached: HashMap<SessionRef, tokio::task::JoinHandle<()>>,
    subscription: Option<tokio::task::JoinHandle<()>>,
    handshaken: bool,
    /// The protocol the client said at Hello: what it can decode
    /// (`ServerEvent::decodable_by`). 0 until then.
    peer_protocol: u32,
    /// The client process's pid, when the OS said.
    peer_pid: Option<u32>,
}

impl ClientConnection {
    fn new(daemon: Arc<Daemon>, out_tx: mpsc::Sender<ServerEvent>) -> Self {
        Self {
            daemon,
            out_tx,
            attached: HashMap::new(),
            subscription: None,
            handshaken: false,
            peer_protocol: 0,
            peer_pid: None,
        }
    }

    async fn handle_request(&mut self, req: ClientRequest) -> Result<bool> {
        // A privileged verb from a process inside some other agent's
        // session, naming the orchestrator as its caller: refused before
        // the verb runs.
        if let (true, Some((req_id, caller)), Some(peer)) =
            (self.handshaken, privileged_caller(&req), self.peer_pid)
        {
            let checked = self
                .blocking_daemon(move |daemon| daemon.check_caller_peer(&caller, Some(peer)))
                .await;
            if let Err(e) = checked {
                reply_done(&self.out_tx, req_id, Err(e)).await;
                return Ok(true);
            }
        }
        match req {
            ClientRequest::Hello { protocol_version } => {
                Ok(self.handle_hello(protocol_version).await)
            }
            _ if !self.handshaken => {
                self.send(ServerEvent::Error {
                    req_id: None,
                    message: "handshake required".into(),
                })
                .await;
                Ok(false)
            }
            ClientRequest::Subscribe => self.handle_subscribe().await,
            ClientRequest::Attach {
                session,
                from_seq,
                cols,
                rows,
            } => self.handle_attach(session, from_seq, cols, rows).await,
            ClientRequest::Detach { session } => {
                self.handle_detach(session);
                Ok(true)
            }
            ClientRequest::Input { session, data } => {
                self.handle_input(session, data).await;
                Ok(true)
            }
            ClientRequest::Resize {
                session,
                cols,
                rows,
            } => {
                self.handle_resize(session, cols, rows).await;
                Ok(true)
            }
            ClientRequest::Shutdown => {
                tracing::info!("shutdown requested by client");
                self.daemon.shutdown.cancel();
                Ok(false)
            }
            ClientRequest::SaveUiState { json } => {
                let _ = self
                    .daemon
                    .store_blocking(move |store| store.save_ui_state(&json))
                    .await;
                Ok(true)
            }
            ClientRequest::MarkPrSeen { url, marker } => {
                let _ = self
                    .daemon
                    .store_blocking(move |store| store.mark_pr_seen(&url, &marker))
                    .await;
                Ok(true)
            }
            ClientRequest::MarkAgentSeen { id } => {
                let result = self
                    .blocking_daemon(move |daemon| daemon.mark_agent_seen(&id))
                    .await;
                if let Err(e) = result {
                    tracing::warn!(error = %e, "mark agent seen failed");
                }
                Ok(true)
            }
            ClientRequest::GetMetrics { req_id } => {
                self.handle_get_metrics(req_id);
                Ok(true)
            }
            ClientRequest::TailOutput {
                req_id,
                session,
                max_bytes,
                after_seq,
            } => {
                self.handle_tail_output(req_id, session, max_bytes, after_seq)
                    .await
            }
            ClientRequest::AddProject {
                req_id,
                path,
                name,
                create_missing,
            } => {
                let result = self
                    .daemon
                    .add_project(&path, name, create_missing)
                    .await
                    .map(Some);
                reply(&self.out_tx, req_id, result).await;
                Ok(true)
            }
            ClientRequest::RemoveProject { req_id, id } => {
                let result = self
                    .blocking_daemon(move |daemon| daemon.remove_project(&id))
                    .await;
                reply_done(&self.out_tx, req_id, result).await;
                Ok(true)
            }
            ClientRequest::RenameProject { req_id, id, name } => {
                let result = self
                    .blocking_daemon(move |daemon| daemon.rename_project(&id, &name))
                    .await;
                reply(&self.out_tx, req_id, result.map(|_| None)).await;
                Ok(true)
            }
            ClientRequest::SetProjectPath { req_id, id, path } => {
                let result = self.daemon.set_project_path(&id, &path).await.map(|_| None);
                reply(&self.out_tx, req_id, result).await;
                Ok(true)
            }
            ClientRequest::CreateWorktree {
                req_id,
                project,
                branch,
                base,
            } => {
                self.spawn_create_worktree(req_id, project, branch, base);
                Ok(true)
            }
            ClientRequest::DeleteWorktree { req_id, id, force } => {
                self.spawn_delete_worktree(req_id, id, force);
                Ok(true)
            }
            ClientRequest::CleanupMergedWorktree {
                req_id,
                id,
                branch,
                pr_number,
                pr_url,
                head_sha,
            } => {
                self.spawn_cleanup_merged_worktree(req_id, id, branch, pr_number, pr_url, head_sha);
                Ok(true)
            }
            ClientRequest::CreateAgent {
                req_id,
                worktree,
                name,
                kind,
                custom_harness,
                model,
                effort,
                auto_title,
                cloud_prompt,
                starting_prompt,
                issue_url,
                role,
                profile,
            } => {
                self.handle_create_agent(
                    req_id,
                    worktree,
                    name,
                    kind,
                    custom_harness,
                    model,
                    effort,
                    auto_title,
                    cloud_prompt,
                    starting_prompt,
                    issue_url,
                    role,
                    profile,
                )
                .await;
                Ok(true)
            }
            ClientRequest::CreatePrAgent {
                req_id,
                project,
                name,
                kind,
                custom_harness,
                model,
                effort,
                auto_title,
                pr_url,
                head,
                starting_prompt,
                profile,
            } => {
                self.spawn_create_pr_agent(
                    req_id,
                    project,
                    name,
                    kind,
                    custom_harness,
                    model,
                    effort,
                    auto_title,
                    pr_url,
                    head,
                    starting_prompt,
                    profile,
                );
                Ok(true)
            }
            ClientRequest::PrewarmAgent {
                worktree,
                kind,
                model,
                effort,
            } => {
                let daemon = self.daemon.clone();
                tokio::spawn(async move {
                    if let Err(e) = daemon.prewarm_agent(&worktree, kind, model, effort).await {
                        tracing::debug!(error = %e, "prewarm failed");
                    }
                });
                Ok(true)
            }
            ClientRequest::PrewarmWorktreeSessions {
                worktree,
                cols,
                rows,
            } => {
                self.daemon.prewarm_worktree_sessions(&worktree, cols, rows);
                Ok(true)
            }
            ClientRequest::RenameAgent { req_id, id, name } => {
                let result = self
                    .blocking_daemon(move |daemon| daemon.rename_agent(&id, &name))
                    .await;
                reply_done(&self.out_tx, req_id, result).await;
                Ok(true)
            }
            ClientRequest::AutoRenameAgent { req_id, id, name } => {
                let result = self
                    .blocking_daemon(move |daemon| daemon.auto_rename_agent(&id, &name))
                    .await;
                reply_done(&self.out_tx, req_id, result).await;
                Ok(true)
            }
            ClientRequest::SpawnSiblingAgent {
                req_id,
                id,
                kind,
                starting_prompt,
                worktree,
                base,
                profile,
            } => {
                self.handle_spawn_sibling(
                    req_id,
                    id,
                    kind,
                    starting_prompt,
                    worktree,
                    base,
                    profile,
                )
                .await;
                Ok(true)
            }
            ClientRequest::OpenFiles { req_id, id, paths } => {
                let result = self
                    .blocking_daemon(move |daemon| daemon.open_files(&id, paths))
                    .await;
                reply_done(&self.out_tx, req_id, result).await;
                Ok(true)
            }
            ClientRequest::OrchestratorList { req_id, caller } => {
                let result = self
                    .blocking_daemon(move |daemon| daemon.orchestrator_tree(&caller))
                    .await;
                match result {
                    Ok((projects, worktrees, agents, terminals, links)) => {
                        self.send(ServerEvent::OrchestratorList {
                            req_id,
                            projects,
                            worktrees,
                            agents,
                            terminals,
                            links,
                        })
                        .await;
                    }
                    Err(err) => reply_done(&self.out_tx, req_id, Err(err)).await,
                }
                Ok(true)
            }
            ClientRequest::OrchestratorRead {
                req_id,
                caller,
                session,
                max_bytes,
            } => {
                let reply_session = session.clone();
                let result = self
                    .blocking_daemon(move |daemon| {
                        daemon.orchestrator_read(&caller, &session, max_bytes)
                    })
                    .await;
                match result {
                    Ok(tail) => {
                        self.send(ServerEvent::OutputTail {
                            req_id,
                            session: reply_session,
                            tail,
                        })
                        .await;
                    }
                    Err(err) => reply_done(&self.out_tx, req_id, Err(err)).await,
                }
                Ok(true)
            }
            ClientRequest::OrchestratorSend {
                req_id,
                caller,
                target,
                message,
            } => {
                // Waits for a busy target: off the request loop, like ask.
                let daemon = self.daemon.clone();
                let out_tx = self.out_tx.clone();
                tokio::spawn(async move {
                    let result = daemon.orchestrator_send(&caller, &target, &message).await;
                    reply_done(&out_tx, req_id, result.map(|_| ())).await;
                });
                Ok(true)
            }
            ClientRequest::OrchestratorSpawn {
                req_id,
                caller,
                project,
                worktree,
                branch,
                base,
                name,
                kind,
                custom_harness,
                model,
                effort,
                starting_prompt,
            } => {
                let daemon = self.daemon.clone();
                let out_tx = self.out_tx.clone();
                tokio::spawn(async move {
                    let result = daemon
                        .orchestrator_spawn(
                            &caller,
                            crate::orchestrator::OrchestratorSpawnSpec {
                                project,
                                worktree,
                                branch,
                                base,
                                name,
                                kind,
                                custom_harness,
                                model,
                                effort,
                                profile: None,
                                starting_prompt,
                            },
                        )
                        .await;
                    reply(&out_tx, req_id, result.map(Some)).await;
                });
                Ok(true)
            }
            ClientRequest::OrchestratorOpenReview {
                req_id,
                caller,
                sessions,
                tabs,
            } => {
                let result = self
                    .blocking_daemon(move |daemon| {
                        daemon.orchestrator_open_review(&caller, sessions, tabs)
                    })
                    .await;
                reply_done(&self.out_tx, req_id, result).await;
                Ok(true)
            }
            ClientRequest::ListSessions { req_id } => {
                let result = self
                    .blocking_daemon(move |daemon| daemon.session_summaries())
                    .await;
                match result {
                    Ok(sessions) => {
                        self.send(ServerEvent::SessionList { req_id, sessions })
                            .await;
                    }
                    Err(err) => reply_done(&self.out_tx, req_id, Err(err)).await,
                }
                Ok(true)
            }
            ClientRequest::ReadSession {
                req_id,
                target,
                max_lines,
            } => {
                let result = self
                    .blocking_daemon(move |daemon| daemon.read_session_context(&target, max_lines))
                    .await;
                match result {
                    Ok(read) => {
                        self.send(ServerEvent::SessionText {
                            req_id,
                            session: read.session,
                            text: read.text,
                        })
                        .await;
                    }
                    Err(err) => reply_done(&self.out_tx, req_id, Err(err)).await,
                }
                Ok(true)
            }
            ClientRequest::AskSession {
                req_id,
                caller,
                target,
                question,
                timeout_ms,
                wait,
            } => {
                let daemon = self.daemon.clone();
                let out_tx = self.out_tx.clone();
                tokio::spawn(async move {
                    let result = daemon
                        .ask_session(
                            caller,
                            &target,
                            &question,
                            std::time::Duration::from_millis(timeout_ms),
                            wait,
                        )
                        .await;
                    match result {
                        Ok((session, answer)) => {
                            let _ = out_tx
                                .send(ServerEvent::SessionAnswer {
                                    req_id,
                                    session,
                                    answer,
                                })
                                .await;
                        }
                        Err(err) => reply_done(&out_tx, req_id, Err(err)).await,
                    }
                });
                Ok(true)
            }
            ClientRequest::EnterWorktree {
                req_id,
                id,
                branch,
                base,
            } => self.handle_enter_worktree(req_id, id, branch, base).await,
            ClientRequest::ArchiveAgent { req_id, id } => {
                let result = self
                    .blocking_daemon(move |daemon| daemon.archive_agent(&id))
                    .await;
                reply_done(&self.out_tx, req_id, result).await;
                Ok(true)
            }
            ClientRequest::UnarchiveAgent { req_id, id } => {
                let result = self
                    .blocking_daemon(move |daemon| daemon.unarchive_agent(&id))
                    .await;
                reply_done(&self.out_tx, req_id, result).await;
                Ok(true)
            }
            ClientRequest::DeleteAgent { req_id, id } => {
                let result = self
                    .blocking_daemon(move |daemon| daemon.delete_agent(&id))
                    .await;
                reply_done(&self.out_tx, req_id, result).await;
                Ok(true)
            }
            ClientRequest::RestartAgent { req_id, id } => {
                let result = self.daemon.restart_agent(&id).await;
                reply_done(&self.out_tx, req_id, result).await;
                Ok(true)
            }
            ClientRequest::SessionChanges { req_id, agent } => {
                let result = self
                    .blocking_daemon(move |daemon| daemon.session_changes(&agent))
                    .await;
                match result {
                    Ok(changes) => {
                        self.send(ServerEvent::SessionChanges { req_id, changes })
                            .await
                    }
                    Err(err) => reply_done(&self.out_tx, req_id, Err(err)).await,
                }
                Ok(true)
            }
            ClientRequest::SwitchAgentAccount {
                req_id,
                id,
                profile,
            } => {
                let result = self
                    .daemon
                    .switch_agent_account(&id, profile.as_deref())
                    .await;
                reply_done(&self.out_tx, req_id, result).await;
                Ok(true)
            }
            ClientRequest::RenameProfileSessions { req_id, old, new } => {
                let result = self
                    .blocking_daemon(move |daemon| daemon.rename_profile_sessions(&old, &new))
                    .await;
                reply_done(&self.out_tx, req_id, result).await;
                Ok(true)
            }
            // Up to two minutes: off the request loop, so the rest of this
            // connection's frames keep flowing.
            ClientRequest::WakeAccount { req_id, account } => {
                let daemon = self.daemon.clone();
                let out_tx = self.out_tx.clone();
                tokio::spawn(async move {
                    let report = daemon.wake_account(account).await;
                    let _ = out_tx.send(ServerEvent::WakeDone { req_id, report }).await;
                });
                Ok(true)
            }
            ClientRequest::AccountActivity { req_id } => {
                let result = self
                    .blocking_daemon(|daemon| daemon.account_activity())
                    .await;
                match result {
                    Ok(accounts) => {
                        self.send(ServerEvent::AccountActivity { req_id, accounts })
                            .await
                    }
                    Err(err) => reply_done(&self.out_tx, req_id, Err(err)).await,
                }
                Ok(true)
            }
            ClientRequest::MoveAgent {
                req_id,
                id,
                worktree,
            } => {
                let result = self
                    .blocking_daemon(move |daemon| daemon.move_agent(&id, &worktree))
                    .await;
                reply_done(&self.out_tx, req_id, result).await;
                Ok(true)
            }
            ClientRequest::ClearEndedSubagents { req_id, parent } => {
                self.daemon.clear_ended_subagents(parent.as_ref());
                reply_done(&self.out_tx, req_id, Ok(())).await;
                Ok(true)
            }
            ClientRequest::Reorder { req_id, scope, ids } => {
                let result = self
                    .blocking_daemon(move |daemon| daemon.reorder(&scope, &ids))
                    .await;
                reply_done(&self.out_tx, req_id, result).await;
                Ok(true)
            }
            ClientRequest::SendCloudMessage {
                req_id,
                id,
                message,
            } => {
                self.spawn_send_cloud_message(req_id, id, message);
                Ok(true)
            }
            ClientRequest::CreateTerminal {
                req_id,
                worktree,
                name,
            } => {
                let result = self
                    .blocking_daemon(move |daemon| {
                        daemon.create_terminal(&worktree, name).map(Some)
                    })
                    .await;
                reply(&self.out_tx, req_id, result).await;
                Ok(true)
            }
            ClientRequest::UpdateLink { req_id, id, url } => {
                let result = self
                    .blocking_daemon(move |daemon| daemon.update_link(&id, &url))
                    .await;
                reply_done(&self.out_tx, req_id, result).await;
                Ok(true)
            }
            ClientRequest::DeleteLink { req_id, id } => {
                let result = self
                    .blocking_daemon(move |daemon| daemon.delete_link(&id))
                    .await;
                reply_done(&self.out_tx, req_id, result).await;
                Ok(true)
            }
            ClientRequest::RenameTerminal { req_id, id, name } => {
                let result = self
                    .blocking_daemon(move |daemon| daemon.rename_terminal(&id, &name))
                    .await;
                reply_done(&self.out_tx, req_id, result).await;
                Ok(true)
            }
            ClientRequest::CloseTerminal { req_id, id } => {
                let result = self
                    .blocking_daemon(move |daemon| daemon.close_terminal(&id))
                    .await;
                reply_done(&self.out_tx, req_id, result).await;
                Ok(true)
            }
            ClientRequest::StartRun { req_id, worktree } => {
                let result = self
                    .blocking_daemon(move |daemon| daemon.start_run(&worktree).map(Some))
                    .await;
                reply(&self.out_tx, req_id, result).await;
                Ok(true)
            }
            ClientRequest::StopRun { req_id, worktree } => {
                let result = self
                    .blocking_daemon(move |daemon| daemon.stop_run(&worktree))
                    .await;
                reply_done(&self.out_tx, req_id, result).await;
                Ok(true)
            }
            ClientRequest::StartOrchestrator {
                req_id,
                kind,
                custom_harness,
                model,
                effort,
                profile,
                project,
                name,
                replace,
            } => {
                if let Err(e) = self.check_provisioning_peer().await {
                    reply_done(&self.out_tx, req_id, Err(e)).await;
                    return Ok(true);
                }
                let daemon = self.daemon.clone();
                let out_tx = self.out_tx.clone();
                tokio::spawn(async move {
                    let result = daemon
                        .start_orchestrator(crate::orchestrator::StartOrchestratorSpec {
                            kind: Some(kind),
                            custom_harness,
                            model,
                            effort,
                            profile,
                            project,
                            name,
                            replace,
                        })
                        .await;
                    reply(&out_tx, req_id, result.map(Some)).await;
                });
                Ok(true)
            }
            ClientRequest::SetOrchestrator { req_id, agent } => {
                let result = match self.check_provisioning_peer().await {
                    Ok(()) => {
                        self.blocking_daemon(move |daemon| daemon.set_orchestrator(&agent))
                            .await
                    }
                    Err(e) => Err(e),
                };
                reply_done(&self.out_tx, req_id, result).await;
                Ok(true)
            }
            ClientRequest::ClearOrchestrator { req_id } => {
                let result = match self.check_provisioning_peer().await {
                    Ok(()) => {
                        self.blocking_daemon(|daemon| daemon.clear_orchestrator())
                            .await
                    }
                    Err(e) => Err(e),
                };
                reply_done(&self.out_tx, req_id, result).await;
                Ok(true)
            }
            ClientRequest::OrchestratorSessions { req_id, caller } => {
                let result = self
                    .blocking_daemon(move |daemon| daemon.orchestrator_sessions(&caller))
                    .await;
                match result {
                    Ok(sessions) => {
                        self.send(ServerEvent::OrchestratorSessions { req_id, sessions })
                            .await;
                    }
                    Err(err) => reply_done(&self.out_tx, req_id, Err(err)).await,
                }
                Ok(true)
            }
            ClientRequest::OrchestratorTell {
                req_id,
                caller,
                target,
                message,
                timeout_ms,
                wait,
            } => {
                let daemon = self.daemon.clone();
                let out_tx = self.out_tx.clone();
                tokio::spawn(async move {
                    let result = daemon
                        .orchestrator_tell(
                            &caller,
                            &target,
                            &message,
                            std::time::Duration::from_millis(timeout_ms),
                            wait,
                        )
                        .await;
                    let created = result.map(|id| Some(nebula_core::EntityId::Agent(id)));
                    reply(&out_tx, req_id, created).await;
                });
                Ok(true)
            }
            ClientRequest::OrchestratorReadSession {
                req_id,
                caller,
                target,
                max_lines,
            } => {
                let result = self
                    .blocking_daemon(move |daemon| {
                        daemon.orchestrator_read_session(&caller, &target, max_lines)
                    })
                    .await;
                match result {
                    Ok(read) => {
                        self.send(ServerEvent::SessionText {
                            req_id,
                            session: read.session,
                            text: read.text,
                        })
                        .await;
                    }
                    Err(err) => reply_done(&self.out_tx, req_id, Err(err)).await,
                }
                Ok(true)
            }
            ClientRequest::OrchestratorSpawnWith {
                req_id,
                caller,
                project,
                worktree,
                branch,
                base,
                name,
                kind,
                custom_harness,
                model,
                effort,
                profile,
                starting_prompt,
            } => {
                let daemon = self.daemon.clone();
                let out_tx = self.out_tx.clone();
                tokio::spawn(async move {
                    let result = daemon
                        .orchestrator_spawn(
                            &caller,
                            crate::orchestrator::OrchestratorSpawnSpec {
                                project,
                                worktree,
                                branch,
                                base,
                                name,
                                kind,
                                custom_harness,
                                model,
                                effort,
                                profile,
                                starting_prompt,
                            },
                        )
                        .await;
                    reply(&out_tx, req_id, result.map(Some)).await;
                });
                Ok(true)
            }
        }
    }

    /// Orchestrator provisioning (start, set, clear) is the user's call,
    /// never a worker's: refused when the client process runs inside a live
    /// agent session. A client whose pid the OS would not report passes;
    /// the peer check is defence in depth over the same-user socket, not
    /// the trust boundary itself.
    async fn check_provisioning_peer(&self) -> Result<()> {
        let Some(pid) = self.peer_pid else {
            return Ok(());
        };
        let inside = self
            .blocking_daemon(move |daemon| Ok(daemon.pid_inside_agent_session(pid)))
            .await?;
        if inside {
            anyhow::bail!(
                "orchestrator provisioning is refused from inside an agent session; start or \
                 pin the orchestrator from kete itself (F10) or your own shell"
            );
        }
        Ok(())
    }

    async fn handle_hello(&mut self, protocol_version: u32) -> bool {
        self.handshaken = nebula_core::protocol_compatible_with(protocol_version);
        self.peer_protocol = protocol_version;
        let reply = if self.handshaken {
            ServerEvent::HelloOk {
                protocol_version: PROTOCOL_VERSION,
                daemon_pid: std::process::id(),
            }
        } else {
            ServerEvent::Incompatible {
                daemon_protocol_version: PROTOCOL_VERSION,
            }
        };
        self.send(reply).await;
        self.handshaken
    }

    async fn handle_subscribe(&mut self) -> Result<bool> {
        // Subscribed before the snapshot is taken, so a change made between
        // the two is not lost: it arrives after the Snapshot, where a client
        // folds it in by id.
        let rx = self.daemon.events.subscribe();
        let snapshot = self
            .blocking_daemon(|daemon| daemon.snapshot())
            .await
            .unwrap_or_else(|_| empty_snapshot());
        self.send(snapshot).await;
        // WORKTREE PORTS ride beside the tree: the last reading, at once.
        self.send(ServerEvent::WorktreePorts {
            ports: self.daemon.worktree_ports(),
        })
        .await;
        // RECORDED BASES, ACCOUNT USAGE, SESSION CONTEXT and STOP NOTES
        // too, to a peer that reads them.
        for ev in base_events(&self.daemon, self.peer_protocol)
            .await
            .into_iter()
            .chain(usage_events(&self.daemon, self.peer_protocol).await)
            .chain(stop_events(&self.daemon, self.peer_protocol))
        {
            self.send(ev).await;
        }
        let tx = self.out_tx.clone();
        let forward = tokio::spawn(forward_subscription_events(
            self.daemon.clone(),
            rx,
            tx,
            self.peer_protocol,
        ));
        if let Some(old) = self.subscription.replace(forward) {
            old.abort();
        }
        Ok(true)
    }

    async fn handle_attach(
        &mut self,
        sref: SessionRef,
        from_seq: Option<u64>,
        cols: u16,
        rows: u16,
    ) -> Result<bool> {
        let target = sref.clone();
        match self
            .blocking_daemon(move |daemon| daemon.ensure_session(&target, cols, rows))
            .await
        {
            Ok(session) => {
                // Replay inline, before the loop takes the next request, so
                // the Scrollback precedes any later reply; the forward task
                // follows from there.
                let size = PaneSize { cols, rows };
                let (events_rx, replay_end) =
                    attach::bind(&session, &sref, &self.out_tx, size, from_seq).await;
                // A RUN TERMINAL whose command already exited replays how it
                // ended. Say it is over, or the pane would offer to type into
                // a process that is gone.
                if let Some(exit_code) = self.daemon.finished_run_exit(&sref) {
                    self.send(ServerEvent::SessionExited {
                        session: sref.clone(),
                        exit_code,
                    })
                    .await;
                }

                let rebind = self.attached.remove(&sref);
                if let Some(old) = &rebind {
                    old.abort();
                }
                let handle = tokio::spawn(attach::forward(
                    self.daemon.clone(),
                    session,
                    sref.clone(),
                    events_rx,
                    self.out_tx.clone(),
                    replay_end,
                    size,
                ));
                // Count this connection once even across re-attaches to the
                // same session.
                if rebind.is_none() {
                    self.daemon.note_attached(&sref);
                }
                self.attached.insert(sref, handle);
            }
            Err(e) => {
                self.send(ServerEvent::AttachRefused {
                    session: sref,
                    message: format!("{e:#}"),
                })
                .await;
            }
        }
        Ok(true)
    }

    fn handle_detach(&mut self, session: SessionRef) {
        if let Some(h) = self.attached.remove(&session) {
            h.abort();
            self.daemon.note_detached(&session);
        }
    }

    async fn handle_input(&self, session: SessionRef, data: Vec<u8>) {
        let result = self
            .blocking_daemon(move |daemon| {
                if let Some(s) = daemon.session(&session) {
                    if let Err(e) = s.write_input(&data) {
                        tracing::warn!(error = %e, "pty write failed");
                    }
                }
                Ok(())
            })
            .await;
        if let Err(e) = result {
            tracing::warn!(error = %e, "input handling failed");
        }
    }

    async fn handle_resize(&self, session: SessionRef, cols: u16, rows: u16) {
        let result = self
            .blocking_daemon(move |daemon| {
                if let Some(s) = daemon.session(&session) {
                    let _ = s.resize(cols, rows);
                }
                Ok(())
            })
            .await;
        if let Err(e) = result {
            tracing::warn!(error = %e, "resize handling failed");
        }
    }

    fn handle_get_metrics(&self, req_id: u64) {
        // A machine-wide `ps` sweep takes tens of ms; keep it off the request
        // loop so Input/Attach frames keep flowing.
        let pids = self.daemon.session_pids();
        let out_tx = self.out_tx.clone();
        tokio::spawn(async move {
            let snapshot = tokio::task::spawn_blocking(move || crate::metrics::collect(pids)).await;
            if let Ok(snapshot) = snapshot {
                let _ = out_tx.send(ServerEvent::Metrics { req_id, snapshot }).await;
            }
        });
    }

    async fn handle_tail_output(
        &self,
        req_id: u64,
        session: SessionRef,
        max_bytes: u32,
        after_seq: Option<u64>,
    ) -> Result<bool> {
        // A few KB copied out of a ring: cheap enough inline.
        let max = max_bytes.min(TAIL_MAX_BYTES) as usize;
        let tail = self
            .daemon
            .session(&session)
            .map(|s| s.tail(max, after_seq));
        self.send(ServerEvent::OutputTail {
            req_id,
            session,
            tail,
        })
        .await;
        Ok(true)
    }

    #[allow(clippy::too_many_arguments)]
    async fn handle_create_agent(
        &self,
        req_id: u64,
        worktree: nebula_core::WorktreeId,
        name: String,
        kind: nebula_core::AgentKind,
        custom_harness: Option<String>,
        model: Option<String>,
        effort: Option<String>,
        auto_title: bool,
        cloud_prompt: Option<String>,
        starting_prompt: Option<String>,
        issue_url: Option<String>,
        role: nebula_core::AgentRole,
        profile: Option<String>,
    ) {
        // The ORCHESTRATOR role is granted only by StartOrchestrator and
        // SetOrchestrator, which keep it to one per machine and refuse a
        // worker's own shell. A create naming it is refused outright.
        if role != nebula_core::AgentRole::Worker {
            let refusal = anyhow::anyhow!(
                "CreateAgent cannot grant the orchestrator role; use StartOrchestrator \
                 (`kete orchestrator start`, or F10 in kete)"
            );
            reply_done(&self.out_tx, req_id, Err(refusal)).await;
            return;
        }
        // Logged by mode only — never the task, prompt text or issue URL.
        let launch_mode = match (&cloud_prompt, &starting_prompt, &issue_url) {
            (Some(_), _, _) => Some("cloud"),
            (None, _, Some(_)) => Some("issue"),
            (None, Some(_), None) => Some("preset"),
            (None, None, None) => None,
        };
        let result = self
            .daemon
            .create_agent(CreateAgentSpec {
                parent: None,
                profile,
                worktree: worktree.clone(),
                name,
                kind,
                custom_harness,
                model,
                effort,
                auto_title,
                cloud_prompt,
                starting_prompt,
                pr_url: None,
                issue_url,
                role,
            })
            .await;
        if let Some(launch_mode) = launch_mode {
            log_spawn_result(
                req_id,
                &result,
                kind,
                launch_mode,
                "agent session",
                |fields| {
                    fields.field("worktree", worktree.to_string());
                },
            );
        }
        reply(&self.out_tx, req_id, result.map(Some)).await;
    }

    fn spawn_create_worktree(
        &self,
        req_id: u64,
        project: nebula_core::ProjectId,
        branch: String,
        base: Option<String>,
    ) {
        // A create fetches `origin` and then runs the WORKTREE HOOK, each
        // bounded by a 30 s timeout; off the request loop, like the delete
        // below, so Input/Attach frames on this connection never wait.
        let daemon = self.daemon.clone();
        let out_tx = self.out_tx.clone();
        tokio::spawn(async move {
            reply(
                &out_tx,
                req_id,
                daemon
                    .create_worktree(&project, &branch, base.as_deref())
                    .await
                    .map(Some),
            )
            .await;
        });
    }

    fn spawn_delete_worktree(&self, req_id: u64, id: nebula_core::WorktreeId, force: bool) {
        // `git worktree remove` can take seconds on a large checkout; run it
        // off the request loop so Input/Attach frames keep flowing while it
        // grinds. `worktree_ops` still serializes it against create/sync.
        let daemon = self.daemon.clone();
        let out_tx = self.out_tx.clone();
        tokio::spawn(async move {
            reply_done(&out_tx, req_id, daemon.delete_worktree(&id, force).await).await;
        });
    }

    fn spawn_cleanup_merged_worktree(
        &self,
        req_id: u64,
        id: nebula_core::WorktreeId,
        branch: String,
        pr_number: u64,
        pr_url: String,
        head_sha: String,
    ) {
        let daemon = self.daemon.clone();
        let out_tx = self.out_tx.clone();
        tokio::spawn(async move {
            reply_done(
                &out_tx,
                req_id,
                daemon
                    .cleanup_merged_worktree(&id, &branch, pr_number, &pr_url, &head_sha)
                    .await,
            )
            .await;
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn spawn_create_pr_agent(
        &self,
        req_id: u64,
        project: nebula_core::ProjectId,
        name: String,
        kind: nebula_core::AgentKind,
        custom_harness: Option<String>,
        model: Option<String>,
        effort: Option<String>,
        auto_title: bool,
        pr_url: String,
        head: String,
        starting_prompt: Option<String>,
        profile: Option<String>,
    ) {
        // A PR SESSION whose checkout does not exist yet is a fetch, a `git
        // worktree add` and the WORKTREE HOOK before the CLI spawns. Off the
        // request loop like `CreateWorktree`; `worktree_ops` still serializes
        // the checkout against every other worktree op.
        let daemon = self.daemon.clone();
        let out_tx = self.out_tx.clone();
        tokio::spawn(async move {
            let result = daemon
                .create_pr_agent(CreatePrAgentSpec {
                    project: project.clone(),
                    name,
                    kind,
                    custom_harness,
                    model,
                    effort,
                    auto_title,
                    pr_url: pr_url.clone(),
                    head,
                    starting_prompt,
                    profile,
                })
                .await;
            match &result {
                Ok(nebula_core::EntityId::Agent(agent)) => tracing::info!(
                    req_id,
                    agent = %agent,
                    kind = kind.as_str(),
                    project = %project,
                    pr_url = %pr_url,
                    launch_mode = "pull_request",
                    "agent session spawned"
                ),
                Err(error) => tracing::warn!(
                    req_id,
                    error = %error,
                    kind = kind.as_str(),
                    project = %project,
                    pr_url = %pr_url,
                    launch_mode = "pull_request",
                    "agent session spawn failed"
                ),
                Ok(_) => unreachable!("CreatePrAgent returned a non-agent id"),
            }
            reply(&out_tx, req_id, result.map(Some)).await;
        });
    }

    #[allow(clippy::too_many_arguments)]
    async fn handle_spawn_sibling(
        &self,
        req_id: u64,
        id: nebula_core::AgentId,
        kind: Option<nebula_core::AgentKind>,
        starting_prompt: String,
        worktree: Option<String>,
        base: Option<String>,
        profile: Option<String>,
    ) {
        // Logged by mode only — never the prompt text.
        let worktree = worktree
            .as_deref()
            .map(|branch| crate::sibling::SiblingWorktree {
                branch,
                base: base.as_deref(),
            });
        let result = self
            .daemon
            .spawn_sibling_agent(&id, kind, worktree, &starting_prompt, profile.as_deref())
            .await;
        match &result {
            Ok(nebula_core::EntityId::Agent(agent)) => tracing::info!(
                req_id,
                agent = %agent,
                spawned_by = %id,
                launch_mode = "sibling",
                "agent session spawned"
            ),
            Err(error) => tracing::warn!(
                req_id,
                error = %error,
                spawned_by = %id,
                launch_mode = "sibling",
                "agent session spawn failed"
            ),
            Ok(_) => unreachable!("SpawnSiblingAgent returned a non-agent id"),
        }
        reply(&self.out_tx, req_id, result.map(Some)).await;
    }

    async fn handle_enter_worktree(
        &self,
        req_id: u64,
        id: nebula_core::AgentId,
        branch: String,
        base: Option<String>,
    ) -> Result<bool> {
        let ev = match self
            .daemon
            .enter_worktree(&id, &branch, base.as_deref())
            .await
        {
            Ok((worktree, outcome)) => ServerEvent::WorktreeEntered {
                req_id,
                worktree,
                outcome,
            },
            Err(e) => ServerEvent::Error {
                req_id: Some(req_id),
                message: format!("{e:#}"),
            },
        };
        self.send(ev).await;
        Ok(true)
    }

    fn spawn_send_cloud_message(&self, req_id: u64, id: nebula_core::AgentId, message: String) {
        tracing::info!(agent = %id, bytes = message.len(), "send to cloud session");
        // `claude -p … --cloud` is a login shell and a network round trip —
        // seconds. Off the request loop: inline, every keystroke and every
        // session switch on this connection waited for it.
        let daemon = self.daemon.clone();
        let out_tx = self.out_tx.clone();
        tokio::spawn(async move {
            reply_done(
                &out_tx,
                req_id,
                daemon.send_cloud_message(&id, &message).await,
            )
            .await;
        });
    }

    async fn blocking_daemon<T, F>(&self, f: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(Arc<Daemon>) -> Result<T> + Send + 'static,
    {
        let daemon = self.daemon.clone();
        tokio::task::spawn_blocking(move || f(daemon))
            .await
            .context("daemon blocking task panicked")?
    }

    async fn send(&self, ev: ServerEvent) {
        let _ = self.out_tx.send(ev).await;
    }

    async fn cleanup(&mut self) {
        for (sref, h) in self.attached.drain() {
            h.abort();
            self.daemon.note_detached(&sref);
        }
        // The forwarder holds a sender and waits on the next broadcast: left
        // running, it keeps the writer, and this client's socket, open until
        // the daemon next has something to say.
        if let Some(forward) = self.subscription.take() {
            forward.abort();
        }
    }
}

struct SpawnLogFields(Vec<(&'static str, String)>);

impl SpawnLogFields {
    fn field(&mut self, name: &'static str, value: String) {
        self.0.push((name, value));
    }
}

fn log_spawn_result<F>(
    req_id: u64,
    result: &anyhow::Result<nebula_core::EntityId>,
    kind: nebula_core::AgentKind,
    launch_mode: &'static str,
    label: &'static str,
    fields: F,
) where
    F: FnOnce(&mut SpawnLogFields),
{
    let mut extra = SpawnLogFields(Vec::new());
    fields(&mut extra);
    match result {
        Ok(nebula_core::EntityId::Agent(agent)) => {
            tracing::info!(
                req_id,
                agent = %agent,
                kind = kind.as_str(),
                launch_mode,
                extra = ?extra.0,
                "{label} spawned"
            );
        }
        Err(error) => {
            tracing::warn!(
                req_id,
                error = %error,
                kind = kind.as_str(),
                launch_mode,
                extra = ?extra.0,
                "{label} spawn failed"
            );
        }
        Ok(_) => unreachable!("CreateAgent returned a non-agent id"),
    }
}

fn empty_snapshot() -> ServerEvent {
    ServerEvent::Snapshot {
        projects: vec![],
        worktrees: vec![],
        agents: vec![],
        terminals: vec![],
        links: vec![],
        pr_seen: vec![],
        ui_state: None,
        subagents: vec![],
    }
}

fn snapshot_or_empty(daemon: &Daemon) -> ServerEvent {
    daemon.snapshot().unwrap_or_else(|error| {
        tracing::warn!(%error, "failed to build resync snapshot");
        empty_snapshot()
    })
}

/// The usage lists a subscriber at `peer` gets beside the Snapshot: none
/// for a peer older than `USAGE_EVENTS_PROTOCOL`.
async fn usage_events(daemon: &Arc<Daemon>, peer: u32) -> Vec<ServerEvent> {
    if peer < nebula_core::USAGE_EVENTS_PROTOCOL {
        return Vec::new();
    }
    let daemon = daemon.clone();
    tokio::task::spawn_blocking(move || {
        vec![
            ServerEvent::AccountUsage {
                accounts: daemon.account_usage_list(),
            },
            ServerEvent::SessionContexts {
                contexts: daemon.session_contexts(),
            },
        ]
    })
    .await
    .unwrap_or_default()
}

/// The STOP NOTES a subscriber at `peer` gets beside the Snapshot (the
/// whole list, so a resync replaces its own): none for a peer older than
/// `LIMITS_PROTOCOL`.
fn stop_events(daemon: &Daemon, peer: u32) -> Vec<ServerEvent> {
    if peer < nebula_core::LIMITS_PROTOCOL {
        return Vec::new();
    }
    vec![ServerEvent::SessionStops {
        stops: daemon.session_stops(),
    }]
}

/// The recorded worktree bases a subscriber at `peer` gets beside the
/// Snapshot: none for a peer older than `RECORDED_BASE_PROTOCOL`.
async fn base_events(daemon: &Arc<Daemon>, peer: u32) -> Vec<ServerEvent> {
    if peer < nebula_core::RECORDED_BASE_PROTOCOL {
        return Vec::new();
    }
    let store = daemon.store.clone();
    match tokio::task::spawn_blocking(move || store.worktree_bases()).await {
        Ok(Ok(bases)) => vec![ServerEvent::WorktreeBases { bases }],
        Ok(Err(error)) => {
            tracing::warn!(%error, "read worktree bases failed");
            Vec::new()
        }
        Err(_) => Vec::new(),
    }
}

async fn send_snapshot(daemon: &Arc<Daemon>, tx: &mpsc::Sender<ServerEvent>, peer: u32) -> bool {
    if !(tx.send(snapshot_or_empty(daemon)).await.is_ok()
        && tx
            .send(ServerEvent::WorktreePorts {
                ports: daemon.worktree_ports(),
            })
            .await
            .is_ok())
    {
        return false;
    }
    for ev in base_events(daemon, peer)
        .await
        .into_iter()
        .chain(usage_events(daemon, peer).await)
        .chain(stop_events(daemon, peer))
    {
        if tx.send(ev).await.is_err() {
            return false;
        }
    }
    true
}

async fn forward_subscription_events(
    daemon: Arc<Daemon>,
    mut rx: broadcast::Receiver<ServerEvent>,
    tx: mpsc::Sender<ServerEvent>,
    peer: u32,
) {
    loop {
        match rx.recv().await {
            Ok(ev) => {
                // An event an older peer cannot decode would drop its
                // connection mid-stream: it never sees it.
                if !ev.decodable_by(peer) {
                    continue;
                }
                if tx.send(ev).await.is_err() {
                    break;
                }
            }
            Err(broadcast::error::RecvError::Lagged(skipped)) => {
                tracing::warn!(skipped, "client subscription lagged; sending snapshot");
                if !send_snapshot(&daemon, &tx, peer).await {
                    break;
                }
            }
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
}

/// [`reply`] for the requests that create nothing: success is a bare Ack.
/// The request id and claimed caller of a privileged orchestrator verb.
fn privileged_caller(req: &ClientRequest) -> Option<(u64, nebula_core::AgentId)> {
    use ClientRequest as R;
    match req {
        R::OrchestratorList { req_id, caller }
        | R::OrchestratorRead { req_id, caller, .. }
        | R::OrchestratorSend { req_id, caller, .. }
        | R::OrchestratorSpawn { req_id, caller, .. }
        | R::OrchestratorOpenReview { req_id, caller, .. }
        | R::OrchestratorSessions { req_id, caller }
        | R::OrchestratorTell { req_id, caller, .. }
        | R::OrchestratorReadSession { req_id, caller, .. }
        | R::OrchestratorSpawnWith { req_id, caller, .. } => Some((*req_id, caller.clone())),
        _ => None,
    }
}

async fn reply_done(out_tx: &mpsc::Sender<ServerEvent>, req_id: u64, result: anyhow::Result<()>) {
    reply(out_tx, req_id, result.map(|_| None)).await
}

async fn reply(
    out_tx: &mpsc::Sender<ServerEvent>,
    req_id: u64,
    result: anyhow::Result<Option<nebula_core::EntityId>>,
) {
    let ev = match result {
        Ok(created) => ServerEvent::Ack { req_id, created },
        Err(e) => ServerEvent::Error {
            req_id: Some(req_id),
            message: format!("{e:#}"),
        },
    };
    let _ = out_tx.send(ev).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::HookEnv;
    use crate::store::Store;

    /// A daemon on an in-memory store whose usage readings live under a
    /// path of its own (absent, so it reads as no readings), never the
    /// real profiles root: a subscription's first look reads them.
    fn test_daemon() -> Arc<Daemon> {
        let daemon = Daemon::new(
            Arc::new(Store::open_in_memory().unwrap()),
            HookEnv {
                port: 0,
                token: String::new(),
            },
        );
        daemon.set_usage_root(
            std::env::temp_dir().join(format!("kete-server-test-usage-{}", std::process::id())),
        );
        daemon
    }

    #[tokio::test]
    async fn lagged_subscription_resyncs_with_a_snapshot() {
        let daemon = test_daemon();
        let (events, rx) = broadcast::channel(1);
        events
            .send(ServerEvent::Ack {
                req_id: 1,
                created: None,
            })
            .unwrap();
        events
            .send(ServerEvent::Ack {
                req_id: 2,
                created: None,
            })
            .unwrap();

        let (tx, mut out) = mpsc::channel(4);
        let forward = tokio::spawn(forward_subscription_events(
            daemon,
            rx,
            tx,
            nebula_core::PROTOCOL_VERSION,
        ));

        let ev = tokio::time::timeout(std::time::Duration::from_secs(1), out.recv())
            .await
            .expect("forwarder sent no resync")
            .expect("forwarder closed");
        forward.abort();

        assert!(
            matches!(ev, ServerEvent::Snapshot { .. }),
            "lag should resync with a snapshot, got {ev:?}"
        );
    }

    /// The recorded bases ride beside a Snapshot to a 59 subscriber and
    /// never to a 58 one, whose decoder would drop the connection on them.
    #[tokio::test]
    async fn worktree_bases_follow_a_snapshot_only_to_peers_that_decode_them() {
        for (peer, expect) in [(58u32, false), (59, true)] {
            let daemon = test_daemon();
            let (tx, mut out) = mpsc::channel(16);
            assert!(send_snapshot(&daemon, &tx, peer).await);
            drop(tx);
            let mut got = Vec::new();
            while let Some(ev) = out.recv().await {
                got.push(ev);
            }
            assert!(matches!(got.first(), Some(ServerEvent::Snapshot { .. })));
            assert_eq!(
                got.iter()
                    .any(|ev| matches!(ev, ServerEvent::WorktreeBases { .. })),
                expect,
                "peer {peer}: {got:?}"
            );
            assert!(got.iter().all(|ev| ev.decodable_by(peer)));
        }
    }

    /// Usage events reach a 55 subscriber and never a 54 one, whose
    /// decoder would fail on the unknown variant and drop the connection.
    #[tokio::test]
    async fn usage_events_are_forwarded_only_to_peers_that_decode_them() {
        let usage = || ServerEvent::AccountUsage {
            accounts: Vec::new(),
        };
        let ack = || ServerEvent::Ack {
            req_id: 7,
            created: None,
        };
        for (peer, expect_usage) in [(54u32, false), (55, true)] {
            let daemon = test_daemon();
            let (events, rx) = broadcast::channel(8);
            events.send(usage()).unwrap();
            events.send(ack()).unwrap();
            let (tx, mut out) = mpsc::channel(8);
            let forward = tokio::spawn(forward_subscription_events(daemon, rx, tx, peer));
            let first = tokio::time::timeout(std::time::Duration::from_secs(1), out.recv())
                .await
                .unwrap()
                .unwrap();
            forward.abort();
            assert_eq!(
                matches!(first, ServerEvent::AccountUsage { .. }),
                expect_usage,
                "peer {peer}: {first:?}"
            );
            if !expect_usage {
                assert!(matches!(first, ServerEvent::Ack { req_id: 7, .. }));
            }
        }
        assert!(usage_events(&test_daemon(), 54).await.is_empty());
    }

    /// A handshaken connection over `daemon`, and the receiving end of
    /// its replies.
    fn connection(daemon: Arc<Daemon>) -> (ClientConnection, mpsc::Receiver<ServerEvent>) {
        let (tx, rx) = mpsc::channel(16);
        let mut client = ClientConnection::new(daemon, tx);
        client.handshaken = true;
        (client, rx)
    }

    async fn error_for(rx: &mut mpsc::Receiver<ServerEvent>, req: u64) -> Option<String> {
        loop {
            let ev = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
                .await
                .expect("no reply")
                .expect("connection closed");
            match ev {
                ServerEvent::Error {
                    req_id: Some(r),
                    message,
                } if r == req => return Some(message),
                ServerEvent::Ack { req_id, .. } if req_id == req => return None,
                _ => continue,
            }
        }
    }

    fn seed(daemon: &Daemon, dir: &std::path::Path) {
        use nebula_core::{Project, ProjectId, Worktree, WorktreeId};
        daemon
            .store
            .insert_project(&Project {
                id: ProjectId("p".into()),
                name: "demo".into(),
                repo_path: dir.into(),
                sort_order: 0,
            })
            .unwrap();
        daemon
            .store
            .insert_worktree(&Worktree {
                id: WorktreeId("w".into()),
                project_id: ProjectId("p".into()),
                path: dir.into(),
                branch: "main".into(),
                is_main: true,
                sort_order: 0,
            })
            .unwrap();
    }

    /// CreateAgent was the escalation door: any client could mint an
    /// orchestrator by naming the role. It is refused before anything is
    /// created.
    #[tokio::test]
    async fn create_agent_cannot_grant_the_orchestrator_role() {
        let daemon = test_daemon();
        let dir = tempfile::tempdir().unwrap();
        seed(&daemon, dir.path());
        let (mut client, mut rx) = connection(daemon.clone());
        client
            .handle_request(ClientRequest::CreateAgent {
                req_id: 7,
                worktree: nebula_core::WorktreeId("w".into()),
                name: "sneaky".into(),
                kind: nebula_core::AgentKind::Claude,
                custom_harness: None,
                model: None,
                effort: None,
                auto_title: false,
                cloud_prompt: None,
                starting_prompt: None,
                issue_url: None,
                role: nebula_core::AgentRole::Orchestrator,
                profile: None,
            })
            .await
            .unwrap();
        let err = error_for(&mut rx, 7).await.expect("refused");
        assert!(err.contains("cannot grant the orchestrator role"), "{err}");
        assert!(daemon.store.load_tree().unwrap().2.is_empty());
        assert!(daemon.store.orchestrator().unwrap().is_none());
    }

    /// `Reorder` answers its req_id: Ack when written, the reason when
    /// refused.
    #[tokio::test]
    async fn reorder_is_acked_or_refused_with_its_reason() {
        use nebula_core::{ProjectId, ReorderScope, Worktree, WorktreeId};
        let daemon = test_daemon();
        let dir = tempfile::tempdir().unwrap();
        seed(&daemon, dir.path());
        daemon
            .store
            .insert_worktree(&Worktree {
                id: WorktreeId("feat".into()),
                project_id: ProjectId("p".into()),
                path: dir.path().join("feat"),
                branch: "feat".into(),
                is_main: false,
                sort_order: 0,
            })
            .unwrap();
        let (mut client, mut rx) = connection(daemon.clone());
        let scope = ReorderScope::Worktrees(ProjectId("p".into()));
        client
            .handle_request(ClientRequest::Reorder {
                req_id: 3,
                scope: scope.clone(),
                ids: vec!["feat".into()],
            })
            .await
            .unwrap();
        assert_eq!(error_for(&mut rx, 3).await, None, "acked");
        client
            .handle_request(ClientRequest::Reorder {
                req_id: 4,
                scope,
                ids: vec!["feat".into(), "w".into()],
            })
            .await
            .unwrap();
        let err = error_for(&mut rx, 4).await.expect("refused");
        assert!(err.contains("main checkout"), "{err}");
    }

    /// A worker claiming the orchestrator's id (`NEBULA_AGENT_ID=<orch>`)
    /// from inside its own session is refused every privileged verb; the
    /// orchestrator's own session, and a process inside no session, pass.
    #[tokio::test]
    async fn a_worker_cannot_borrow_the_orchestrators_id() {
        #[cfg(unix)]
        let stand_in = "/bin/cat";
        #[cfg(windows)]
        let stand_in = "cmd.exe";
        let _cli = crate::registry::tests::EnvGuard::set(nebula_core::env::AGENT_CMD, stand_in);
        let daemon = test_daemon();
        let dir = tempfile::tempdir().unwrap();
        seed(&daemon, dir.path());
        let row = |id: &str, role| nebula_core::Agent {
            parent: None,
            profile: None,
            id: nebula_core::AgentId(id.into()),
            worktree_id: nebula_core::WorktreeId("w".into()),
            name: id.into(),
            status: nebula_core::AgentStatus::Fresh,
            archived: false,
            archived_at: 0,
            unseen: false,
            kind: nebula_core::AgentKind::Claude,
            custom_harness: None,
            model: None,
            effort: None,
            session_id: None,
            cloud_session_id: None,
            sort_order: 0,
            status_changed_at: 0,
            alive: false,
            issue_url: None,
            role,
            recent_prompts: Vec::new(),
        };
        let mut roots = std::collections::HashMap::new();
        for (id, role) in [
            ("worker", nebula_core::AgentRole::Worker),
            ("orch", nebula_core::AgentRole::Orchestrator),
        ] {
            daemon.store.insert_agent(&row(id, role)).unwrap();
            let sref = SessionRef::Agent(nebula_core::AgentId(id.into()));
            daemon.ensure_session(&sref, 80, 24).unwrap();
            let pid = daemon
                .session_pids()
                .into_iter()
                .find(|(s, _, _)| *s == sref)
                .map(|(_, pid, _)| pid)
                .unwrap();
            roots.insert(id, pid);
        }
        let orch = nebula_core::AgentId("orch".into());
        let (mut client, mut rx) = connection(daemon.clone());
        for (req_id, peer, refused) in [
            (1, roots["worker"], true),
            (2, roots["orch"], false),
            (3, std::process::id(), false),
        ] {
            client.peer_pid = Some(peer);
            client
                .handle_request(ClientRequest::OrchestratorList {
                    req_id,
                    caller: orch.clone(),
                })
                .await
                .unwrap();
            let answer = loop {
                match rx.recv().await.expect("closed") {
                    ServerEvent::Error {
                        req_id: Some(r),
                        message,
                    } if r == req_id => break Err(message),
                    ServerEvent::OrchestratorList { req_id: r, .. } if r == req_id => break Ok(()),
                    _ => continue,
                }
            };
            if refused {
                let err = answer.unwrap_err();
                assert!(err.contains("another session"), "{err}");
            } else {
                assert_eq!(answer, Ok(()), "peer {peer}");
            }
        }

        for id in ["worker", "orch"] {
            daemon.kill_session(&SessionRef::Agent(nebula_core::AgentId(id.into())));
        }
    }

    /// A process running inside an agent session (here: the session's own
    /// PTY child) cannot pin, clear or start the orchestrator; the same
    /// request from outside every session goes through.
    #[tokio::test]
    async fn provisioning_is_refused_from_inside_an_agent_session() {
        #[cfg(unix)]
        let stand_in = "/bin/cat";
        #[cfg(windows)]
        let stand_in = "cmd.exe";
        let _cli = crate::registry::tests::EnvGuard::set(nebula_core::env::AGENT_CMD, stand_in);
        let daemon = test_daemon();
        let dir = tempfile::tempdir().unwrap();
        seed(&daemon, dir.path());
        let worker = nebula_core::Agent {
            parent: None,
            profile: None,
            id: nebula_core::AgentId("worker".into()),
            worktree_id: nebula_core::WorktreeId("w".into()),
            name: "worker".into(),
            status: nebula_core::AgentStatus::Fresh,
            archived: false,
            archived_at: 0,
            unseen: false,
            kind: nebula_core::AgentKind::Claude,
            custom_harness: None,
            model: None,
            effort: None,
            session_id: None,
            cloud_session_id: None,
            sort_order: 0,
            status_changed_at: 0,
            alive: false,
            issue_url: None,
            role: nebula_core::AgentRole::Worker,
            recent_prompts: Vec::new(),
        };
        daemon.store.insert_agent(&worker).unwrap();
        let sref = SessionRef::Agent(worker.id.clone());
        daemon.ensure_session(&sref, 80, 24).unwrap();
        let root = daemon
            .session_pids()
            .into_iter()
            .find(|(s, _, _)| *s == sref)
            .map(|(_, pid, _)| pid)
            .expect("the session has a pid");

        let (mut client, mut rx) = connection(daemon.clone());
        client.peer_pid = Some(root);
        for (req_id, req) in [
            (
                1,
                ClientRequest::SetOrchestrator {
                    req_id: 1,
                    agent: worker.id.clone(),
                },
            ),
            (2, ClientRequest::ClearOrchestrator { req_id: 2 }),
            (
                3,
                ClientRequest::StartOrchestrator {
                    req_id: 3,
                    kind: nebula_core::AgentKind::Claude,
                    custom_harness: None,
                    model: None,
                    effort: None,
                    profile: None,
                    project: None,
                    name: None,
                    replace: true,
                },
            ),
        ] {
            client.handle_request(req).await.unwrap();
            let err = error_for(&mut rx, req_id).await.expect("refused");
            assert!(err.contains("inside an agent session"), "{err}");
        }
        assert!(daemon.store.orchestrator().unwrap().is_none());

        // From outside every session: allowed.
        client.peer_pid = Some(std::process::id());
        client
            .handle_request(ClientRequest::SetOrchestrator {
                req_id: 4,
                agent: worker.id.clone(),
            })
            .await
            .unwrap();
        assert_eq!(error_for(&mut rx, 4).await, None);
        assert_eq!(daemon.store.orchestrator().unwrap().unwrap().id, worker.id);
        daemon.kill_session(&sref);
    }
}
