//! The studio board (K4): departments as columns, built from the repo's
//! manifest, inbox and verdicts and the sessions, roles and spend a client
//! already holds.
//!
//! Two halves. [`read_files`] is the file half: the departments table, the
//! open notes, the newest verdict per department, this repo's refusals and
//! a failed renewal. It reads the repo's files and keeps no copy; the
//! desktop's `studio_board` command returns it as it is, and the TUI runs
//! it as a background read. [`build`] is pure: it joins that half with the
//! live sessions, their roles and today's spend into a [`Board`]. The
//! desktop repeats the join in `lib/studio/board/board.svelte.ts`, so a
//! rule changed here changes there too.

use std::collections::{BTreeMap, HashMap};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::inbox::{self, InboxNote};
use super::manifest::{self, StudioManifest};
use super::refusals::{self, Refusal};
use super::settings::{self, StudioSettings};
use super::verdicts::{self, Verdict};
use super::wire::{SessionRole, SpendRow};
use crate::{AgentId, AgentStatus};

/// Context at or past this percent of the role's budget shows amber.
pub const BUDGET_AMBER: u32 = 60;
/// Context at or past this percent of the role's budget shows red.
pub const BUDGET_RED: u32 = 90;
/// The What needs you strip lists at most this many refusals.
pub const MAX_REFUSALS: usize = 20;
/// The slug of the column that holds sessions with no department.
pub const UNASSIGNED: &str = "unassigned";
/// The departments file is read up to this many bytes.
const MAX_DEPARTMENTS_BYTES: u64 = 262_144;
/// The renewal log's last line is looked for in this many trailing bytes.
const RENEWAL_TAIL_BYTES: u64 = 16_384;

/// One department: a row of the manifest's departments table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Department {
    pub slug: String,
    pub label: String,
}

/// The departments in `markdown`: the first table, first column, one per
/// row after the header and its separator. Markdown emphasis and a
/// trailing parenthetical ("Animation (sub-department of Art and 3D)")
/// are dropped from the label. A row with an empty cell, or a slug seen
/// already, is skipped.
pub fn departments(markdown: &str) -> Vec<Department> {
    let mut out: Vec<Department> = Vec::new();
    let mut lines = markdown.lines().map(str::trim).peekable();
    // The header is a `|` line followed by a `|---|` separator.
    while let Some(line) = lines.next() {
        if line.starts_with('|') && lines.peek().is_some_and(|next| is_separator(next)) {
            lines.next();
            break;
        }
    }
    for line in lines {
        if !line.starts_with('|') {
            break;
        }
        let cell = line.trim_start_matches('|').split('|').next().unwrap_or("");
        let label = label_of(cell);
        let slug = manifest::slug(&label);
        if slug.is_empty() || out.iter().any(|d| d.slug == slug) {
            continue;
        }
        out.push(Department { slug, label });
    }
    out
}

fn is_separator(line: &str) -> bool {
    line.starts_with('|')
        && line.contains('-')
        && line
            .chars()
            .all(|c| matches!(c, '|' | '-' | ':' | ' ' | '\t'))
}

/// A first cell as the board shows it.
fn label_of(cell: &str) -> String {
    let mut label = cell.replace(['*', '`'], "").trim().to_string();
    if label.ends_with(')') {
        if let Some(open) = label.find(" (") {
            label.truncate(open);
        }
    }
    label.trim().to_string()
}

/// The last renewal, when it failed: what the strip shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FailedRenewal {
    /// Epoch ms, when the line says.
    pub at: Option<i64>,
    /// The orchestrator that was to hand over.
    pub old: Option<String>,
    pub reason: String,
}

/// The file half of the board, as the desktop's `studio_board` returns it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BoardFiles {
    /// The repo root, `/`-separated.
    pub root: String,
    /// The repo has `.kete/studio.toml`.
    pub manifest: bool,
    /// Why the manifest could not be read, in its own words.
    pub error: Option<String>,
    pub departments: Vec<Department>,
    /// The inbox's open notes (new or open), in file-name order. Left out
    /// of the JSON when empty, so a reply can carry its own `notes`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<InboxNote>,
    /// The newest verdict per department slug, over the kinds mapped to it.
    pub verdicts: BTreeMap<String, Verdict>,
    /// This repo's refusals, newest first, at most [`MAX_REFUSALS`].
    pub refusals: Vec<Refusal>,
    /// The last renewal, when it failed.
    pub renewal: Option<FailedRenewal>,
}

