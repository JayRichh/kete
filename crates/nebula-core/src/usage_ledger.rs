//! The usage ledger (K7): token totals tallied from session transcripts,
//! and the CSV `kete usage --csv` writes.
//!
//! Claude's statusLine carries no running token total (its
//! `context_window` totals are the latest response's context), so tokens
//! come from the transcript: every `assistant` line's `message.usage`,
//! counted once per `message.id`. The cost is the statusLine's
//! `cost.total_cost_usd`, a list-price estimate that accumulates over the
//! session and resets on `/clear` ([`cost_delta`]).

use crate::studio::wire::LedgerRow;
use serde_json::Value;
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// Tokens one or more assistant messages used.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Tally {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
}

impl Tally {
    /// The tally a `message.usage` object reports.
    fn of_usage(u: &Value) -> Self {
        let n = |k: &str| u.get(k).and_then(Value::as_u64).unwrap_or(0);
        Self {
            input_tokens: n("input_tokens"),
            output_tokens: n("output_tokens"),
            cache_read_tokens: n("cache_read_input_tokens"),
            cache_write_tokens: n("cache_creation_input_tokens"),
        }
    }

    pub fn is_zero(&self) -> bool {
        *self == Self::default()
    }

    pub fn add(&mut self, o: Tally) {
        self.input_tokens += o.input_tokens;
        self.output_tokens += o.output_tokens;
        self.cache_read_tokens += o.cache_read_tokens;
        self.cache_write_tokens += o.cache_write_tokens;
    }

    /// Each field at the larger of the two.
    fn max(self, o: Tally) -> Tally {
        Tally {
            input_tokens: self.input_tokens.max(o.input_tokens),
            output_tokens: self.output_tokens.max(o.output_tokens),
            cache_read_tokens: self.cache_read_tokens.max(o.cache_read_tokens),
            cache_write_tokens: self.cache_write_tokens.max(o.cache_write_tokens),
        }
    }

    /// What `self` has over `prev`, field by field (never below 0).
    fn over(self, prev: Tally) -> Tally {
        Tally {
            input_tokens: self.input_tokens.saturating_sub(prev.input_tokens),
            output_tokens: self.output_tokens.saturating_sub(prev.output_tokens),
            cache_read_tokens: self
                .cache_read_tokens
                .saturating_sub(prev.cache_read_tokens),
            cache_write_tokens: self
                .cache_write_tokens
                .saturating_sub(prev.cache_write_tokens),
        }
    }
}

/// One session's transcripts, read incrementally: the main transcript and
/// every subagent transcript Claude writes beside it
/// (`<dir>/<session id>/subagents/agent-<id>.jsonl`).
#[derive(Debug, Clone, Default)]
pub struct TranscriptTally {
    /// Bytes read so far, per file: always the end of a whole line, so a
    /// line still being written is read again once it is complete.
    pub offsets: HashMap<PathBuf, u64>,
    /// What each `message.id` has been counted at. Claude writes one line
    /// per content block of a message, each with the message's usage; a
    /// later line that reports more adds only the difference.
    pub seen_ids: HashMap<String, Tally>,
    /// Epoch ms: lines stamped before it are history this tally does not
    /// count (a resumed conversation, or a session older than the daemon).
    pub since_ms: Option<i64>,
}

impl TranscriptTally {
    pub fn new(since_ms: Option<i64>) -> Self {
        Self {
            since_ms,
            ..Self::default()
        }
    }

    /// The tokens added to `path` and the subagent transcripts beside it
    /// since the last call.
    pub fn read_new(&mut self, path: &Path) -> Tally {
        let mut tally = self.read_file(path);
        for file in subagent_files(path) {
            tally.add(self.read_file(&file));
        }
        tally
    }

    fn read_file(&mut self, path: &Path) -> Tally {
        let mut tally = Tally::default();
        let Ok(mut file) = std::fs::File::open(path) else {
            return tally;
        };
        let len = file.metadata().map(|m| m.len()).unwrap_or(0);
        let mut offset = self.offsets.get(path).copied().unwrap_or(0);
        // Rewritten from the start: read it again; seen ids keep what was
        // counted from counting twice.
        if len < offset {
            offset = 0;
        }
        let mut bytes = Vec::new();
        if file.seek(SeekFrom::Start(offset)).is_err()
            || file.take(len - offset).read_to_end(&mut bytes).is_err()
        {
            return tally;
        }
        let Some(end) = bytes.iter().rposition(|&b| b == b'\n') else {
            return tally;
        };
        for line in String::from_utf8_lossy(&bytes[..end]).lines() {
            tally.add(self.line(line));
        }
        self.offsets
            .insert(path.to_path_buf(), offset + end as u64 + 1);
        tally
    }

