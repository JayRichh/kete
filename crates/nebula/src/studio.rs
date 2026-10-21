//! `kete studio` (K2): turn a repo's studio mode on and off, set its
//! autonomy and exclusions, and see what the queue runner would send.
//!
//! Every change goes through the daemon (`SetStudio`), which refuses it
//! from inside an agent session: only a person changes what a studio may
//! do. Reads of the current entry come straight from `config.local.json`.
//! `kete studio note` is the triage lane's `studio_note.rs`.

use crate::cli::StudioCommand;
use anyhow::{bail, Result};
use nebula_core::studio::wire::StudioQueueRow;
use nebula_core::studio::{settings, Autonomy, StudioSettings};
use nebula_core::Project;
use nebula_tui::ipc_studio;
use std::path::Path;

pub fn run(command: StudioCommand) -> Result<()> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    rt.block_on(run_async(command))
}

async fn run_async(command: StudioCommand) -> Result<()> {
    match command {
        StudioCommand::Enable { project, inbox } => {
            let project = find(&project).await?;
            let current = settings::for_repo(&project.repo_path);
            let studio = match (current, inbox) {
                (Some(current), None) => {
                    println!(
                        "Studio is already on for {} at {}, inbox {}.",
                        project.name, current.autonomy, current.studio_inbox
                    );
                    return Ok(());
                }
                (Some(current), Some(inbox)) => StudioSettings {
                    studio_inbox: inbox,
                    ..current
                },
                (None, inbox) => StudioSettings {
                    studio_inbox: inbox.unwrap_or_default(),
                    autonomy: Autonomy::A1,
                    queue_exclude: Vec::new(),
                },
            };
            ipc_studio::set_studio(project.id.clone(), Some(studio)).await?;
            let now = settings::for_repo(&project.repo_path).unwrap_or_default();
            println!(
                "Studio on for {} at {}, inbox {}.",
                project.name, now.autonomy, now.studio_inbox
            );
        }
        StudioCommand::Disable { project } => {
            let project = find(&project).await?;
            ipc_studio::set_studio(project.id.clone(), None).await?;
            println!("Studio off for {}.", project.name);
        }
        StudioCommand::Status { json } => {
            let rows = ipc_studio::studio_queue(None, false).await?;
            if json {
                println!("{}", serde_json::to_string_pretty(&rows)?);
            } else if rows.is_empty() {
                println!("No repo has studio mode. `kete studio enable <repo>` turns it on.");
            } else {
                for row in &rows {
                    print!("{}", status_text(row, nebula_core::clock::now_ms()));
                }
            }
        }
        StudioCommand::Autonomy { project, level } => {
            let project = find(&project).await?;
            let current = entry(&project)?;
            let Some(level) = level else {
                println!("{}: {}", project.name, current.autonomy);
                return Ok(());
            };
            let Some(level) = Autonomy::parse(&level) else {
                bail!("autonomy {level}: expected A0, A1, A2 or A3");
            };
            let studio = StudioSettings {
                autonomy: level,
                ..current
            };
            ipc_studio::set_studio(project.id.clone(), Some(studio)).await?;
            println!("{}: {level}", project.name);
        }
        StudioCommand::Exclude { project, value } => {
            let project = find(&project).await?;
            let mut studio = entry(&project)?;
            let value = value.trim().to_string();
            if studio
                .queue_exclude
                .iter()
                .any(|v| v.trim().eq_ignore_ascii_case(&value))
            {
                println!("{}: {value} is already left alone.", project.name);
                return Ok(());
            }
            studio.queue_exclude.push(value.clone());
            ipc_studio::set_studio(project.id.clone(), Some(studio)).await?;
            println!("{}: the queue leaves {value} alone.", project.name);
        }
        StudioCommand::Include { project, value } => {
            let project = find(&project).await?;
            let mut studio = entry(&project)?;
            let before = studio.queue_exclude.len();
            studio
                .queue_exclude
                .retain(|v| !v.trim().eq_ignore_ascii_case(value.trim()));
            if studio.queue_exclude.len() == before {
                println!("{}: {} was not excluded.", project.name, value.trim());
                return Ok(());
            }
            ipc_studio::set_studio(project.id.clone(), Some(studio)).await?;
            println!("{}: the queue takes {} again.", project.name, value.trim());
        }
        StudioCommand::Queue { project, now, json } => {
            let id = match &project {
                Some(p) => Some(find(p).await?.id),
                None => None,
            };
            let before = ipc_studio::studio_queue(id.clone(), false).await?;
            let rows = if now {
                ipc_studio::studio_queue(id, true).await?
            } else {
                before.clone()
            };
            if json {
                println!("{}", serde_json::to_string_pretty(&rows)?);
                return Ok(());
            }
            if rows.is_empty() {
                println!("No repo has studio mode. `kete studio enable <repo>` turns it on.");
                return Ok(());
            }
            for row in &rows {
                let was = before.iter().find(|b| b.project == row.project);
                println!("{}", queue_text(row, was, now));
            }
        }
        StudioCommand::Note { .. } => unreachable!("main.rs hands `note` to studio_note.rs"),
    }
    Ok(())
}