/// The file half for `repo_root`, from the real data dir.
pub fn read_files(repo_root: &Path) -> BoardFiles {
    read_files_in(
        repo_root,
        settings::for_repo(repo_root).as_ref(),
        &crate::paths::data_dir(),
    )
}

/// The file half for `repo_root`, with this machine's `studio` entry for it
/// and the data dir the refusal and renewal logs live in.
pub fn read_files_in(
    repo_root: &Path,
    studio: Option<&StudioSettings>,
    data_dir: &Path,
) -> BoardFiles {
    let mut files = BoardFiles {
        root: settings::repo_key(repo_root),
        refusals: repo_refusals(&data_dir.join("studio").join("refusals.jsonl"), repo_root),
        renewal: failed_renewal(&data_dir.join("orchestrator-renewals.jsonl")),
        ..BoardFiles::default()
    };
    let m = match manifest::load(repo_root) {
        Ok(Some(m)) => m,
        Ok(None) => StudioManifest::default(),
        Err(e) => {
            files.error = Some(e);
            StudioManifest::default()
        }
    };
    files.manifest = repo_root.join(manifest::MANIFEST_PATH).is_file();
    if let Some(rel) = &m.departments {
        files.departments = read_departments(&repo_root.join(rel));
    }
    let inbox = studio
        .map(|s| s.studio_inbox.trim())
        .filter(|s| !s.is_empty())
        .or(m.inbox.as_deref());
    if let Some(rel) = inbox {
        files.notes = inbox::read_dir(&repo_root.join(rel))
            .into_iter()
            .filter(|n| n.status.is_open())
            .collect();
    }
    if let Some(rel) = &m.signoffs {
        files.verdicts = newest_per_department(&repo_root.join(rel), &m.kinds);
    }
    files
}

/// The departments in the file at `path`; none when it is missing or too big.
fn read_departments(path: &Path) -> Vec<Department> {
    let fits = std::fs::metadata(path).is_ok_and(|m| m.len() <= MAX_DEPARTMENTS_BYTES);
    match std::fs::read_to_string(path) {
        Ok(text) if fits => departments(&text),
        _ => Vec::new(),
    }
}

/// `kinds` maps a verdict folder to a department slug; several folders may
/// share one, and the newest of them wins.
fn newest_per_department(
    signoffs: &Path,
    kinds: &BTreeMap<String, String>,
) -> BTreeMap<String, Verdict> {
    let mut out: BTreeMap<String, Verdict> = BTreeMap::new();
    for (kind, dept) in kinds {
        let Some(v) = verdicts::newest(signoffs, kind) else {
            continue;
        };
        let slug = manifest::slug(dept);
        let newer = out
            .get(&slug)
            .is_none_or(|old| (&v.at, v.modified) > (&old.at, old.modified));
        if newer {
            out.insert(slug, v);
        }
    }
    out
}

/// This repo's refusals in the log at `path`, newest first.
fn repo_refusals(path: &Path, repo_root: &Path) -> Vec<Refusal> {
    let key = settings::repo_key(repo_root);
    // Other repos' lines share the log: read past them.
    let mut mine: Vec<Refusal> = refusals::read_last_in(path, MAX_REFUSALS * 10)
        .into_iter()
        .filter(|r| settings::same_repo(&settings::repo_key(Path::new(&r.project)), &key))
        .collect();
    mine.reverse();
    mine.truncate(MAX_REFUSALS);
    mine
}