    /// The tokens one transcript line adds.
    fn line(&mut self, line: &str) -> Tally {
        let Ok(v) = serde_json::from_str::<Value>(line.trim()) else {
            return Tally::default();
        };
        if v.get("type").and_then(Value::as_str) != Some("assistant") {
            return Tally::default();
        }
        let message = v.get("message");
        let Some(usage) = message.and_then(|m| m.get("usage")) else {
            return Tally::default();
        };
        let now = Tally::of_usage(usage);
        let history = match (self.since_ms, v.get("timestamp").and_then(Value::as_str)) {
            (Some(since), Some(stamp)) => crate::usage::stamp_ms(stamp).is_some_and(|t| t < since),
            _ => false,
        };
        let id = message
            .and_then(|m| m.get("id"))
            .or_else(|| v.get("uuid"))
            .and_then(Value::as_str);
        let Some(id) = id else {
            return if history { Tally::default() } else { now };
        };
        let prev = self.seen_ids.get(id).copied().unwrap_or_default();
        let counted = prev.max(now);
        self.seen_ids.insert(id.to_string(), counted);
        if history {
            Tally::default()
        } else {
            counted.over(prev)
        }
    }
}

/// The subagent transcripts beside `transcript`, by name.
fn subagent_files(transcript: &Path) -> Vec<PathBuf> {
    let (Some(dir), Some(stem)) = (transcript.parent(), transcript.file_stem()) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(dir.join(stem).join("subagents")) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "jsonl"))
        .collect();
    files.sort();
    files
}

/// What a turn added to the session's cost estimate: `now` less `prev`, or
/// `now` whole when there is no earlier reading or the value dropped (a
/// `/clear` starts the estimate again from 0).
pub fn cost_delta(prev: Option<f64>, now: f64) -> f64 {
    match prev {
        Some(prev) if now >= prev => now - prev,
        _ => now.max(0.0),
    }
}

/// The local date at `epoch_secs`, `YYYY-MM-DD`.
pub fn local_day(epoch_secs: i64) -> String {
    day_at(epoch_secs, crate::usage_window::local)
}

