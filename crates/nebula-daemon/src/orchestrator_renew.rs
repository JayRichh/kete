//! ORCHESTRATOR RENEWAL (K9): a fresh orchestrator started from a handover
//! note once the current one's turn ends, holding the role, the old one
//! archived.
//!
//! Every refusal happens before anything changes: who asks (the server
//! gates the peer, [`Daemon::renew_orchestrator`] the caller's role), the
//! note (where it lives, its size, UTF-8), and one renewal at a time. The
//! swap then runs on its own task: it waits for the old session's turn to
//! end, starts the new orchestrator with `replace` (the old one demoted,
//! the new one created holding the role), and waits for the new session's
//! first hook. A new session that never reports is stopped and archived
//! and the old one re-pinned, so a failure leaves it in charge.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use nebula_core::studio::wire::RenewState;
use nebula_core::{Agent, AgentId, AgentStatus, EntityId, ProjectId, ServerEvent};

use crate::orchestrator::StartOrchestratorSpec;
use crate::registry::Daemon;

/// The largest handover note a renewal takes.
pub const NOTE_MAX_BYTES: u64 = 65_536;

/// The renewal log, one JSON line per renewal, under the data dir.
pub const RENEWALS_FILE: &str = "orchestrator-renewals.jsonl";

/// What `RenewOrchestrator` asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenewRequest {
    /// The orchestrator renewing itself; None for a person (the server
    /// refuses that from inside an agent session).
    pub caller: Option<AgentId>,
    /// The handover note, the new orchestrator's first prompt.
    pub note: PathBuf,
    /// The account the new one runs on; None keeps the old one's.
    pub profile: Option<String>,
    /// The model the new one runs; None keeps the old one's.
    pub model: Option<String>,
    /// Leave the old session running as a worker.
    pub keep_old: bool,
    /// Swap at once instead of at the old session's turn end.
    pub now: bool,
}

/// Renewals waiting on a turn end, on `Daemon::renewals`, taken alone.
#[derive(Debug)]
pub struct RenewBook {
    /// The orchestrator a scheduled renewal replaces; one at a time.
    pending: Option<AgentId>,
    /// How long the swap waits for the old session's turn to end.
    turn_wait: Duration,
    /// How long the new session has to send its first hook.
    report_wait: Duration,
    /// How often both waits look.
    poll: Duration,
    /// The renewal log, when not the data dir's (tests).
    log: Option<PathBuf>,
}

impl Default for RenewBook {
    fn default() -> Self {
        Self {
            pending: None,
            turn_wait: Duration::from_secs(300),
            report_wait: Duration::from_secs(60),
            poll: Duration::from_millis(250),
            log: None,
        }
    }
}

/// A checked renewal, ready to carry out.
struct Plan {
    old: Agent,
    project: ProjectId,
    note: PathBuf,
    text: String,
    profile: Option<String>,
    model: Option<String>,
    role_card: Option<String>,
    keep_old: bool,
    wait: bool,
}

impl Daemon {
    /// Schedule a renewal; answered `Scheduled`, then `Done` or `Failed`
    /// is broadcast as `ServerEvent::OrchestratorRenewal`.
    pub fn renew_orchestrator(self: &Arc<Self>, req: RenewRequest) -> Result<RenewState, String> {
        let plan = self.plan_renewal(req).map_err(|e| format!("{e:#}"))?;
        {
            let mut book = self.renewals.lock();
            if let Some(old) = &book.pending {
                return Err(format!(
                    "a renewal of {old} is already scheduled; wait for it to finish"
                ));
            }
            book.pending = Some(plan.old.id.clone());
        }
        let daemon = self.clone();
        tokio::runtime::Handle::current().spawn(async move { daemon.carry_out(plan).await });
        Ok(RenewState::Scheduled)
    }

