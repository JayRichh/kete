//! ACCOUNT USAGE and SESSION CONTEXT, as the daemon gathers them
//! (`nebula_core::usage`): Claude's statusLine readings (`kete
//! _statusline` POSTs them), Codex rollouts and Copilot event logs tailed
//! on turn hooks, Copilot's quota from `gh`, and Claude's on-screen usage
//! notice. Readings are kept per ACCOUNT (written to the profiles root,
//! where `kete profile list` and the apps also read them) and per session,
//! and pushed to 55+ subscribers as `AccountUsage` / `SessionContexts`
//! only when they change. Nothing here can fail a launch: every source
//! that is missing or unreadable just leaves its numbers unknown.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use nebula_core::usage::{
    self, AccountUsage, ContextUsage, SessionContext, TranscriptScan, UsageSource,
};
use nebula_core::{Agent, AgentId, AgentKind, ServerEvent};
use serde_json::Value;

use super::Daemon;

/// A statusLine reading this recent makes the transcript fallback moot.
const STATUSLINE_FRESH_MS: i64 = 5 * 60 * 1000;
/// A reading whose windows did not change is still re-sent (its "updated"
/// time moves) at most this often.
const RESEND_MS: i64 = 60 * 1000;
/// How often an account's Copilot quota is asked of `gh`.
const COPILOT_POLL: Duration = Duration::from_secs(15 * 60);
/// How long `gh api` may take.
const GH_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Default)]
pub(crate) struct UsageBook {
    /// Where readings are kept when not the profiles root: tests point it
    /// at a temp dir.
    root: Option<PathBuf>,
    loaded: bool,
    accounts: HashMap<String, AccountUsage>,
    contexts: HashMap<AgentId, ContextUsage>,
    scans: HashMap<AgentId, TranscriptScan>,
    statusline_at: HashMap<AgentId, i64>,
    copilot_polled: HashMap<String, Instant>,
    /// The effort each Claude session last reported (`adopt_claude_effort`).
    effort_seen: HashMap<AgentId, String>,
}

/// The same windows with the same numbers, read times aside: a new read
/// time alone is no news (it rides the next push, a minute on at most).
fn same_values(a: &[usage::UsageWindow], b: &[usage::UsageWindow]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(x, y)| {
            x.kind == y.kind
                && x.label == y.label
                && x.percent == y.percent
                && x.resets_at == y.resets_at
        })
}

/// Did a context reading change enough to send?
fn context_changed(old: Option<&ContextUsage>, new: &ContextUsage) -> bool {
    let Some(old) = old else {
        return true;
    };
    let moved = match (old.percent, new.percent) {
        (Some(a), Some(b)) => (a - b).abs() >= 0.5,
        (a, b) => a.is_some() != b.is_some(),
    };
    moved || old.compactions != new.compactions || old.window != new.window
}

/// The harness id an agent's account belongs to.
pub(crate) fn harness_of(agent: &Agent) -> String {
    match (&agent.kind, agent.custom_harness.as_deref()) {
        (AgentKind::Custom, Some(id)) => id.to_string(),
        (kind, _) => kind.as_str().to_string(),
    }
}

impl Daemon {
    /// Keep readings under `dir` instead of the profiles root (tests).
    #[cfg(test)]
    pub(crate) fn set_usage_root(&self, dir: PathBuf) {
        let mut book = self.usage.lock();
        *book = UsageBook {
            root: Some(dir),
            ..UsageBook::default()
        };
    }

    fn usage_root(&self) -> PathBuf {
        self.usage
            .lock()
            .root
            .clone()
            .unwrap_or_else(nebula_core::profiles::profiles_root)
    }

    fn with_usage<T>(&self, f: impl FnOnce(&mut UsageBook) -> T) -> T {
        let mut book = self.usage.lock();
        if !book.loaded {
            book.loaded = true;
            let root = book
                .root
                .clone()
                .unwrap_or_else(nebula_core::profiles::profiles_root);
            for u in usage::read_all_in(&root) {
                book.accounts.insert(u.account.clone(), u);
            }
        }
        f(&mut book)
    }

