//! `kete orchestrator renew --from <note>` (K9): hand the orchestrator over
//! to a fresh session that starts on a handover note.
//!
//! Run inside the orchestrator's session it renews itself (its
//! `NEBULA_AGENT_ID` is the caller); run from a person's shell it renews
//! the pinned orchestrator. The daemon checks both, and the note, and
//! answers `Scheduled`: the swap itself happens once the current turn
//! ends, and its outcome reaches every client.

use anyhow::{bail, Context, Result};
use nebula_core::studio::wire::RenewState;
use nebula_core::AgentId;
use nebula_tui::ipc_studio::{renew_orchestrator, RenewRequestArgs};
use std::path::{Path, PathBuf};

/// Everything `kete orchestrator renew` takes.
#[derive(Debug, Clone)]
pub struct RenewArgs {
    /// The handover note.
    pub from: PathBuf,
    pub profile: Option<String>,
    /// Let kete pick the account (the automation pick).
    pub pick: bool,
    pub model: Option<String>,
    pub keep_old: bool,
    pub now: bool,
    pub json: bool,
}

pub fn run(args: RenewArgs) -> Result<()> {
    let note = absolute(&args.from, &std::env::current_dir()?);
    let profile = match (args.pick, args.profile) {
        (true, _) => Some(pick_account()?),
        (false, profile) => profile,
    };
    let req = RenewRequestArgs {
        caller: nebula_core::env::non_empty(nebula_core::env::AGENT_ID).map(AgentId),
        note: note.clone(),
        profile: profile.clone(),
        model: args.model,
        keep_old: args.keep_old,
        now: args.now,
    };
    let state = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(renew_orchestrator(req))?;
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "state": state_word(&state),
                "note": note.display().to_string(),
                "profile": profile,
            }))?
        );
    } else {
        println!("{}", scheduled_line(&state, args.now));
    }
    Ok(())
}

/// The note as the daemon must see it: the daemon's working folder is not
/// the caller's.
fn absolute(note: &Path, cwd: &Path) -> PathBuf {
    if note.is_absolute() {
        note.to_path_buf()
    } else {
        cwd.join(note)
    }
}

fn state_word(state: &RenewState) -> &'static str {
    match state {
        RenewState::Scheduled => "scheduled",
        RenewState::Done => "done",
        RenewState::Failed { .. } => "failed",
    }
}

fn scheduled_line(state: &RenewState, now: bool) -> String {
    match state {
        RenewState::Scheduled if now => {
            "Renewal scheduled: a fresh orchestrator starts on the note now.".into()
        }
        RenewState::Scheduled => "Renewal scheduled: a fresh orchestrator starts on the note \
                                  once this turn ends. End your turn now."
            .into(),
        RenewState::Done => "Orchestrator renewed.".into(),
        RenewState::Failed { reason } => format!("Renewal failed: {reason}"),
    }
}

/// Why `--pick` refuses.
const NO_ACCOUNT: &str = "--pick found no eligible claude account: every one is reserved or \
                          switched off, or none is set up (kete profile list)";

/// `--pick`: the account `kete profile pick` would name for a Claude
/// launch the orchestrator starts, counting live sessions and holds as
/// load, and held like its pick so the next one spreads. Refused, with
/// the reason, when no account is eligible.
fn pick_account() -> Result<String> {
    let root = nebula_core::profiles::profiles_root();
    let now = nebula_core::clock::now_secs() as i64;
    let live = crate::profile::live_sessions_per_account();
    let all = nebula_core::profiles::load();
    let policy = nebula_core::accounts::load_policy();
    let Some(name) = choose(&all, policy, &root, now, &live) else {
        bail!("{NO_ACCOUNT}");
    };
    nebula_core::accounts::holds::add_in(&root, &name, now).context("hold the pick")?;
    Ok(name)
}

// shortcut: picks for Claude, the only harness role cards and the
// orchestrator's guidance target in 0.60; ask the daemon for the old
// orchestrator's harness if renewals reach other harnesses.
fn choose(
    all: &[nebula_core::profiles::Profile],
    policy: nebula_core::accounts::Policy,
    root: &Path,
    now: i64,
    live: &std::collections::BTreeMap<String, u32>,
) -> Option<String> {
    use nebula_core::accounts::{self, holds, Launcher};
    let held = holds::active_in(root, now);
    let count = |map: &std::collections::BTreeMap<String, u32>, name: &str| {
        map.get(name).copied().unwrap_or(0)
    };
    accounts::pick_for(
        all,
        "claude",
        None,
        policy,
        Launcher::Automation,
        now,
        |name| nebula_core::usage::read_account_in(root, name),
        |name| count(live, name) + count(&held, name),
    )
    .profile
    .map(|p| p.name.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_relative_note_is_read_from_the_callers_folder() {
        let cwd = std::env::temp_dir();
        assert_eq!(
            absolute(Path::new("handover.md"), &cwd),
            cwd.join("handover.md")
        );
        let abs = cwd.join("x").join("note.md");
        assert_eq!(absolute(&abs, Path::new("elsewhere")), abs);
    }

    #[test]
    fn the_line_tells_the_orchestrator_to_end_its_turn_unless_now() {
        assert!(scheduled_line(&RenewState::Scheduled, false).contains("End your turn"));
        assert!(!scheduled_line(&RenewState::Scheduled, true).contains("End your turn"));
        assert_eq!(state_word(&RenewState::Scheduled), "scheduled");
    }

    #[test]
    fn the_pick_counts_live_sessions_as_load() {
        let p = |name: &str| nebula_core::profiles::Profile {
            name: name.into(),
            harness: "claude".into(),
            env: Default::default(),
            default: false,
            use_default: false,
            disabled: false,
            reserved: false,
        };
        let all = vec![p("alpha"), p("beta")];
        let root = std::env::temp_dir().join(format!("kete-renew-pick-{}", std::process::id()));
        let policy = nebula_core::accounts::Policy::default();
        let idle = std::collections::BTreeMap::new();
        assert_eq!(
            choose(&all, policy, &root, 0, &idle).as_deref(),
            Some("alpha")
        );
        let busy = std::collections::BTreeMap::from([("alpha".to_string(), 2)]);
        assert_eq!(
            choose(&all, policy, &root, 0, &busy).as_deref(),
            Some("beta")
        );
    }

    #[test]
    fn the_refusal_reads_as_one_sentence() {
        assert!(!NO_ACCOUNT.contains("  "), "{NO_ACCOUNT}");
        assert!(NO_ACCOUNT.contains("switched off, or none is set up"));
    }
}