    /// Every check, before anything changes.
    fn plan_renewal(&self, req: RenewRequest) -> Result<Plan> {
        let old = match &req.caller {
            Some(id) => self.require_orchestrator(id)?,
            None => self
                .store
                .orchestrator()?
                .filter(|a| !a.archived)
                .context("there is no orchestrator to renew")?,
        };
        let (note, text) = self.read_note(&req.note)?;
        let project = self
            .store
            .get_worktree(&old.worktree_id)?
            .context("the orchestrator's worktree is gone")?
            .project_id;
        // Only a CLI mid-turn is waited on; one at its prompt, or not
        // running, has no turn to finish.
        let busy = matches!(
            old.status,
            AgentStatus::Running | AgentStatus::NeedsFeedback
        );
        Ok(Plan {
            role_card: self.role_card_of(&old.id),
            profile: req.profile.or_else(|| old.profile.clone()),
            model: req.model.or_else(|| old.model.clone()),
            project,
            note,
            text,
            keep_old: req.keep_old,
            wait: busy && !req.now,
            old,
        })
    }

    /// The handover note, canonical, and its text: a file inside the
    /// orchestrator's workspace or a registered project, UTF-8, 1 to
    /// [`NOTE_MAX_BYTES`] bytes.
    fn read_note(&self, note: &Path) -> Result<(PathBuf, String)> {
        let path = nebula_core::paths::canonical(note)
            .with_context(|| format!("the handover note {} is not there", note.display()))?;
        let meta = std::fs::metadata(&path)
            .with_context(|| format!("read the handover note {}", path.display()))?;
        if !meta.is_file() {
            bail!("the handover note {} is not a file", path.display());
        }
        if !self.note_roots()?.iter().any(|root| path.starts_with(root)) {
            bail!(
                "the handover note {} is outside the orchestrator's workspace ({}) and every \
                 registered project",
                path.display(),
                self.orchestrator_home().display()
            );
        }
        let len = meta.len();
        if len == 0 || len > NOTE_MAX_BYTES {
            bail!("the handover note is {len} bytes; it must be 1 to {NOTE_MAX_BYTES}");
        }
        let bytes = std::fs::read(&path)
            .with_context(|| format!("read the handover note {}", path.display()))?;
        // The file may have changed since the size check.
        if bytes.is_empty() || bytes.len() as u64 > NOTE_MAX_BYTES {
            bail!(
                "the handover note is {} bytes; it must be 1 to {NOTE_MAX_BYTES}",
                bytes.len()
            );
        }
        let text = String::from_utf8(bytes)
            .map_err(|_| anyhow::anyhow!("the handover note {} is not UTF-8", path.display()))?;
        Ok((path, text))
    }

    /// Where a handover note may live: the orchestrator's workspace, every
    /// registered project and every checkout of one, canonical.
    fn note_roots(&self) -> Result<Vec<PathBuf>> {
        let (projects, worktrees, ..) = self.store.load_tree()?;
        Ok(std::iter::once(self.orchestrator_home())
            .chain(projects.into_iter().map(|p| p.repo_path))
            .chain(worktrees.into_iter().map(|w| w.path))
            .filter_map(|p| nebula_core::paths::canonical(&p).ok())
            .collect())
    }

    /// The swap, then the record and the broadcast, whatever the outcome.
    async fn carry_out(self: Arc<Self>, plan: Plan) {
        let old = plan.old.id.clone();
        let note = plan.note.clone();
        let (new, state) = match self.swap(plan).await {
            Ok(new) => (Some(new), RenewState::Done),
            Err((new, e)) => (
                new,
                RenewState::Failed {
                    reason: format!("{e:#}"),
                },
            ),
        };
        let log = {
            let mut book = self.renewals.lock();
            book.pending = None;
            book.log
                .clone()
                .unwrap_or_else(|| nebula_core::paths::data_dir().join(RENEWALS_FILE))
        };
        if let Err(e) = append_record(&log, &old, new.as_ref(), &note, &state) {
            tracing::warn!(error = %e, "renewal record failed");
        }
        tracing::info!(%old, new = ?new, state = ?state, "orchestrator renewal");
        self.broadcast(ServerEvent::OrchestratorRenewal {
            req_id: None,
            old,
            new,
            state,
        });
    }

