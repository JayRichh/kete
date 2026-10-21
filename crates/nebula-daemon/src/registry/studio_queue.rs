//! STUDIO QUEUE (K2): the runner that hands an idle orchestrator a short
//! `[from kete studio]` note naming new inbox notes and account headroom,
//! and the `SetStudio` and `StudioQueue` requests.
//!
//! The rules are `nebula_core::studio::queue`'s; this file gathers their
//! inputs (the `studio` entries in `config.local.json`, each repo's inbox,
//! the orchestrator's status, context and last keystroke, the accounts),
//! writes the note through `session_context::write_prompt`, and keeps the
//! runner's state in `<data dir>/studio/queue-state.json`, read on first
//! use, so a restart never repeats a note. The runner never spawns.

use super::*;
use nebula_core::studio::inbox::{self, InboxNote};
use nebula_core::studio::queue::{self, OrchestratorView, QueueState, TickInputs};
use nebula_core::studio::wire::StudioQueueRow;
use nebula_core::studio::{manifest, settings, StudioSettings};
use std::collections::{BTreeMap, BTreeSet};

/// The runner's state (announced notes, the last idle note), on
/// `Daemon::studio_queue`, taken alone: a tick holds it from first read
/// to last write, so two ticks never interleave.
#[derive(Debug, Default)]
pub(crate) struct QueueBook {
    /// Tests: one folder for `config.json`, `config.local.json`,
    /// `queue-state.json` and `refusals.jsonl`, never the user's. None in
    /// production, where each is its usual path.
    root: Option<PathBuf>,
    /// None until first read.
    state: Option<QueueState>,
}

impl QueueBook {
    pub(super) fn config_path(&self) -> PathBuf {
        match &self.root {
            Some(root) => root.join(nebula_core::paths::CONFIG_FILE_NAME),
            None => nebula_core::paths::config_path(),
        }
    }

    pub(super) fn local_path(&self) -> PathBuf {
        match &self.root {
            Some(root) => root.join("config.local.json"),
            None => nebula_core::paths::config_local_path(),
        }
    }

    pub(super) fn refusals_path(&self) -> PathBuf {
        match &self.root {
            Some(root) => root.join("refusals.jsonl"),
            None => nebula_core::studio::refusals::refusals_path(),
        }
    }

    fn state_path(&self) -> PathBuf {
        match &self.root {
            Some(root) => root.join("queue-state.json"),
            None => nebula_core::paths::data_dir()
                .join("studio")
                .join("queue-state.json"),
        }
    }

    fn state(&mut self) -> &mut QueueState {
        if self.state.is_none() {
            self.state = Some(QueueState::load(&self.state_path()));
        }
        self.state.as_mut().expect("loaded above")
    }
}

/// One pass of the runner, from the daemon's 30 s tick (`lib.rs`): off the
/// runtime's threads, as it reads inboxes and writes the state file.
pub fn tick(daemon: &Arc<Daemon>) {
    let daemon = daemon.clone();
    tokio::task::spawn_blocking(move || {
        if let Err(error) = daemon.studio_queue(None, true) {
            tracing::warn!(%error, "studio queue tick failed");
        }
    });
}

impl Daemon {
    /// Tests: keep the studio settings, state and refusal log under `dir`.
    #[cfg(test)]
    pub(crate) fn set_studio_root(&self, dir: PathBuf) {
        *self.studio_queue.lock() = QueueBook {
            root: Some(dir),
            state: None,
        };
    }

