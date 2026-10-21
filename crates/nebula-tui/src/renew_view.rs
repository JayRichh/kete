//! ORCHESTRATOR RENEWAL in the TUI (K9): the prompt for the handover
//! note, and what the view does when a renewal lands.
//!
//! The renew lane owns this file; the signatures below are fixed (the verb
//! dispatch, the `PromptKind::RenewNote` submit and the
//! `OrchestratorRenewal` arm call them). None of them is handed the
//! request outbox, so the request goes off the loop as a one-shot client
//! (`ipc_studio::renew_orchestrator`) and the view's re-attach is armed as
//! the pane's deferred attach, which the loop sends on its next turn.

use crate::app::{App, AttachedTerm, PromptKind};
use nebula_core::studio::wire::RenewState;
use nebula_core::{AgentId, SessionRef};
use std::path::{Path, PathBuf};

/// Open the renew prompt (the `renew_orchestrator` verb).
pub fn open(app: &mut App) {
    crate::event_loop::open_prompt(app, PromptKind::RenewNote);
}

/// The renew prompt's Enter, with the note path typed.
pub fn submit(app: &mut App, text: String) {
    app.chrome.dirty = true;
    let text = text.trim();
    if text.is_empty() {
        app.chrome.flash = Some("Renew orchestrator: name the handover note".into());
        return;
    }
    let note = match std::env::current_dir() {
        Ok(cwd) => absolute(Path::new(text), &cwd),
        Err(_) => PathBuf::from(text),
    };
    let Some(jobs) = app.jobs.view_jobs.clone() else {
        app.chrome.flash = Some("Renew orchestrator: not connected yet".into());
        return;
    };
    app.chrome.flash = Some("Renew orchestrator: asking the daemon…".into());
    let req = crate::ipc_studio::RenewRequestArgs {
        caller: None,
        note,
        profile: None,
        model: None,
        keep_old: false,
        now: false,
    };
    jobs.run(move || {
        let ask = crate::ipc_studio::renew_orchestrator(req);
        let answer = tokio::runtime::Handle::current().block_on(async {
            tokio::time::timeout(std::time::Duration::from_secs(15), ask).await
        });
        Some(crate::view_jobs::Answer::Flash(match answer {
            Ok(Ok(state)) => scheduled_flash(&state),
            Ok(Err(e)) => format!("Renew orchestrator: {e:#}"),
            Err(_) => "Renew orchestrator: the daemon did not answer".into(),
        }))
    });
}

/// `ServerEvent::OrchestratorRenewal`: `Scheduled` answering this
/// client's request, then `Done` or `Failed` for everyone.
pub fn on_renewal(
    app: &mut App,
    req_id: Option<u64>,
    old: AgentId,
    new: Option<AgentId>,
    state: RenewState,
) {
    // This client's own request rides a one-shot connection, whose
    // `Scheduled` the submit job flashes.
    let _ = req_id;
    match (state, new) {
        (RenewState::Done, Some(new)) => {
            follow(app, &old, new.clone());
            app.chrome.flash = Some(match note_of(&new) {
                Some(note) => format!("Orchestrator renewed from {note}"),
                None => "Orchestrator renewed".into(),
            });
        }
        (RenewState::Failed { reason }, _) => {
            app.chrome.flash = Some(format!("Orchestrator renewal failed: {reason}"));
        }
        _ => return,
    }
    app.chrome.dirty = true;
}

/// Put `new` in the pane where `old` was showing: the orchestrator view
/// (F10) stays up on the new session instead of falling off it.
fn follow(app: &mut App, old: &AgentId, new: AgentId) {
    let showing_old = app
        .pane
        .term
        .as_ref()
        .is_some_and(|t| t.sref == SessionRef::Agent(old.clone()));
    if !showing_old {
        return;
    }
    let sref = SessionRef::Agent(new);
    if let Some(leaving) = app.pane.term.take() {
        let (cols, rows) = (leaving.cols, leaving.rows);
        app.stash_term(leaving);
        let term = app
            .take_cached_term(&sref)
            .unwrap_or_else(|| AttachedTerm::new(sref.clone(), cols, rows));
        app.pane.term = Some(term);
    }
    app.pane.term_selection = None;
    app.pane.pending_attach = Some((sref, std::time::Instant::now()));
}

/// The note a renewal to `new` started from: the renewal log's last line
/// (the daemon writes it before it tells clients), by file name. None when
/// the log is not on this machine or names another renewal.
fn note_of(new: &AgentId) -> Option<String> {
    let path = nebula_core::paths::data_dir().join("orchestrator-renewals.jsonl");
    let text = std::fs::read_to_string(path).ok()?;
    note_in(text.lines().last()?, new)
}

fn note_in(line: &str, new: &AgentId) -> Option<String> {
    let line: serde_json::Value = serde_json::from_str(line).ok()?;
    if line["new"].as_str()? != new.as_str() {
        return None;
    }
    let note = Path::new(line["note"].as_str()?);
    Some(note.file_name()?.to_string_lossy().into_owned())
}

