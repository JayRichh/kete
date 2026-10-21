//! The triage desk's note writer (K6): a new inbox note from one line,
//! with its duplicate check, and the rules for where the note is sent.
//!
//! A note is `<inbox>/<id>.md`, its id the UTC date and the slug of its
//! first six words (`2026-10-22-storm-sky-bands-at-dusk`). It is refused as
//! a [`Duplicate`] when an open note already holds that id or shares the
//! first five words of its title. It is written to a dot-named temp file
//! beside it and renamed into place, so [`inbox::read_dir`] never lists a
//! half-written note.
//!
//! Then one turn goes to the repo's triage desk ([`choose`]): a live
//! session holding the `triage` role card, else a new one when the roles
//! file defines the card, else the orchestrator, labelled `[triage]`. The
//! turn is [`desk_prompt`]; the desk's answer comes back as one line
//! ([`reply_line`]).

use crate::studio::inbox::{self, Priority};
use crate::{Agent, AgentId, ProjectId, Worktree, WorktreeId};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The role card a triage desk runs under.
pub const DESK_ROLE: &str = "triage";
/// The name a new desk is given.
pub const DESK_NAME: &str = "triage desk";
/// A desk at or over this share of its context is left, and a fresh one
/// started for the next note.
pub const CONTEXT_LIMIT_PERCENT: f32 = 60.0;
/// How long the sender waits for the desk's one line.
pub const REPLY_WAIT: Duration = Duration::from_secs(120);
/// Words of the text the id's slug takes.
pub const ID_WORDS: usize = 6;
/// Words of two titles that must match for a duplicate.
pub const KEY_WORDS: usize = 5;
/// The longest title, in characters: the frontmatter must end inside the
/// 4 KiB [`inbox::NOTE_READ_BYTES`] the reader looks at. The full text is
/// the body.
pub const MAX_TITLE_CHARS: usize = 160;
/// The longest note, in bytes.
pub const MAX_TEXT_BYTES: usize = 8_192;
/// The longest route line shown.
const MAX_REPLY_CHARS: usize = 200;

/// A note to file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NewNote {
    /// What was typed. Its first line, cut, is the title; all of it is the body.
    pub text: String,
    /// Who it is for; `triage` when None.
    pub to: Option<String>,
    pub about: Option<String>,
    /// Left out of the file when None (a reader takes normal).
    pub priority: Option<Priority>,
    /// Who wrote it; `jay` when None.
    pub by: Option<String>,
}

/// A note on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Written {
    pub id: String,
    pub path: PathBuf,
}

/// An open note the new one repeats.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Duplicate {
    pub existing: PathBuf,
    /// Why it counts as the same note, in a few words.
    pub reason: String,
}

/// Why [`write`] filed nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refused {
    Duplicate(Duplicate),
    /// The note is empty or too long, or the folder could not be written.
    Failed(String),
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refused::Duplicate(d) => write!(f, "{} repeats {}", d.reason, d.existing.display()),
            Refused::Failed(why) => f.write_str(why),
        }
    }
}

