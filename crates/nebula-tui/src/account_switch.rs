//! MOVE TO ACCOUNT: a session restarted on another ACCOUNT of its harness
//! (`ClientRequest::SwitchAgentAccount`), from a card's menu, the pane
//! header's account chip, or `⇧U`'s row. The DAEMON kills the CLI and
//! boots it again in the same worktree, FRESH: each account keeps its
//! conversations in its own config home, so the old one cannot be resumed
//! there. The choices and the confirm's words are the desktop's
//! (`desktop/src/lib/accounts/view.ts`).

use crate::accounts_overlay::AccountRow;
use crate::app::{
    App, ConfirmDialog, ContextMenu, MenuAction, MenuItem, Overlay, PendingAction, PendingIntent,
};
use nebula_core::accounts::{session_account, session_harness};
use nebula_core::{AgentId, ClientRequest, SWITCH_ACCOUNT_PROTOCOL};

/// What a move says to a daemon too old to take one.
#[cfg(unix)]
pub const DAEMON_TOO_OLD: &str =
    "the running daemon predates moving a session to another account: run `kete reload` (every session keeps running), then reopen kete";
/// Windows daemons can't restart in place: `kete reload` refuses there.
#[cfg(windows)]
pub const DAEMON_TOO_OLD: &str =
    "the running daemon predates moving a session to another account: run `kete kill` (stops all sessions), then reopen kete";

/// Can the daemon take `SwitchAgentAccount`? Below
/// [`SWITCH_ACCOUNT_PROTOCOL`] it cannot decode the request and would drop
/// the connection on it. None (no handshake yet: tests) counts as yes, as
/// `order::daemon_reorders` does.
pub fn daemon_switches(app: &App) -> bool {
    app.orch
        .daemon_protocol
        .is_none_or(|v| v >= SWITCH_ACCOUNT_PROTOCOL)
}

/// One account a session can move to: a profile of its harness, or the
/// harness's own login (`profile` None).
#[derive(Debug, Clone, PartialEq)]
pub struct SwitchTarget {
    pub profile: Option<String>,
    pub label: String,
    /// Switched off: no launch picks it, but a move may name it.
    pub off: bool,
    /// Not signed in: the move still goes, and the CLI asks for a login.
    pub needs_login: bool,
}

/// Where `agent` can move: every usable profile of its harness but the one
/// it runs on, switched-on ones first, then its harness's own login unless
/// it already runs there. `rows` are the accounts list's
/// (`accounts_overlay::build_rows`).
pub fn switch_targets(agent: &nebula_core::Agent, rows: &[AccountRow]) -> Vec<SwitchTarget> {
    let harness = session_harness(agent);
    let current = session_account(agent);
    let needs = |r: &AccountRow| r.login == nebula_core::accounts::LoginState::NeedsLogin;
    let mut out: Vec<SwitchTarget> = rows
        .iter()
        .filter(|r| r.profile.is_some() && r.problem.is_none())
        .filter(|r| r.harness == harness && r.key != current)
        .map(|r| {
            let mut label = r.key.clone();
            if r.default {
                label.push_str(" (default)");
            }
            if r.disabled() {
                label.push_str(" (off)");
            }
            if needs(r) {
                label.push_str(" (not signed in)");
            }
            SwitchTarget {
                profile: Some(r.key.clone()),
                label,
                off: r.disabled(),
                needs_login: needs(r),
            }
        })
        .collect();
    out.sort_by_key(|t| t.off);
    let own = nebula_core::usage::account_key(None, &harness);
    if current != own {
        out.push(SwitchTarget {
            profile: None,
            label: format!("{harness}'s own login"),
            off: false,
            needs_login: rows.iter().any(|r| r.key == own && needs(r)),
        });
    }
    out
}

/// The move's confirm, true to what the daemon does: a fresh start in the
/// same worktree, since the conversation lives in the old account's config
/// home. `(title, message)`.
pub fn confirm_text(agent: &nebula_core::Agent, to: &SwitchTarget) -> (String, String) {
    let harness = session_harness(agent);
    let own = format!("{harness}'s own login");
    let from = agent
        .profile
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map_or(own.clone(), str::to_string);
    let to_name = to.profile.clone().unwrap_or(own);
    let verb = if agent.alive {
        "Stop the agent and start"
    } else {
        "Start"
    };
    let mut message = format!(
        "{verb} '{}' fresh on {to_name} (from {from}), in the same worktree. It does not continue the conversation: each account keeps its conversations in its own config home, so {harness} cannot resume this one there. The worktree, its files and the session row stay.",
        agent.name
    );
    if to.needs_login {
        message.push_str(&format!(
            " {to_name} is not signed in: the new session asks for a login first."
        ));
    }
    if to.off {
        message.push_str(&format!(
            " {to_name} is switched off: no launch picks it, but this move names it."
        ));
    }
    (format!("Move to {to_name}"), message)
}

