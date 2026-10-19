//! WHAKAOHO's hidden session (`ClientRequest::WakeAccount`): one tiny
//! prompt to the cheapest model under an account, so its five-hour window
//! starts at the hour the user chose (`nebula_core::wake`).
//!
//! It is a real interactive Claude session — plainly subscription use,
//! and Kete's status line reads the true reset time off it — but it is no
//! one's session: it has no row and never enters `sessions`, so no client
//! lists, attaches to or reaps it, the metrics never count it, and an
//! IN-PLACE RESTART never carries it (its master closes on exec and the
//! CLI goes with it). Its hooks and status line come in by its own agent
//! id and are taken here before anything else sees them. It runs in
//! Kete's empty wake folder with the slimmest launch — no guidance, no
//! scope, no port block — and is killed as soon as its turn ends, or
//! after [`WAKE_TIMEOUT`].

use super::*;
use nebula_core::usage::{self, AccountUsage, UsageSource, WindowKind};
use nebula_core::wake::WakeReport;
use tokio::sync::mpsc;

/// The longest a heartbeat may take, boot to kill.
pub(crate) const WAKE_TIMEOUT: Duration = Duration::from_secs(120);
/// After the turn ends, how long to wait for the status line's reading
/// (Claude runs it after the reply lands, a moment after `Stop`).
const READING_GRACE: Duration = Duration::from_secs(8);

/// What a wake session's hooks and status line report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WakeSignal {
    Submitted,
    Busy,
    Idle,
    TurnEnded,
    Reading,
}

/// A live wake session, by its agent id (`Daemon::wakes`).
pub(crate) struct WakeSession {
    account: String,
    harness: String,
    tx: mpsc::UnboundedSender<WakeSignal>,
}

/// The five-hour window's reset in `reading`, if it has one.
fn session_reset(reading: Option<&AccountUsage>) -> Option<i64> {
    reading
        .and_then(|u| u.window(WindowKind::Session))
        .filter(|w| !w.estimated)
        .and_then(|w| w.resets_at)
        .filter(|&at| at > 0)
}

impl Daemon {
    /// A hook for `id`: when it is a wake session's, note it and say so,
    /// and the caller does nothing else with it.
    pub(crate) fn wake_hook(&self, id: &AgentId, event: &HookEvent) -> bool {
        let wakes = self.wakes.lock();
        let Some(wake) = wakes.get(id) else {
            return false;
        };
        let signal = match event {
            HookEvent::UserPromptSubmit => Some(WakeSignal::Submitted),
            HookEvent::Stop { .. } => Some(WakeSignal::TurnEnded),
            // An API error ends the turn too (a usage limit, a login): the
            // report then carries no fresh reading, and the log says so.
            HookEvent::StopFailure { .. } => Some(WakeSignal::TurnEnded),
            HookEvent::Notification { notification_type }
                if notification_type.as_deref() == Some("idle_prompt") =>
            {
                Some(WakeSignal::TurnEnded)
            }
            HookEvent::Progress { busy: true } => Some(WakeSignal::Busy),
            HookEvent::Progress { busy: false } => Some(WakeSignal::Idle),
            _ => None,
        };
        if let Some(signal) = signal {
            let _ = wake.tx.send(signal);
        }
        true
    }

    /// A status line reading for `id`: a wake session's goes to its
    /// account, as any session's would. Says whether it was one.
    pub(crate) fn wake_statusline(&self, id: &AgentId, body: &serde_json::Value) -> bool {
        let Some((account, harness, tx)) = self
            .wakes
            .lock()
            .get(id)
            .map(|w| (w.account.clone(), w.harness.clone(), w.tx.clone()))
        else {
            return false;
        };
        let reading = usage::claude_statusline(body);
        if !reading.windows.is_empty() {
            self.record_account(AccountUsage {
                account,
                harness,
                windows: reading.windows,
                source: UsageSource::Statusline,
                at: nebula_core::clock::now_ms(),
                plan: None,
            });
            let _ = tx.send(WakeSignal::Reading);
        }
        true
    }

    /// The account's kept reading, from the daemon's book.
    fn account_reading(&self, key: &str) -> Option<AccountUsage> {
        self.account_usage_list()
            .into_iter()
            .find(|u| u.account == key)
    }