/// Files `note` in `dir` (made when missing) at `now` (epoch seconds).
pub fn write(dir: &Path, note: &NewNote, now: i64) -> Result<Written, Refused> {
    let text = note.text.trim();
    if text.is_empty() {
        return Err(Refused::Failed("the note is empty".into()));
    }
    if text.len() > MAX_TEXT_BYTES {
        return Err(Refused::Failed(format!(
            "the note is over {} KiB",
            MAX_TEXT_BYTES / 1024
        )));
    }
    let title = title_of(text);
    let key = title_key(&title);
    std::fs::create_dir_all(dir)
        .map_err(|e| Refused::Failed(format!("could not make {}: {e}", dir.display())))?;

    let notes = inbox::read_dir(dir);
    if !key.is_empty() {
        if let Some(open) = notes
            .iter()
            .find(|n| n.status.is_open() && title_key(&n.title) == key)
        {
            return Err(Refused::Duplicate(Duplicate {
                existing: open.path.clone(),
                reason: format!("its first {KEY_WORDS} words match an open note"),
            }));
        }
    }

    let (date, at) = utc(now);
    let base = format!("{date}-{}", slug(text, ID_WORDS));
    let mut id = base.clone();
    for n in 2.. {
        let path = dir.join(format!("{id}.md"));
        // The file decides, not the listing: `read_dir` skips what it
        // cannot parse, and a rename would replace it.
        let held_by_file = path.exists().then(|| {
            std::fs::read_to_string(&path)
                .ok()
                .and_then(|t| inbox::parse(&path, &t))
        });
        let held_by_key = notes.iter().find(|n| n.id == id && n.path != path);
        let open = match (&held_by_file, held_by_key) {
            (Some(None), _) => Some(path.clone()),
            (Some(Some(n)), _) if n.status.is_open() => Some(path.clone()),
            (_, Some(n)) if n.status.is_open() => Some(n.path.clone()),
            _ => None,
        };
        if let Some(existing) = open {
            return Err(Refused::Duplicate(Duplicate {
                existing,
                reason: "a note with its id is open".into(),
            }));
        }
        if held_by_file.is_none() && held_by_key.is_none() {
            break;
        }
        id = format!("{base}-{n}");
    }

    let mut out = format!("---\nid: {id}\nstatus: new\n");
    let to = note
        .to
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(DESK_ROLE);
    out.push_str(&format!("to: {}\n", value(to)));
    if let Some(about) = note
        .about
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        out.push_str(&format!("about: {}\n", value(about)));
    }
    if let Some(p) = note.priority {
        out.push_str(&format!("priority: {}\n", p.as_str()));
    }
    let by = note
        .by
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("jay");
    out.push_str(&format!(
        "title: {}\nat: {at}\nby: {}\n---\n{text}\n",
        value(&title),
        value(by)
    ));

    let path = dir.join(format!("{id}.md"));
    put(dir, &id, &out)
        .map_err(|e| Refused::Failed(format!("could not write {}: {e}", path.display())))?;
    Ok(Written { id, path })
}