    /// Wait, start the new orchestrator, wait for its first hook. An error
    /// names the new session when one was started (and rolled back).
    async fn swap(
        self: &Arc<Self>,
        plan: Plan,
    ) -> std::result::Result<AgentId, (Option<AgentId>, anyhow::Error)> {
        let (turn_wait, report_wait, poll) = {
            let book = self.renewals.lock();
            (book.turn_wait, book.report_wait, book.poll)
        };
        let old = &plan.old;
        if plan.wait {
            let deadline = Instant::now() + turn_wait;
            loop {
                match self.store.get_agent(&old.id) {
                    Ok(Some(a))
                        if matches!(
                            a.status,
                            AgentStatus::Running | AgentStatus::NeedsFeedback
                        ) => {}
                    Ok(Some(_)) => break,
                    Ok(None) => {
                        return Err((None, anyhow::anyhow!("the orchestrator was deleted")))
                    }
                    Err(e) => return Err((None, e)),
                }
                if Instant::now() >= deadline {
                    return Err((
                        None,
                        anyhow::anyhow!(
                            "the orchestrator's turn did not end within {} s",
                            turn_wait.as_secs()
                        ),
                    ));
                }
                tokio::time::sleep(poll).await;
            }
        }
        // The old one may have been archived or replaced while it waited.
        match self.store.orchestrator() {
            Ok(Some(a)) if a.id == old.id && !a.archived => {}
            Ok(_) => {
                return Err((
                    None,
                    anyhow::anyhow!("the orchestrator changed while the renewal waited"),
                ))
            }
            Err(e) => return Err((None, e)),
        }
        let started = self
            .start_orchestrator(StartOrchestratorSpec {
                kind: Some(old.kind),
                custom_harness: old.custom_harness.clone(),
                model: plan.model.clone(),
                effort: old.effort.clone(),
                profile: plan.profile.clone(),
                project: Some(plan.project.clone()),
                name: Some(old.name.clone()),
                replace: true,
                role_card: plan.role_card.clone(),
                starting_prompt: Some(starting_prompt(
                    &old.name,
                    &plan.note,
                    &plan.text,
                    nebula_core::clock::now_secs() as i64,
                )),
            })
            .await
            .map_err(|e| (None, e.context("start the new orchestrator")))?;
        let EntityId::Agent(new) = started else {
            return Err((
                None,
                anyhow::anyhow!("the new orchestrator is not a session"),
            ));
        };
        let deadline = Instant::now() + report_wait;
        loop {
            match self.store.get_agent(&new) {
                // Only a hook saves the CLI's session id: the submit of
                // its first prompt, the handover itself.
                Ok(Some(a)) if a.session_id.is_some() => break,
                // Died before it said a word: no point waiting out the rest.
                Ok(Some(a)) if a.status == AgentStatus::Terminated => {
                    return Err(self
                        .roll_back(old, new, "the new orchestrator exited before it reported")
                        .await)
                }
                Ok(Some(_)) => {}
                Ok(None) => {
                    return Err(self
                        .roll_back(old, new, "the new orchestrator was deleted")
                        .await)
                }
                Err(e) => return Err((Some(new), e)),
            }
            if Instant::now() >= deadline {
                let why = format!(
                    "the new orchestrator sent no hook within {} s",
                    report_wait.as_secs()
                );
                return Err(self.roll_back(old, new, &why).await);
            }
            tokio::time::sleep(poll).await;
        }
        if !plan.keep_old {
            if let Err(e) = self.archive_agent(&old.id) {
                tracing::warn!(agent = %old.id, error = %e, "archive the renewed orchestrator");
            }
        }
        Ok(new)
    }

    /// The new session stopped and archived, the old one pinned again. The
    /// pin goes through the store, not `Daemon::set_orchestrator`: that
    /// would type the guidance into the old session as a turn, and its own
    /// system prompt already holds it.
    async fn roll_back(
        self: &Arc<Self>,
        old: &Agent,
        new: AgentId,
        why: &str,
    ) -> (Option<AgentId>, anyhow::Error) {
        let _gate = self.provision_gate.lock().await;
        self.kill_session(&nebula_core::SessionRef::Agent(new.clone()));
        // One transaction: the new row demoted, the old one promoted, so
        // there is never a second orchestrator row.
        match self.store.set_orchestrator(Some(&old.id)) {
            Ok(changed) => {
                for id in changed.iter().chain([&old.id]) {
                    let _ = self.broadcast_agent(id);
                }
            }
            Err(e) => tracing::warn!(error = %e, "re-pin the old orchestrator"),
        }
        if let Err(e) = self.archive_agent(&new) {
            tracing::warn!(agent = %new, error = %e, "archive the failed orchestrator");
        }
        (
            Some(new),
            anyhow::anyhow!("{why}; {} is still the orchestrator", old.name),
        )
    }

