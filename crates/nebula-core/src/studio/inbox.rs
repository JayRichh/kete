//! The studio inbox: Markdown notes with YAML-style frontmatter.
//!
//! The Studio app writes one note per file. This module reads a folder of
//! them without ever loading a whole file: only the first
//! [`NOTE_READ_BYTES`] bytes of each note are read, a folder lists at most
//! [`MAX_NOTES`] notes, and a file over [`MAX_NOTE_FILE`] is skipped.
//!
//! A note starts with a `---` line, then `key: value` lines, then a closing
//! `---` line. Keys are case-insensitive and unknown keys are ignored.

use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Only this many bytes at the start of a note are read.
pub const NOTE_READ_BYTES: usize = 4_096;
/// A folder lists at most this many notes.
pub const MAX_NOTES: usize = 2_000;
/// A file over this many bytes is skipped.
pub const MAX_NOTE_FILE: u64 = 1_048_576;

/// One inbox note, as far as its first 4 KiB says.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InboxNote {
    /// The `id` key, else the file name without `.md`.
    pub id: String,
    /// The note's file.
    pub path: PathBuf,
    /// The `title` key, else the first heading, else the first line, else the id.
    pub title: String,
    /// The `status` key; a note with none is new.
    pub status: NoteStatus,
    /// Who the note is for: a recipient, a department slug, or `jay`.
    pub to: Option<String>,
    /// What the note is about, often a department label.
    pub about: Option<String>,
    /// The `priority` key; normal when missing or unknown.
    pub priority: Priority,
    /// When it was written, as the note says it.
    pub at: Option<String>,
    /// Who wrote it.
    pub by: Option<String>,
}

/// Where a note stands.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NoteStatus {
    New,
    Open,
    Resolved,
    Done,
    Closed,
    Rejected,
    /// Any other word, as written.
    Other(String),
}

impl NoteStatus {
    /// Reads a status word, ignoring case.
    pub fn parse(word: &str) -> NoteStatus {
        let word = word.trim();
        match word.to_ascii_lowercase().as_str() {
            "new" => NoteStatus::New,
            "open" => NoteStatus::Open,
            "resolved" => NoteStatus::Resolved,
            "done" => NoteStatus::Done,
            "closed" => NoteStatus::Closed,
            "rejected" => NoteStatus::Rejected,
            _ => NoteStatus::Other(word.to_string()),
        }
    }

    /// The status as a lowercase word; an unknown one as written.
    pub fn as_str(&self) -> &str {
        match self {
            NoteStatus::New => "new",
            NoteStatus::Open => "open",
            NoteStatus::Resolved => "resolved",
            NoteStatus::Done => "done",
            NoteStatus::Closed => "closed",
            NoteStatus::Rejected => "rejected",
            NoteStatus::Other(raw) => raw,
        }
    }

    /// True for a note still waiting on someone: new or open.
    pub fn is_open(&self) -> bool {
        matches!(self, NoteStatus::New | NoteStatus::Open)
    }
}

/// How urgent a note is. The derived order puts high first.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub enum Priority {
    High,
    #[default]
    Normal,
    Low,
}

impl Priority {
    /// Reads a priority word, ignoring case. An unknown word is `None`.
    pub fn parse(word: &str) -> Option<Priority> {
        match word.trim().to_ascii_lowercase().as_str() {
            "high" => Some(Priority::High),
            "normal" => Some(Priority::Normal),
            "low" => Some(Priority::Low),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Priority::High => "high",
            Priority::Normal => "normal",
            Priority::Low => "low",
        }
    }
}

/// Lists the notes directly in `dir` (not below it), in file-name order.
///
/// Only `*.md` files count. A file over [`MAX_NOTE_FILE`] is skipped, a note
/// with no readable frontmatter is skipped, and a missing folder is empty.
pub fn read_dir(dir: &Path) -> Vec<InboxNote> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<(String, PathBuf)> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let is_md = Path::new(&name)
                .extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("md"));
            let is_file = e.file_type().map(|t| t.is_file()).unwrap_or(false);
            (is_md && is_file).then(|| (name, e.path()))
        })
        .collect();
    files.sort_by(|a, b| a.0.cmp(&b.0));
    files.truncate(MAX_NOTES);

    files
        .into_iter()
        .filter_map(|(_, path)| {
            let text = read_head(&path)?;
            parse(&path, &text)
        })
        .collect()
}