/// The renewal log's last line, when its state is a failure. The renew
/// lane writes `{ at, old, new, note, state, reason }`; `state` is read as
/// `"Failed"` in any case or as `{ "Failed": { "reason": … } }`.
fn failed_renewal(path: &Path) -> Option<FailedRenewal> {
    let mut file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    file.seek(SeekFrom::Start(len.saturating_sub(RENEWAL_TAIL_BYTES)))
        .ok()?;
    let mut tail = Vec::new();
    file.read_to_end(&mut tail).ok()?;
    let tail = String::from_utf8_lossy(&tail);
    let line = tail.lines().rev().find(|l| !l.trim().is_empty())?;
    let v: Value = serde_json::from_str(line).ok()?;
    let state = v.get("state")?;
    let nested = state
        .as_object()
        .and_then(|o| o.iter().find(|(k, _)| k.eq_ignore_ascii_case("failed")))
        .map(|(_, inner)| inner);
    let failed = nested.is_some()
        || state
            .as_str()
            .is_some_and(|s| s.eq_ignore_ascii_case("failed"));
    if !failed {
        return None;
    }
    let reason = v
        .get("reason")
        .and_then(Value::as_str)
        .or_else(|| nested?.get("reason")?.as_str())
        .unwrap_or("no reason given")
        .to_string();
    Some(FailedRenewal {
        at: v.get("at").and_then(Value::as_i64),
        old: v.get("old").and_then(Value::as_str).map(str::to_string),
        reason,
    })
}

/// One live session of the repo, as a client holds it.
#[derive(Debug, Clone, PartialEq)]
pub struct LiveSession {
    pub agent: AgentId,
    pub name: String,
    pub status: AgentStatus,
    pub model: Option<String>,
    pub profile: Option<String>,
    /// Tokens in its context window now (`ContextUsage::tokens`).
    pub context_tokens: Option<u64>,
}

/// Everything [`build`] joins, borrowed from where the client keeps it.
#[derive(Debug, Clone, Copy)]
pub struct BoardInputs<'a> {
    pub files: &'a BoardFiles,
    /// The repo's live sessions only.
    pub sessions: &'a [LiveSession],
    pub roles: &'a HashMap<AgentId, SessionRole>,
    /// The latest spend row per session; only today's counts.
    pub spend: &'a HashMap<AgentId, SpendRow>,
}

/// One session in a column.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionCell {
    pub agent: AgentId,
    /// The role's label, else the session's name.
    pub label: String,
    pub status: AgentStatus,
    pub model: Option<String>,
    pub profile: Option<String>,
    /// Context against the role's budget; None without a budget or a reading.
    pub context_percent_of_budget: Option<u32>,
    /// The role's tier; None for a session with no role.
    pub tier: Option<u8>,
}

/// Today's tokens (input plus output) and the estimated cost.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SpendSum {
    pub tokens: u64,
    pub cost_usd: f64,
}

/// One department's column.
#[derive(Debug, Clone, PartialEq)]
pub struct Column {
    pub department: Department,
    /// Open notes to this department, high priority first, then oldest.
    pub queue: Vec<InboxNote>,
    /// The tier 1 or 2 session with the lowest tier; the first by name on a tie.
    pub lead: Option<SessionCell>,
    /// Every other session in the column, by tier, then name.
    pub runners: Vec<SessionCell>,
    pub last_verdict: Option<Verdict>,
    /// Open notes to Jay about this department, plus its sessions that
    /// need feedback.
    pub blocked: u32,
    pub spend: SpendSum,
}

/// One entry of the What needs you strip.
#[derive(Debug, Clone, PartialEq)]
pub enum NeedsYou {
    /// A session waiting on a prompt or a question.
    Session(SessionCell),
    /// An open note to Jay.
    Note(InboxNote),
    Refusal(Refusal),
    Renewal(FailedRenewal),
}

/// The board: the What needs you strip, then one column per department,
/// with "Unassigned" last when any session has no department.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Board {
    pub needs_you: Vec<NeedsYou>,
    pub columns: Vec<Column>,
}

/// The colour a budget percent shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetLevel {
    Ok,
    Amber,
    Red,
}

pub fn budget_level(percent: u32) -> BudgetLevel {
    if percent >= BUDGET_RED {
        BudgetLevel::Red
    } else if percent >= BUDGET_AMBER {
        BudgetLevel::Amber
    } else {
        BudgetLevel::Ok
    }
}