    /// Shorten the waits and move the log (tests).
    #[cfg(test)]
    fn set_renew_waits(&self, turn: Duration, report: Duration, log: PathBuf) {
        let mut book = self.renewals.lock();
        book.log = Some(log);
        book.turn_wait = turn;
        book.report_wait = report;
        book.poll = Duration::from_millis(20);
    }
}

/// The new orchestrator's first prompt (spec section 13, word for word),
/// with the note quoted after it. `now` is epoch seconds, shown as local
/// `HH:MM`.
pub fn starting_prompt(old_name: &str, note: &Path, text: &str, now: i64) -> String {
    let local = now + i64::from(nebula_core::usage_window::local(now));
    let time = nebula_core::wake::hhmm((local.rem_euclid(86_400) / 60) as u16);
    format!(
        "[from kete renew] You are the orchestrator, renewed from {old_name} at {time}. Your \
         handover note is {}; it is quoted below. Read it, then carry on.\n\n{text}",
        note.display()
    )
}

/// One line of the renewal log: `{ at, old, new, note, state, reason }`,
/// `at` in epoch ms, `state` `done` or `failed`.
fn append_record(
    file: &Path,
    old: &AgentId,
    new: Option<&AgentId>,
    note: &Path,
    state: &RenewState,
) -> Result<()> {
    use std::io::Write;
    let (state, reason) = match state {
        RenewState::Scheduled => ("scheduled", None),
        RenewState::Done => ("done", None),
        RenewState::Failed { reason } => ("failed", Some(reason.as_str())),
    };
    let line = serde_json::json!({
        "at": nebula_core::clock::now_ms(),
        "old": old.as_str(),
        "new": new.map(AgentId::as_str),
        "note": note.display().to_string(),
        "state": state,
        "reason": reason,
    });
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(file)?;
    writeln!(f, "{line}")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::HookEnv;
    use crate::registry::tests::EnvGuard;
    use crate::status::HookEvent;
    use crate::store::Store;
    use nebula_core::{env, AgentKind, AgentRole, Project, SessionRef, Worktree, WorktreeId};

    struct Fixture {
        daemon: Arc<Daemon>,
        repo: tempfile::TempDir,
        data: tempfile::TempDir,
        _env: EnvGuard,
    }

    impl Fixture {
        /// A note at `name` in the project's repo.
        fn note(&self, name: &str, body: &[u8]) -> PathBuf {
            let path = self.repo.path().join(name);
            std::fs::write(&path, body).unwrap();
            path
        }

        fn log(&self) -> PathBuf {
            self.data.path().join(RENEWALS_FILE)
        }
    }

    /// A daemon with project `p` (root worktree `w`) in a temp repo, a temp
    /// data dir and workspace, and a stand-in CLI for every spawn.
    fn fixture() -> Fixture {
        let data = tempfile::tempdir().unwrap();
        #[cfg(unix)]
        let program = "/bin/cat";
        #[cfg(windows)]
        let program = "cmd.exe";
        let env = EnvGuard::set_all(&[
            (env::AGENT_CMD, program),
            (env::DATA_DIR, data.path().to_str().unwrap()),
            (env::CONFIG_FILE, ""),
        ]);
        let repo = tempfile::tempdir().unwrap();
        let daemon = Daemon::new(
            Arc::new(Store::open_in_memory().unwrap()),
            HookEnv {
                port: 0,
                token: String::new(),
            },
        );
        daemon.set_orchestrator_home(data.path().join("orchestrator"));
        daemon
            .store
            .insert_project(&Project {
                id: ProjectId("p".into()),
                name: "demo".into(),
                repo_path: repo.path().to_path_buf(),
                sort_order: 0,
            })
            .unwrap();
        daemon
            .store
            .insert_worktree(&Worktree {
                id: WorktreeId("w".into()),
                project_id: ProjectId("p".into()),
                path: repo.path().to_path_buf(),
                branch: "main".into(),
                is_main: true,
                sort_order: 0,
            })
            .unwrap();
        daemon.set_renew_waits(
            Duration::from_secs(5),
            Duration::from_secs(5),
            data.path().join(RENEWALS_FILE),
        );
        Fixture {
            daemon,
            repo,
            data,
            _env: env,
        }
    }

    /// A stored row, no process behind it.
    fn row(id: &str, role: AgentRole) -> Agent {
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

    fn ask(caller: Option<&str>, note: PathBuf) -> RenewRequest {
        RenewRequest {
            caller: caller.map(|c| AgentId(c.into())),
            note,
            profile: None,
            model: None,
            keep_old: false,
            now: false,
        }
    }

    /// A live orchestrator started the real way, its CLI the stand-in.
    async fn started(f: &Fixture) -> AgentId {
        let EntityId::Agent(id) = f
            .daemon
            .start_orchestrator(StartOrchestratorSpec {
                project: Some(ProjectId("p".into())),
                name: Some("orch".into()),
                ..Default::default()
            })
            .await
            .unwrap()
        else {
            panic!("not a session");
        };
        id
    }

    fn pinned(f: &Fixture) -> Option<AgentId> {
        f.daemon.store.orchestrator().unwrap().map(|a| a.id)
    }

    fn orchestrator_rows(f: &Fixture) -> usize {
        let agents = f.daemon.store.load_tree().unwrap().2;
        agents.iter().filter(|a| a.role.is_orchestrator()).count()
    }

    /// The new orchestrator, once the swap has started it; at every look
    /// there is at most one orchestrator row.
    async fn new_orchestrator(f: &Fixture, old: &AgentId) -> AgentId {
        for _ in 0..500 {
            assert!(orchestrator_rows(f) <= 1, "two orchestrator rows");
            if let Some(id) = pinned(f).filter(|id| id != old) {
                return id;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("no new orchestrator was started");
    }

    /// The broadcast that ends the renewal.
    async fn outcome(
        rx: &mut tokio::sync::broadcast::Receiver<ServerEvent>,
    ) -> (AgentId, Option<AgentId>, RenewState) {
        let wait = async {
            loop {
                if let Ok(ServerEvent::OrchestratorRenewal {
                    req_id,
                    old,
                    new,
                    state,
                }) = rx.recv().await
                {
                    assert_eq!(req_id, None);
                    return (old, new, state);
                }
            }
        };
        tokio::time::timeout(Duration::from_secs(20), wait)
            .await
            .expect("no renewal broadcast")
    }

    /// The new session's first hooks, as Claude sends them: SessionStart
    /// (which saves nothing on its own), then its first prompt's submit.
    fn report(f: &Fixture, id: &AgentId) {
        f.daemon.apply_hook_event(
            id,
            HookEvent::SessionStart {
                source: Some("startup".into()),
            },
            Some("sid-new".into()),
        );
        f.daemon
            .apply_hook_event(id, HookEvent::UserPromptSubmit, Some("sid-new".into()));
    }

    fn log_lines(f: &Fixture) -> Vec<serde_json::Value> {
        std::fs::read_to_string(f.log())
            .unwrap_or_default()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    #[test]
    fn the_orchestrator_may_renew_and_a_worker_may_not() {
        let f = fixture();
        f.daemon
            .store
            .insert_agent(&row("orch", AgentRole::Orchestrator))
            .unwrap();
        f.daemon
            .store
            .insert_agent(&row("worker", AgentRole::Worker))
            .unwrap();
        let note = f.note("handover.md", b"carry on");
        assert!(f
            .daemon
            .plan_renewal(ask(Some("orch"), note.clone()))
            .is_ok());
        let err = f
            .daemon
            .plan_renewal(ask(Some("worker"), note.clone()))
            .err()
            .unwrap();
        assert!(
            err.to_string()
                .contains("only available to the orchestrator"),
            "{err}"
        );
        // A person (the server has refused one inside a session) renews the
        // pinned orchestrator.
        assert_eq!(
            f.daemon.plan_renewal(ask(None, note)).unwrap().old.id,
            AgentId("orch".into())
        );
    }

    #[test]
    fn with_no_live_orchestrator_there_is_nothing_to_renew() {
        let f = fixture();
        let note = f.note("handover.md", b"carry on");
        let err = f
            .daemon
            .plan_renewal(ask(None, note.clone()))
            .err()
            .unwrap();
        assert!(
            err.to_string().contains("no orchestrator to renew"),
            "{err}"
        );
        let mut archived = row("orch", AgentRole::Orchestrator);
        archived.archived = true;
        f.daemon.store.insert_agent(&archived).unwrap();
        assert!(f.daemon.plan_renewal(ask(None, note.clone())).is_err());
        let err = f
            .daemon
            .plan_renewal(ask(Some("orch"), note))
            .err()
            .unwrap();
        assert!(err.to_string().contains("archived"), "{err}");
    }

    /// Outside the allowed folders, empty, too big, missing, a folder, or
    /// not UTF-8: refused, and nothing is scheduled.
    #[tokio::test]
    async fn a_bad_note_is_refused_before_anything_changes() {
        let f = fixture();
        f.daemon
            .store
            .insert_agent(&row("orch", AgentRole::Orchestrator))
            .unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        let outside = elsewhere.path().join("handover.md");
        std::fs::write(&outside, "carry on").unwrap();
        let big = vec![b'a'; NOTE_MAX_BYTES as usize + 1];
        for (note, why) in [
            (outside, "outside the orchestrator's workspace"),
            (f.note("empty.md", b""), "must be 1 to 65536"),
            (f.note("big.md", &big), "65537 bytes"),
            (f.repo.path().join("gone.md"), "is not there"),
            (f.repo.path().to_path_buf(), "is not a file"),
            (f.note("bin.md", &[0xff, 0xfe, 0x00]), "not UTF-8"),
        ] {
            let err = f
                .daemon
                .renew_orchestrator(ask(Some("orch"), note.clone()))
                .unwrap_err();
            assert!(err.contains(why), "{}: {err}", note.display());
        }
        assert!(f.daemon.renewals.lock().pending.is_none());
        assert_eq!(pinned(&f), Some(AgentId("orch".into())));
        assert!(!f.log().exists());
        // Exactly the cap, in the orchestrator's own workspace, passes.
        let home = f.daemon.orchestrator_home();
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(
            home.join("handover.md"),
            vec![b'a'; NOTE_MAX_BYTES as usize],
        )
        .unwrap();
        let plan = f
            .daemon
            .plan_renewal(ask(Some("orch"), home.join("handover.md")))
            .unwrap();
        assert_eq!(plan.text.len(), NOTE_MAX_BYTES as usize);
    }

    /// The new one takes the old one's profile, model and role card, or
    /// the profile and model asked for, and its project.
    #[test]
    fn the_role_card_profile_and_model_carry_over() {
        let f = fixture();
        let mut old = row("orch", AgentRole::Orchestrator);
        old.profile = Some("work".into());
        old.model = Some("opus".into());
        f.daemon.store.insert_agent(&old).unwrap();
        f.daemon
            .store
            .set_agent_role_card(&old.id, Some("manager"))
            .unwrap();
        let note = f.note("handover.md", b"carry on");
        let plan = f.daemon.plan_renewal(ask(None, note.clone())).unwrap();
        assert_eq!(plan.role_card.as_deref(), Some("manager"));
        assert_eq!(plan.profile.as_deref(), Some("work"));
        assert_eq!(plan.model.as_deref(), Some("opus"));
        assert_eq!(plan.project, ProjectId("p".into()));
        let plan = f
            .daemon
            .plan_renewal(RenewRequest {
                profile: Some("home".into()),
                model: Some("sonnet".into()),
                ..ask(None, note)
            })
            .unwrap();
        assert_eq!(plan.profile.as_deref(), Some("home"));
        assert_eq!(plan.model.as_deref(), Some("sonnet"));
        assert_eq!(plan.role_card.as_deref(), Some("manager"));
    }

    /// Mid-turn, the swap waits for `Finished`; the new session's first
    /// hook completes it, the old one is archived and stopped, and the
    /// log and every client hear `Done`.
    #[tokio::test]
    async fn the_swap_waits_for_the_turn_to_end_then_hands_over() {
        let f = fixture();
        let old = started(&f).await;
        f.daemon
            .store
            .set_agent_status(&old, AgentStatus::Running)
            .unwrap();
        let mut rx = f.daemon.events.subscribe();
        let note = f.note("handover.md", "what is next: ship 0.60".as_bytes());
        let state = f
            .daemon
            .renew_orchestrator(ask(Some(old.as_str()), note.clone()))
            .unwrap();
        assert_eq!(state, RenewState::Scheduled);
        // A second one while it waits is refused.
        let err = f
            .daemon
            .renew_orchestrator(ask(Some(old.as_str()), note.clone()))
            .unwrap_err();
        assert!(err.contains("already scheduled"), "{err}");
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(pinned(&f), Some(old.clone()), "swapped mid-turn");
        f.daemon
            .store
            .set_agent_status(&old, AgentStatus::Finished)
            .unwrap();
        let new = new_orchestrator(&f, &old).await;
        let row = f.daemon.store.get_agent(&new).unwrap().unwrap();
        // No hook yet: only one would save the CLI's session id.
        assert_eq!(row.session_id, None);
        assert_eq!(row.name, "orch");
        assert!(f.daemon.is_alive(&SessionRef::Agent(old.clone())));
        report(&f, &new);
        let (o, n, state) = outcome(&mut rx).await;
        assert_eq!(
            (o, n.as_ref(), state),
            (old.clone(), Some(&new), RenewState::Done)
        );
        let was = f.daemon.store.get_agent(&old).unwrap().unwrap();
        assert!(was.archived);
        assert_eq!(was.role, AgentRole::Worker);
        assert!(!f.daemon.is_alive(&SessionRef::Agent(old.clone())));
        assert_eq!(pinned(&f), Some(new.clone()));
        assert!(f.daemon.renewals.lock().pending.is_none());
        let lines = log_lines(&f);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0]["old"], old.as_str());
        assert_eq!(lines[0]["new"], new.as_str());
        assert_eq!(lines[0]["state"], "done");
        assert!(lines[0]["reason"].is_null());
        assert!(lines[0]["at"].as_i64().unwrap() > 0);
        f.daemon.kill_session(&SessionRef::Agent(new));
    }

    #[tokio::test]
    async fn a_turn_that_never_ends_gives_up_and_changes_nothing() {
        let f = fixture();
        f.daemon
            .set_renew_waits(Duration::from_millis(200), Duration::from_secs(5), f.log());
        let mut old = row("orch", AgentRole::Orchestrator);
        old.status = AgentStatus::Running;
        f.daemon.store.insert_agent(&old).unwrap();
        let mut rx = f.daemon.events.subscribe();
        let note = f.note("handover.md", b"carry on");
        f.daemon
            .renew_orchestrator(ask(Some("orch"), note))
            .unwrap();
        let (_, new, state) = outcome(&mut rx).await;
        assert_eq!(new, None);
        let RenewState::Failed { reason } = state else {
            panic!("{state:?}");
        };
        assert!(reason.contains("did not end within"), "{reason}");
        assert_eq!(pinned(&f), Some(old.id));
        assert_eq!(f.daemon.store.load_tree().unwrap().2.len(), 1);
        assert_eq!(log_lines(&f)[0]["state"], "failed");
    }

    /// A new session that never reports is stopped and archived, and the
    /// old one holds the role again without being typed the guidance.
    #[tokio::test]
    async fn a_new_session_that_never_reports_is_rolled_back() {
        let f = fixture();
        f.daemon
            .set_renew_waits(Duration::from_secs(5), Duration::from_millis(400), f.log());
        let old = started(&f).await;
        // At its prompt and alive: what `Daemon::set_orchestrator` would
        // type the guidance into.
        f.daemon
            .store
            .set_agent_status(&old, AgentStatus::Finished)
            .unwrap();
        let mut rx = f.daemon.events.subscribe();
        let note = f.note("handover.md", b"carry on");
        f.daemon.renew_orchestrator(ask(None, note)).unwrap();
        let new = new_orchestrator(&f, &old).await;
        let (o, n, state) = outcome(&mut rx).await;
        assert_eq!((o, n), (old.clone(), Some(new.clone())));
        let RenewState::Failed { reason } = state else {
            panic!("{state:?}");
        };
        assert!(reason.contains("sent no hook within"), "{reason}");
        assert!(
            reason.contains("orch is still the orchestrator"),
            "{reason}"
        );
        assert_eq!(pinned(&f), Some(old.clone()));
        assert_eq!(orchestrator_rows(&f), 1);
        let failed = f.daemon.store.get_agent(&new).unwrap().unwrap();
        assert!(failed.archived);
        assert_eq!(failed.role, AgentRole::Worker);
        assert!(!f.daemon.is_alive(&SessionRef::Agent(new)));
        let kept = f.daemon.store.get_agent(&old).unwrap().unwrap();
        assert!(!kept.archived);
        let session = f.daemon.session(&SessionRef::Agent(old.clone())).unwrap();
        let screen = String::from_utf8_lossy(&session.snapshot(None).1).into_owned();
        assert!(
            !screen.contains("The user just made this session the orchestrator"),
            "{screen}"
        );
        f.daemon.kill_session(&SessionRef::Agent(old));
    }

    #[tokio::test]
    async fn keep_old_leaves_the_old_session_running_as_a_worker() {
        let f = fixture();
        let old = started(&f).await;
        let mut rx = f.daemon.events.subscribe();
        let note = f.note("handover.md", b"carry on");
        f.daemon
            .renew_orchestrator(RenewRequest {
                keep_old: true,
                now: true,
                ..ask(Some(old.as_str()), note)
            })
            .unwrap();
        let new = new_orchestrator(&f, &old).await;
        report(&f, &new);
        let (_, _, state) = outcome(&mut rx).await;
        assert_eq!(state, RenewState::Done);
        let kept = f.daemon.store.get_agent(&old).unwrap().unwrap();
        assert!(!kept.archived);
        assert_eq!(kept.role, AgentRole::Worker);
        assert!(f.daemon.is_alive(&SessionRef::Agent(old.clone())));
        f.daemon.kill_session(&SessionRef::Agent(old));
        f.daemon.kill_session(&SessionRef::Agent(new));
    }

    /// Windows caps a command line at 32,767 characters and the note rides
    /// the new CLI's argv: a note within the 65,536-byte cap that does not
    /// fit fails the start, and the old orchestrator keeps the role.
    #[cfg(windows)]
    #[tokio::test]
    async fn a_note_too_long_for_the_command_line_leaves_the_old_one_in_charge() {
        let f = fixture();
        let old = started(&f).await;
        let mut rx = f.daemon.events.subscribe();
        let note = f.note("handover.md", &vec![b'a'; 40_000]);
        f.daemon
            .renew_orchestrator(RenewRequest {
                now: true,
                ..ask(None, note)
            })
            .unwrap();
        let (_, new, state) = outcome(&mut rx).await;
        let RenewState::Failed { reason } = state else {
            panic!("{state:?}");
        };
        assert_eq!(new, None, "{reason}");
        assert!(reason.contains("start the new orchestrator"), "{reason}");
        assert_eq!(pinned(&f), Some(old.clone()));
        assert_eq!(orchestrator_rows(&f), 1);
        assert!(f.daemon.is_alive(&SessionRef::Agent(old.clone())));
        f.daemon.kill_session(&SessionRef::Agent(old));
    }

    #[test]
    fn the_starting_prompt_is_the_spec_text_then_the_note() {
        let prompt = starting_prompt("orch", Path::new("handover.md"), "one\ntwo", 0);
        let local = i64::from(nebula_core::usage_window::local(0));
        let time = nebula_core::wake::hhmm((local.rem_euclid(86_400) / 60) as u16);
        assert_eq!(
            prompt,
            format!(
                "[from kete renew] You are the orchestrator, renewed from orch at {time}. Your \
                 handover note is handover.md; it is quoted below. Read it, then carry on.\n\n\
                 one\ntwo"
            )
        );
    }
}