    /// Every account's latest reading, for a subscriber's first look.
    pub fn account_usage_list(&self) -> Vec<AccountUsage> {
        self.with_usage(|book| {
            let mut list: Vec<AccountUsage> = book.accounts.values().cloned().collect();
            list.sort_by(|a, b| a.account.cmp(&b.account));
            list
        })
    }

    /// Every session's latest context.
    pub fn session_contexts(&self) -> Vec<SessionContext> {
        self.with_usage(|book| {
            book.contexts
                .iter()
                .map(|(agent, context)| SessionContext {
                    agent: agent.clone(),
                    context: context.clone(),
                })
                .collect()
        })
    }

    /// Fold a reading into its account; kept on disk and pushed when the
    /// windows changed (or the last push is a minute old).
    pub(crate) fn record_account(&self, mut reading: AccountUsage) {
        // Each window remembers when it was read: one with no reset time
        // ages out from there (`UsageWindow::expired_at`).
        reading.stamp(reading.at / 1000);
        let changed = self.with_usage(|book| {
            let merged = match book.accounts.get(&reading.account) {
                Some(old) => {
                    let mut next = old.clone();
                    next.merge(reading.clone());
                    let moved =
                        !same_values(&next.windows, &old.windows) || next.at - old.at >= RESEND_MS;
                    if !moved {
                        return None;
                    }
                    next
                }
                None => reading,
            };
            book.accounts.insert(merged.account.clone(), merged.clone());
            Some(merged)
        });
        let Some(merged) = changed else {
            return;
        };
        if let Err(e) = usage::write_account_in(&self.usage_root(), &merged) {
            tracing::warn!(account = %merged.account, error = %e, "keeping account usage failed");
        }
        self.broadcast(ServerEvent::AccountUsage {
            accounts: self.account_usage_list(),
        });
    }

    /// Keep a session's context, pushed when it moved.
    pub(crate) fn record_context(&self, agent: &AgentId, context: ContextUsage) {
        let changed = self.with_usage(|book| {
            let moved = context_changed(book.contexts.get(agent), &context);
            book.contexts.insert(agent.clone(), context.clone());
            moved
        });
        if changed {
            self.broadcast(ServerEvent::SessionContexts {
                contexts: vec![SessionContext {
                    agent: agent.clone(),
                    context,
                }],
            });
        }
    }

    /// Forget a deleted session's context.
    pub(crate) fn forget_context(&self, agent: &AgentId) {
        self.with_usage(|book| {
            book.contexts.remove(agent);
            book.scans.remove(agent);
            book.statusline_at.remove(agent);
            book.effort_seen.remove(agent);
        });
    }