/// Writes `text` to `<dir>/.<id>.md.tmp` and renames it to `<dir>/<id>.md`;
/// on any error the temp file is removed.
fn put(dir: &Path, id: &str, text: &str) -> std::io::Result<()> {
    let tmp = dir.join(format!(".{id}.md.tmp"));
    let written = std::fs::File::create(&tmp)
        .and_then(|mut f| {
            f.write_all(text.as_bytes())?;
            f.sync_all()
        })
        .and_then(|()| {
            // shortcut: a writer racing for this id between the check in
            // `write` and here would be replaced; notes come from one
            // person typing.
            std::fs::rename(&tmp, dir.join(format!("{id}.md")))
        });
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

/// The first `words` words of `text`, lowercased and joined with `-`.
/// Letters of any script are kept (`ā` stays `ā`); an apostrophe is
/// dropped; everything else separates words. `note` when nothing is left.
pub fn slug(text: &str, words: usize) -> String {
    let slug = words_of(text).take(words).collect::<Vec<_>>().join("-");
    if slug.is_empty() {
        "note".into()
    } else {
        slug
    }
}

/// What two titles must share to be one note: their first five words,
/// lowercased, letters and digits only. Empty for a title with none,
/// which never matches.
pub fn title_key(title: &str) -> String {
    words_of(title)
        .take(KEY_WORDS)
        .collect::<Vec<_>>()
        .join(" ")
}

fn words_of(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split(|c: char| !(c.is_alphanumeric() || c == '\'' || c == '\u{2019}'))
        .map(|w| {
            w.chars()
                .filter(|c| c.is_alphanumeric())
                .flat_map(char::to_lowercase)
                .collect::<String>()
        })
        .filter(|w| !w.is_empty())
}

/// The text's first line, whitespace runs made one space, cut on a word
/// boundary to [`MAX_TITLE_CHARS`].
fn title_of(text: &str) -> String {
    let line = text.lines().next().unwrap_or_default();
    let one = line.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() <= MAX_TITLE_CHARS {
        return one;
    }
    let cut: String = one.chars().take(MAX_TITLE_CHARS).collect();
    match cut.rfind(' ') {
        Some(at) if at > 0 => format!("{}…", &cut[..at]),
        _ => format!("{cut}…"),
    }
}

/// A frontmatter value that reads back as written: one line, and quoted
/// when the reader (or a YAML one) would take part of it as a comment, a
/// quote or a key. A `"` inside a quoted value becomes `'`, since the
/// reader ends a quoted value at the next quote.
fn value(raw: &str) -> String {
    let one = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let needs = one.contains(" #")
        || one.contains(": ")
        || one.ends_with(':')
        || one.starts_with(|c: char| "#\"'[]{}&*!|>%@`,-?".contains(c));
    if needs {
        format!("\"{}\"", one.replace('"', "'"))
    } else {
        one
    }
}

/// `now` (epoch seconds) as the id's date and the note's `at`, both UTC.
fn utc(now: i64) -> (String, String) {
    let days = now.div_euclid(86_400);
    let tod = now.rem_euclid(86_400);
    // Howard Hinnant's civil_from_days, as crashlog.rs has it.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    let date = format!("{y:04}-{m:02}-{d:02}");
    let at = format!(
        "{date}T{:02}:{:02}:{:02}Z",
        tod / 3_600,
        tod % 3_600 / 60,
        tod % 60
    );
    (date, at)
}

/// The one turn a desk is sent: it names the note's file (absolute, `/`
/// separators, so it reads the same from any worktree) and asks for the
/// route in one line. It starts with "Triage", one word, so a test's fake
/// agent can be scripted on it.
pub fn desk_prompt(note: &Path) -> String {
    format!(
        "Triage the inbox note {}: read it, then in its frontmatter set to (a recipient, a \
         department slug, or jay), about, priority (high, normal or low) and status: open. \
         Change nothing else. Answer in one line: where it goes and why.",
        crate::studio::settings::repo_key(note)
    )
}

/// [`desk_prompt`] for the orchestrator, which takes it in place of a desk.
pub fn orchestrator_prompt(note: &Path) -> String {
    format!("[triage] {}", desk_prompt(note))
}

/// The desk's one line from what a read of its session returned: the last
/// `Assistant:` line of a transcript, else the last line with text.
pub fn reply_line(text: &str) -> Option<String> {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let line = lines
        .iter()
        .rev()
        .find_map(|l| l.strip_prefix("Assistant:"))
        .map(str::trim)
        .or_else(|| lines.last().copied())?;
    let line: String = line.chars().take(MAX_REPLY_CHARS).collect();
    (!line.is_empty()).then_some(line)
}

/// What the strip says when the desk did not answer within [`REPLY_WAIT`].
pub fn busy_line(id: &str) -> String {
    format!("filed {id}: the desk is busy, so the note waits in the inbox")
}

/// Who a note is sent to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Target {
    /// The triage desk, else the orchestrator.
    #[default]
    Desk,
    /// The orchestrator only.
    Orchestrator,
}

impl Target {
    /// `desk` or `orchestrator`; anything else is the desk.
    pub fn parse(word: Option<&str>) -> Target {
        match word.map(str::trim) {
            Some(w) if w.eq_ignore_ascii_case("orchestrator") => Target::Orchestrator,
            _ => Target::Desk,
        }
    }
}

/// Where [`choose`] sends a note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Desk {
    /// A live desk, asked with [`desk_prompt`].
    Live(AgentId),
    /// A new desk in the repo's root worktree, started on [`desk_prompt`].
    Start(WorktreeId),
    /// The orchestrator, asked with [`orchestrator_prompt`].
    Orchestrator(AgentId),
}

/// The sessions [`choose`] reads, as each client holds them.
pub struct Sessions<'a> {
    pub worktrees: &'a [Worktree],
    pub agents: &'a [Agent],
    /// The role card `agent` runs under (`SessionRoles`).
    pub card: &'a dyn Fn(&AgentId) -> Option<String>,
    /// How full `agent`'s context is, in percent ([`context_percent`]).
    pub context: &'a dyn Fn(&AgentId) -> Option<f32>,
}