/// The accounts list as the move reads it, from what the app holds.
fn rows(app: &App) -> Vec<AccountRow> {
    crate::accounts_overlay::build_rows(
        &app.launcher.profiles,
        &app.launcher.account_usage,
        &app.tree.agents,
        crate::accounts_overlay::login_lookup,
    )
}

/// Why session `id` cannot move, for the flash; None when it can.
fn refusal(app: &App, id: &AgentId) -> Option<String> {
    let Some(agent) = app.tree.agents.iter().find(|a| &a.id == id) else {
        return Some("that session is gone".into());
    };
    if agent.archived {
        return Some("an archived session stays on its account: unarchive it first".into());
    }
    if agent.cloud_session_url().is_some() {
        return Some("a cloud session runs on claude.ai's account, not one of kete's".into());
    }
    if !daemon_switches(app) {
        return Some(DAEMON_TOO_OLD.into());
    }
    None
}

/// "Move to account…": a picker of the accounts session `id` can move to;
/// a pick opens the confirm ([`confirm`]).
pub fn open_picker(app: &mut App, id: AgentId) {
    if let Some(why) = refusal(app, &id) {
        app.chrome.flash = Some(why);
        app.chrome.dirty = true;
        return;
    }
    crate::quick_prompt::refresh_profiles(app);
    let Some(agent) = app.tree.agents.iter().find(|a| a.id == id).cloned() else {
        return;
    };
    let harness = session_harness(&agent);
    let targets = switch_targets(&agent, &rows(app));
    if targets.is_empty() {
        app.chrome.flash = Some(format!(
            "no other {harness} account: add one in Accounts (⇧U)"
        ));
        app.chrome.dirty = true;
        return;
    }
    let items: Vec<MenuItem> = targets
        .into_iter()
        .map(|t| {
            MenuItem::new(
                format!("Restart on {}…", t.label),
                MenuAction::SwitchAccount {
                    id: id.clone(),
                    profile: t.profile,
                },
            )
        })
        .collect();
    let on = agent
        .profile
        .as_deref()
        .filter(|p| !p.trim().is_empty())
        .unwrap_or("its own login");
    app.modals.overlay = Some(Overlay::Menu(ContextMenu {
        title: Some(format!("Move '{}' ({harness} · {on})", agent.name)),
        items,
        at: None,
        hover: 0,
        area: ratatui::layout::Rect::default(),
        scroll: 0,
        parent: None,
        filter: None,
    }));
    app.chrome.dirty = true;
}

/// A picker row: the confirm, saying the session restarts fresh.
pub fn confirm(app: &mut App, id: AgentId, profile: Option<String>) {
    if let Some(why) = refusal(app, &id) {
        app.chrome.flash = Some(why);
        app.chrome.dirty = true;
        return;
    }
    let Some(agent) = app.tree.agents.iter().find(|a| a.id == id).cloned() else {
        return;
    };
    // Only an account the session can move to: one of its harness's, not
    // the one it is on, not broken.
    let Some(target) = switch_targets(&agent, &rows(app))
        .into_iter()
        .find(|t| t.profile == profile)
    else {
        app.chrome.flash = Some(format!(
            "{} cannot move to {}: pick another {} account",
            agent.name,
            profile.as_deref().unwrap_or("its own login"),
            session_harness(&agent)
        ));
        app.chrome.dirty = true;
        return;
    };
    let (title, message) = confirm_text(&agent, &target);
    app.modals.overlay = Some(Overlay::Confirm(ConfirmDialog {
        title,
        message,
        action: PendingAction::SwitchAccount { id, profile },
        area: ratatui::layout::Rect::default(),
    }));
    app.chrome.dirty = true;
}