/// `project`'s studio entry, or the way to turn studio mode on.
fn entry(project: &Project) -> Result<StudioSettings> {
    match settings::for_repo(&project.repo_path) {
        Some(s) => Ok(s),
        None => bail!(
            "studio is off for {0}: `kete studio enable {0}` turns it on",
            project.name
        ),
    }
}

/// What `kete studio status` prints for one repo.
fn status_text(row: &StudioQueueRow, now_ms: i64) -> String {
    let mut out = format!(
        "{}  {}  inbox {}\n  {} open, {} for you, {} new\n",
        row.name, row.autonomy, row.inbox, row.open, row.to_jay, row.unannounced
    );
    for note in &row.next {
        out.push_str(&format!(
            "  next: {} · {} · {}\n",
            note.priority, note.to, note.title
        ));
    }
    match &row.held {
        Some(held) => out.push_str(&format!("  holding: {held}\n")),
        None => out.push_str("  ready: the next tick sends an idle note\n"),
    }
    if let Some(at) = row.last_note_at {
        out.push_str(&format!(
            "  last idle note: {}\n",
            ago((now_ms - at).max(0) / 1000)
        ));
    }
    out
}

/// What `kete studio queue` prints for one repo: what the runner would
/// send, or with `--now` whether it sent (`was` is the row before).
fn queue_text(row: &StudioQueueRow, was: Option<&StudioQueueRow>, now: bool) -> String {
    let sent = now && row.last_note_at.is_some() && was.map(|w| w.last_note_at) != Some(row.last_note_at);
    if sent {
        return format!("{}: sent the idle note to the orchestrator.", row.name);
    }
    let reason = was.unwrap_or(row).held.clone();
    match (reason, now) {
        (Some(held), true) => format!("{}: not sent, holding: {held}.", row.name),
        (Some(held), false) => format!("{}: holding: {held}.", row.name),
        (None, _) => {
            let names: Vec<&str> = row.next.iter().map(|n| n.id.as_str()).collect();
            format!(
                "{}: would send {} new note(s): {}.",
                row.name,
                row.unannounced,
                names.join(", ")
            )
        }
    }
}

/// `40 s ago`, `12 min ago`, `3 h ago`.
fn ago(secs: i64) -> String {
    match secs {
        s if s < 60 => format!("{s} s ago"),
        s if s < 3600 => format!("{} min ago", s / 60),
        s => format!("{} h ago", s / 3600),
    }
}

/// The project `query` names: its id, its name (any case; refused when two
/// share it), or its root path.
async fn find(query: &str) -> Result<Project> {
    let projects = fetch_projects().await?;
    pick(&projects, query).cloned()
}

