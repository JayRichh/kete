//! `kete studio note` (K6): file one inbox note and, with `--triage`, hand
//! it to the triage desk.
//!
//! The daemon side is the TUI strip's (`nebula_tui::triage_strip::
//! file_and_send`): the note is written, then one turn goes to the repo's
//! desk (or the orchestrator, labelled `[triage]`) and its one line is
//! printed. A note that repeats an open one exits 3, naming it.

use anyhow::{bail, Result};
use nebula_core::studio::inbox::Priority;
use nebula_core::studio::inbox_write::{NewNote, Target};
use nebula_tui::triage_strip::{file_and_send, outcome_line, Outcome};

/// Everything `kete studio note` takes.
#[derive(Debug, Clone)]
pub struct NoteArgs {
    /// The note, its words joined.
    pub text: String,
    pub project: Option<String>,
    pub to: Option<String>,
    pub about: Option<String>,
    /// `high`, `normal` or `low`.
    pub priority: Option<String>,
    /// Send it to the triage desk to route.
    pub triage: bool,
    pub json: bool,
}

/// The exit code for a note that repeats an open one.
const DUPLICATE_EXIT: i32 = 3;

pub fn run(args: NoteArgs) -> Result<()> {
    let priority = match args.priority.as_deref() {
        None => None,
        Some(word) => match Priority::parse(word) {
            Some(p) => Some(p),
            None => bail!("--priority is high, normal or low"),
        },
    };
    let project = match args.project {
        Some(p) => p,
        None => std::env::current_dir()?.display().to_string(),
    };
    let note = NewNote {
        text: args.text,
        to: args.to,
        about: args.about,
        priority,
        by: None,
    };
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let outcome = rt.block_on(file_and_send(&project, note, args.triage, Target::Desk))?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&json(&outcome))?);
    } else if let Outcome::Duplicate(_) = outcome {
        eprintln!("{}", outcome_line(&outcome));
    } else {
        println!("{}", outcome_line(&outcome));
    }
    if matches!(outcome, Outcome::Duplicate(_)) {
        std::process::exit(DUPLICATE_EXIT);
    }
    Ok(())
}

/// The `--json` shape: the filed note and where it went, or the open note
/// it repeats.
fn json(outcome: &Outcome) -> serde_json::Value {
    match outcome {
        Outcome::Filed(sent) => {
            let mut v = serde_json::to_value(sent).unwrap_or_default();
            v["kind"] = "filed".into();
            v
        }
        Outcome::Duplicate(d) => serde_json::json!({
            "kind": "duplicate",
            "path": d.existing,
            "reason": d.reason,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nebula_core::studio::inbox_write::Duplicate;
    use nebula_tui::triage_strip::Sent;

    #[test]
    fn json_names_the_kind() {
        let filed = json(&Outcome::Filed(Sent {
            id: "a".into(),
            path: "/r/a.md".into(),
            to: Some("desk"),
            route: Some("art".into()),
            busy: false,
            unsent: None,
        }));
        assert_eq!(filed["kind"], "filed");
        assert_eq!(filed["to"], "desk");
        assert_eq!(filed["route"], "art");
        let dup = json(&Outcome::Duplicate(Duplicate {
            existing: "/r/b.md".into(),
            reason: "why".into(),
        }));
        assert_eq!(dup["kind"], "duplicate");
        assert_eq!(dup["path"], "/r/b.md");
    }
}
