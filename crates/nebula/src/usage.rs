//! `kete usage` (K7): the usage ledger as a table or CSV.
//!
//! The daemon keeps the ledger (`session_spend`, one row per session per
//! local day); this asks it for a range of days and prints the rows: a
//! table grouped by session, day or role, or every row as CSV
//! (`nebula_core::usage_ledger::write_csv`). With no dates, today.

use anyhow::{bail, Result};
use nebula_core::studio::wire::LedgerRow;
use nebula_core::usage_ledger::{cost_text, local_day, write_csv};

/// Everything `kete usage` takes.
#[derive(Debug, Clone)]
pub struct UsageArgs {
    /// First local date, `YYYY-MM-DD`.
    pub since: Option<String>,
    /// Last local date, `YYYY-MM-DD`.
    pub until: Option<String>,
    pub project: Option<String>,
    /// `session` (default), `day` or `role`.
    pub by: Option<String>,
    pub csv: bool,
}

pub fn run(args: UsageArgs) -> Result<()> {
    for date in [&args.since, &args.until].into_iter().flatten() {
        if !is_date(date) {
            bail!("{date}: expected a date as YYYY-MM-DD");
        }
    }
    let (since, until) = match (args.since, args.until) {
        (None, None) => {
            let today = local_day(nebula_core::clock::now_secs() as i64);
            (Some(today.clone()), Some(today))
        }
        range => range,
    };
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let rows = rt.block_on(async {
        let project = match &args.project {
            Some(query) => Some(project_id(query).await?),
            None => None,
        };
        nebula_tui::ipc_studio::usage_ledger(since.clone(), until.clone(), project).await
    })?;
    let mut out = std::io::stdout().lock();
    if args.csv {
        write_csv(&rows, &mut out)?;
        return Ok(());
    }
    use std::io::Write;
    let range = match (since.as_deref(), until.as_deref()) {
        (Some(a), Some(b)) if a == b => a.to_string(),
        (Some(a), Some(b)) => format!("{a} to {b}"),
        (Some(a), None) => format!("since {a}"),
        (None, Some(b)) => format!("until {b}"),
        (None, None) => "every day".into(),
    };
    if rows.is_empty() {
        writeln!(out, "No usage recorded for {range}.")?;
        return Ok(());
    }
    writeln!(
        out,
        "Usage, {range} (cost is Claude Code's estimate, not a bill)\n"
    )?;
    write!(
        out,
        "{}",
        table(&rows, args.by.as_deref().unwrap_or("session"))
    )?;
    Ok(())
}

/// `YYYY-MM-DD`, digits where digits go.
fn is_date(s: &str) -> bool {
    s.len() == 10
        && s.bytes().enumerate().all(|(i, b)| {
            if i == 4 || i == 7 {
                b == b'-'
            } else {
                b.is_ascii_digit()
            }
        })
}

/// The id of the project `query` names: its id, or its name in any case.
async fn project_id(query: &str) -> Result<nebula_core::ProjectId> {
    use nebula_core::codec::{read_frame, write_frame};
    use nebula_core::{ClientRequest, ServerEvent};
    let mut conn = nebula_tui::ipc::connect_or_spawn().await?;
    write_frame(&mut conn.stream, &ClientRequest::Subscribe).await?;
    let projects = loop {
        match read_frame::<ServerEvent, _>(&mut conn.stream).await? {
            Some(ServerEvent::Snapshot { projects, .. }) => break projects,
            Some(_) => continue,
            None => bail!("the daemon closed the connection before sending its projects"),
        }
    };
    let q = query.trim();
    if let Some(p) = projects.iter().find(|p| p.id.as_str() == q) {
        return Ok(p.id.clone());
    }
    let named: Vec<_> = projects
        .iter()
        .filter(|p| p.name.eq_ignore_ascii_case(q))
        .collect();
    match named.as_slice() {
        [p] => Ok(p.id.clone()),
        [] => bail!("no kete project named `{q}`"),
        _ => bail!("{} projects are named `{q}`: give its id", named.len()),
    }
}