/// Whether a note is to `who`: its `to`, slugged, is `who`.
fn to_is(n: &InboxNote, who: &str) -> bool {
    n.to.as_deref().is_some_and(|to| manifest::slug(to) == who)
}

/// Join the file half with the live sessions at `now` (epoch ms).
pub fn build(inputs: &BoardInputs, now: i64) -> Board {
    let today = crate::usage_ledger::local_day(now.div_euclid(1000));
    let files = inputs.files;
    let cell = |s: &LiveSession| {
        let role = inputs.roles.get(&s.agent);
        SessionCell {
            agent: s.agent.clone(),
            label: role.map_or_else(|| s.name.clone(), |r| r.label.clone()),
            status: s.status,
            model: s.model.clone(),
            profile: s.profile.clone(),
            context_percent_of_budget: match (s.context_tokens, role.and_then(|r| r.context_budget))
            {
                (Some(t), Some(b)) if b > 0 => Some((t.saturating_mul(100) / b) as u32),
                _ => None,
            },
            tier: role.map(|r| r.tier),
        }
    };
    let spent = |agent: &AgentId| {
        inputs
            .spend
            .get(agent)
            .filter(|r| r.day == today)
            .map_or_else(SpendSum::default, |r| SpendSum {
                tokens: r.input_tokens + r.output_tokens,
                cost_usd: r.cost_usd,
            })
    };

    let mut by_dept: HashMap<&str, Vec<&LiveSession>> = HashMap::new();
    for s in inputs.sessions {
        let dept = inputs
            .roles
            .get(&s.agent)
            .and_then(|r| r.department.as_deref())
            .map(manifest::slug);
        let key = files
            .departments
            .iter()
            .find(|x| dept.as_ref() == Some(&x.slug))
            .map_or(UNASSIGNED, |x| x.slug.as_str());
        by_dept.entry(key).or_default().push(s);
    }

    let unassigned = Department {
        slug: UNASSIGNED.into(),
        label: "Unassigned".into(),
    };
    let mut depts: Vec<&Department> = files.departments.iter().collect();
    if by_dept.contains_key(UNASSIGNED) {
        depts.push(&unassigned);
    }

    let columns = depts
        .into_iter()
        .map(|d| {
            let mut cells: Vec<SessionCell> = by_dept
                .get(d.slug.as_str())
                .map(|v| v.iter().map(|s| cell(s)).collect())
                .unwrap_or_default();
            cells.sort_by(|a, b| {
                (a.tier.unwrap_or(u8::MAX), &a.label).cmp(&(b.tier.unwrap_or(u8::MAX), &b.label))
            });
            let is_unassigned = d.slug == UNASSIGNED;
            let lead_at = (!is_unassigned)
                .then(|| cells.iter().position(|c| matches!(c.tier, Some(1 | 2))))
                .flatten();
            let lead = lead_at.map(|i| cells.remove(i));
            let mut queue: Vec<InboxNote> = files
                .notes
                .iter()
                .filter(|n| !is_unassigned && to_is(n, &d.slug))
                .cloned()
                .collect();
            queue.sort_by(|a, b| {
                (a.priority, a.at.is_none(), &a.at, &a.id).cmp(&(
                    b.priority,
                    b.at.is_none(),
                    &b.at,
                    &b.id,
                ))
            });
            let to_jay = files
                .notes
                .iter()
                .filter(|n| {
                    to_is(n, "jay")
                        && n.about
                            .as_deref()
                            .is_some_and(|a| manifest::slug(a) == d.slug)
                })
                .count();
            let waiting = lead
                .iter()
                .chain(&cells)
                .filter(|c| c.status == AgentStatus::NeedsFeedback)
                .count();
            let spend = lead
                .iter()
                .chain(&cells)
                .fold(SpendSum::default(), |sum, c| {
                    let s = spent(&c.agent);
                    SpendSum {
                        tokens: sum.tokens + s.tokens,
                        cost_usd: sum.cost_usd + s.cost_usd,
                    }
                });
            Column {
                department: d.clone(),
                queue,
                lead,
                runners: cells,
                last_verdict: files.verdicts.get(&d.slug).cloned(),
                blocked: (to_jay + waiting) as u32,
                spend,
            }
        })
        .collect();

    let mut needs_you: Vec<NeedsYou> = inputs
        .sessions
        .iter()
        .filter(|s| s.status == AgentStatus::NeedsFeedback)
        .map(|s| NeedsYou::Session(cell(s)))
        .collect();
    needs_you.extend(
        files
            .notes
            .iter()
            .filter(|n| to_is(n, "jay"))
            .cloned()
            .map(NeedsYou::Note),
    );
    needs_you.extend(files.refusals.iter().cloned().map(NeedsYou::Refusal));
    needs_you.extend(files.renewal.clone().map(NeedsYou::Renewal));

    Board { needs_you, columns }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::studio::inbox::{NoteStatus, Priority};
    use std::path::PathBuf;

    /// The real departments file's shape: a heading, a paragraph, then a
    /// seven-column table whose first cells carry sub-department notes.
    const TABLE: &str = "# Departments

Every aspect has one department.

| Department | Owns | Source of truth | Data home | Generated | Review rubric | Harness |
|---|---|---|---|---|---|---|
| World and weather | clock, seasons | specs | data | out | bands | world.* |
| Ecology and world simulation (sub-department of World and weather; its lead leads it) | grid | none yet | none | none | standard | none |
| Gameplay systems | movement | specs | csv | none | review | play.ps1 |
| NPCs and relationships (sub-department of Gameplay systems) | ai | none | none | none | standard | none |
| Economy and progression | items | specs | csv | none | validator | econ.* |
| UX and UI | hud | specs | ui | none | review | ui.* |
| **Art and 3D** | models | specs | art | out | review | art.* |
| Animation (sub-department of Art and 3D) | rigs | specs | anim | out | review | anim.* |
| Audio | sound | specs | audio | out | review | audio.* |
| Creative and narrative | story | specs | text | none | review | none |
| Web, community, release | site | specs | web | none | review | web.* |
| Production and tooling | tools | specs | tools | none | review | tools.* |

| Second | table |
|---|---|
| Not a department | x |
";

    #[test]
    fn departments_come_from_the_first_table_first_column() {
        let d = departments(TABLE);
        let slugs: Vec<&str> = d.iter().map(|d| d.slug.as_str()).collect();
        assert_eq!(
            slugs,
            [
                "world-and-weather",
                "ecology-and-world-simulation",
                "gameplay-systems",
                "npcs-and-relationships",
                "economy-and-progression",
                "ux-and-ui",
                "art-and-3d",
                "animation",
                "audio",
                "creative-and-narrative",
                "web-community-release",
                "production-and-tooling",
            ]
        );
        assert_eq!(d[6].label, "Art and 3D", "emphasis dropped");
        assert_eq!(d[7].label, "Animation", "trailing note dropped");
    }

    #[test]
    fn a_table_with_no_rows_or_no_table_has_no_departments() {
        assert!(departments("| Department | Owns |\n|---|---|\n").is_empty());
        assert!(departments("| Department | Owns |\n|---|---|\n\nprose\n| A | b |").is_empty());
        assert!(departments("# Just prose\n\nNo table here.").is_empty());
        assert!(departments("").is_empty());
    }

    #[test]
    fn rows_stop_at_the_first_line_outside_the_table() {
        let md = "| D |\n|:--|\n| Audio |\nprose\n| Later |\n";
        let d = departments(md);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].slug, "audio");
    }

    #[test]
    fn a_missing_departments_file_has_no_departments() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_departments(&dir.path().join("nope.md")).is_empty());
    }

    #[test]
    fn budget_colours_turn_at_sixty_and_ninety() {
        assert_eq!(budget_level(59), BudgetLevel::Ok);
        assert_eq!(budget_level(60), BudgetLevel::Amber);
        assert_eq!(budget_level(89), BudgetLevel::Amber);
        assert_eq!(budget_level(90), BudgetLevel::Red);
    }

    fn note(id: &str, to: &str, about: Option<&str>, priority: Priority, at: &str) -> InboxNote {
        InboxNote {
            id: id.into(),
            path: PathBuf::from(format!("/repo/inbox/{id}.md")),
            title: id.into(),
            status: NoteStatus::Open,
            to: Some(to.into()),
            about: about.map(str::to_string),
            priority,
            at: (!at.is_empty()).then(|| at.to_string()),
            by: None,
        }
    }

    fn session(id: &str, status: AgentStatus, tokens: Option<u64>) -> LiveSession {
        LiveSession {
            agent: AgentId(id.into()),
            name: format!("session {id}"),
            status,
            model: Some("opus".into()),
            profile: Some("main".into()),
            context_tokens: tokens,
        }
    }

    fn role(id: &str, label: &str, tier: u8, dept: Option<&str>) -> (AgentId, SessionRole) {
        (
            AgentId(id.into()),
            SessionRole {
                agent: AgentId(id.into()),
                card: label.into(),
                label: label.into(),
                tier,
                department: dept.map(str::to_string),
                permission: "allowlist".into(),
                context_budget: Some(200_000),
            },
        )
    }

    fn spend(id: &str, day: &str, input: u64, output: u64, cost: f64) -> (AgentId, SpendRow) {
        (
            AgentId(id.into()),
            SpendRow {
                agent: AgentId(id.into()),
                day: day.into(),
                input_tokens: input,
                output_tokens: output,
                cache_read_tokens: 9_999,
                cache_write_tokens: 9_999,
                cost_usd: cost,
                tool_uses: 1,
                turns: 1,
            },
        )
    }

    const NOW: i64 = 1_792_000_000_000;

    struct Fixture {
        files: BoardFiles,
        sessions: Vec<LiveSession>,
        roles: HashMap<AgentId, SessionRole>,
        spend: HashMap<AgentId, SpendRow>,
    }

    impl Fixture {
        fn board(&self) -> Board {
            let inputs = BoardInputs {
                files: &self.files,
                sessions: &self.sessions,
                roles: &self.roles,
                spend: &self.spend,
            };
            build(&inputs, NOW)
        }
    }

    fn fixture() -> Fixture {
        let today = crate::usage_ledger::local_day(NOW / 1000);
        Fixture {
            files: BoardFiles {
                root: "/repo".into(),
                manifest: true,
                departments: vec![
                    Department {
                        slug: "audio".into(),
                        label: "Audio".into(),
                    },
                    Department {
                        slug: "art-and-3d".into(),
                        label: "Art and 3D".into(),
                    },
                ],
                notes: vec![
                    note(
                        "a-low",
                        "audio",
                        None,
                        Priority::Low,
                        "2026-10-01T00:00:00Z",
                    ),
                    note(
                        "a-new",
                        "audio",
                        None,
                        Priority::High,
                        "2026-10-03T00:00:00Z",
                    ),
                    note(
                        "a-old",
                        "audio",
                        None,
                        Priority::High,
                        "2026-10-02T00:00:00Z",
                    ),
                    note("art", "Art and 3D", None, Priority::Normal, ""),
                    note("jay-audio", "jay", Some("Audio"), Priority::Normal, ""),
                    note("jay-other", "Jay", None, Priority::Normal, ""),
                    note("orch", "orchestrator", Some("Audio"), Priority::Normal, ""),
                ],
                verdicts: BTreeMap::new(),
                refusals: vec![Refusal {
                    at: 5,
                    project: "/repo".into(),
                    agent: None,
                    role: Some("runner".into()),
                    reason: "A0 holds spawns".into(),
                }],
                renewal: Some(FailedRenewal {
                    at: Some(7),
                    old: Some("o1".into()),
                    reason: "no hook in 60 s".into(),
                }),
                error: None,
            },
            sessions: vec![
                session("lead2", AgentStatus::Running, Some(130_000)),
                session("lead1", AgentStatus::Finished, Some(10_000)),
                session("run", AgentStatus::NeedsFeedback, None),
                session("mgr", AgentStatus::Running, None),
                session("plain", AgentStatus::Finished, None),
                session("stray", AgentStatus::Finished, None),
            ],
            roles: [
                role("lead2", "lead audio b", 2, Some("audio")),
                role("lead1", "lead audio", 1, Some("Audio")),
                role("run", "runner", 3, Some("audio")),
                role("mgr", "manager", 1, None),
                role("stray", "lead tide", 1, Some("tides")),
            ]
            .into_iter()
            .collect(),
            spend: [
                spend("lead1", &today, 100, 50, 0.5),
                spend("run", &today, 10, 5, 0.25),
                spend("lead2", "2000-01-01", 1_000_000, 0, 99.0),
            ]
            .into_iter()
            .collect(),
        }
    }

    #[test]
    fn build_fills_each_department_column() {
        let board = fixture().board();
        let slugs: Vec<&str> = board
            .columns
            .iter()
            .map(|c| c.department.slug.as_str())
            .collect();
        assert_eq!(
            slugs,
            ["audio", "art-and-3d", UNASSIGNED],
            "Unassigned last"
        );

        let audio = &board.columns[0];
        let queue: Vec<&str> = audio.queue.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(queue, ["a-old", "a-new", "a-low"], "priority, then oldest");
        let lead = audio.lead.as_ref().expect("a lead");
        assert_eq!(lead.agent.0, "lead1", "tier 1 before tier 2");
        assert_eq!(lead.context_percent_of_budget, Some(5));
        let runners: Vec<&str> = audio.runners.iter().map(|c| c.agent.0.as_str()).collect();
        assert_eq!(runners, ["lead2", "run"], "the second lead, then tier 3");
        assert_eq!(audio.runners[0].context_percent_of_budget, Some(65));
        assert_eq!(
            audio.blocked, 2,
            "one note to Jay about it, one session waiting"
        );
        assert_eq!(audio.spend.tokens, 165, "today's input and output only");
        assert!((audio.spend.cost_usd - 0.75).abs() < 1e-9);

        let art = &board.columns[1];
        assert_eq!(art.queue.len(), 1, "a note to the label counts");
        assert!(art.lead.is_none() && art.runners.is_empty());
        assert_eq!(art.blocked, 0);

        let un = &board.columns[2];
        assert!(un.lead.is_none(), "Unassigned has no lead");
        let un_ids: Vec<&str> = un.runners.iter().map(|c| c.agent.0.as_str()).collect();
        assert_eq!(
            un_ids,
            ["stray", "mgr", "plain"],
            "by tier, then label; no role last"
        );
        assert!(un.queue.is_empty());
        assert_eq!(un.runners[2].label, "session plain", "no role: its name");
    }

    #[test]
    fn needs_you_lists_sessions_then_notes_then_refusals_then_a_renewal() {
        let board = fixture().board();
        let kinds: Vec<String> = board
            .needs_you
            .iter()
            .map(|n| match n {
                NeedsYou::Session(c) => format!("session {}", c.agent.0),
                NeedsYou::Note(n) => format!("note {}", n.id),
                NeedsYou::Refusal(r) => format!("refusal {}", r.reason),
                NeedsYou::Renewal(r) => format!("renewal {}", r.reason),
            })
            .collect();
        assert_eq!(
            kinds,
            [
                "session run",
                "note jay-audio",
                "note jay-other",
                "refusal A0 holds spawns",
                "renewal no hook in 60 s",
            ]
        );
    }

    #[test]
    fn no_sessions_without_a_department_means_no_unassigned_column() {
        let mut fx = fixture();
        fx.sessions.retain(|s| s.agent.0.starts_with("lead"));
        let board = fx.board();
        assert!(board
            .columns
            .iter()
            .all(|c| c.department.slug != UNASSIGNED));
    }

    #[test]
    fn read_files_reads_the_manifest_inbox_verdicts_and_logs() {
        let repo = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let r = repo.path();
        std::fs::create_dir_all(r.join(".kete")).unwrap();
        std::fs::write(
            r.join(".kete/studio.toml"),
            "version = 1\ninbox = \"notes\"\ndepartments = \"DEPTS.md\"\nsignoffs = \"signoffs\"\n\
             [kinds]\nart = \"Art and 3D\"\nmodels = \"art-and-3d\"\n",
        )
        .unwrap();
        std::fs::write(r.join("DEPTS.md"), TABLE).unwrap();
        std::fs::create_dir_all(r.join("notes")).unwrap();
        std::fs::write(
            r.join("notes/one.md"),
            "---\nstatus: open\nto: audio\n---\nOne\n",
        )
        .unwrap();
        std::fs::write(
            r.join("notes/two.md"),
            "---\nstatus: done\nto: audio\n---\nTwo\n",
        )
        .unwrap();
        for (kind, id, at) in [("art", "a1", "2026-10-01"), ("models", "m1", "2026-10-05")] {
            std::fs::create_dir_all(r.join("signoffs").join(kind)).unwrap();
            std::fs::write(
                r.join("signoffs").join(kind).join(format!("{id}.json")),
                format!("{{\"verdict\":\"pass\",\"at\":\"{at}\"}}"),
            )
            .unwrap();
        }
        let refusal = |project: &str, reason: &str| Refusal {
            at: 1,
            project: project.into(),
            agent: None,
            role: None,
            reason: reason.into(),
        };
        let log = data.path().join("studio/refusals.jsonl");
        refusals::append_in(&log, &refusal(&r.to_string_lossy(), "first")).unwrap();
        refusals::append_in(&log, &refusal("/elsewhere", "theirs")).unwrap();
        refusals::append_in(&log, &refusal(&settings::repo_key(r), "second")).unwrap();
        std::fs::write(
            data.path().join("orchestrator-renewals.jsonl"),
            "{\"at\":1,\"old\":\"o\",\"new\":\"n\",\"state\":\"Done\"}\n\
             {\"at\":2,\"old\":\"o\",\"new\":null,\"state\":{\"Failed\":{\"reason\":\"no hook\"}}}\n",
        )
        .unwrap();

        let files = read_files_in(r, None, data.path());
        assert!(files.manifest);
        assert_eq!(files.error, None);
        assert_eq!(files.departments.len(), 12);
        let ids: Vec<&str> = files.notes.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids, ["one"], "open notes only");
        assert_eq!(
            files.verdicts["art-and-3d"].id, "m1",
            "newest across both kinds"
        );
        let reasons: Vec<&str> = files.refusals.iter().map(|r| r.reason.as_str()).collect();
        assert_eq!(reasons, ["second", "first"], "this repo's, newest first");
        let renewal = files.renewal.expect("the last renewal failed");
        assert_eq!(renewal.reason, "no hook");
        assert_eq!(renewal.at, Some(2));

        // The machine's inbox setting wins over the manifest's.
        let studio = StudioSettings {
            studio_inbox: "elsewhere".into(),
            ..StudioSettings::default()
        };
        assert!(read_files_in(r, Some(&studio), data.path())
            .notes
            .is_empty());
    }

    #[test]
    fn a_repo_with_no_manifest_reads_empty() {
        let repo = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let files = read_files_in(repo.path(), None, data.path());
        assert!(!files.manifest);
        assert!(files.departments.is_empty() && files.notes.is_empty());
        assert_eq!(files.renewal, None, "no log, no renewal");
    }

    #[test]
    fn a_renewal_that_did_not_fail_is_not_shown() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("r.jsonl");
        for state in ["\"Done\"", "\"Scheduled\""] {
            std::fs::write(&log, format!("{{\"at\":1,\"state\":{state}}}\n")).unwrap();
            assert_eq!(failed_renewal(&log), None);
        }
        std::fs::write(
            &log,
            "{\"at\":1,\"state\":\"failed\",\"reason\":\"gone\"}\n\n",
        )
        .unwrap();
        assert_eq!(failed_renewal(&log).unwrap().reason, "gone");
        std::fs::write(&log, "not json\n").unwrap();
        assert_eq!(failed_renewal(&log), None);
    }
}