/// The date at `epoch_secs` in the zone `tz` (seconds east of UTC at an
/// instant), `YYYY-MM-DD`.
pub fn day_at(epoch_secs: i64, tz: impl Fn(i64) -> i32) -> String {
    let days = (epoch_secs + tz(epoch_secs) as i64).div_euclid(86_400);
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// When a session was created, from its id (a ULID); None for an id that
/// is not one.
pub fn created_ms(agent: &str) -> Option<i64> {
    ulid::Ulid::from_string(agent)
        .ok()
        .map(|u| u.timestamp_ms() as i64)
}

/// The CSV header `kete usage --csv` writes.
pub const CSV_HEADER: &str = "date,project,branch,session,role,account,model,input_tokens,output_tokens,cache_read_tokens,cache_write_tokens,cost_usd_estimate,tool_uses,turns,status,pr_url";

/// The cost estimate as the CSV and the table write it: four decimals.
pub fn cost_text(usd: f64) -> String {
    format!("{usd:.4}")
}

/// `rows` as CSV, header first (RFC 4180 quoting).
pub fn write_csv(rows: &[LedgerRow], out: &mut impl Write) -> std::io::Result<()> {
    writeln!(out, "{CSV_HEADER}")?;
    for row in rows {
        let s = &row.spend;
        let opt = |v: &Option<String>| v.clone().unwrap_or_default();
        let fields = [
            s.day.clone(),
            row.project.clone(),
            opt(&row.branch),
            row.session.clone(),
            opt(&row.role_card),
            opt(&row.profile),
            opt(&row.model),
            s.input_tokens.to_string(),
            s.output_tokens.to_string(),
            s.cache_read_tokens.to_string(),
            s.cache_write_tokens.to_string(),
            cost_text(s.cost_usd),
            s.tool_uses.to_string(),
            s.turns.to_string(),
            row.status.as_str().to_string(),
            opt(&row.pr_url),
        ];
        let line: Vec<String> = fields.iter().map(|f| csv_field(f)).collect();
        writeln!(out, "{}", line.join(","))?;
    }
    Ok(())
}

fn csv_field(f: &str) -> String {
    if f.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", f.replace('"', "\"\""))
    } else {
        f.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::studio::wire::SpendRow;
    use crate::{AgentId, AgentStatus};

    /// One assistant line, as Claude writes it, by hand.
    fn assistant(id: &str, stamp: &str, input: u64, output: u64) -> String {
        serde_json::json!({
            "type": "assistant",
            "timestamp": stamp,
            "message": {
                "id": id,
                "role": "assistant",
                "usage": {
                    "input_tokens": input,
                    "output_tokens": output,
                    "cache_read_input_tokens": 7,
                    "cache_creation_input_tokens": 3,
                },
            },
        })
        .to_string()
    }

    fn append(path: &Path, text: &str) {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap();
        f.write_all(text.as_bytes()).unwrap();
    }

    #[test]
    fn a_tally_counts_each_message_once_waits_for_whole_lines_and_reads_subagents() {
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("sid-1.jsonl");
        let user = r#"{"type":"user","message":{"role":"user","content":"hi"}}"#;
        let a1 = assistant("msg_1", "2026-10-21T10:00:00.000Z", 100, 10);
        // The same message again, one line per content block.
        append(&main, &format!("{user}\n{a1}\n{a1}\n"));
        // A truncated last line: not counted until it is complete.
        let a2 = assistant("msg_2", "2026-10-21T10:00:05.000Z", 50, 5);
        let (head, tail) = a2.split_at(20);
        append(&main, head);
        let subagents = dir.path().join("sid-1").join("subagents");
        std::fs::create_dir_all(&subagents).unwrap();
        append(
            &subagents.join("agent-def456.jsonl"),
            &format!(
                "{}\n",
                assistant("msg_sub", "2026-10-21T10:00:02.000Z", 1000, 100)
            ),
        );

        let mut tally = TranscriptTally::new(None);
        let first = tally.read_new(&main);
        assert_eq!(
            first,
            Tally {
                input_tokens: 1100,
                output_tokens: 110,
                cache_read_tokens: 14,
                cache_write_tokens: 6,
            }
        );
        // Nothing new: nothing counted.
        assert!(tally.read_new(&main).is_zero());
        // The truncated line completes, and a later block of msg_1 reports
        // more output: only what is new counts.
        let a1_more = assistant("msg_1", "2026-10-21T10:00:00.000Z", 100, 25);
        append(&main, &format!("{tail}\n{a1_more}\n"));
        let second = tally.read_new(&main);
        assert_eq!((second.input_tokens, second.output_tokens), (50, 20));
        assert_eq!(
            (second.cache_read_tokens, second.cache_write_tokens),
            (7, 3)
        );
    }

    #[test]
    fn history_before_the_cutoff_is_not_counted() {
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("sid-2.jsonl");
        let old = assistant("msg_old", "2026-10-20T09:00:00.000Z", 900, 90);
        let new = assistant("msg_new", "2026-10-21T09:00:00.000Z", 10, 1);
        append(&main, &format!("{old}\n{new}\n"));
        let since = crate::usage::stamp_ms("2026-10-21T00:00:00Z");
        let got = TranscriptTally::new(since).read_new(&main);
        assert_eq!((got.input_tokens, got.output_tokens), (10, 1));
    }

    #[test]
    fn a_rewritten_transcript_is_read_again_without_recounting() {
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("sid-3.jsonl");
        let a = assistant("msg_a", "2026-10-21T09:00:00.000Z", 10, 1);
        append(&main, &format!("{a}\n{a}\n{a}\n"));
        let mut tally = TranscriptTally::new(None);
        assert_eq!(tally.read_new(&main).input_tokens, 10);
        std::fs::write(&main, format!("{a}\n")).unwrap();
        let b = assistant("msg_b", "2026-10-21T09:01:00.000Z", 5, 1);
        append(&main, &format!("{b}\n"));
        assert_eq!(tally.read_new(&main).input_tokens, 5);
    }

    #[test]
    fn cost_deltas_follow_the_estimate_across_a_clear() {
        assert_eq!(cost_delta(None, 0.05), 0.05);
        assert!((cost_delta(Some(0.05), 0.08) - 0.03).abs() < 1e-12);
        assert_eq!(cost_delta(Some(0.08), 0.08), 0.0);
        // `/clear` started the estimate again: the new value is all new.
        assert_eq!(cost_delta(Some(0.80), 0.02), 0.02);
    }

    #[test]
    fn local_days_turn_at_local_midnight() {
        // NZDT, 13 hours east.
        let nz = |_| 13 * 3600;
        // 2026-10-21 23:59 and 2026-10-22 00:00 NZDT.
        let before = crate::usage::stamp_ms("2026-10-21T10:59:00Z").unwrap() / 1000;
        assert_eq!(day_at(before, nz), "2026-10-21");
        assert_eq!(day_at(before + 60, nz), "2026-10-22");
        assert_eq!(day_at(before + 60, |_| 0), "2026-10-21");
        assert_eq!(day_at(0, |_| 0), "1970-01-01");
        assert_eq!(day_at(951_782_400, |_| 0), "2000-02-29");
    }

    #[test]
    fn a_ulid_says_when_its_session_was_created() {
        let id = AgentId::generate();
        let at = created_ms(id.as_str()).unwrap();
        assert!((crate::clock::now_ms() - at).abs() < 60_000);
        assert_eq!(created_ms("not-a-ulid"), None);
    }

    #[test]
    fn the_csv_quotes_commas_and_quotes() {
        let row = LedgerRow {
            spend: SpendRow {
                agent: AgentId("a".into()),
                day: "2026-10-21".into(),
                input_tokens: 1500,
                output_tokens: 300,
                cache_read_tokens: 14,
                cache_write_tokens: 6,
                cost_usd: 0.05 + 0.030000000000000002,
                tool_uses: 3,
                turns: 2,
            },
            session: r#"fix login, "the" redirect"#.into(),
            project: "demo".into(),
            branch: Some("main".into()),
            role_card: Some("runner".into()),
            profile: None,
            model: Some("opus".into()),
            status: AgentStatus::Finished,
            pr_url: None,
        };
        let mut out = Vec::new();
        write_csv(&[row], &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[0], CSV_HEADER);
        assert_eq!(lines[0].split(',').count(), 16);
        assert_eq!(
            lines[1],
            r#"2026-10-21,demo,main,"fix login, ""the"" redirect",runner,,opus,1500,300,14,6,0.0800,3,2,finished,"#
        );
    }
}