/// The rows summed by `by` (`session`, `day` or `role`), in the order
/// each group first appears, with a total line.
fn table(rows: &[LedgerRow], by: &str) -> String {
    let key = |r: &LedgerRow| match by {
        "day" => r.spend.day.clone(),
        "role" => r.role_card.clone().unwrap_or_else(|| "(no role)".into()),
        _ => format!("{} / {}", r.project, r.session),
    };
    let mut groups: Vec<(String, Sum)> = Vec::new();
    let mut total = Sum::default();
    for row in rows {
        let k = key(row);
        let at = match groups.iter().position(|(g, _)| *g == k) {
            Some(at) => at,
            None => {
                groups.push((k, Sum::default()));
                groups.len() - 1
            }
        };
        groups[at].1.add(row);
        total.add(row);
    }
    groups.push(("total".into(), total));
    let head = match by {
        "day" => "day",
        "role" => "role",
        _ => "session",
    };
    let width = groups
        .iter()
        .map(|(k, _)| k.chars().count())
        .chain([head.len()])
        .max()
        .unwrap_or(0);
    let mut out = format!(
        "{head:<width$}  {:>10}  {:>10}  {:>12}  {:>12}  {:>9}  {:>9}  {:>5}\n",
        "input", "output", "cache read", "cache write", "est. US$", "tool uses", "turns"
    );
    for (k, s) in &groups {
        out.push_str(&format!(
            "{k:<width$}  {:>10}  {:>10}  {:>12}  {:>12}  {:>9}  {:>9}  {:>5}\n",
            s.input,
            s.output,
            s.cache_read,
            s.cache_write,
            cost_text(s.cost),
            s.tool_uses,
            s.turns
        ));
    }
    out
}

#[derive(Debug, Default)]
struct Sum {
    input: u64,
    output: u64,
    cache_read: u64,
    cache_write: u64,
    cost: f64,
    tool_uses: u64,
    turns: u64,
}

impl Sum {
    fn add(&mut self, r: &LedgerRow) {
        let s = &r.spend;
        self.input += s.input_tokens;
        self.output += s.output_tokens;
        self.cache_read += s.cache_read_tokens;
        self.cache_write += s.cache_write_tokens;
        self.cost += s.cost_usd;
        self.tool_uses += u64::from(s.tool_uses);
        self.turns += u64::from(s.turns);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nebula_core::studio::wire::SpendRow;
    use nebula_core::{AgentId, AgentStatus};

    fn row(day: &str, session: &str, role: Option<&str>, input: u64, cost: f64) -> LedgerRow {
        LedgerRow {
            spend: SpendRow {
                agent: AgentId(session.into()),
                day: day.into(),
                input_tokens: input,
                output_tokens: input / 10,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
                cost_usd: cost,
                tool_uses: 2,
                turns: 1,
            },
            session: session.into(),
            project: "demo".into(),
            branch: None,
            role_card: role.map(str::to_string),
            profile: None,
            model: None,
            status: AgentStatus::Finished,
            pr_url: None,
        }
    }

    #[test]
    fn dates_are_checked() {
        assert!(is_date("2026-10-21"));
        for bad in ["2026-1-21", "21-10-2026", "2026/10/21", "today"] {
            assert!(!is_date(bad), "{bad}");
        }
    }

    #[test]
    fn the_table_sums_by_session_day_or_role_with_a_total() {
        let rows = [
            row("2026-10-21", "fix login", Some("runner"), 1000, 0.05),
            row("2026-10-22", "fix login", Some("runner"), 500, 0.03),
            row("2026-10-22", "triage", None, 100, 0.01),
        ];
        let by_session = table(&rows, "session");
        let lines: Vec<&str> = by_session.lines().collect();
        assert_eq!(lines.len(), 4, "{by_session}");
        assert!(lines[1].starts_with("demo / fix login"), "{by_session}");
        assert!(
            lines[1].contains(" 1500 ") && lines[1].contains("0.0800"),
            "{by_session}"
        );
        assert!(
            lines[3].starts_with("total") && lines[3].contains(" 1600 "),
            "{by_session}"
        );

        let by_day = table(&rows, "day");
        assert!(
            by_day.lines().nth(2).unwrap().starts_with("2026-10-22"),
            "{by_day}"
        );
        assert!(by_day.lines().nth(2).unwrap().contains(" 600 "), "{by_day}");

        let by_role = table(&rows, "role");
        assert!(by_role.contains("(no role)"), "{by_role}");
        assert!(
            by_role.lines().nth(1).unwrap().starts_with("runner"),
            "{by_role}"
        );
    }
}