    /// Run one heartbeat under `account` and report how it went. Never
    /// fails: what went wrong is in the report.
    pub async fn wake_account(self: &Arc<Self>, account: String) -> WakeReport {
        let key = account.trim().to_string();
        let before = session_reset(self.account_reading(&key).as_ref());
        let mut report = WakeReport {
            resets_before: before,
            resets_after: before,
            error: None,
            reason: "fired".into(),
            submitted: false,
            finished: false,
            read: false,
        };
        let fail = |mut report: WakeReport, reason: &str, error: String| {
            report.reason = reason.into();
            report.error = Some(error);
            report
        };
        let (id, session, mut rx) = match self.spawn_wake(&key).await {
            Ok(spawned) => spawned,
            Err((reason, e)) => return fail(report, reason, e),
        };
        let mut events = session.events.subscribe();
        let deadline = tokio::time::Instant::now() + WAKE_TIMEOUT;
        let mut busy_since_submit = false;
        let mut exited = None;
        loop {
            let tick = tokio::select! {
                _ = tokio::time::sleep_until(deadline) => None,
                signal = rx.recv() => signal.map(Ok),
                event = events.recv() => match event {
                    Ok(PtyEvent::Progress { busy }) => Some(Ok(if busy {
                        WakeSignal::Busy
                    } else {
                        WakeSignal::Idle
                    })),
                    Ok(PtyEvent::Exited { exit_code }) => Some(Err(exit_code)),
                    Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => Some(Err(None)),
                },
            };
            match tick {
                None => break,
                Some(Err(code)) => {
                    exited = Some(code);
                    break;
                }
                Some(Ok(WakeSignal::Submitted)) => report.submitted = true,
                Some(Ok(WakeSignal::Busy)) => busy_since_submit |= report.submitted,
                Some(Ok(WakeSignal::Reading)) => report.read = true,
                // An idle edge ends the turn only once the prompt is in and
                // the CLI has been busy with it: the boot's own edges and
                // a cancelled turn's are no reply.
                Some(Ok(WakeSignal::Idle)) if !busy_since_submit => {}
                Some(Ok(WakeSignal::Idle | WakeSignal::TurnEnded)) => {
                    report.finished = true;
                    break;
                }
            }
        }
        // The reply has landed; the status line runs a moment after it.
        if report.finished && !report.read {
            let grace = tokio::time::Instant::now() + READING_GRACE;
            while let Ok(Some(signal)) = tokio::time::timeout_at(grace, rx.recv()).await {
                if signal == WakeSignal::Reading {
                    report.read = true;
                    break;
                }
            }
        }
        self.end_wake(&id, &session);
        report.resets_after = session_reset(self.account_reading(&key).as_ref());
        if report.finished {
            return report;
        }
        let dir = nebula_core::wake::wake_dir();
        match (exited, report.submitted) {
            (Some(code), _) => fail(
                report,
                "exited",
                format!(
                    "Claude exited before the reply (code {}): run `claude` once in {} under this account to see why",
                    code.map_or("none".into(), |c| c.to_string()),
                    dir.display()
                ),
            ),
            (None, false) => fail(
                report,
                "no_prompt",
                format!(
                    "the prompt never went in within {}s: a login or trust prompt may be waiting; run `claude` once in {} under this account",
                    WAKE_TIMEOUT.as_secs(),
                    dir.display()
                ),
            ),
            (None, true) => fail(
                report,
                "timeout",
                format!("no reply within {}s", WAKE_TIMEOUT.as_secs()),
            ),
        }
    }