    /// `SetStudio`: write (None: remove) `project`'s entry under `studio`
    /// in `config.local.json`. The server refuses it from inside an agent
    /// session before this runs. An empty inbox takes the manifest's; an
    /// inbox outside the repo is refused.
    pub fn studio_set(
        &self,
        project: &ProjectId,
        studio: Option<StudioSettings>,
    ) -> std::result::Result<(), String> {
        let project = self
            .store
            .get_project(project)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("no project {project}"))?;
        let root = project.repo_path.clone();
        let studio = match studio {
            None => None,
            Some(mut s) => {
                if s.studio_inbox.trim().is_empty() {
                    s.studio_inbox = manifest::load(&root)?
                        .and_then(|m| m.inbox)
                        .unwrap_or_default();
                }
                s.studio_inbox = s.studio_inbox.trim().replace('\\', "/");
                if s.studio_inbox.is_empty() {
                    return Err(format!(
                        "{} names no inbox: pass --inbox <dir>, or set `inbox` in {}",
                        project.name,
                        manifest::MANIFEST_PATH
                    ));
                }
                inbox_inside(&root, &s.studio_inbox)?;
                Some(s)
            }
        };
        let (config, local) = {
            let book = self.studio_queue.lock();
            (book.config_path(), book.local_path())
        };
        settings::write_in(&config, &local, &root, studio.as_ref())
    }

    /// `StudioQueue`: what the runner would do now for `project` (or every
    /// studio repo); with `run`, one tick at once.
    pub fn studio_queue(
        self: &Arc<Self>,
        project: Option<&ProjectId>,
        run: bool,
    ) -> std::result::Result<Vec<StudioQueueRow>, String> {
        let this = self.clone();
        let mut send = move |agent: &AgentId, text: &str| -> std::result::Result<(), String> {
            let sref = SessionRef::Agent(agent.clone());
            let session = this
                .ensure_session(&sref, DEFAULT_COLS, DEFAULT_ROWS)
                .map_err(|e| e.to_string())?;
            crate::session_context::write_prompt(&session, text).map_err(|e| e.to_string())
        };
        self.queue_pass(project, run, &mut send)
    }

    /// The runner's pass over every studio repo (or `filter`'s): a row
    /// each, and with `run` at most one idle note, through `send`.
    pub(crate) fn queue_pass(
        &self,
        filter: Option<&ProjectId>,
        run: bool,
        send: &mut dyn FnMut(&AgentId, &str) -> std::result::Result<(), String>,
    ) -> std::result::Result<Vec<StudioQueueRow>, String> {
        let mut book = self.studio_queue.lock();
        let entries = settings::load_all_in(&book.local_path());
        if entries.is_empty() {
            return Ok(Vec::new());
        }
        let now = nebula_core::clock::now_ms();
        let config: crate::config::Config =
            nebula_core::settings::load(&book.config_path(), &book.local_path()).value;
        let idle_after_secs = config.studio_idle_after().as_secs() as i64;
        let projects = self.store.load_tree().map_err(|e| e.to_string())?.0;
        let orchestrator = self
            .store
            .orchestrator()
            .map_err(|e| e.to_string())?
            .filter(|a| !a.archived);
        let mut view = orchestrator.as_ref().map(|a| OrchestratorView {
            status: a.status,
            status_changed_at: a.status_changed_at,
            last_input_at: self
                .last_input_at(&a.id)
                .map(|at| now - at.elapsed().as_millis() as i64),
        });
        let headroom = if book.root.is_none() {
            self.headroom(now)
        } else {
            None
        };
        let before = book.state().clone();
        let nudge = orchestrator
            .as_ref()
            .and_then(|a| self.context_nudge(a, now, book.state().last_nudge_at));

        let mut rows = Vec::new();
        let mut sent = false;
        for (key, entry) in &entries {
            let key = key.to_string_lossy().into_owned();
            let Some(project) = projects
                .iter()
                .find(|p| settings::same_repo(&settings::repo_key(&p.repo_path), &key))
            else {
                continue;
            };
            if filter.is_some_and(|f| f != &project.id) {
                continue;
            }
            let root = &project.repo_path;
            let manifest = manifest::load(root).ok().flatten();
            let inbox_rel = if entry.studio_inbox.trim().is_empty() {
                manifest
                    .as_ref()
                    .and_then(|m| m.inbox.clone())
                    .unwrap_or_default()
            } else {
                entry.studio_inbox.clone()
            };
            let all = if inbox_rel.is_empty() {
                Vec::new()
            } else {
                inbox::read_dir(&root.join(&inbox_rel))
            };
            let recipients = manifest.map(|m| m.recipients).unwrap_or_default();
            let eligible: Vec<(&InboxNote, i64)> =
                inbox::eligible(&all, &recipients, &entry.queue_exclude)
                    .into_iter()
                    .map(|n| (n, mtime_ms(&n.path)))
                    .collect();
            let present: BTreeSet<String> = all.iter().map(|n| n.id.clone()).collect();
            book.state().forget_missing(&key, &present);

            let decide_now = |view: Option<OrchestratorView>,
                              announced: Option<&BTreeMap<String, i64>>,
                              sent_last_hour: usize,
                              nudge_due: bool| {
                queue::decide(
                    now,
                    &TickInputs {
                        autonomy: Some(entry.autonomy),
                        orchestrator: view,
                        idle_after_secs,
                        notes: &eligible,
                        announced,
                        sent_last_hour,
                        nudge_due,
                    },
                )
                .map(|n| n.len())
            };
            let mut announced = book.state().announced_for(&key).cloned();
            let mut decision = decide_now(
                view,
                announced.as_ref(),
                book.state().sent_in_last_hour(now),
                nudge.is_some() && !sent,
            );
            if run && !sent && decision.is_ok() {
                if let Some(orch) = &orchestrator {
                    let fresh = queue::unannounced(&eligible, announced.as_ref());
                    let notes: Vec<_> = fresh
                        .iter()
                        .map(|(n, _)| queue::queue_note(n, root))
                        .collect();
                    let local = now / 1000 + nebula_core::usage_window::local(now / 1000) as i64;
                    let note = queue::compose_note(
                        local,
                        &project.name,
                        entry.autonomy,
                        &notes,
                        headroom.as_deref(),
                        nudge,
                    );
                    let listed: Vec<(String, i64)> = fresh
                        .iter()
                        .take(note.listed)
                        .map(|(n, mtime)| (n.id.clone(), *mtime))
                        .collect();
                    match send(&orch.id, &note.text) {
                        Ok(()) => {
                            tracing::info!(
                                project = %project.id,
                                listed = listed.len(),
                                nudge = nudge.is_some(),
                                "studio idle note sent"
                            );
                            book.state().record(&key, &listed, now, nudge.is_some());
                            sent = true;
                            // The note starts a turn: every later repo of
                            // this pass reads the orchestrator as working.
                            if let Some(v) = view.as_mut() {
                                v.status = AgentStatus::Running;
                                v.status_changed_at = now;
                            }
                        }
                        Err(error) => {
                            tracing::warn!(project = %project.id, %error, "studio idle note failed");
                        }
                    }
                    announced = book.state().announced_for(&key).cloned();
                    decision = decide_now(
                        view,
                        announced.as_ref(),
                        book.state().sent_in_last_hour(now),
                        false,
                    );
                }
            }
            let fresh = queue::unannounced(&eligible, announced.as_ref());
            rows.push(StudioQueueRow {
                project: project.id.clone(),
                name: project.name.clone(),
                autonomy: entry.autonomy,
                inbox: inbox_rel,
                open: all.iter().filter(|n| n.status.is_open()).count() as u32,
                to_jay: all
                    .iter()
                    .filter(|n| n.status.is_open())
                    .filter(|n| {
                        n.to.as_deref()
                            .is_some_and(|t| t.trim().eq_ignore_ascii_case("jay"))
                    })
                    .count() as u32,
                unannounced: fresh.len() as u32,
                next: fresh
                    .iter()
                    .take(queue::MAX_LISTED)
                    .map(|(n, _)| queue::queue_note(n, root))
                    .collect(),
                held: decision.err().map(|h| h.to_string()),
                last_note_at: book.state().last_note_at,
            });
        }
        if *book.state() != before {
            let path = book.state_path();
            book.state().save(&path)?;
        }
        Ok(rows)
    }

    /// The accounts line of an idle note: each Claude account's 5-hour
    /// window and when it resets, and the reserved accounts by name.
    fn headroom(&self, now_ms: i64) -> Option<String> {
        let now = now_ms / 1000;
        let profiles = nebula_core::profiles::load();
        let reserved: Vec<String> = profiles
            .iter()
            .filter(|p| p.reserved && !p.disabled)
            .map(|p| p.name.clone())
            .collect();
        let disabled: Vec<&str> = profiles
            .iter()
            .filter(|p| p.disabled)
            .map(|p| p.name.as_str())
            .collect();
        let accounts: Vec<(String, Option<f32>, Option<String>)> = self
            .account_usage_list()
            .into_iter()
            .filter(|a| a.harness == "claude" && !disabled.contains(&a.account.as_str()))
            .filter_map(|a| {
                let window = nebula_core::usage_window::headline(&a.windows, now)?;
                let label = nebula_core::usage_window::format(
                    window,
                    now,
                    nebula_core::usage_window::local,
                );
                let name = if a.account.starts_with('@') {
                    "default".to_string()
                } else {
                    a.account.clone()
                };
                Some((name, label.percent, label.clock))
            })
            .collect();
        queue::headroom_line(&accounts, &reserved)
    }

    /// The context nudge for the orchestrator: its context against its
    /// role card's budget, else its model window.
    fn context_nudge(
        &self,
        orchestrator: &Agent,
        now_ms: i64,
        last_nudge_at: Option<i64>,
    ) -> Option<queue::ContextNudge> {
        let context = self
            .session_contexts()
            .into_iter()
            .find(|c| c.agent == orchestrator.id)?
            .context;
        let window = context.window.unwrap_or_else(|| {
            nebula_core::usage::claude_window_for(orchestrator.model.as_deref())
        });
        let budget = self
            .session_roles()
            .into_iter()
            .find(|r| r.agent == orchestrator.id)
            .and_then(|r| r.context_budget)
            .unwrap_or(window);
        let tokens = context.tokens.or_else(|| {
            context
                .percent
                .map(|p| (f64::from(p) / 100.0 * window as f64) as u64)
        })?;
        queue::nudge(now_ms, tokens, budget, last_nudge_at)
    }
}