/// Where a note for `project` goes. `desk_defined`: the roles file
/// committed in the repo defines the `triage` card.
///
/// To the desk: the live, unarchived `triage` session in the project's
/// root worktree with the least context used, while under
/// [`CONTEXT_LIMIT_PERCENT`]; else a new desk when the card is defined;
/// else the orchestrator. To the orchestrator: only it.
pub fn choose(
    project: &ProjectId,
    s: &Sessions,
    desk_defined: bool,
    target: Target,
) -> Result<Desk, String> {
    let root = s
        .worktrees
        .iter()
        .find(|w| &w.project_id == project && w.is_main);
    let orchestrator = s
        .agents
        .iter()
        .find(|a| a.role.is_orchestrator() && !a.archived)
        .map(|a| Desk::Orchestrator(a.id.clone()));
    if target == Target::Orchestrator {
        return orchestrator.ok_or_else(|| "no orchestrator is running".into());
    }
    if let Some(root) = root {
        let live = s
            .agents
            .iter()
            .filter(|a| a.alive && !a.archived && a.worktree_id == root.id)
            .filter(|a| (s.card)(&a.id).as_deref() == Some(DESK_ROLE))
            .map(|a| (a, (s.context)(&a.id).unwrap_or(0.0)))
            .filter(|(_, used)| *used < CONTEXT_LIMIT_PERCENT)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((desk, _)) = live {
            return Ok(Desk::Live(desk.id.clone()));
        }
        if desk_defined {
            return Ok(Desk::Start(root.id.clone()));
        }
    }
    orchestrator.ok_or_else(|| {
        "no triage desk and no orchestrator: the repo's roles file defines no `triage` card, \
         and none is running"
            .into()
    })
}

/// How full a session's context is: the larger of its window's percent
/// and its share of the role's `context_budget`.
pub fn context_percent(
    usage: Option<&crate::usage::ContextUsage>,
    budget: Option<u64>,
) -> Option<f32> {
    let usage = usage?;
    let of_budget = match (usage.tokens, budget) {
        (Some(tokens), Some(budget)) if budget > 0 => Some(tokens as f32 * 100.0 / budget as f32),
        _ => None,
    };
    match (usage.percent, of_budget) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (a, b) => a.or(b),
    }
}

/// The inbox folder of `repo_root`: this machine's `studio_inbox` for the
/// repo, else the manifest's `inbox`. Refused when neither names one, and
/// for a path that is absolute or climbs out of the repo.
pub fn inbox_dir(
    repo_root: &Path,
    settings: Option<&crate::studio::StudioSettings>,
) -> Result<PathBuf, String> {
    let from_settings = settings
        .map(|s| s.studio_inbox.trim().to_string())
        .filter(|s| !s.is_empty());
    let rel = match from_settings {
        Some(rel) => rel,
        None => crate::studio::manifest::load(repo_root)?
            .and_then(|m| m.inbox)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                format!(
                    "{} names no inbox: add `inbox = \"notes/inbox\"` to its {}",
                    repo_root.display(),
                    crate::studio::manifest::MANIFEST_PATH
                )
            })?,
    };
    let path = Path::new(&rel);
    let climbs = path.components().any(|c| {
        !matches!(
            c,
            std::path::Component::Normal(_) | std::path::Component::CurDir
        )
    });
    if climbs {
        return Err(format!(
            "the inbox `{rel}` must be a folder inside the repo"
        ));
    }
    Ok(repo_root.join(path))
}

