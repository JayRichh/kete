//! A session's follow-ups that run as sessions of their own: ^N in the
//! follow-up box starts the turn beside the card's session, and ASK ASIDE
//! asks a side question there. Both are `SpawnSiblingAgent` from the card.

use super::*;
use crate::keymap::Action;
use nebula_core::AgentId;

/// The spawn `out` holds: its req_id, the session it came from and its
/// first prompt.
fn spawned(out: &[ClientRequest]) -> (u64, AgentId, String) {
    out.iter()
        .find_map(|r| match r {
            ClientRequest::SpawnSiblingAgent {
                req_id,
                id,
                kind: None,
                starting_prompt,
                worktree: None,
                base: None,
                profile: None,
            } => Some((*req_id, id.clone(), starting_prompt.clone())),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no spawn in {out:?}"))
}

fn type_text(app: &mut App, text: &str, out: &mut Vec<ClientRequest>) {
    for c in text.chars() {
        press(app, KeyCode::Char(c), KeyModifiers::NONE, out);
    }
}

#[test]
fn ctrl_n_in_the_follow_up_box_starts_the_turn_as_a_session_beside_the_card() {
    let mut app = follow_up_app();
    let mut out = Vec::new();
    press(&mut app, KeyCode::Char(' '), KeyModifiers::NONE, &mut out);
    type_text(&mut app, "write the tests", &mut out);
    out.clear();
    press(
        &mut app,
        KeyCode::Char('n'),
        KeyModifiers::CONTROL,
        &mut out,
    );

    let (req_id, from, prompt) = spawned(&out);
    assert_eq!(
        (from.0.as_str(), prompt.as_str()),
        ("a1", "write the tests")
    );
    assert!(
        !out.iter().any(|r| matches!(r, ClientRequest::Input { .. })),
        "nothing typed into the card's own session: {out:?}"
    );
    assert!(follow_up_text(&app).is_none(), "the box closes");

    // Refused: the box comes back on the card with the turn in it.
    hse(
        &mut app,
        ServerEvent::Error {
            req_id: Some(req_id),
            message: "agent is archived".into(),
        },
    );
    assert_eq!(follow_up_text(&app).as_deref(), Some("write the tests"));
}

#[test]
fn ctrl_n_in_the_card_composer_starts_a_session_and_folds_the_card() {
    let mut app = follow_up_app();
    app.modals.follow_up = Some(crate::app::FollowUp {
        agent: AgentId("a1".into()),
        input: crate::text_input::TextInput::multiline_with_text("check the logs"),
    });
    let mut out = Vec::new();
    let key = KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL);
    assert!(crate::event_loop::follow_up_key(&mut app, key, &mut out));
    let (_, from, prompt) = spawned(&out);
    assert_eq!((from.0.as_str(), prompt.as_str()), ("a1", "check the logs"));
    assert!(app.modals.follow_up.is_none());
}

#[test]
fn ask_aside_frames_the_question_and_spawns_beside_the_session() {
    let mut app = follow_up_app();
    let mut out = Vec::new();
    run_action(&mut app, Action::AskAside, &mut out);
    let Some(Overlay::Prompt(p)) = &app.modals.overlay else {
        panic!("no box: {:?}", app.modals.overlay);
    };
    assert_eq!(
        p.kind,
        PromptKind::AskAside {
            id: AgentId("a1".into())
        }
    );
    type_text(&mut app, "why two refreshes?", &mut out);
    out.clear();
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE, &mut out);

    let (_, from, prompt) = spawned(&out);
    assert_eq!(from.0, "a1");
    assert_eq!(
        prompt,
        nebula_core::follow_up::aside_prompt("agent-1", "why two refreshes?")
    );
    assert!(app.modals.overlay.is_none());
}

#[test]
fn an_archived_session_starts_nothing_and_says_why() {
    let mut app = follow_up_app();
    app.tree.agents[0].archived = true;
    let mut out = Vec::new();
    let sent = crate::event_loop::start_follow_up_session(
        &mut app,
        &AgentId("a1".into()),
        "go",
        PromptKind::FollowUp {
            id: AgentId("a1".into()),
        },
        &mut out,
    );
    assert!(!sent);
    assert!(out.is_empty(), "{out:?}");
    assert!(app
        .chrome
        .flash
        .as_deref()
        .is_some_and(|f| f.contains("archived")));
}