/// The confirm answered: queue the move. A daemon too old for it gets
/// nothing and the flash says what to run; false then.
pub fn send(
    app: &mut App,
    id: AgentId,
    profile: Option<String>,
    out: &mut Vec<ClientRequest>,
) -> bool {
    if let Some(why) = refusal(app, &id) {
        app.chrome.flash = Some(why);
        app.chrome.dirty = true;
        return false;
    }
    let profile = profile
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty());
    let name = app
        .tree
        .agents
        .iter()
        .find(|a| a.id == id)
        .map(|a| (a.name.clone(), session_harness(a)));
    let req_id = app.alloc_req_id(PendingIntent::None);
    if let Some((name, harness)) = name {
        let on = profile
            .clone()
            .unwrap_or_else(|| format!("{harness}'s own login"));
        app.chrome.flash = Some(format!("restarting {name} fresh on {on}"));
    }
    out.push(ClientRequest::SwitchAgentAccount {
        req_id,
        id,
        profile,
    });
    app.chrome.dirty = true;
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts_overlay::build_rows;
    use nebula_core::accounts::{LoginCheck, LoginState};
    use nebula_core::profiles::Profile;
    use std::collections::BTreeMap;

    fn profile(name: &str, harness: &str) -> Profile {
        Profile {
            name: name.into(),
            harness: harness.into(),
            env: BTreeMap::new(),
            default: false,
            use_default: false,
            disabled: false,
            reserved: false,
        }
    }

    fn agent(name: &str, profile: Option<&str>) -> nebula_core::Agent {
        nebula_core::Agent {
            parent: None,
            profile: profile.map(str::to_string),
            id: AgentId(name.into()),
            worktree_id: nebula_core::WorktreeId("w".into()),
            name: name.into(),
            status: nebula_core::AgentStatus::Running,
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
            alive: true,
            issue_url: None,
            role: nebula_core::AgentRole::Worker,
            recent_prompts: Vec::new(),
        }
    }

    fn signed_out(p: Option<&Profile>, _: &str) -> LoginCheck {
        LoginCheck {
            state: if p.is_some_and(|p| p.name == "home") {
                LoginState::NeedsLogin
            } else {
                LoginState::LoggedIn
            },
            expired: false,
        }
    }

    /// Every other usable account of the session's harness, switched-off
    /// last, then its own login; never the one it runs on, a broken one or
    /// another harness's.
    #[test]
    fn targets_are_the_other_accounts_of_the_harness() {
        let mut off = profile("off", "claude");
        off.disabled = true;
        let all = vec![
            off,
            profile("work", "claude"),
            profile("home", "claude"),
            profile("gpt", "codex"),
            profile("bad name", "claude"),
        ];
        let a = agent("fix", Some("work"));
        let rows = build_rows(&all, &[], std::slice::from_ref(&a), signed_out);
        let targets = switch_targets(&a, &rows);
        let names: Vec<Option<&str>> = targets.iter().map(|t| t.profile.as_deref()).collect();
        assert_eq!(names, [Some("home"), Some("off"), None]);
        assert_eq!(targets[0].label, "home (not signed in)");
        assert!(targets[0].needs_login && targets[1].off);
        assert_eq!(targets[2].label, "claude's own login");

        // On its own login: that one is not offered again.
        let bare = agent("bare", None);
        let rows = build_rows(&all, &[], std::slice::from_ref(&bare), signed_out);
        let targets = switch_targets(&bare, &rows);
        assert!(targets.iter().all(|t| t.profile.is_some()), "{targets:?}");
    }

    /// The confirm says what the daemon does: fresh, same worktree, no
    /// conversation carried over.
    #[test]
    fn the_confirm_says_it_restarts_fresh_in_the_same_worktree() {
        let a = agent("fix", Some("work"));
        let to = SwitchTarget {
            profile: Some("home".into()),
            label: "home".into(),
            off: true,
            needs_login: true,
        };
        let (title, message) = confirm_text(&a, &to);
        assert_eq!(title, "Move to home");
        assert!(message.contains("fresh on home (from work)"), "{message}");
        assert!(message.contains("in the same worktree"), "{message}");
        assert!(
            message.contains("does not continue the conversation"),
            "{message}"
        );
        assert!(message.contains("not signed in"), "{message}");
        assert!(message.contains("switched off"), "{message}");
        let own = SwitchTarget {
            profile: None,
            label: String::new(),
            off: false,
            needs_login: false,
        };
        let (title, _) = confirm_text(&a, &own);
        assert_eq!(title, "Move to claude's own login");
    }

    /// A 56 daemon never gets the request (it would drop the connection):
    /// the flash names the fix. A 57 one gets it, the profile trimmed.
    #[test]
    fn a_switch_goes_only_to_a_daemon_that_takes_it() {
        let mut app = App::new();
        app.tree.agents.push(agent("fix", Some("work")));
        let id = AgentId("fix".into());
        let mut out = Vec::new();

        app.orch.daemon_protocol = Some(SWITCH_ACCOUNT_PROTOCOL - 1);
        assert!(!send(&mut app, id.clone(), Some("home".into()), &mut out));
        assert!(out.is_empty(), "an older daemon would drop the connection");
        assert_eq!(app.chrome.flash.as_deref(), Some(DAEMON_TOO_OLD));
        open_picker(&mut app, id.clone());
        assert!(app.modals.overlay.is_none(), "no picker for an old daemon");
        confirm(&mut app, id.clone(), None);
        assert!(app.modals.overlay.is_none(), "no confirm either");

        app.orch.daemon_protocol = Some(SWITCH_ACCOUNT_PROTOCOL);
        for (asked, sent) in [
            (Some(" home "), Some("home")),
            (Some(""), None),
            (None, None),
        ] {
            out.clear();
            assert!(send(
                &mut app,
                id.clone(),
                asked.map(str::to_string),
                &mut out
            ));
            match out.as_slice() {
                [ClientRequest::SwitchAgentAccount {
                    id: got, profile, ..
                }] => assert_eq!((got, profile.as_deref()), (&id, sent)),
                other => panic!("expected one SwitchAgentAccount, got {other:?}"),
            }
        }
    }
}