/// The newest `n` notes in `dir`, by `at`, newest first; a note with no
/// `at` last.
pub fn latest(dir: &Path, n: usize) -> Vec<inbox::InboxNote> {
    let mut notes = inbox::read_dir(dir);
    notes.sort_by(|a, b| {
        b.at.is_some()
            .cmp(&a.at.is_some())
            .then_with(|| b.at.cmp(&a.at))
            .then_with(|| b.id.cmp(&a.id))
    });
    notes.truncate(n);
    notes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::studio::inbox::NoteStatus;
    use crate::{AgentKind, AgentRole, AgentStatus};

    /// 2026-10-22T01:22:00Z.
    const NOW: i64 = 1_792_632_120;

    fn note(text: &str) -> NewNote {
        NewNote {
            text: text.into(),
            ..NewNote::default()
        }
    }

    #[test]
    fn utc_dates_and_times() {
        assert_eq!(utc(0), ("1970-01-01".into(), "1970-01-01T00:00:00Z".into()));
        assert_eq!(utc(NOW).1, "2026-10-22T01:22:00Z");
        assert_eq!(utc(951_782_400).0, "2000-02-29");
    }

    #[test]
    fn slugs_keep_letters_of_any_script_and_cut_at_six_words() {
        assert_eq!(
            slug("The storm sky bands at dusk, again and again", 6),
            "the-storm-sky-bands-at-dusk"
        );
        assert_eq!(slug("Fix: the HUD's #3 ring!!", 6), "fix-the-huds-3-ring");
        assert_eq!(
            slug("Māori place names: Ōtautahi", 6),
            "māori-place-names-ōtautahi"
        );
        assert_eq!(slug("?!", 6), "note");
        assert_eq!(slug("a b c", 2), "a-b");
    }

    #[test]
    fn title_keys_take_five_words() {
        assert_eq!(
            title_key("Storm sky bands at dusk, again"),
            "storm sky bands at dusk"
        );
        assert_eq!(
            title_key("STORM  sky-bands at Dusk!"),
            title_key("storm sky bands at dusk")
        );
        assert_eq!(title_key("!!"), "");
    }

    #[test]
    fn a_note_parses_back_with_every_field() {
        let dir = tempfile::tempdir().unwrap();
        let w = write(
            dir.path(),
            &NewNote {
                text: "\"Storm\" sky #3: bands at dusk in Ōtautahi".into(),
                to: Some("art".into()),
                about: Some("World: weather".into()),
                priority: Some(Priority::High),
                by: Some("jay".into()),
            },
            NOW,
        )
        .unwrap();
        assert_eq!(w.id, "2026-10-22-storm-sky-3-bands-at-dusk");
        assert_eq!(w.path, dir.path().join(format!("{}.md", w.id)));
        let text = std::fs::read_to_string(&w.path).unwrap();
        let n = inbox::parse(&w.path, &text).unwrap();
        assert_eq!(n.id, w.id);
        assert_eq!(n.title, "'Storm' sky #3: bands at dusk in Ōtautahi");
        assert_eq!(n.status, NoteStatus::New);
        assert_eq!(n.to.as_deref(), Some("art"));
        assert_eq!(n.about.as_deref(), Some("World: weather"));
        assert_eq!(n.priority, Priority::High);
        assert_eq!(n.at.as_deref(), Some("2026-10-22T01:22:00Z"));
        assert_eq!(n.by.as_deref(), Some("jay"));
        assert!(text.ends_with("\n---\n\"Storm\" sky #3: bands at dusk in Ōtautahi\n"));
    }

    #[test]
    fn defaults_are_to_triage_by_jay() {
        let dir = tempfile::tempdir().unwrap();
        let w = write(dir.path(), &note("storm sky bands at dusk"), NOW).unwrap();
        let n = inbox::read_dir(dir.path()).pop().unwrap();
        assert_eq!(n.id, w.id);
        assert_eq!(n.to.as_deref(), Some("triage"));
        assert_eq!(n.by.as_deref(), Some("jay"));
        assert_eq!(n.priority, Priority::Normal);
    }

    #[test]
    fn a_newline_cannot_inject_a_key() {
        let dir = tempfile::tempdir().unwrap();
        let w = write(
            dir.path(),
            &NewNote {
                text: "line one".into(),
                about: Some("x\nstatus: resolved\n---".into()),
                ..NewNote::default()
            },
            NOW,
        )
        .unwrap();
        let n = inbox::parse(&w.path, &std::fs::read_to_string(&w.path).unwrap()).unwrap();
        assert_eq!(n.status, NoteStatus::New);
        assert_eq!(n.about.as_deref(), Some("x status: resolved ---"));
    }

    #[test]
    fn a_long_title_is_cut_and_the_body_kept() {
        let dir = tempfile::tempdir().unwrap();
        let long = "word ".repeat(1_000);
        let w = write(dir.path(), &note(&long), NOW).unwrap();
        let notes = inbox::read_dir(dir.path());
        assert_eq!(notes.len(), 1, "the note is still listed");
        assert!(notes[0].title.chars().count() <= MAX_TITLE_CHARS + 1);
        assert!(notes[0].title.ends_with('…'));
        assert!(std::fs::read_to_string(&w.path)
            .unwrap()
            .contains(long.trim()));
    }

    #[test]
    fn a_duplicate_by_id_names_the_open_note() {
        let dir = tempfile::tempdir().unwrap();
        let first = write(dir.path(), &note("storm sky bands at dusk"), NOW).unwrap();
        let again = write(dir.path(), &note("Storm sky bands at dusk"), NOW);
        let Err(Refused::Duplicate(d)) = again else {
            panic!("{again:?}")
        };
        assert_eq!(d.existing, first.path);
    }

    #[test]
    fn a_duplicate_by_title_words_on_another_day() {
        let dir = tempfile::tempdir().unwrap();
        let first = write(dir.path(), &note("storm sky bands at dusk"), NOW).unwrap();
        let again = write(
            dir.path(),
            &note("Storm sky bands at dusk, worse now"),
            NOW + 86_400,
        );
        let Err(Refused::Duplicate(d)) = again else {
            panic!("{again:?}")
        };
        assert_eq!(d.existing, first.path);
        assert!(d.reason.contains("first 5 words"), "{}", d.reason);
    }

    #[test]
    fn a_resolved_note_does_not_block_and_its_id_takes_a_suffix() {
        let dir = tempfile::tempdir().unwrap();
        let first = write(dir.path(), &note("storm sky bands at dusk"), NOW).unwrap();
        let text = std::fs::read_to_string(&first.path).unwrap();
        std::fs::write(&first.path, text.replace("status: new", "status: resolved")).unwrap();
        let second = write(dir.path(), &note("storm sky bands at dusk"), NOW).unwrap();
        assert_eq!(second.id, format!("{}-2", first.id));
        let third = write(dir.path(), &note("other thing"), NOW).unwrap();
        assert_eq!(third.id, "2026-10-22-other-thing");
    }

    #[test]
    fn an_unreadable_file_holding_the_id_blocks() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("2026-10-22-storm.md");
        std::fs::write(&path, "no frontmatter").unwrap();
        let got = write(dir.path(), &note("storm"), NOW);
        let Err(Refused::Duplicate(d)) = got else {
            panic!("{got:?}")
        };
        assert_eq!(d.existing, path);
    }

    #[test]
    fn empty_and_oversized_notes_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            write(dir.path(), &note("  "), NOW),
            Err(Refused::Failed(_))
        ));
        assert!(matches!(
            write(dir.path(), &note(&"x".repeat(MAX_TEXT_BYTES + 1)), NOW),
            Err(Refused::Failed(_))
        ));
    }

    #[test]
    fn no_temp_file_is_left_behind_on_an_error() {
        let dir = tempfile::tempdir().unwrap();
        // A folder that is not empty where the note's file goes: the
        // rename onto it fails on every platform.
        let id = "2026-10-22-storm";
        let blocker = dir.path().join(format!("{id}.md"));
        std::fs::create_dir(&blocker).unwrap();
        std::fs::write(blocker.join("x"), "x").unwrap();
        assert!(put(dir.path(), id, "---\n---\n").is_err());
        let names: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, [format!("{id}.md")], "no .tmp left");
        // And a good write leaves only the note.
        put(dir.path(), "ok", "---\n---\n").unwrap();
        assert!(dir.path().join("ok.md").is_file());
        assert!(!dir.path().join(".ok.md.tmp").exists());
    }

    #[test]
    fn an_open_note_holding_the_id_blocks_whatever_its_title() {
        let dir = tempfile::tempdir().unwrap();
        // Same file name, another title: only the id rule can refuse it.
        let path = dir.path().join("2026-10-22-storm.md");
        std::fs::write(&path, "---\nstatus: open\ntitle: weather\n---\n").unwrap();
        let got = write(dir.path(), &note("storm"), NOW);
        let Err(Refused::Duplicate(d)) = got else {
            panic!("{got:?}")
        };
        assert_eq!(
            (d.existing, d.reason.as_str()),
            (path, "a note with its id is open")
        );
        // An `id:` key that differs from its file name counts too.
        let dir = tempfile::tempdir().unwrap();
        let other = dir.path().join("elsewhere.md");
        std::fs::write(
            &other,
            "---\nid: 2026-10-22-storm\nstatus: new\ntitle: weather\n---\n",
        )
        .unwrap();
        let got = write(dir.path(), &note("storm"), NOW);
        let Err(Refused::Duplicate(d)) = got else {
            panic!("{got:?}")
        };
        assert_eq!(d.existing, other);
    }

    #[test]
    fn prompts_name_the_note_and_start_with_triage() {
        let p = desk_prompt(Path::new(r"C:\Code\game\notes\inbox\a.md"));
        assert!(p.starts_with("Triage the inbox note C:/Code/game/notes/inbox/a.md:"));
        assert!(!p.contains('\n'));
        assert!(orchestrator_prompt(Path::new("/r/a.md")).starts_with("[triage] Triage "));
    }

    #[test]
    fn reply_lines_prefer_the_last_assistant_line() {
        let text = "User: Triage it\nAssistant: thinking\nUser: more\nAssistant: art, high: \
                    the dusk sky is the art department's\n";
        assert_eq!(
            reply_line(text).as_deref(),
            Some("art, high: the dusk sky is the art department's")
        );
        assert_eq!(reply_line("one\n\ntwo  \n").as_deref(), Some("two"));
        assert_eq!(reply_line(" \n"), None);
        assert_eq!(reply_line(&"y".repeat(500)).unwrap().len(), MAX_REPLY_CHARS);
    }

    fn worktree(id: &str, project: &str, main: bool) -> Worktree {
        Worktree {
            id: WorktreeId(id.into()),
            project_id: ProjectId(project.into()),
            path: PathBuf::from(format!("/{id}")),
            branch: "main".into(),
            is_main: main,
            sort_order: 0,
        }
    }

    fn agent(id: &str, worktree: &str) -> Agent {
        Agent {
            id: AgentId(id.into()),
            worktree_id: WorktreeId(worktree.into()),
            name: id.into(),
            status: AgentStatus::Finished,
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
            role: AgentRole::Worker,
            sort_order: 0,
            alive: true,
            recent_prompts: Vec::new(),
            profile: None,
            parent: None,
        }
    }

    fn pick(
        agents: &[Agent],
        used: &[(&str, f32)],
        defined: bool,
        target: Target,
    ) -> Result<Desk, String> {
        let worktrees = [worktree("root", "p", true), worktree("lane", "p", false)];
        let card = |id: &AgentId| id.0.starts_with("desk").then(|| DESK_ROLE.to_string());
        let context = |id: &AgentId| used.iter().find(|(a, _)| *a == id.0).map(|(_, u)| *u);
        choose(
            &ProjectId("p".into()),
            &Sessions {
                worktrees: &worktrees,
                agents,
                card: &card,
                context: &context,
            },
            defined,
            target,
        )
    }

    #[test]
    fn the_desk_with_room_wins_and_a_full_one_is_left() {
        let mut orch = agent("orch", "root");
        orch.role = AgentRole::Orchestrator;
        let agents = [orch, agent("desk-a", "root"), agent("desk-b", "root")];
        let live = |id: &str| Ok(Desk::Live(AgentId(id.into())));
        assert_eq!(
            pick(
                &agents,
                &[("desk-a", 30.0), ("desk-b", 10.0)],
                true,
                Target::Desk
            ),
            live("desk-b")
        );
        assert_eq!(
            pick(
                &agents,
                &[("desk-a", 30.0), ("desk-b", 60.0)],
                true,
                Target::Desk
            ),
            live("desk-a")
        );
        // Both at or over 60 percent: a fresh desk.
        assert_eq!(
            pick(
                &agents,
                &[("desk-a", 75.0), ("desk-b", 60.0)],
                true,
                Target::Desk
            ),
            Ok(Desk::Start(WorktreeId("root".into())))
        );
        // Full, and no card to start a new one with: the orchestrator.
        assert_eq!(
            pick(
                &agents,
                &[("desk-a", 75.0), ("desk-b", 60.0)],
                false,
                Target::Desk
            ),
            Ok(Desk::Orchestrator(AgentId("orch".into())))
        );
        assert_eq!(
            pick(&agents, &[], true, Target::Orchestrator),
            Ok(Desk::Orchestrator(AgentId("orch".into())))
        );
    }

    #[test]
    fn only_a_live_desk_in_the_root_worktree_counts() {
        let mut dead = agent("desk-dead", "root");
        dead.alive = false;
        let mut archived = agent("desk-archived", "root");
        archived.archived = true;
        let agents = [dead, archived, agent("desk-lane", "lane")];
        assert_eq!(
            pick(&agents, &[], true, Target::Desk),
            Ok(Desk::Start(WorktreeId("root".into())))
        );
        let err = pick(&agents, &[], false, Target::Desk).unwrap_err();
        assert!(err.contains("no triage desk"), "{err}");
        assert!(pick(&agents, &[], true, Target::Orchestrator).is_err());
    }

    #[test]
    fn context_takes_the_fuller_of_window_and_budget() {
        let usage = crate::usage::ContextUsage {
            percent: Some(20.0),
            tokens: Some(70_000),
            window: Some(200_000),
            compactions: 0,
            last_compact_at: None,
            source: crate::usage::UsageSource::Statusline,
            at: 0,
        };
        assert_eq!(context_percent(Some(&usage), None), Some(20.0));
        assert_eq!(context_percent(Some(&usage), Some(100_000)), Some(70.0));
        assert_eq!(context_percent(None, Some(100_000)), None);
    }

    #[test]
    fn the_inbox_is_the_settings_folder_then_the_manifests() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        assert!(inbox_dir(root, None)
            .unwrap_err()
            .contains("names no inbox"));
        std::fs::create_dir_all(root.join(".kete")).unwrap();
        std::fs::write(
            root.join(".kete/studio.toml"),
            "version = 1\ninbox = \"notes/inbox\"\n",
        )
        .unwrap();
        assert_eq!(inbox_dir(root, None).unwrap(), root.join("notes/inbox"));
        let mine = crate::studio::StudioSettings {
            studio_inbox: "Design/inbox".into(),
            ..Default::default()
        };
        assert_eq!(
            inbox_dir(root, Some(&mine)).unwrap(),
            root.join("Design/inbox")
        );
        for bad in ["../elsewhere", "/abs/inbox"] {
            let s = crate::studio::StudioSettings {
                studio_inbox: bad.into(),
                ..Default::default()
            };
            assert!(inbox_dir(root, Some(&s)).is_err(), "{bad}");
        }
    }

    #[test]
    fn latest_is_newest_first() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), &note("first one"), NOW).unwrap();
        write(dir.path(), &note("second one"), NOW + 60).unwrap();
        write(dir.path(), &note("third one"), NOW + 120).unwrap();
        let ids: Vec<String> = latest(dir.path(), 2).into_iter().map(|n| n.title).collect();
        assert_eq!(ids, ["third one", "second one"]);
    }
}