fn pick<'a>(projects: &'a [Project], query: &str) -> Result<&'a Project> {
    let q = query.trim();
    if let Some(p) = projects.iter().find(|p| p.id.0 == q) {
        return Ok(p);
    }
    let named: Vec<&Project> = projects
        .iter()
        .filter(|p| p.name.eq_ignore_ascii_case(q))
        .collect();
    match named.len() {
        1 => return Ok(named[0]),
        0 => {}
        n => bail!("{n} projects are named `{q}`: give its path instead"),
    }
    let want = settings::repo_key(&std::fs::canonicalize(q).unwrap_or_else(|_| q.into()));
    projects
        .iter()
        .find(|p| {
            let root = std::fs::canonicalize(&p.repo_path).unwrap_or_else(|_| p.repo_path.clone());
            settings::same_repo(&settings::repo_key(&root), &want)
                || settings::same_repo(&settings::repo_key(&p.repo_path), &settings::repo_key(Path::new(q)))
        })
        .ok_or_else(|| anyhow::anyhow!("no kete project named `{q}`: `kete add` adds one"))
}

/// The daemon's projects, from one snapshot.
async fn fetch_projects() -> Result<Vec<Project>> {
    use nebula_core::codec::{read_frame, write_frame};
    use nebula_core::{ClientRequest, ServerEvent};
    let mut conn = nebula_tui::ipc::connect_or_spawn().await?;
    write_frame(&mut conn.stream, &ClientRequest::Subscribe).await?;
    loop {
        match read_frame::<ServerEvent, _>(&mut conn.stream).await? {
            Some(ServerEvent::Snapshot { projects, .. }) => return Ok(projects),
            Some(_) => continue,
            None => bail!("the daemon closed the connection before sending its projects"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nebula_core::studio::wire::QueueNote;
    use nebula_core::ProjectId;

    fn project(id: &str, name: &str) -> Project {
        Project {
            id: ProjectId(id.into()),
            name: name.into(),
            repo_path: format!("/repos/{name}").into(),
            sort_order: 0,
        }
    }

    fn row(held: Option<&str>, last: Option<i64>) -> StudioQueueRow {
        StudioQueueRow {
            project: ProjectId("p".into()),
            name: "wheako".into(),
            autonomy: Autonomy::A1,
            inbox: "notes/inbox".into(),
            open: 3,
            to_jay: 1,
            unannounced: 2,
            next: vec![QueueNote {
                id: "storm".into(),
                title: "Storm sky".into(),
                to: "orchestrator".into(),
                priority: "high".into(),
                path: "notes/inbox/storm.md".into(),
            }],
            held: held.map(String::from),
            last_note_at: last,
        }
    }

    #[test]
    fn a_project_is_named_by_id_or_name_in_any_case() {
        let all = [project("p1", "wheako"), project("p2", "web")];
        assert_eq!(pick(&all, "p2").unwrap().name, "web");
        assert_eq!(pick(&all, "WHEAKO").unwrap().id.0, "p1");
        assert!(pick(&all, "nope").is_err());
        let twins = [project("a", "dup"), project("b", "Dup")];
        assert!(pick(&twins, "dup").unwrap_err().to_string().contains("2 projects"));
    }

    #[test]
    fn queue_says_whether_now_sent() {
        let before = row(None, None);
        let after = row(Some("nothing new"), Some(5));
        assert_eq!(
            queue_text(&after, Some(&before), true),
            "wheako: sent the idle note to the orchestrator."
        );
        let held = row(Some("you typed 40 s ago"), None);
        assert_eq!(
            queue_text(&held, Some(&held), true),
            "wheako: not sent, holding: you typed 40 s ago."
        );
        assert_eq!(
            queue_text(&before, Some(&before), false),
            "wheako: would send 2 new note(s): storm."
        );
    }

    #[test]
    fn status_shows_the_counts_and_the_hold() {
        let text = status_text(&row(Some("nothing new"), Some(1_000)), 61_000);
        assert!(text.starts_with("wheako  A1 (Plan)  inbox notes/inbox\n"), "{text}");
        assert!(text.contains("3 open, 1 for you, 2 new"));
        assert!(text.contains("holding: nothing new"));
        assert!(text.contains("last idle note: 1 min ago"));
    }
}