/// A file's modification time, epoch ms; 0 when unreadable.
fn mtime_ms(path: &Path) -> i64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Refuse an inbox folder that is not inside `root`: absolute, with a
/// drive or `..`, or (when it exists) resolving outside the repo.
pub(crate) fn inbox_inside(root: &Path, rel: &str) -> std::result::Result<(), String> {
    use std::path::Component;
    let path = Path::new(rel);
    let lexical_ok = path
        .components()
        .all(|c| matches!(c, Component::Normal(_) | Component::CurDir));
    if !lexical_ok || path.components().next().is_none() {
        return Err(format!(
            "the inbox must be a folder inside the repo, relative to its root: {rel}"
        ));
    }
    let joined = root.join(path);
    if let (Ok(real), Ok(real_root)) = (joined.canonicalize(), root.canonicalize()) {
        if !real.starts_with(&real_root) {
            return Err(format!("the inbox {rel} resolves outside the repo"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::HookEnv;
    use nebula_core::studio::Autonomy;
    use nebula_core::AgentRole;

    /// A daemon over an in-memory store with project `p` ("demo") rooted at
    /// a temp repo with an inbox of three notes, its studio files in a
    /// temp folder, and an orchestrator idle for a minute.
    struct Fixture {
        daemon: Arc<Daemon>,
        repo: tempfile::TempDir,
        studio: tempfile::TempDir,
    }

    fn write_note(dir: &Path, id: &str, extra: &str) {
        std::fs::write(
            dir.join(format!("{id}.md")),
            format!("---\nid: {id}\nstatus: open\n{extra}\ntitle: Note {id}\n---\nBody.\n"),
        )
        .unwrap();
    }

    fn fixture() -> Fixture {
        let repo = tempfile::tempdir().unwrap();
        let studio = tempfile::tempdir().unwrap();
        let inbox = repo.path().join("notes").join("inbox");
        std::fs::create_dir_all(&inbox).unwrap();
        write_note(&inbox, "a-normal", "to: orchestrator\npriority: normal");
        write_note(&inbox, "b-high", "to: orchestrator\npriority: high");
        write_note(&inbox, "c-jay", "to: jay\npriority: high");
        let daemon = Daemon::new(
            Arc::new(Store::open_in_memory().unwrap()),
            HookEnv {
                port: 0,
                token: String::new(),
            },
        );
        daemon.set_studio_root(studio.path().to_path_buf());
        // Usage readings too: the first look would read the real profiles.
        daemon.set_usage_root(studio.path().join("usage"));
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
        daemon
            .store
            .insert_agent(&Agent {
                parent: None,
                profile: None,
                id: AgentId("orch".into()),
                worktree_id: WorktreeId("w".into()),
                name: "orch".into(),
                status: AgentStatus::Finished,
                archived: false,
                archived_at: 0,
                unseen: false,
                status_changed_at: nebula_core::clock::now_ms() - 60_000,
                kind: AgentKind::Claude,
                custom_harness: None,
                model: None,
                effort: None,
                session_id: None,
                cloud_session_id: None,
                issue_url: None,
                role: AgentRole::Orchestrator,
                sort_order: 0,
                alive: false,
                recent_prompts: Vec::new(),
            })
            .unwrap();
        daemon
            .store
            .set_orchestrator(Some(&AgentId("orch".into())))
            .unwrap();
        Fixture {
            daemon,
            repo,
            studio,
        }
    }

    fn enable(f: &Fixture, autonomy: Autonomy) {
        f.daemon
            .studio_set(
                &ProjectId("p".into()),
                Some(StudioSettings {
                    studio_inbox: "notes/inbox".into(),
                    autonomy,
                    queue_exclude: Vec::new(),
                }),
            )
            .unwrap();
    }

    fn pass(daemon: &Daemon, run: bool) -> (Vec<StudioQueueRow>, Vec<String>) {
        let mut sent = Vec::new();
        let rows = daemon
            .queue_pass(None, run, &mut |_, text| {
                sent.push(text.to_string());
                Ok(())
            })
            .unwrap();
        (rows, sent)
    }

    #[test]
    fn an_idle_note_names_the_new_notes_once() {
        let f = fixture();
        enable(&f, Autonomy::A1);

        let (rows, sent) = pass(&f.daemon, false);
        assert!(sent.is_empty(), "a report sends nothing");
        let row = &rows[0];
        assert_eq!((row.open, row.to_jay, row.unannounced), (3, 1, 2));
        assert_eq!(row.held, None);
        assert_eq!(row.next[0].id, "b-high");

        let (rows, sent) = pass(&f.daemon, true);
        assert_eq!(sent.len(), 1);
        let text = &sent[0];
        assert!(text.starts_with("[from kete studio] "), "{text}");
        assert!(text.contains("demo A1 · 2 new inbox notes"), "{text}");
        let high = text.find("notes/inbox/b-high.md").unwrap();
        let normal = text.find("notes/inbox/a-normal.md").unwrap();
        assert!(high < normal, "high first: {text}");
        assert!(!text.contains("c-jay"), "{text}");
        assert_eq!(rows[0].held.as_deref(), Some("nothing new"));

        // The next pass (the orchestrator still idle): nothing new.
        let (rows, sent) = pass(&f.daemon, true);
        assert!(sent.is_empty());
        assert_eq!(rows[0].held.as_deref(), Some("nothing new"));
        assert!(rows[0].last_note_at.is_some());
    }

    #[test]
    fn queue_state_survives_a_restart_so_nothing_repeats() {
        let f = fixture();
        enable(&f, Autonomy::A2);
        assert_eq!(pass(&f.daemon, true).1.len(), 1);
        assert!(f.studio.path().join("queue-state.json").is_file());

        // A fresh book on the same folder: the daemon after a restart.
        f.daemon.set_studio_root(f.studio.path().to_path_buf());
        let (rows, sent) = pass(&f.daemon, true);
        assert!(sent.is_empty(), "{sent:?}");
        assert_eq!(rows[0].held.as_deref(), Some("nothing new"));

        // An edited note is news again.
        let note = f.repo.path().join("notes/inbox/a-normal.md");
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
        std::fs::File::options()
            .write(true)
            .open(&note)
            .unwrap()
            .set_modified(later)
            .unwrap();
        let (_, sent) = pass(&f.daemon, true);
        assert_eq!(sent.len(), 1);
        assert!(sent[0].contains("1 new inbox note\n"), "{}", sent[0]);
    }

    #[test]
    fn a0_and_typing_hold_the_note() {
        let f = fixture();
        enable(&f, Autonomy::A0);
        let (rows, sent) = pass(&f.daemon, true);
        assert!(sent.is_empty());
        assert_eq!(rows[0].held.as_deref(), Some("autonomy A0 (Hold)"));

        enable(&f, Autonomy::A1);
        f.daemon
            .note_input(&SessionRef::Agent(AgentId("orch".into())));
        let (rows, sent) = pass(&f.daemon, true);
        assert!(sent.is_empty());
        assert!(
            rows[0].held.as_deref().unwrap().starts_with("you typed "),
            "{:?}",
            rows[0].held
        );
    }

    #[test]
    fn an_inbox_outside_the_repo_is_refused() {
        let f = fixture();
        let set = |inbox: &str| {
            f.daemon.studio_set(
                &ProjectId("p".into()),
                Some(StudioSettings {
                    studio_inbox: inbox.into(),
                    ..Default::default()
                }),
            )
        };
        for bad in [
            "../elsewhere",
            "notes/../../x",
            "/abs/inbox",
            "C:/abs/inbox",
        ] {
            if bad.starts_with("C:") && !cfg!(windows) {
                continue;
            }
            let err = set(bad).expect_err(bad);
            assert!(err.contains("inside the repo"), "{bad}: {err}");
        }
        assert!(set("").unwrap_err().contains("names no inbox"));
        set("notes/inbox").unwrap();
        let local = f.studio.path().join("config.local.json");
        let entry = settings::for_repo_in(&local, f.repo.path()).unwrap();
        assert_eq!(entry.studio_inbox, "notes/inbox");
        assert_eq!(entry.autonomy, Autonomy::A1);

        // Off again.
        f.daemon.studio_set(&ProjectId("p".into()), None).unwrap();
        assert!(settings::for_repo_in(&local, f.repo.path()).is_none());
    }

    #[test]
    fn studio_check_spawn_refuses_at_a0_only() {
        let f = fixture();
        let p = ProjectId("p".into());
        // Off: passes.
        assert!(f.daemon.studio_check_spawn(&p).is_ok());
        for level in [Autonomy::A1, Autonomy::A2, Autonomy::A3] {
            enable(&f, level);
            assert!(f.daemon.studio_check_spawn(&p).is_ok(), "{level:?}");
        }
        enable(&f, Autonomy::A0);
        let err = f.daemon.studio_check_spawn(&p).unwrap_err();
        assert_eq!(err, "autonomy A0 (Hold) for demo");
        let log = nebula_core::studio::refusals::read_last_in(
            &f.studio.path().join("refusals.jsonl"),
            10,
        );
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].reason, err);
        // An unknown project is the spawn's to refuse.
        assert!(f
            .daemon
            .studio_check_spawn(&ProjectId("nope".into()))
            .is_ok());
    }
}