    /// Boot the hidden session for `key`: its id, its PTY, and where its
    /// hooks and readings arrive. Err is a log reason and a line.
    async fn spawn_wake(
        self: &Arc<Self>,
        key: &str,
    ) -> std::result::Result<
        (
            AgentId,
            Arc<PtySession>,
            mpsc::UnboundedReceiver<WakeSignal>,
        ),
        (&'static str, String),
    > {
        let harness =
            resolve_harness(AgentKind::Claude, None).map_err(|e| ("spawn", e.to_string()))?;
        let profile = match usage::default_login_harness(key) {
            Some("claude") => None,
            Some(other) => {
                return Err((
                    "account",
                    format!("wake-up runs Claude accounts only, not {other}"),
                ))
            }
            None => {
                let config = crate::config::Config::load();
                match nebula_core::profiles::resolve(&config.profiles, &harness.id, Some(key)) {
                    Ok(profile) => profile.cloned(),
                    Err(e) => return Err(("account", e)),
                }
            }
        };
        if self.wakes.lock().values().any(|w| w.account == key) {
            return Err(("busy", format!("a wake-up for {key} is already running")));
        }
        let dir = nebula_core::wake::wake_dir();
        std::fs::create_dir_all(&dir).map_err(|e| ("spawn", format!("{}: {e}", dir.display())))?;
        let launch_env = match &profile {
            Some(profile) => {
                if let Err(e) = profile.prepare(Some(&dir)) {
                    tracing::warn!(profile = %profile.name, error = %e, "profile home setup failed");
                }
                profile.launch_env()
            }
            None => Default::default(),
        };
        // No one is there to answer the trust dialog: trust Kete's own
        // empty folder for this account, as answering it once would.
        if let Some(file) = nebula_core::profiles::claude_json_for(
            profile.as_ref(),
            &nebula_core::profiles::profiles_root(),
        ) {
            if let Err(e) = nebula_core::profiles::trust_folder(&file, &dir) {
                tracing::warn!(file = %file.display(), error = %e, "trusting the wake folder failed");
            }
        }
        // The turn's end comes in on the Stop hook, as any session's.
        if let Err(e) = hooks::installer::install_claude_hooks(&dir)
            .and_then(|()| hooks::installer::strip_kete_statusline(&dir))
        {
            tracing::warn!(error = %e, "wake folder hook install failed");
        }
        let cmd_override = std::env::var(env::AGENT_CMD).ok();
        let (program, mut args, _) = agent_spawn_command_with(
            &harness,
            None,
            Some(&dir),
            Some(nebula_core::wake::MODEL),
            None,
            cmd_override.as_deref(),
            Some(nebula_core::wake::PROMPT),
            None,
            false,
        );
        if cmd_override.is_none() {
            if let Some(path) = super::spawn::statusline_settings() {
                super::spawn::insert_settings_flag(&mut args, &path);
            }
        }
        let (program, args) = if cmd_override.is_some() {
            (program, args)
        } else {
            login_shell_wrap_env(
                &nebula_core::shell::user_shell(),
                &program,
                &args,
                &launch_env,
            )
        };
        let id = AgentId::generate();
        let mut session_env = vec![
            (env::AGENT_ID.into(), id.to_string()),
            (
                env::API_URL.into(),
                format!("http://127.0.0.1:{}", self.hook_env.port),
            ),
            (env::API_TOKEN.into(), self.hook_env.token.clone()),
        ];
        session_env.extend(launch_env.set.iter().cloned());
        let spec = SpawnSpec {
            program,
            args,
            cwd: dir,
            env: session_env,
            scrub_env: env::AGENT_SESSION_VARS,
            unset_env: launch_env.removals(),
            cols: DEFAULT_COLS,
            rows: DEFAULT_ROWS,
        };
        // Listening before the CLI can fire its first hook.
        let (tx, rx) = mpsc::unbounded_channel();
        self.wakes.lock().insert(
            id.clone(),
            WakeSession {
                account: key.to_string(),
                harness: harness.id.clone(),
                tx,
            },
        );
        let sref = SessionRef::Agent(id.clone());
        let spawned = tokio::task::spawn_blocking(move || PtySession::spawn(sref, spec)).await;
        match spawned {
            Ok(Ok(session)) => {
                tracing::info!(agent = %id, account = %key, "wake-up session started");
                Ok((id, session, rx))
            }
            Ok(Err(e)) => {
                self.wakes.lock().remove(&id);
                Err(("spawn", format!("{e:#}")))
            }
            Err(e) => {
                self.wakes.lock().remove(&id);
                Err(("spawn", e.to_string()))
            }
        }
    }

    /// Kill a wake session and forget everything kept by its id.
    fn end_wake(&self, id: &AgentId, session: &Arc<PtySession>) {
        session.kill();
        self.wakes.lock().remove(id);
        self.forget_context(id);
        self.transcripts.lock().remove(id);
        self.close_subagents(id);
        tracing::info!(agent = %id, "wake-up session ended");
    }

    /// Every account's turns over the last two weeks, by local quarter
    /// hour (`ClientRequest::AccountActivity`).
    pub fn account_activity(&self) -> Result<Vec<nebula_core::wake::AccountActivity>> {
        use nebula_core::wake::activity;
        let now = nebula_core::clock::now_secs() as i64;
        let offset = nebula_core::clock::local_offset_secs();
        let since_ms = (now - activity::DAYS as i64 * 86_400) * 1000;
        let mut starts: HashMap<String, Vec<i64>> = HashMap::new();
        for moment in self.store.activity_since(since_ms)? {
            starts
                .entry(moment.account())
                .or_default()
                .push(moment.at / 1000);
        }
        let mut last: HashMap<String, i64> = HashMap::new();
        for moment in self.store.last_activity()? {
            let at = last.entry(moment.account()).or_insert(0);
            *at = (*at).max(moment.at / 1000);
        }
        let mut accounts: Vec<String> = starts.keys().chain(last.keys()).cloned().collect();
        accounts.sort();
        accounts.dedup();
        Ok(accounts
            .into_iter()
            .map(|account| nebula_core::wake::AccountActivity {
                bins: activity::histogram(
                    starts.get(&account).into_iter().flatten().copied(),
                    now,
                    offset,
                    activity::DAYS,
                ),
                last_turn_at: last.get(&account).copied().filter(|&at| at > 0),
                account,
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nebula_core::session_snapshot::SnapshotPoint;
    use nebula_core::{Project, ProjectId, WorktreeId};

    fn daemon() -> Arc<Daemon> {
        let store = Arc::new(Store::open_in_memory().unwrap());
        Daemon::new(
            store,
            HookEnv {
                port: 0,
                token: String::new(),
            },
        )
    }

    fn watch(daemon: &Daemon, id: &str, account: &str) -> mpsc::UnboundedReceiver<WakeSignal> {
        let (tx, rx) = mpsc::unbounded_channel();
        daemon.wakes.lock().insert(
            AgentId(id.into()),
            WakeSession {
                account: account.into(),
                harness: "claude".into(),
                tx,
            },
        );
        rx
    }

    #[test]
    fn a_wake_session_takes_its_own_hooks_and_no_one_else_s() {
        let daemon = daemon();
        let mut rx = watch(&daemon, "wake-1", "work");
        let id = AgentId("wake-1".into());
        assert!(daemon.wake_hook(&id, &HookEvent::UserPromptSubmit));
        assert!(daemon.wake_hook(&id, &HookEvent::SessionStart { source: None }));
        assert!(daemon.wake_hook(&id, &HookEvent::Stop { background: None }));
        assert_eq!(rx.try_recv(), Ok(WakeSignal::Submitted));
        assert_eq!(rx.try_recv(), Ok(WakeSignal::TurnEnded));
        assert!(rx.try_recv().is_err(), "SessionStart says nothing");
        assert!(!daemon.wake_hook(
            &AgentId("row-1".into()),
            &HookEvent::Stop { background: None }
        ));
    }

    #[test]
    fn a_wake_session_s_status_line_is_its_account_s_reading() {
        let daemon = daemon();
        let dir = tempfile::tempdir().unwrap();
        daemon.set_usage_root(dir.path().to_path_buf());
        let mut rx = watch(&daemon, "wake-1", "work");
        let body = serde_json::json!({
            "rate_limits": {"five_hour": {"used_percentage": 1, "resets_at": 1_800_000_000}}
        });
        assert!(daemon.wake_statusline(&AgentId("wake-1".into()), &body));
        assert_eq!(rx.try_recv(), Ok(WakeSignal::Reading));
        assert_eq!(
            session_reset(daemon.account_reading("work").as_ref()),
            Some(1_800_000_000)
        );
        assert!(
            nebula_core::usage::read_account_in(dir.path(), "work").is_some(),
            "kept on disk where the CLI reads it"
        );
        // A row-less id that isn't a wake session is no one's.
        assert!(!daemon.wake_statusline(&AgentId("stranger".into()), &body));
        // The hook path drops it too (no row): nothing recorded for it.
        daemon.on_statusline(&AgentId("stranger".into()), &body);
        assert_eq!(daemon.account_usage_list().len(), 1);
    }

    #[test]
    fn activity_counts_each_account_s_turns_by_local_quarter_hour() {
        let daemon = daemon();
        daemon
            .store
            .insert_project(&Project {
                id: ProjectId("p".into()),
                name: "p".into(),
                repo_path: "/tmp/p".into(),
                sort_order: 0,
            })
            .unwrap();
        daemon
            .store
            .insert_worktree(&Worktree {
                id: WorktreeId("w".into()),
                project_id: ProjectId("p".into()),
                path: "/tmp/p".into(),
                branch: "main".into(),
                is_main: true,
                sort_order: 0,
            })
            .unwrap();
        let agent = |id: &str, profile: Option<&str>| Agent {
            parent: None,
            profile: profile.map(str::to_string),
            id: AgentId(id.into()),
            worktree_id: WorktreeId("w".into()),
            name: id.into(),
            status: AgentStatus::Finished,
            archived: false,
            archived_at: 0,
            unseen: false,
            kind: AgentKind::Claude,
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
        daemon
            .store
            .insert_agent(&agent("a", Some("work")))
            .unwrap();
        daemon.store.insert_agent(&agent("b", None)).unwrap();
        let now_ms = nebula_core::clock::now_ms();
        let turn = |id: &str, ago_ms: i64| {
            daemon
                .store
                .begin_turn(
                    &AgentId(id.into()),
                    &SnapshotPoint {
                        head: None,
                        tree: None,
                        at: now_ms - ago_ms,
                    },
                )
                .unwrap();
        };
        turn("a", 3_600_000);
        turn("a", 2 * 3_600_000);
        turn("a", 20 * 86_400_000); // past the lookback
        turn("b", 60_000);
        let list = daemon.account_activity().unwrap();
        let by = |key: &str| list.iter().find(|a| a.account == key).unwrap();
        let work = by("work");
        assert_eq!(work.bins.len(), nebula_core::wake::activity::BINS);
        assert_eq!(
            work.bins.iter().sum::<u32>(),
            3,
            "two turns and the session's start"
        );
        assert!(work.last_turn_at.unwrap() >= now_ms / 1000 - 5);
        assert_eq!(by("@claude").bins.iter().sum::<u32>(), 2);
        assert_eq!(list.len(), 2);
    }
}