/// The first [`NOTE_READ_BYTES`] bytes of a file as text, cut on a
/// character boundary. `None` for a file over [`MAX_NOTE_FILE`] or one that
/// cannot be read.
fn read_head(path: &Path) -> Option<String> {
    let file = std::fs::File::open(path).ok()?;
    if file.metadata().ok()?.len() > MAX_NOTE_FILE {
        return None;
    }
    let mut buf = Vec::with_capacity(NOTE_READ_BYTES);
    file.take(NOTE_READ_BYTES as u64)
        .read_to_end(&mut buf)
        .ok()?;
    if let Err(e) = std::str::from_utf8(&buf) {
        if e.error_len().is_none() {
            buf.truncate(e.valid_up_to());
        }
    }
    Some(String::from_utf8_lossy(&buf).into_owned())
}

/// Reads one note from its text. `None` when the text has no complete
/// frontmatter block.
pub fn parse(path: &Path, text: &str) -> Option<InboxNote> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut lines = text.lines();
    if lines.next()?.trim_end() != "---" {
        return None;
    }

    let mut fields: Vec<(String, String)> = Vec::new();
    let mut closed = false;
    for line in lines.by_ref() {
        if line.trim_end() == "---" {
            closed = true;
            break;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        if key.is_empty() || key.contains(char::is_whitespace) {
            continue;
        }
        if let Some(value) = clean_value(value) {
            fields.push((key, value));
        }
    }
    if !closed {
        return None;
    }
    let field = |name: &str| {
        fields
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.clone())
    };

    let id = field("id").unwrap_or_else(|| {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        match name.strip_suffix(".md") {
            Some(stem) => stem.to_string(),
            None => name,
        }
    });

    let body: Vec<&str> = lines.collect();
    let title = field("title")
        .or_else(|| first_heading(&body))
        .or_else(|| {
            body.iter()
                .map(|l| l.trim())
                .find(|l| !l.is_empty())
                .map(str::to_string)
        })
        .unwrap_or_else(|| id.clone());

    Some(InboxNote {
        title,
        status: field("status")
            .map(|s| NoteStatus::parse(&s))
            .unwrap_or(NoteStatus::New),
        to: field("to"),
        about: field("about"),
        priority: field("priority")
            .and_then(|p| Priority::parse(&p))
            .unwrap_or_default(),
        at: field("at"),
        by: field("by"),
        id,
        path: path.to_path_buf(),
    })
}

/// The text of the first Markdown heading line (`#` to `######`, then a
/// space), without the marks.
fn first_heading(body: &[&str]) -> Option<String> {
    body.iter().find_map(|line| {
        let line = line.trim();
        let rest = line.trim_start_matches('#');
        let hashes = line.len() - rest.len();
        if hashes == 0 || hashes > 6 || !rest.starts_with(char::is_whitespace) {
            return None;
        }
        let text = rest.trim();
        (!text.is_empty()).then(|| text.to_string())
    })
}