    /// When the limit `agent`'s account most likely stopped on resets
    /// (epoch seconds): of its plan windows still running, the fullest —
    /// one at or past its limit first — and of equals the later reset.
    /// None with no running window: an API-billed account has none, and
    /// neither has one nothing has read yet. A stop note's fallback when
    /// the session's screen gave no time (`registry::stops`).
    pub(crate) fn account_limit_reset(&self, agent: &AgentId) -> Option<i64> {
        let agent = self.store.get_agent(agent).ok().flatten()?;
        let key = usage::account_key(agent.profile.as_deref(), &harness_of(&agent));
        let now = nebula_core::clock::now_secs() as i64;
        self.with_usage(|book| {
            book.accounts
                .get(&key)?
                .windows
                .iter()
                .filter(|w| w.kind != usage::WindowKind::Monthly)
                // A guessed reset never drives a stop note.
                .filter(|w| !w.estimated)
                .filter_map(|w| Some((w.percent, w.resets_at.filter(|at| *at > now)?)))
                .max_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)))
                .map(|(_, at)| at)
        })
    }

    /// EFFORT SYNC: Claude runs session `id` at `level` now (the
    /// statusLine's `effort.level`, or a foreground hook payload's), which
    /// a mid-session `/effort` or the model's own cap can make differ from
    /// the `--effort` it was launched with. Stored and broadcast as the
    /// model is (`session_model`), so the card says what it runs at and
    /// the next spawn or resume passes it on. Built-in Claude rows only;
    /// a level outside Claude's list is not one `--effort` takes. An absent
    /// level is never reported here, so nothing ever clears the row's.
    ///
    /// A row launched on the default (no effort) is not pinned to the
    /// level that default runs at: only a later change from the first
    /// level the session reported is the user's choice. Returns whether
    /// the row changed.
    pub fn adopt_claude_effort(&self, id: &AgentId, level: &str) -> bool {
        // Every tool call reports it: a repeat costs no store read.
        let previous =
            self.with_usage(|book| book.effort_seen.insert(id.clone(), level.to_string()));
        if previous.as_deref() == Some(level) {
            return false;
        }
        let Ok(Some(agent)) = self.store.get_agent(id) else {
            return false;
        };
        if agent.kind != AgentKind::Claude || agent.cloud_session_id.is_some() {
            return false;
        }
        let known = nebula_core::harness::builtin("claude")
            .is_some_and(|claude| claude.effort.efforts.iter().any(|e| e == level));
        if !known {
            return false;
        }
        let adopt = match agent
            .effort
            .as_deref()
            .map(str::trim)
            .filter(|e| !e.is_empty())
        {
            Some(current) => current != level,
            None => previous.is_some(),
        };
        if !adopt {
            return false;
        }
        if let Err(e) = self.store.set_agent_effort(id, Some(level)) {
            tracing::warn!(agent = %id, error = %e, "effort change not adopted");
            return false;
        }
        tracing::info!(agent = %id, effort = %level, "adopted the effort Claude runs at");
        self.try_broadcast_agent(id);
        true
    }

    /// Feed `agent`'s log scan at `path` (a new scan when the path moved)
    /// and hand back a copy.
    fn scan_log(&self, agent: &AgentId, path: PathBuf) -> Option<TranscriptScan> {
        let mut scan = self.with_usage(|book| match book.scans.get(agent) {
            Some(scan) if scan.path == path => scan.clone(),
            _ => TranscriptScan::new(path),
        });
        // File reading outside the lock.
        if !scan.feed() {
            return None;
        }
        self.with_usage(|book| book.scans.insert(agent.clone(), scan.clone()));
        Some(scan)
    }

    /// A Claude statusLine JSON for `agent`, from `kete _statusline`.
    /// The route takes the wrapper's `agentId` on trust (it is display
    /// data behind the hook token), so a reading only lands for a Claude
    /// session the daemon holds and has not archived.
    pub fn on_statusline(&self, agent_id: &AgentId, body: &Value) {
        // A WHAKAOHO session has no row; its reading is its account's.
        if self.wake_statusline(agent_id, body) {
            return;
        }
        let Ok(Some(agent)) = self.store.get_agent(agent_id) else {
            return;
        };
        if agent.archived || agent.kind != AgentKind::Claude {
            return;
        }
        let now = nebula_core::clock::now_ms();
        let reading = usage::claude_statusline(body);
        // The USAGE LEDGER keeps the latest cost estimate per session
        // (`ledger.rs`), from the parsed reading and the raw JSON.
        self.ledger_on_statusline(agent_id, &reading, body);
        if let Some(level) = &reading.effort {
            self.adopt_claude_effort(agent_id, level);
        }
        let harness = harness_of(&agent);
        let key = usage::account_key(agent.profile.as_deref(), &harness);
        if !reading.windows.is_empty() {
            self.record_account(AccountUsage {
                account: key,
                harness,
                windows: reading.windows.clone(),
                source: UsageSource::Statusline,
                at: now,
                plan: None,
            });
        }
        let scan = reading
            .transcript_path
            .as_deref()
            .filter(|p| !p.is_empty())
            .and_then(|p| self.scan_log(agent_id, PathBuf::from(p)));
        self.with_usage(|book| book.statusline_at.insert(agent_id.clone(), now));
        if reading.context_percent.is_none() && reading.context_tokens.is_none() {
            return;
        }
        self.record_context(
            agent_id,
            ContextUsage {
                percent: reading.context_percent,
                tokens: reading.context_tokens,
                window: reading.context_window,
                compactions: scan.as_ref().map_or(0, |s| s.compactions),
                last_compact_at: scan.as_ref().and_then(|s| s.last_compact_at),
                source: UsageSource::Statusline,
                at: now,
            },
        );
    }

    /// A turn hook landed for `agent`: read what its session log added —
    /// a Codex rollout's limits and context, a Copilot session's context,
    /// a Claude transcript's when no status line is reporting.
    pub fn refresh_log_usage(&self, agent_id: &AgentId) {
        let Ok(Some(agent)) = self.store.get_agent(agent_id) else {
            return;
        };
        let now = nebula_core::clock::now_ms();
        let profile = agent.profile.as_deref().and_then(|name| {
            crate::config::Config::load()
                .profiles
                .into_iter()
                .find(|p| p.name.trim() == name)
        });
        let root = nebula_core::profiles::profiles_root();
        match agent.kind {
            AgentKind::Claude => {
                let fresh = self.with_usage(|book| {
                    book.statusline_at
                        .get(agent_id)
                        .is_some_and(|at| now - at < STATUSLINE_FRESH_MS)
                });
                if fresh {
                    return;
                }
                let Some(transcript) = self.transcripts.lock().get(agent_id).cloned() else {
                    return;
                };
                if let Some(scan) = self.scan_log(agent_id, transcript.transcript_path) {
                    let window = usage::claude_window_for(agent.model.as_deref());
                    let mut ctx = scan.context_usage(UsageSource::Transcript, now);
                    if let Some(ctx) = ctx.as_mut() {
                        // The model on the row knows about `[1m]`; the
                        // transcript's model id does not.
                        if window > ctx.window.unwrap_or(0) {
                            ctx.window = Some(window);
                            ctx.percent = ctx.tokens.map(|t| t as f32 * 100.0 / window as f32);
                        }
                    }
                    if let Some(ctx) = ctx {
                        self.record_context(agent_id, ctx);
                    }
                }
            }
            AgentKind::Codex => {
                let Some(sid) = agent.session_id.as_deref() else {
                    return;
                };
                let home = profile
                    .as_ref()
                    .and_then(|p| p.dir_for(&root, nebula_core::env::CODEX_HOME))
                    .unwrap_or_else(crate::hooks::installer::codex_home);
                let known = self.with_usage(|book| {
                    book.scans
                        .get(agent_id)
                        .map(|s| s.path.clone())
                        // A cached path is this session's only while the
                        // row still holds its id (a fresh start changes it).
                        .filter(|p| usage::is_codex_rollout_for(p, sid) && p.exists())
                });
                let Some(path) = known.or_else(|| usage::find_codex_rollout(&home, sid)) else {
                    return;
                };
                let Some(scan) = self.scan_log(agent_id, path) else {
                    return;
                };
                if !scan.windows.is_empty() {
                    self.record_account(AccountUsage {
                        account: usage::account_key(agent.profile.as_deref(), "codex"),
                        harness: "codex".into(),
                        windows: scan.windows.clone(),
                        source: UsageSource::Rollout,
                        at: now,
                        plan: scan.plan.clone(),
                    });
                }
                if let Some(ctx) = scan.context_usage(UsageSource::Rollout, now) {
                    self.record_context(agent_id, ctx);
                }
            }
            AgentKind::Copilot => {
                if let Some(sid) = agent.session_id.as_deref() {
                    let home = profile
                        .as_ref()
                        .and_then(|p| p.dir_for(&root, nebula_core::env::COPILOT_HOME))
                        .unwrap_or_else(crate::hooks::installer::copilot_home);
                    let path = usage::copilot_events_path(&home, sid);
                    if let Some(scan) = self.scan_log(agent_id, path) {
                        if let Some(ctx) = scan.context_usage(UsageSource::CopilotEvents, now) {
                            self.record_context(agent_id, ctx);
                        }
                    }
                }
                self.poll_copilot_quota(profile.as_ref(), false);
            }
            _ => {}
        }
    }

    /// Ask `gh` for a Copilot account's quota — the profile's own `gh`
    /// login, or the default one — unless asked within [`COPILOT_POLL`]
    /// (`force` ignores that). Blocking; run it off the async workers.
    pub fn poll_copilot_quota(
        &self,
        profile: Option<&nebula_core::profiles::Profile>,
        force: bool,
    ) {
        let key = usage::account_key(profile.map(|p| p.name.as_str()), "copilot");
        let due = self.with_usage(|book| {
            let due = force
                || book
                    .copilot_polled
                    .get(&key)
                    .is_none_or(|at| at.elapsed() >= COPILOT_POLL);
            if due {
                book.copilot_polled.insert(key.clone(), Instant::now());
            }
            due
        });
        if !due {
            return;
        }
        let env = profile.map(|p| p.launch_env()).unwrap_or_default();
        let Some(body) = gh_copilot_user(&env) else {
            return;
        };
        let (windows, plan) = usage::copilot_quota(&body);
        if windows.is_empty() && plan.is_none() {
            return;
        }
        self.record_account(AccountUsage {
            account: key,
            harness: "copilot".into(),
            windows,
            source: UsageSource::CopilotApi,
            at: nebula_core::clock::now_ms(),
            plan,
        });
    }

    /// The periodic Copilot sweep: every enabled Copilot profile, and the
    /// default login once any Copilot session exists.
    pub fn poll_copilot_accounts(&self) {
        let config = crate::config::Config::load();
        let copilot: Vec<_> = nebula_core::profiles::enabled_for(&config.profiles, "copilot")
            .into_iter()
            .cloned()
            .collect();
        for profile in &copilot {
            self.poll_copilot_quota(Some(profile), false);
        }
        let any_default = self
            .store
            .load_tree()
            .map(|(_, _, agents, _)| {
                agents
                    .iter()
                    .any(|a| a.kind == AgentKind::Copilot && a.profile.is_none())
            })
            .unwrap_or(false);
        if any_default {
            self.poll_copilot_quota(None, false);
        }
    }

    /// The periodic Claude sweep: each enabled Claude profile's login, and
    /// the default login once a Claude session runs under it, asked for the
    /// account's usage (`nebula_core::claude_usage`). The statusLine feed
    /// moves only while a session draws a status line and reaches the
    /// daemon; this reads the account itself, so the meter follows the real
    /// figure whichever session spent it. Blocking; run it off the async
    /// workers.
    pub fn poll_claude_accounts(&self) {
        let config = crate::config::Config::load();
        let root = nebula_core::profiles::profiles_root();
        let mut logins: Vec<(Option<String>, PathBuf)> =
            nebula_core::profiles::enabled_for(&config.profiles, "claude")
                .into_iter()
                .filter_map(|p| {
                    let dir = p
                        .dirs_in(&root)
                        .into_iter()
                        .find(|(var, _)| *var == nebula_core::env::CLAUDE_CONFIG_DIR)?
                        .1;
                    Some((Some(p.name.clone()), dir))
                })
                .collect();
        let any_default = self
            .store
            .load_tree()
            .map(|(_, _, agents, _)| {
                agents
                    .iter()
                    .any(|a| a.kind == AgentKind::Claude && a.profile.is_none() && !a.archived)
            })
            .unwrap_or(false);
        if any_default {
            if let Some(dir) = nebula_core::paths::claude_config_dir() {
                logins.push((None, dir));
            }
        }
        for (profile, dir) in logins {
            let now = nebula_core::clock::now_ms();
            let Some(windows) = nebula_core::claude_usage::fetch(&dir, now) else {
                continue;
            };
            self.record_account(AccountUsage {
                account: usage::account_key(profile.as_deref(), "claude"),
                harness: "claude".into(),
                windows,
                // The wire's source names are fixed per protocol; this is
                // a plan reading like the statusLine's.
                source: UsageSource::Statusline,
                at: now,
                plan: None,
            });
        }
    }

    /// The on-screen usage notice of a Claude session (`pty::usage`),
    /// folded into its account: it names one window, the rest stay.
    pub(crate) fn on_pty_usage(&self, id: &AgentId, notice: nebula_core::profiles::Usage) {
        let Ok(Some(agent)) = self.store.get_agent(id) else {
            return;
        };
        let harness = harness_of(&agent);
        self.record_account(AccountUsage {
            account: usage::account_key(agent.profile.as_deref(), &harness),
            harness,
            windows: vec![usage::window_from_legacy(
                &notice.window,
                notice.percent as f32,
                None,
            )],
            source: UsageSource::Screen,
            at: nebula_core::clock::now_ms(),
            plan: None,
        });
    }
}