fn absolute(note: &Path, cwd: &Path) -> PathBuf {
    if note.is_absolute() {
        note.to_path_buf()
    } else {
        cwd.join(note)
    }
}

fn scheduled_flash(state: &RenewState) -> String {
    match state {
        RenewState::Scheduled => {
            "Renewal scheduled: a fresh orchestrator starts once this turn ends".into()
        }
        RenewState::Done => "Orchestrator renewed".into(),
        RenewState::Failed { reason } => format!("Orchestrator renewal failed: {reason}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nebula_core::{Agent, AgentKind, AgentRole, AgentStatus, WorktreeId};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn agent(id: &str, role: AgentRole, archived: bool) -> Agent {
        Agent {
            parent: None,
            profile: None,
            id: AgentId(id.into()),
            worktree_id: WorktreeId("w1".into()),
            name: id.into(),
            status: AgentStatus::Running,
            archived,
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
            alive: true,
            issue_url: None,
            role,
            recent_prompts: Vec::new(),
        }
    }

    fn sref(id: &str) -> SessionRef {
        SessionRef::Agent(AgentId(id.into()))
    }

    /// The orchestrator view on `old`, then `new` holding the role and
    /// `old` archived, as the daemon's upserts leave the tree.
    fn renewed_app() -> App {
        let mut app = App::new();
        app.tree.agents = vec![
            agent("old", AgentRole::Worker, true),
            agent("new", AgentRole::Orchestrator, false),
        ];
        app.pane.term = Some(AttachedTerm::new(sref("old"), 80, 20));
        app.pane.collapsed = true;
        app.orch.view = true;
        app
    }

    #[test]
    fn on_done_the_view_follows_the_new_orchestrator() {
        let mut app = renewed_app();
        on_renewal(
            &mut app,
            None,
            AgentId("old".into()),
            Some(AgentId("new".into())),
            RenewState::Done,
        );
        let term = app.pane.term.as_ref().unwrap();
        assert_eq!(term.sref, sref("new"));
        assert_eq!((term.cols, term.rows), (80, 20));
        assert_eq!(
            app.pane.pending_attach.as_ref().map(|(s, _)| s.clone()),
            Some(sref("new"))
        );
        assert!(crate::orchestrator_view::active(&app));
        assert!(app
            .chrome
            .flash
            .as_deref()
            .unwrap()
            .starts_with("Orchestrator renewed"));
    }

    #[test]
    fn a_pane_on_another_session_stays_put() {
        let mut app = renewed_app();
        app.pane.term = Some(AttachedTerm::new(sref("other"), 80, 20));
        on_renewal(
            &mut app,
            None,
            AgentId("old".into()),
            Some(AgentId("new".into())),
            RenewState::Done,
        );
        assert_eq!(app.pane.term.as_ref().unwrap().sref, sref("other"));
        assert!(app.pane.pending_attach.is_none());
    }

    #[test]
    fn a_failure_flashes_the_reason_and_moves_nothing() {
        let mut app = renewed_app();
        on_renewal(
            &mut app,
            None,
            AgentId("old".into()),
            None,
            RenewState::Failed {
                reason: "the new orchestrator sent no hook within 60 s".into(),
            },
        );
        assert_eq!(app.pane.term.as_ref().unwrap().sref, sref("old"));
        assert_eq!(
            app.chrome.flash.as_deref(),
            Some("Orchestrator renewal failed: the new orchestrator sent no hook within 60 s")
        );
    }

    /// The flash is drawn at 80x24, the smallest size the TUI supports.
    #[test]
    fn the_flash_is_drawn_at_80_by_24() {
        let mut app = App::new();
        on_renewal(
            &mut app,
            None,
            AgentId("old".into()),
            None,
            RenewState::Failed {
                reason: "turn did not end".into(),
            },
        );
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|f| crate::ui::draw(f, &mut app)).unwrap();
        let screen: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(
            screen.contains("Orchestrator renewal failed: turn did not end"),
            "{screen}"
        );
    }

    #[test]
    fn the_log_names_the_note_only_for_this_renewal() {
        let line = r#"{"at":1,"old":"a","new":"b","note":"C:\\ws\\handover.md","state":"done","reason":null}"#;
        let line = if cfg!(windows) {
            line.to_string()
        } else {
            line.replace("C:\\\\ws\\\\", "/ws/")
        };
        assert_eq!(
            note_in(&line, &AgentId("b".into())).as_deref(),
            Some("handover.md")
        );
        assert_eq!(note_in(&line, &AgentId("c".into())), None);
        assert_eq!(note_in("not json", &AgentId("b".into())), None);
    }

    #[test]
    fn an_empty_note_is_refused_and_a_relative_one_made_absolute() {
        let mut app = App::new();
        submit(&mut app, "   ".into());
        assert_eq!(
            app.chrome.flash.as_deref(),
            Some("Renew orchestrator: name the handover note")
        );
        let cwd = std::env::temp_dir();
        assert_eq!(absolute(Path::new("h.md"), &cwd), cwd.join("h.md"));
    }
}