/// A frontmatter value: quotes removed, an inline ` #` comment cut off
/// (unless inside quotes), whitespace trimmed. Empty is `None`.
fn clean_value(raw: &str) -> Option<String> {
    let raw = raw.trim();
    let value = match raw.chars().next() {
        Some(q @ ('"' | '\'')) => match raw[1..].find(q) {
            Some(end) => &raw[1..1 + end],
            None => strip_comment(raw),
        },
        _ => strip_comment(raw),
    };
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn strip_comment(raw: &str) -> &str {
    if raw.starts_with('#') {
        return "";
    }
    let bytes = raw.as_bytes();
    for i in 1..bytes.len() {
        if bytes[i] == b'#' && (bytes[i - 1] == b' ' || bytes[i - 1] == b'\t') {
            return &raw[..i];
        }
    }
    raw
}

/// The notes an orchestrator should hear about, in the order to tell them.
///
/// A note is kept when all of these hold:
/// - its status is new or open;
/// - its `to` is set and not empty;
/// - neither its id nor its `about` is in `exclude` (trimmed, ignoring case);
/// - when `recipients` is not empty, its `to` equals one of them (ignoring
///   case); when it is empty, its `to` is anything but `jay`.
///
/// The order is priority (high first), then `at` oldest first with a note
/// that has no `at` last, then id.
pub fn eligible<'a>(
    notes: &'a [InboxNote],
    recipients: &[String],
    exclude: &[String],
) -> Vec<&'a InboxNote> {
    let norm = |s: &str| s.trim().to_ascii_lowercase();
    let excluded: Vec<String> = exclude.iter().map(|s| norm(s)).collect();
    let wanted: Vec<String> = recipients.iter().map(|s| norm(s)).collect();

    let mut out: Vec<&InboxNote> = notes
        .iter()
        .filter(|n| n.status.is_open())
        .filter(|n| {
            let Some(to) = n.to.as_deref().map(norm) else {
                return false;
            };
            if to.is_empty() {
                return false;
            }
            if excluded.contains(&norm(&n.id)) {
                return false;
            }
            if let Some(about) = n.about.as_deref() {
                if excluded.contains(&norm(about)) {
                    return false;
                }
            }
            if wanted.is_empty() {
                to != "jay"
            } else {
                wanted.contains(&to)
            }
        })
        .collect();
    out.sort_by(|a, b| {
        a.priority
            .cmp(&b.priority)
            .then_with(|| a.at.is_none().cmp(&b.at.is_none()))
            .then_with(|| a.at.cmp(&b.at))
            .then_with(|| a.id.cmp(&b.id))
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(id: &str, to: Option<&str>, priority: Priority, at: Option<&str>) -> InboxNote {
        InboxNote {
            id: id.into(),
            path: PathBuf::from(format!("{id}.md")),
            title: id.into(),
            status: NoteStatus::Open,
            to: to.map(String::from),
            about: None,
            priority,
            at: at.map(String::from),
            by: None,
        }
    }

    fn ids(notes: &[&InboxNote]) -> Vec<String> {
        notes.iter().map(|n| n.id.clone()).collect()
    }

    #[test]
    fn studio_shape_with_no_id_or_priority() {
        let text = "---\nstatus: open\nabout: World and weather\nto: orchestrator\n---\n\n\
                    # Storm sky banding at dusk\n\nBody.\n";
        let n = parse(Path::new("C:/x/inbox/2026-10-21-storm.md"), text).unwrap();
        assert_eq!(n.id, "2026-10-21-storm");
        assert_eq!(n.title, "Storm sky banding at dusk");
        assert_eq!(n.status, NoteStatus::Open);
        assert_eq!(n.priority, Priority::Normal);
        assert_eq!(n.to.as_deref(), Some("orchestrator"));
        assert_eq!(n.about.as_deref(), Some("World and weather"));
        assert_eq!(n.at, None);
    }

    #[test]
    fn frontmatter_keys_win() {
        let text = "---\nID: custom\nTitle: \"Quoted: title\"\nStatus: DONE\nPriority: LOW\n\
                    at: 2026-10-21T05:52:13Z\nby: 'jay'\nextra: ignored\n---\n# Heading\n";
        let n = parse(Path::new("file.md"), text).unwrap();
        assert_eq!(n.id, "custom");
        assert_eq!(n.title, "Quoted: title");
        assert_eq!(n.status, NoteStatus::Done);
        assert_eq!(n.priority, Priority::Low);
        assert_eq!(n.at.as_deref(), Some("2026-10-21T05:52:13Z"));
        assert_eq!(n.by.as_deref(), Some("jay"));
    }

    #[test]
    fn status_and_priority_defaults() {
        let n = parse(
            Path::new("a.md"),
            "---\npriority: urgent\nstatus: parked\n---\n",
        )
        .unwrap();
        assert_eq!(n.status, NoteStatus::Other("parked".into()));
        assert_eq!(n.status.as_str(), "parked");
        assert_eq!(n.priority, Priority::Normal);
        let n = parse(Path::new("a.md"), "---\nto: x\n---\n").unwrap();
        assert_eq!(n.status, NoteStatus::New);
        assert!(n.status.is_open());
        assert!(!NoteStatus::Rejected.is_open());
        assert_eq!(Priority::High.as_str(), "high");
    }

    #[test]
    fn title_falls_back_to_first_line_then_id() {
        let n = parse(Path::new("a.md"), "---\nto: x\n---\n\n  Just a line.\n").unwrap();
        assert_eq!(n.title, "Just a line.");
        let n = parse(Path::new("a.md"), "---\nto: x\n---\n").unwrap();
        assert_eq!(n.title, "a");
        let n = parse(
            Path::new("a.md"),
            "---\nto: x\n---\nintro\n## Real heading ##\n",
        )
        .unwrap();
        assert_eq!(n.title, "Real heading ##");
    }

    #[test]
    fn no_frontmatter_is_none() {
        assert!(parse(Path::new("a.md"), "# Just a note\n").is_none());
        assert!(parse(Path::new("a.md"), "").is_none());
        assert!(parse(Path::new("a.md"), "---\nto: x\nno closing line\n").is_none());
    }

    #[test]
    fn inline_comments_are_stripped() {
        let text =
            "---\npriority: high   # urgent\nto: \"a # b\"  # why\nabout: C#\nby: # nobody\n---\n";
        let n = parse(Path::new("a.md"), text).unwrap();
        assert_eq!(n.priority, Priority::High);
        assert_eq!(n.to.as_deref(), Some("a # b"));
        assert_eq!(n.about.as_deref(), Some("C#"));
        assert_eq!(n.by, None);
    }

    #[test]
    fn crlf_and_bom() {
        let text =
            "\u{feff}---\r\nstatus: open\r\nto: manager\r\npriority: high\r\n---\r\n# Title\r\n";
        let n = parse(Path::new("a.md"), text).unwrap();
        assert_eq!(n.status, NoteStatus::Open);
        assert_eq!(n.to.as_deref(), Some("manager"));
        assert_eq!(n.priority, Priority::High);
        assert_eq!(n.title, "Title");
    }

    #[test]
    fn read_dir_lists_md_files_in_name_order() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("b.md"), "---\nto: x\n---\n# B\n").unwrap();
        std::fs::write(dir.path().join("a.md"), "---\nto: x\n---\n# A\n").unwrap();
        std::fs::write(dir.path().join("c.txt"), "---\nto: x\n---\n").unwrap();
        std::fs::write(dir.path().join("d.md"), "no frontmatter").unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub").join("e.md"), "---\nto: x\n---\n").unwrap();
        let got = read_dir(dir.path());
        assert_eq!(
            got.iter().map(|n| n.id.as_str()).collect::<Vec<_>>(),
            vec!["a", "b"]
        );
        assert!(read_dir(&dir.path().join("missing")).is_empty());
    }

    #[test]
    fn a_file_over_one_mib_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let head = "---\nto: x\n---\n# Big\n";
        let mut big = head.as_bytes().to_vec();
        big.resize(MAX_NOTE_FILE as usize + 1, b'z');
        std::fs::write(dir.path().join("big.md"), &big).unwrap();
        big.truncate(MAX_NOTE_FILE as usize);
        std::fs::write(dir.path().join("edge.md"), &big).unwrap();
        let got = read_dir(dir.path());
        assert_eq!(
            got.iter().map(|n| n.id.as_str()).collect::<Vec<_>>(),
            vec!["edge"]
        );
    }

    #[test]
    fn only_the_first_4096_bytes_are_read() {
        let dir = tempfile::tempdir().unwrap();
        // The closing line sits past the cap: the note is skipped.
        let late = format!("---\npad: {}\ntitle: Late\n---\nbody\n", "p".repeat(5_000));
        std::fs::write(dir.path().join("late.md"), late).unwrap();
        // The frontmatter closes in time, but the heading is past the cap.
        let body = format!(
            "---\nto: x\n---\nfirst line\n{}\n# Far heading\n",
            "q".repeat(5_000)
        );
        std::fs::write(dir.path().join("body.md"), body).unwrap();
        let got = read_dir(dir.path());
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].id, "body");
        assert!(got[0].title.starts_with("first line"), "{}", got[0].title);
    }

    #[test]
    fn the_cap_cuts_on_a_character_boundary() {
        let dir = tempfile::tempdir().unwrap();
        let head = "---\nto: x\n---\n";
        let fill = "é".repeat(NOTE_READ_BYTES);
        std::fs::write(dir.path().join("u.md"), format!("{head}{fill}")).unwrap();
        let got = read_dir(dir.path());
        assert_eq!(got.len(), 1);
        assert!(!got[0].title.contains('\u{fffd}'));
    }

    #[test]
    fn at_most_max_notes_are_listed() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..MAX_NOTES + 3 {
            std::fs::write(dir.path().join(format!("n{i:05}.md")), "---\nto: x\n---\n").unwrap();
        }
        assert_eq!(read_dir(dir.path()).len(), MAX_NOTES);
    }

    #[test]
    fn eligible_orders_by_priority_then_at_then_id() {
        let notes = vec![
            note(
                "low-old",
                Some("manager"),
                Priority::Low,
                Some("2026-01-01"),
            ),
            note(
                "norm-new",
                Some("manager"),
                Priority::Normal,
                Some("2026-03-01"),
            ),
            note(
                "norm-old",
                Some("manager"),
                Priority::Normal,
                Some("2026-02-01"),
            ),
            note("norm-none-b", Some("manager"), Priority::Normal, None),
            note("norm-none-a", Some("manager"), Priority::Normal, None),
            note("high", Some("manager"), Priority::High, Some("2026-09-09")),
        ];
        let got = eligible(&notes, &[], &[]);
        assert_eq!(
            ids(&got),
            vec![
                "high",
                "norm-old",
                "norm-new",
                "norm-none-a",
                "norm-none-b",
                "low-old"
            ]
        );
    }

    #[test]
    fn eligible_drops_closed_notes_and_notes_with_no_to() {
        let mut closed = note("closed", Some("manager"), Priority::Normal, None);
        closed.status = NoteStatus::Closed;
        let notes = vec![
            closed,
            note("no-to", None, Priority::Normal, None),
            note("ok", Some("manager"), Priority::Normal, None),
        ];
        assert_eq!(ids(&eligible(&notes, &[], &[])), vec!["ok"]);
    }

    #[test]
    fn eligible_excludes_by_id_and_by_about() {
        let mut a = note("a", Some("manager"), Priority::Normal, None);
        a.about = Some("World and weather".into());
        let notes = vec![a, note("b", Some("manager"), Priority::Normal, None)];
        let by_id = eligible(&notes, &[], &[" A ".to_string()]);
        assert_eq!(ids(&by_id), vec!["b"]);
        let by_about = eligible(&notes, &[], &["world AND weather".to_string()]);
        assert_eq!(ids(&by_about), vec!["b"]);
    }

    #[test]
    fn eligible_filters_by_recipients() {
        let notes = vec![
            note("m", Some("Manager"), Priority::Normal, None),
            note("o", Some("orchestrator"), Priority::Normal, None),
            note("j", Some("jay"), Priority::Normal, None),
            note("d", Some("art-and-3d"), Priority::Normal, None),
        ];
        let named = eligible(&notes, &["manager".into(), "jay".into()], &[]);
        assert_eq!(ids(&named), vec!["j", "m"]);
        // With no recipients, everything but jay's notes is eligible.
        assert_eq!(ids(&eligible(&notes, &[], &[])), vec!["d", "m", "o"]);
    }
}