/// `gh api copilot_internal/user` under `env` (a profile's `GH_CONFIG_DIR`,
/// its credential variables removed), parsed; None on any failure. Run
/// without a shell, stdin closed, and killed past [`GH_TIMEOUT`]. Only the
/// parsed JSON leaves here, and callers read only quota fields from it.
fn gh_copilot_user(env: &nebula_core::profiles::LaunchEnv) -> Option<Value> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    let mut cmd = Command::new("gh");
    cmd.args(["api", "copilot_internal/user"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .env("GH_PROMPT_DISABLED", "1");
    for name in env.removals() {
        cmd.env_remove(name);
    }
    for (k, v) in &env.set {
        cmd.env(k, v);
    }
    #[cfg(windows)]
    nebula_core::platform::no_window(&mut cmd);
    let mut child = cmd.spawn().ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut out = Vec::new();
        let _ = stdout.read_to_end(&mut out);
        out
    });
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(Some(_)) => return None,
            Ok(None) if started.elapsed() > GH_TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Err(_) => return None,
        }
    }
    let out = reader.join().ok()?;
    serde_json::from_slice(&out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use nebula_core::usage::UsageSource;

    fn ctx(percent: Option<f32>, compactions: u32) -> ContextUsage {
        ContextUsage {
            percent,
            tokens: None,
            window: Some(200_000),
            compactions,
            last_compact_at: None,
            source: UsageSource::Statusline,
            at: 0,
        }
    }

    fn test_daemon() -> std::sync::Arc<Daemon> {
        let daemon = Daemon::new(
            std::sync::Arc::new(crate::store::Store::open_in_memory().unwrap()),
            crate::hooks::HookEnv {
                port: 0,
                token: String::new(),
            },
        );
        use nebula_core::{Project, ProjectId, Worktree, WorktreeId};
        daemon
            .store
            .insert_project(&Project {
                id: ProjectId("p".into()),
                name: "demo".into(),
                repo_path: "/repo".into(),
                sort_order: 0,
            })
            .unwrap();
        daemon
            .store
            .insert_worktree(&Worktree {
                id: WorktreeId("w".into()),
                project_id: ProjectId("p".into()),
                path: "/repo".into(),
                branch: "main".into(),
                is_main: true,
                sort_order: 0,
            })
            .unwrap();
        daemon
    }

    fn agent(id: &str, kind: AgentKind, profile: Option<&str>) -> Agent {
        Agent {
            id: AgentId(id.into()),
            worktree_id: nebula_core::WorktreeId("w".into()),
            name: id.into(),
            status: nebula_core::AgentStatus::Fresh,
            archived: false,
            archived_at: 0,
            unseen: false,
            status_changed_at: 0,
            kind,
            custom_harness: None,
            model: None,
            effort: None,
            session_id: None,
            cloud_session_id: None,
            issue_url: None,
            role: nebula_core::AgentRole::Worker,
            sort_order: 0,
            alive: false,
            recent_prompts: Vec::new(),
            profile: profile.map(str::to_string),
            parent: None,
        }
    }

    /// A statusLine reading files its windows under the session's account
    /// (`@claude` with no profile), keeps it on disk, pushes it, and the
    /// context with the transcript's compactions; an identical reading
    /// pushes nothing.
    #[test]
    fn a_statusline_reading_lands_on_its_account_and_session() {
        let dir = tempfile::tempdir().unwrap();
        let daemon = test_daemon();
        daemon.set_usage_root(dir.path().to_path_buf());
        daemon
            .store
            .insert_agent(&agent("a1", AgentKind::Claude, None))
            .unwrap();
        let transcript = dir.path().join("t.jsonl");
        std::fs::write(
            &transcript,
            "{\"type\":\"system\",\"subtype\":\"compact_boundary\"}\n",
        )
        .unwrap();
        let mut events = daemon.events.subscribe();
        let body = serde_json::json!({
            "transcript_path": transcript.to_string_lossy(),
            "context_window": {"context_window_size": 200000, "used_percentage": 40,
                "current_usage": {"input_tokens": 80000}},
            "rate_limits": {"five_hour": {"used_percentage": 12, "resets_at": 99}}
        });
        daemon.on_statusline(&AgentId("a1".into()), &body);
        let list = daemon.account_usage_list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].account, "@claude");
        assert_eq!(list[0].windows[0].percent, 12.0);
        assert!(list[0].windows[0].read_at > 0, "stamped as read");
        assert!(
            usage::read_account_in(dir.path(), "@claude").is_some(),
            "kept on disk"
        );
        assert!(matches!(
            events.try_recv().unwrap(),
            ServerEvent::AccountUsage { .. }
        ));
        let ServerEvent::SessionContexts { contexts } = events.try_recv().unwrap() else {
            panic!("no context pushed");
        };
        assert_eq!(contexts[0].context.percent, Some(40.0));
        assert_eq!(contexts[0].context.compactions, 1);
        daemon.on_statusline(&AgentId("a1".into()), &body);
        assert!(events.try_recv().is_err(), "nothing moved: nothing pushed");
        daemon.forget_context(&AgentId("a1".into()));
        assert!(daemon.session_contexts().is_empty());
    }

    /// The route trusts the `agentId` the wrapper sends, so a reading
    /// names a session the daemon knows: one for a non-Claude session, an
    /// archived one, or an unknown id is dropped.
    #[test]
    fn a_statusline_reading_for_no_current_claude_session_is_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let daemon = test_daemon();
        daemon.set_usage_root(dir.path().to_path_buf());
        daemon
            .store
            .insert_agent(&agent("cx", AgentKind::Codex, None))
            .unwrap();
        daemon
            .store
            .insert_agent(&Agent {
                archived: true,
                ..agent("old", AgentKind::Claude, None)
            })
            .unwrap();
        let body = serde_json::json!({"rate_limits": {"five_hour": {"used_percentage": 50}}});
        for id in ["cx", "old", "nobody"] {
            daemon.on_statusline(&AgentId(id.into()), &body);
        }
        assert!(daemon.account_usage_list().is_empty());
        assert!(usage::read_account_in(dir.path(), "@claude").is_none());
        assert!(usage::read_account_in(dir.path(), "@codex").is_none());
    }

    /// EFFORT SYNC: a statusLine naming another effort than the row's —
    /// a `/effort`, or the model capping `max` at `high` — is stored and
    /// pushed, and the next spawn passes it; a repeat or a reading with no
    /// effort changes nothing. A row on the default is not pinned to the
    /// level the default runs at, only to a change from it. Never a
    /// non-Claude row, never a level `--effort` doesn't take.
    #[test]
    fn a_statusline_effort_follows_the_row_into_the_next_spawn() {
        let dir = tempfile::tempdir().unwrap();
        let daemon = test_daemon();
        daemon.set_usage_root(dir.path().to_path_buf());
        daemon
            .store
            .insert_agent(&Agent {
                effort: Some("max".into()),
                session_id: Some("s1".into()),
                ..agent("a1", AgentKind::Claude, None)
            })
            .unwrap();
        let id = AgentId("a1".into());
        let effort = |daemon: &Daemon, id: &str| {
            daemon
                .store
                .get_agent(&AgentId(id.into()))
                .unwrap()
                .unwrap()
                .effort
        };
        let mut events = daemon.events.subscribe();
        daemon.on_statusline(&id, &serde_json::json!({"effort": {"level": "high"}}));
        assert_eq!(effort(&daemon, "a1").as_deref(), Some("high"));
        assert!(matches!(
            events.try_recv(),
            Ok(ServerEvent::EntityUpserted { .. })
        ));
        let row = daemon.store.get_agent(&id).unwrap().unwrap();
        let (_, args, _) = super::super::agent_spawn_command(
            AgentKind::Claude,
            row.session_id.as_deref(),
            row.model.as_deref(),
            row.effort.as_deref(),
            None,
        );
        let at = args.iter().position(|a| a == "--effort").expect("--effort");
        assert_eq!(args[at + 1], "high", "{args:?}");
        daemon.on_statusline(&id, &serde_json::json!({"effort": {"level": "high"}}));
        daemon.on_statusline(&id, &serde_json::json!({"context_window": {}}));
        assert_eq!(effort(&daemon, "a1").as_deref(), Some("high"));
        assert!(events.try_recv().is_err(), "nothing moved: nothing pushed");
        assert!(!daemon.adopt_claude_effort(&id, "ludicrous"));
        assert_eq!(effort(&daemon, "a1").as_deref(), Some("high"));

        // On the default: its first level is the default's, then a change.
        daemon
            .store
            .insert_agent(&agent("d1", AgentKind::Claude, None))
            .unwrap();
        let d1 = AgentId("d1".into());
        assert!(!daemon.adopt_claude_effort(&d1, "high"));
        assert_eq!(effort(&daemon, "d1"), None, "not pinned to the default");
        assert!(daemon.adopt_claude_effort(&d1, "low"));
        assert_eq!(effort(&daemon, "d1").as_deref(), Some("low"));

        daemon
            .store
            .insert_agent(&Agent {
                effort: Some("high".into()),
                ..agent("cx", AgentKind::Codex, None)
            })
            .unwrap();
        assert!(!daemon.adopt_claude_effort(&AgentId("cx".into()), "low"));
        assert_eq!(effort(&daemon, "cx").as_deref(), Some("high"));
    }

    /// A screen notice keeps the five-hour window the status line read.
    #[test]
    fn a_screen_notice_merges_into_the_account() {
        let dir = tempfile::tempdir().unwrap();
        let daemon = test_daemon();
        daemon.set_usage_root(dir.path().to_path_buf());
        daemon
            .store
            .insert_agent(&agent("a2", AgentKind::Claude, Some("work")))
            .unwrap();
        daemon.on_statusline(
            &AgentId("a2".into()),
            &serde_json::json!({"rate_limits": {"five_hour": {"used_percentage": 5}}}),
        );
        daemon.on_pty_usage(
            &AgentId("a2".into()),
            nebula_core::profiles::Usage::parse("You've used 91% of your weekly limit").unwrap(),
        );
        let work = usage::read_account_in(dir.path(), "work").unwrap();
        assert_eq!(work.windows.len(), 2);
        assert_eq!(work.source, UsageSource::Screen);
    }

    #[test]
    fn a_context_is_resent_only_when_it_moved() {
        assert!(context_changed(None, &ctx(Some(1.0), 0)));
        assert!(!context_changed(
            Some(&ctx(Some(10.0), 0)),
            &ctx(Some(10.3), 0)
        ));
        assert!(context_changed(
            Some(&ctx(Some(10.0), 0)),
            &ctx(Some(10.6), 0)
        ));
        assert!(context_changed(
            Some(&ctx(Some(10.0), 0)),
            &ctx(Some(10.0), 1)
        ));
        assert!(context_changed(Some(&ctx(None, 0)), &ctx(Some(0.0), 0)));
    }
}
