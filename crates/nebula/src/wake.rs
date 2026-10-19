//! `kete wake`: WHAKAOHO (wake-up) from the command line — switch an
//! account's heartbeat on at an hour, see when it goes out and what it
//! did, and run it. `kete wake --all` is what the scheduled task runs.
//!
//! The schedules, the log and the decision are `nebula_core::wake`'s, read
//! straight off this machine's files, so all of it works with Kete closed.
//! The daemon is asked two things only: the accounts' usual hours
//! (`AccountActivity`), and — in the default interactive mode — to run
//! the hidden session itself (`WakeAccount`). `--dry-run` and `status`
//! never start one.

use anyhow::{bail, Context, Result};
use nebula_core::accounts::{self, LoginState};
use nebula_core::profiles::{self, Profile};
use nebula_core::usage::{self, WindowKind};
use nebula_core::wake::{
    self, activity, decide, log, task, AccountActivity, Decision, LocalTime, Skip, WakeMode,
    WakeReport, WakeSchedule,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::time::Duration;

/// How long to wait on the daemon's hidden session, past its own limit.
const WAKE_REPLY_TIMEOUT: Duration = Duration::from_secs(150);
/// How long `claude -p` may take in print mode.
const PRINT_TIMEOUT: Duration = Duration::from_secs(120);

/// What `kete wake` was asked to do.
pub enum WakeOp {
    /// Wake one account, or with `all` every switched-on one.
    Run {
        account: Option<String>,
        all: bool,
        force: bool,
        dry_run: bool,
        mode: Option<String>,
        json: bool,
    },
    Set(SetArgs),
    Off {
        account: String,
    },
    Status {
        json: bool,
    },
}

/// `kete wake set`'s arguments: what is left out keeps its value.
pub struct SetArgs {
    pub account: String,
    pub at: Option<String>,
    pub days: Option<String>,
    pub keep_rolling: Option<bool>,
    pub skip_idle: Option<u32>,
    pub mode: Option<String>,
    pub wake_pc: Option<bool>,
}

pub fn run(op: WakeOp) -> Result<()> {
    match op {
        WakeOp::Run {
            account,
            all,
            force,
            dry_run,
            mode,
            json,
        } => {
            let mode = mode
                .as_deref()
                .map(WakeMode::parse_choice)
                .transpose()
                .map_err(anyhow::Error::msg)?;
            match (account, all) {
                (Some(_), true) => bail!("name one account or --all, not both"),
                (None, false) => bail!("name an account (kete wake <account>), or --all"),
                (Some(account), false) => wake(Some(&account), force, dry_run, mode, json),
                (None, true) => {
                    if force {
                        bail!("--force takes one account: kete wake <account> --force");
                    }
                    wake(None, false, dry_run, mode, json)
                }
            }
        }
        WakeOp::Set(args) => set(args),
        WakeOp::Off { account } => off(&account),
        WakeOp::Status { json } => status(json),
    }
}

/// A Claude account: `@claude` (the CLI's own login) or a profile.
struct Account {
    key: String,
    profile: Option<Profile>,
}

impl Account {
    fn disabled(&self) -> bool {
        self.profile.as_ref().is_some_and(|p| p.disabled)
    }

    fn logged_in(&self) -> bool {
        accounts::login_check(self.profile.as_ref(), "claude").state == LoginState::LoggedIn
    }

    fn login_hint(&self) -> String {
        accounts::login_command(self.profile.as_ref(), "claude")
            .unwrap_or_else(|| "claude auth login".into())
    }
}

/// Every Claude account on this machine: `@claude`, then the profiles.
fn claude_accounts() -> Vec<Account> {
    let mut out = vec![Account {
        key: "@claude".into(),
        profile: None,
    }];
    out.extend(
        profiles::for_harness(&profiles::load(), "claude")
            .into_iter()
            .map(|p| Account {
                key: p.name.trim().to_string(),
                profile: Some(p.clone()),
            }),
    );
    out
}

fn resolve_account(key: &str) -> Result<Account> {
    let key = key.trim();
    if key == "@claude" {
        return Ok(Account {
            key: key.into(),
            profile: None,
        });
    }
    if let Some(harness) = usage::default_login_harness(key) {
        bail!("wake-up runs Claude accounts only, not {harness}");
    }
    let all = profiles::load();
    match profiles::resolve(&all, "claude", Some(key)) {
        Ok(Some(profile)) => Ok(Account {
            key: key.into(),
            profile: Some(profile.clone()),
        }),
        Ok(None) | Err(_) => {
            let known: Vec<String> = claude_accounts().into_iter().map(|a| a.key).collect();
            bail!(
                "no Claude account `{key}`: use one of {} (kete profile list)",
                known.join(", ")
            )
        }
    }
}

/// What the decision weighs about `account`.
fn state_of(account: &Account, activity: Option<&AccountActivity>) -> decide::AccountState {
    let reading = usage::read_account(&account.key);
    let read_at = reading.as_ref().map(|r| r.at / 1000).filter(|&at| at > 0);
    let turn_at = activity.and_then(|a| a.last_turn_at);
    decide::AccountState {
        disabled: account.disabled(),
        logged_in: account.logged_in(),
        session: reading
            .as_ref()
            .and_then(|r| r.window(WindowKind::Session))
            .cloned(),
        last_active_at: read_at.max(turn_at),
    }
}

fn session_reset(key: &str) -> Option<i64> {
    usage::read_account(key)
        .and_then(|r| {
            r.window(WindowKind::Session)
                .filter(|w| !w.estimated)
                .and_then(|w| w.resets_at)
        })
        .filter(|&at| at > 0)
}

// ---- the daemon ----

/// A connection to the daemon: started when `spawn`, else only one that
/// is already running.
async fn connect(spawn: bool) -> Result<Option<nebula_tui::ipc::Connection>> {
    if spawn {
        nebula_tui::ipc::connect_or_spawn().await.map(Some)
    } else {
        nebula_tui::ipc::connect_existing().await
    }
}

/// Every account's turns, from a daemon that knows how (protocol 61).
async fn fetch_activity(conn: &mut nebula_tui::ipc::Connection) -> Result<Vec<AccountActivity>> {
    use nebula_core::codec::{read_frame, write_frame};
    use nebula_core::{ClientRequest, ServerEvent};
    const REQ_ID: u64 = 61_001;
    if conn.protocol_version < nebula_core::WAKE_PROTOCOL {
        return Ok(Vec::new());
    }
    write_frame(
        &mut conn.stream,
        &ClientRequest::AccountActivity { req_id: REQ_ID },
    )
    .await?;
    let read = async {
        loop {
            match read_frame::<ServerEvent, _>(&mut conn.stream).await? {
                Some(ServerEvent::AccountActivity { req_id, accounts }) if req_id == REQ_ID => {
                    return Ok(accounts)
                }
                Some(ServerEvent::Error {
                    req_id: Some(REQ_ID),
                    message,
                }) => bail!(message),
                Some(_) => continue,
                None => bail!("the daemon closed the connection before answering"),
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(15), read)
        .await
        .context("the daemon didn't send the accounts' hours in time")?
}

/// Ask the daemon to run the hidden session for `key`.
async fn send_wake(conn: &mut nebula_tui::ipc::Connection, key: &str) -> Result<WakeReport> {
    use nebula_core::codec::{read_frame, write_frame};
    use nebula_core::{ClientRequest, ServerEvent};
    const REQ_ID: u64 = 61_002;
    let req = ClientRequest::WakeAccount {
        req_id: REQ_ID,
        account: key.to_string(),
    };
    write_frame(&mut conn.stream, &req).await?;
    let read = async {
        loop {
            match read_frame::<ServerEvent, _>(&mut conn.stream).await? {
                Some(ServerEvent::WakeDone { req_id, report }) if req_id == REQ_ID => {
                    return Ok(report)
                }
                Some(ServerEvent::Error {
                    req_id: Some(REQ_ID),
                    message,
                }) => bail!(message),
                Some(_) => continue,
                None => bail!("the daemon closed the connection before the wake-up ended"),
            }
        }
    };
    tokio::time::timeout(WAKE_REPLY_TIMEOUT, read)
        .await
        .context("the daemon didn't finish the wake-up in time")?
}

fn failed(reason: &str, error: String, before: Option<i64>) -> WakeReport {
    WakeReport {
        resets_before: before,
        resets_after: before,
        error: Some(error),
        reason: reason.into(),
        submitted: false,
        finished: false,
        read: false,
    }
}

/// The interactive heartbeat: the daemon's hidden session.
fn fire_interactive(
    rt: &tokio::runtime::Runtime,
    conn: &mut Option<nebula_tui::ipc::Connection>,
    account: &Account,
) -> WakeReport {
    let before = session_reset(&account.key);
    if conn.is_none() {
        match rt.block_on(connect(true)) {
            Ok(c) => *conn = c,
            Err(e) => {
                return failed(
                    "no_daemon",
                    format!("couldn't reach the daemon: {e:#}"),
                    before,
                )
            }
        }
    }
    let Some(c) = conn.as_mut() else {
        return failed("no_daemon", "couldn't reach the daemon".into(), before);
    };
    if c.protocol_version < nebula_core::WAKE_PROTOCOL {
        return failed(
            "old_daemon",
            format!(
                "the running daemon is older (protocol {}) and can't run a wake-up: \
                 run `kete kill` (Kete restarts it on its next start), then try again",
                c.protocol_version
            ),
            before,
        );
    }
    match rt.block_on(send_wake(c, &account.key)) {
        Ok(report) => report,
        // Asked, then lost (a timeout, a dropped connection): the daemon
        // may have sent the prompt already, so this is never a reason to
        // send another through print mode.
        Err(e) => {
            *conn = None;
            failed("daemon_lost", format!("{e:#}"), before)
        }
    }
}

/// The print-mode heartbeat: `claude -p` under the account, here. It may
/// bill the plan's API credits rather than start the window, and no
/// status line runs: what reset time the CLI reports goes in the log only.
fn fire_print(account: &Account) -> WakeReport {
    let before = session_reset(&account.key);
    let dir = wake::wake_dir();
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return failed("spawn", format!("{}: {e}", dir.display()), before);
    }
    let env = match &account.profile {
        Some(profile) => {
            let _ = profile.prepare(Some(&dir));
            profile.launch_env()
        }
        None => Default::default(),
    };
    let program = nebula_config::Config::load()
        .effective_harness(nebula_core::AgentKind::Claude, None)
        .program
        .trim()
        .to_string();
    let args: Vec<String> = [
        "-p",
        wake::PROMPT,
        "--model",
        wake::MODEL,
        "--no-session-persistence",
        "--output-format",
        "stream-json",
        "--verbose",
    ]
    .into_iter()
    .map(str::to_string)
    .collect();
    // No project's hooks, CLAUDE.md or settings: the empty wake folder.
    if let Err(e) = std::env::set_current_dir(&dir) {
        return failed("spawn", format!("{}: {e}", dir.display()), before);
    }
    let Some(out) = nebula_daemon::claude_bg::run_captured(&program, &args, &env, PRINT_TIMEOUT)
    else {
        return failed(
            "timeout",
            format!(
                "`{program} -p` didn't finish within {}s (or didn't start)",
                PRINT_TIMEOUT.as_secs()
            ),
            before,
        );
    };
    let (error, reported) = read_print_output(&out);
    WakeReport {
        resets_before: before,
        resets_after: reported,
        reason: if error.is_some() {
            "print_failed"
        } else {
            "fired"
        }
        .into(),
        submitted: true,
        finished: error.is_none(),
        read: reported.is_some(),
        error,
    }
}

/// From `claude -p --output-format stream-json`: the turn's error, if it
/// ended in one, and the five-hour reset time a `rate_limit_event` named.
fn read_print_output(out: &str) -> (Option<String>, Option<i64>) {
    fn find_reset(value: &Value) -> Option<i64> {
        match value {
            Value::Object(map) => map.iter().find_map(|(k, v)| match (k.as_str(), v) {
                ("resetsAt" | "resets_at", Value::Number(n)) => n.as_i64(),
                _ => find_reset(v),
            }),
            Value::Array(items) => items.iter().find_map(find_reset),
            _ => None,
        }
    }
    let mut error = None;
    let mut reset = None;
    for value in out
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l.trim()).ok())
    {
        match value.get("type").and_then(Value::as_str) {
            Some("rate_limit_event") => {
                let kind = value
                    .pointer("/rate_limit_info/rateLimitType")
                    .or_else(|| value.get("rateLimitType"))
                    .and_then(Value::as_str);
                if kind.is_none_or(|k| k == "five_hour") {
                    reset =
                        find_reset(&value)
                            .map(|at| if at > 10_000_000_000 { at / 1000 } else { at });
                }
            }
            Some("result") if value.get("is_error").and_then(Value::as_bool) == Some(true) => {
                error = Some(
                    value
                        .get("result")
                        .and_then(Value::as_str)
                        .unwrap_or("the turn ended in an error")
                        .to_string(),
                );
            }
            _ => {}
        }
    }
    (error, reset)
}

// ---- kete wake [<account> | --all] ----

/// Does this decision depend on the usual hours (or the last use), so the
/// daemon is worth asking before it stands?
fn needs_activity(decision: &Decision, schedule: &WakeSchedule) -> bool {
    match decision {
        Decision::Fire { .. } => true,
        Decision::Skip(Skip::Late { .. } | Skip::OutsideHours { .. } | Skip::Idle { .. }) => true,
        Decision::Skip(_) => schedule.skip_idle_days.is_some(),
    }
}

struct Outcome {
    account: String,
    decision: Decision,
    report: Option<WakeReport>,
    mode: WakeMode,
}

/// `mode` is this run's `--mode`: None keeps each schedule's, Some(None)
/// is auto.
fn wake(
    one: Option<&str>,
    force: bool,
    dry_run: bool,
    mode: Option<Option<WakeMode>>,
    json: bool,
) -> Result<()> {
    let now = nebula_core::clock::now_secs() as i64;
    let offset = nebula_core::clock::local_offset_secs();
    let schedules = wake::load();
    let mut targets: Vec<(Account, WakeSchedule)> = Vec::new();
    match one {
        Some(key) => {
            let account = resolve_account(key)?;
            let schedule = schedules.get(&account.key).cloned().unwrap_or_default();
            if force && !account.logged_in() {
                bail!(
                    "{} isn't logged in, so a wake-up can't run: {}",
                    account.key,
                    account.login_hint()
                );
            }
            targets.push((account, schedule));
        }
        None => {
            for (key, schedule) in schedules.iter().filter(|(_, s)| s.enabled) {
                match resolve_account(key) {
                    Ok(account) => targets.push((account, schedule.clone())),
                    Err(e) => eprintln!("{key}: skipped: {e}"),
                }
            }
            if targets.is_empty() {
                if !json {
                    println!("No account has wake-up on. Switch one on: kete wake set <account> --at 05:00");
                } else {
                    println!("[]");
                }
                return Ok(());
            }
        }
    }
    let log_path = wake::log_path();
    let history = log::read_in(&log_path);
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let forced = Decision::Fire {
        slot: now,
        slot_min: LocalTime::at(now, offset).minute,
        catch_up: false,
    };
    let first = |activity: &BTreeMap<String, AccountActivity>| -> Vec<Decision> {
        targets
            .iter()
            .map(|(account, schedule)| {
                if force {
                    return forced.clone();
                }
                let act = activity.get(&account.key);
                let entries: Vec<log::Entry> = history
                    .iter()
                    .filter(|e| e.account == account.key)
                    .cloned()
                    .collect();
                decide::decide(decide::Inputs {
                    now,
                    offset_secs: offset,
                    schedule,
                    state: &state_of(account, act),
                    log: &entries,
                    usual: act.and_then(|a| wake::usual_hours(&a.bins)),
                })
            })
            .collect()
    };
    let mut conn: Option<nebula_tui::ipc::Connection> = None;
    let mut activity: BTreeMap<String, AccountActivity> = BTreeMap::new();
    let mut decisions = first(&activity);
    let worth_asking = decisions
        .iter()
        .zip(&targets)
        .any(|(d, (_, s))| !force && needs_activity(d, s));
    if worth_asking {
        // A dry run asks a daemon that is up; a real run starts one, as the
        // interactive heartbeat needs it anyway.
        if let Ok(Some(mut c)) = rt.block_on(connect(!dry_run)) {
            if let Ok(list) = rt.block_on(fetch_activity(&mut c)) {
                activity = list.into_iter().map(|a| (a.account.clone(), a)).collect();
                decisions = first(&activity);
            }
            conn = Some(c);
        }
    }
    let mut outcomes = Vec::new();
    let mut any_failed = false;
    for ((account, schedule), decision) in targets.iter().zip(decisions) {
        // A mode named (here or in the schedule) is pinned; auto runs
        // interactive and may fall back to print.
        let chosen = mode.unwrap_or(schedule.mode);
        let may_fall_back = chosen.is_none();
        let mode = chosen.unwrap_or_default();
        if dry_run {
            if !json {
                println!("{}: {}", account.key, dry_line(&decision, offset, chosen));
            }
            outcomes.push(Outcome {
                account: account.key.clone(),
                decision,
                report: None,
                mode,
            });
            continue;
        }
        match &decision {
            Decision::Skip(skip) => {
                let slot = decide::current_slot(now, offset, schedule).map(|(at, _)| at);
                // A slot's skip is logged once, however often the task runs
                // that day; one asked for by name always is.
                let logged = one.is_some()
                    || slot.is_some_and(|slot| {
                        !history
                            .iter()
                            .any(|e| e.account == account.key && e.slot == Some(slot))
                    });
                if logged && !matches!(skip, Skip::Off) {
                    let _ = log::append_in(
                        &log_path,
                        &log::Entry {
                            at: now,
                            account: account.key.clone(),
                            decision: log::Outcome::Skipped,
                            reason: skip.code().into(),
                            mode,
                            resets_before: session_reset(&account.key),
                            resets_after: None,
                            detail: Some(skip.describe(offset)),
                            slot,
                            forced: false,
                        },
                    );
                }
                if !json {
                    let hint = match skip {
                        Skip::Off | Skip::LiveWindow { .. } | Skip::AlreadyFired { .. }
                            if one.is_some() =>
                        {
                            " (--force wakes it now anyway)"
                        }
                        _ => "",
                    };
                    let login = if matches!(skip, Skip::LoggedOut) {
                        format!(": {}", account.login_hint())
                    } else {
                        String::new()
                    };
                    println!(
                        "{}: skipped: {}{login}{hint}",
                        account.key,
                        skip.describe(offset)
                    );
                }
            }
            Decision::Fire { slot, .. } => {
                if !json {
                    let how = match mode {
                        WakeMode::Interactive => "a hidden Claude session, up to 2 minutes",
                        WakeMode::Print => "claude -p, up to 2 minutes",
                    };
                    println!("{}: waking ({how})…", account.key);
                }
                let mut ran = mode;
                let mut report = match mode {
                    WakeMode::Interactive => fire_interactive(&rt, &mut conn, account),
                    WakeMode::Print => fire_print(account),
                };
                // Auto mode, and the daemon couldn't run it (older than 61
                // until its restart, or not there): print mode still does
                // something useful.
                let mut fallback: Option<(String, String)> = None;
                if falls_back(may_fall_back, mode, &report) {
                    let why = report.error.clone().unwrap_or_default();
                    if !json {
                        println!("{}: {why}; falling back to claude -p…", account.key);
                    }
                    fallback = Some((report.reason.clone(), why.clone()));
                    report = fire_print(account);
                    ran = WakeMode::Print;
                    if let Some(e) = &report.error {
                        report.error = Some(format!("interactive: {why}; print: {e}"));
                    }
                }
                let ok = report.error.is_none();
                any_failed |= !ok;
                let entry = log::Entry {
                    at: nebula_core::clock::now_secs() as i64,
                    account: account.key.clone(),
                    decision: if ok {
                        log::Outcome::Fired
                    } else {
                        log::Outcome::Failed
                    },
                    reason: match (&fallback, ok) {
                        (_, false) => report.reason.clone(),
                        (Some((why, _)), true) => format!("{}{why}", log::PRINT_FALLBACK),
                        (None, true) if force => "forced".into(),
                        (None, true) => decision.code().into(),
                    },
                    mode: ran,
                    resets_before: report.resets_before,
                    // No reading after it: unknown, not "didn't move".
                    resets_after: report.read.then_some(report.resets_after).flatten(),
                    detail: match (&report.error, &fallback) {
                        (Some(e), _) => Some(e.clone()),
                        (None, Some((_, why))) => Some(why.clone()),
                        (None, None) => None,
                    },
                    slot: (!force).then_some(*slot),
                    forced: force,
                };
                if let Err(e) = log::append_in(&log_path, &entry) {
                    eprintln!("{}: couldn't write the wake log: {e}", account.key);
                }
                if !json {
                    println!("{}: {}", account.key, fired_line(&entry, offset));
                }
                outcomes.push(Outcome {
                    account: account.key.clone(),
                    decision,
                    report: Some(report),
                    mode: ran,
                });
                continue;
            }
        }
        outcomes.push(Outcome {
            account: account.key.clone(),
            decision,
            report: None,
            mode,
        });
    }
    if json {
        let list: Vec<Value> = outcomes
            .iter()
            .map(|o| {
                json!({
                    "account": o.account,
                    "decision": match &o.decision {
                        Decision::Fire { .. } => "fire",
                        Decision::Skip(_) => "skip",
                    },
                    "reason": o.decision.code(),
                    "detail": match &o.decision {
                        Decision::Skip(skip) => Some(skip.describe(offset)),
                        Decision::Fire { .. } => None,
                    },
                    "mode": o.mode.as_str(),
                    "dry_run": dry_run,
                    "report": o.report,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&list)?);
    }
    if any_failed {
        bail!("a wake-up failed: see above, or kete wake status");
    }
    Ok(())
}

/// Does an interactive run that ended in `report` go again in print
/// mode? Only in auto mode, and only when the daemon was never asked: one
/// older than protocol 61, or none to reach. A turn that ran and failed
/// (no prompt, a timeout) would fail the same way under `-p`, and one
/// asked and then lost (`daemon_lost`) may have gone out already.
fn falls_back(auto: bool, mode: WakeMode, report: &WakeReport) -> bool {
    auto && mode == WakeMode::Interactive
        && matches!(report.reason.as_str(), "old_daemon" | "no_daemon")
}

fn dry_line(decision: &Decision, offset: i32, mode: Option<WakeMode>) -> String {
    match decision {
        Decision::Fire {
            slot_min, catch_up, ..
        } => format!(
            "would wake now ({} {}, {} mode)",
            wake::hhmm(*slot_min),
            if *catch_up { "catch-up" } else { "on time" },
            mode.map_or("auto", WakeMode::as_str)
        ),
        Decision::Skip(skip) => format!("would skip: {}", skip.describe(offset)),
    }
}

fn fired_line(entry: &log::Entry, offset: i32) -> String {
    let via = entry
        .fallback()
        .map(|why| format!(" (print fallback: {why})"))
        .unwrap_or_default();
    match entry.decision {
        log::Outcome::Failed => format!(
            "failed: {}",
            entry.detail.as_deref().unwrap_or(&entry.reason)
        ),
        _ if !via.is_empty() => format!(
            "fired{via}: claude -p sent the prompt; it may bill API credits rather than start the window. `kete kill` lets Kete restart the daemon for interactive wake-ups"
        ),
        _ => match (entry.resets_before, entry.resets_after, entry.started_window()) {
            (_, Some(after), Some(true)) => format!(
                "woke: the window now resets at {}",
                wake::hhmm(LocalTime::at(after, offset).minute)
            ),
            (Some(before), _, Some(false)) => format!(
                "sent, but the window didn't move (still resets at {}): this plan may not start a window this way",
                wake::hhmm(LocalTime::at(before, offset).minute)
            ),
            _ => "sent; no usage reading came back, so the new reset time isn't known yet".into(),
        },
    }
}

// ---- kete wake set / off ----

fn set(args: SetArgs) -> Result<()> {
    let account = resolve_account(&args.account)?;
    let stored = wake::load();
    let existing = stored.get(&account.key).cloned();
    let mut s = existing.clone().unwrap_or_default();
    s.enabled = true;
    match args.at.as_deref().map(str::trim) {
        Some("suggested" | "suggest") => {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            let activity = rt.block_on(async {
                match connect(false).await {
                    Ok(Some(mut c)) => fetch_activity(&mut c).await.unwrap_or_default(),
                    _ => Vec::new(),
                }
            });
            let Some(anchor) = activity
                .iter()
                .find(|a| a.account == account.key)
                .and_then(|a| activity::suggest_anchor(&a.bins))
            else {
                bail!(
                    "no suggestion for {} yet: it needs a few days of sessions in Kete (and Kete running). Pick one: --at 05:00",
                    account.key
                );
            };
            s.anchor_min = anchor;
        }
        Some(at) => s.anchor_min = wake::parse_hhmm(at).map_err(anyhow::Error::msg)?,
        None if existing.is_none() => bail!("say when: kete wake set {} --at 05:00", account.key),
        None => {}
    }
    if let Some(days) = &args.days {
        s.days = wake::parse_days(days).map_err(anyhow::Error::msg)?;
    }
    if let Some(on) = args.keep_rolling {
        s.keep_rolling = on;
    }
    if let Some(n) = args.skip_idle {
        s.skip_idle_days = (n > 0).then_some(n);
    }
    if let Some(mode) = &args.mode {
        s.mode = WakeMode::parse_choice(mode).map_err(anyhow::Error::msg)?;
    }
    if let Some(on) = args.wake_pc {
        s.wake_pc = on;
    }
    wake::set(&account.key, Some(&s)).map_err(anyhow::Error::msg)?;
    println!("{}: wake-up on, {}", account.key, describe_schedule(&s));
    let now = nebula_core::clock::now_secs() as i64;
    let offset = nebula_core::clock::local_offset_secs();
    if let Some(next) = decide::next_slot(now, offset, &s, None) {
        println!("  next: {}", when(next, offset));
    }
    if !account.logged_in() {
        println!(
            "  note: {} isn't logged in, so nothing goes out until it is: {}",
            account.key,
            account.login_hint()
        );
    }
    if account.disabled() {
        println!(
            "  note: {} is switched off in the accounts list, so nothing goes out until it's on",
            account.key
        );
    }
    sync_task()
}

fn off(key: &str) -> Result<()> {
    let account = resolve_account(key)?;
    let mut s = wake::load().get(&account.key).cloned().unwrap_or_default();
    s.enabled = false;
    // The hour and options stay for the next `set`.
    wake::set(&account.key, Some(&s)).map_err(anyhow::Error::msg)?;
    println!("{}: wake-up off", account.key);
    sync_task()
}

fn describe_schedule(s: &WakeSchedule) -> String {
    let mut parts = vec![
        format!("at {}", wake::hhmm(s.anchor())),
        wake::days_text(s.day_mask()),
    ];
    if s.keep_rolling {
        parts.push("keep rolling".into());
    }
    if let Some(n) = s.skip_idle_days {
        parts.push(format!("skip after {n} idle days"));
    }
    if s.wake_pc {
        parts.push("wakes the PC".into());
    }
    parts.push(format!("{} mode", s.mode_word()));
    parts.join(", ")
}

/// `Sat 05:00`.
fn when(at: i64, offset: i32) -> String {
    const DAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
    let local = LocalTime::at(at, offset);
    format!(
        "{} {}",
        DAYS[local.weekday as usize % 7],
        wake::hhmm(local.minute)
    )
}

/// kete's own binary, as a scheduled task names it.
fn own_exe() -> Result<String> {
    let exe = std::env::current_exe().context("resolve kete's own path")?;
    let text = exe.to_string_lossy().into_owned();
    Ok(text.strip_prefix(r"\\?\").unwrap_or(&text).to_string())
}

/// Bring the scheduled task (Windows) in line with the schedules, or show
/// the crontab lines that would (macOS, Linux).
fn sync_task() -> Result<()> {
    let schedules = wake::load();
    let exe = own_exe()?;
    if cfg!(windows) {
        // Under NEBULA_WAKE_NO_TASK the runner does nothing: the definition
        // is still written to the data dir, to read, but not registered.
        let runner = task::system_runner();
        let synced = task::sync(
            &*runner,
            &schedules,
            &task::program_for(&exe).0,
            task::current_user().as_deref(),
            &task::xml_path(),
        );
        if nebula_core::env::non_empty(nebula_core::env::WAKE_NO_TASK).is_some() {
            println!(
                "  {} is set: the scheduled task was left alone ({} shows what it would be)",
                nebula_core::env::WAKE_NO_TASK,
                task::xml_path().display()
            );
            return synced.map(|_| ()).map_err(anyhow::Error::msg);
        }
        match synced {
            Ok(task::Synced::Written { .. }) => {
                println!("  {}", task::status_line(&schedules, Some(&exe)));
                Ok(())
            }
            Ok(task::Synced::Removed) => {
                println!(
                    "  Task Scheduler: no account is on, so {} is removed",
                    task::TASK_NAME
                );
                Ok(())
            }
            Err(e) => {
                bail!("the schedule is saved, but the scheduled task couldn't be updated: {e}")
            }
        }
    } else {
        let lines = task::crontab_lines(&schedules, &exe);
        if lines.is_empty() {
            println!("  No account is on: remove any `kete wake --all` lines from your crontab.");
        } else {
            println!("  Kete installs nothing here. Add these to `crontab -e`:");
            for line in lines {
                println!("    {line}");
            }
        }
        Ok(())
    }
}

// ---- kete wake status ----

fn status(as_json: bool) -> Result<()> {
    let now = nebula_core::clock::now_secs() as i64;
    let offset = nebula_core::clock::local_offset_secs();
    let schedules = wake::load();
    let mut accounts = claude_accounts();
    for key in schedules.keys() {
        if !accounts.iter().any(|a| &a.key == key) {
            accounts.push(Account {
                key: key.clone(),
                profile: None,
            });
        }
    }
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let (daemon_up, activity) = rt.block_on(async {
        match connect(false).await {
            Ok(Some(mut c)) => {
                let newer = c.protocol_version >= nebula_core::WAKE_PROTOCOL;
                (
                    Some(newer),
                    fetch_activity(&mut c).await.unwrap_or_default(),
                )
            }
            _ => (None, Vec::new()),
        }
    });
    let history = log::read_in(&wake::log_path());
    let mut rows = Vec::new();
    for account in &accounts {
        let schedule = schedules.get(&account.key).cloned().unwrap_or_default();
        let act = activity.iter().find(|a| a.account == account.key);
        let usual = act.and_then(|a| wake::usual_hours(&a.bins));
        let suggested = usual.and_then(activity::suggest_for);
        let last = history
            .iter()
            .rev()
            .find(|e| e.account == account.key)
            .cloned();
        let reading = usage::read_account(&account.key);
        let session = reading
            .as_ref()
            .and_then(|r| r.window(WindowKind::Session))
            .cloned();
        rows.push((account, schedule, act, usual, suggested, last, session));
    }
    if as_json {
        // What both apps draw from (the desktop's Whakaoho view, the TUI's
        // Wake tab): one shape, worked out here once.
        let list: Vec<Value> = rows
            .iter()
            .map(
                |(account, schedule, act, usual, suggested, last, session)| {
                    let recent: Vec<Value> = history
                        .iter()
                        .rev()
                        .filter(|e| e.account == account.key)
                        .take(5)
                        .map(|e| json!({ "entry": e, "summary": e.summary(offset) }))
                        .collect();
                    json!({
                        "account": account.key,
                        "known": account.key == "@claude" || account.profile.is_some(),
                        "disabled": account.disabled(),
                        "logged_in": account.logged_in(),
                        "login_hint": account.login_hint(),
                        "schedule": schedule,
                        "mode": schedule.mode_word(),
                        "days_text": wake::days_text(schedule.day_mask()),
                        "slots": schedule.slots(),
                        "next": decide::next_slot(now, offset, schedule, *usual),
                        "usual": usual,
                        "suggested": suggested,
                        "preview": wake::preview(schedule.anchor(), *usual),
                        // Every anchor's preview, by anchor / 15 minutes,
                        // so a slider shows its effect without asking again.
                        "previews": usual.map(|u| {
                            (0..wake::DAY_MIN / wake::STEP_MIN)
                                .map(|i| wake::preview(i * wake::STEP_MIN, Some(u)))
                                .collect::<Vec<_>>()
                        }),
                        "bins": act.map(|a| a.bins.clone()),
                        "last_turn_at": act.and_then(|a| a.last_turn_at),
                        "last": last,
                        "last_summary": last.as_ref().map(|e| e.summary(offset)),
                        "recent": recent,
                        "session": session,
                    })
                },
            )
            .collect();
        let out = json!({
            "now": now,
            "offset": offset,
            "daemon": daemon_up,
            "windows": cfg!(windows),
            "task_name": task::TASK_NAME,
            "task_line": task::status_line(&schedules, Some(&own_exe()?)),
            // The task runs kete-w.exe, which shows no window; false: kete
            // itself, whose console shows while each run lasts.
            "task_hidden": task::program_for(&own_exe()?).1,
            "task": task::triggers(&schedules).keys().map(|m| wake::hhmm(*m)).collect::<Vec<_>>(),
            "crontab": task::crontab_lines(&schedules, &own_exe()?),
            "log": wake::log_path(),
            "wake_dir": wake::wake_dir(),
            "model": wake::MODEL,
            "accounts": list,
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }
    println!("Whakaoho \u{b7} wake-up: at the hour you pick, one tiny prompt to {} with your own subscription starts the account's 5-hour window early.", wake::MODEL);
    println!();
    for (account, schedule, _, usual, suggested, last, session) in &rows {
        let mut flags = Vec::new();
        if account.disabled() {
            flags.push("switched off".to_string());
        }
        if !account.logged_in() {
            flags.push(format!("not logged in ({})", account.login_hint()));
        }
        if !(account.key == "@claude" || account.profile.is_some()) {
            flags.push("no such account".into());
        }
        let flags = if flags.is_empty() {
            String::new()
        } else {
            format!("  [{}]", flags.join(", "))
        };
        if schedule.enabled {
            println!(
                "{}  on  {}{flags}",
                account.key,
                describe_schedule(schedule)
            );
            if let Some(next) = decide::next_slot(now, offset, schedule, *usual) {
                println!("    next      {}", when(next, offset));
            }
        } else {
            println!("{}  off{flags}", account.key);
        }
        if let Some(w) = session.as_ref().filter(|w| !w.expired_at(now)) {
            match w.resets_at.filter(|&at| at > 0) {
                Some(at) => println!(
                    "    window    live, {}% used, resets {}",
                    w.percent.round(),
                    when(at, offset)
                ),
                None => println!("    window    live, {}% used", w.percent.round()),
            }
        }
        match usual {
            Some(u) => {
                let p = wake::preview(schedule.anchor(), Some(*u));
                let mut line = format!(
                    "    usual     {}\u{2013}{}",
                    wake::hhmm(u.start),
                    wake::hhmm(u.end())
                );
                if let Some(anchor) = suggested {
                    let best = wake::preview(*anchor, Some(*u));
                    line.push_str(&format!(
                        " \u{b7} suggested {} ({} fresh windows in those hours, {} without wake-up)",
                        wake::hhmm(*anchor),
                        best.fresh_in_active.unwrap_or(0),
                        best.baseline.unwrap_or(0)
                    ));
                }
                println!("{line}");
                if schedule.enabled {
                    println!(
                        "    preview   {} gives {} fresh windows in your usual hours",
                        wake::hhmm(schedule.anchor()),
                        p.fresh_in_active.unwrap_or(0)
                    );
                }
            }
            None if daemon_up.is_none() => {
                println!("    usual     unknown (Kete isn't running)")
            }
            None => println!("    usual     not enough sessions yet"),
        }
        if let Some(entry) = last {
            println!(
                "    last      {} {}",
                day_of(entry.at, offset),
                entry.summary(offset)
            );
        }
    }
    println!();
    if daemon_up == Some(false) {
        println!("The running daemon is older: it can't run interactive wake-ups. Run `kete kill`; Kete restarts it.");
    }
    let times: Vec<String> = task::triggers(&schedules)
        .keys()
        .map(|m| wake::hhmm(*m))
        .collect();
    if times.is_empty() {
        println!("Nothing scheduled. Switch an account on: kete wake set <account> --at 05:00");
    } else if cfg!(windows) {
        println!("{}.", task::status_line(&schedules, Some(&own_exe()?)));
    } else {
        println!("Add to `crontab -e` (Kete installs nothing here):");
        for line in task::crontab_lines(&schedules, &own_exe()?) {
            println!("  {line}");
        }
    }
    println!("Log: {}", wake::log_path().display());
    Ok(())
}

/// `Sat` for a stamp.
fn day_of(at: i64, offset: i32) -> &'static str {
    const DAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
    DAYS[LocalTime::at(at, offset).weekday as usize % 7]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn print_output_yields_the_five_hour_reset_and_errors() {
        let out = r#"{"type":"system","subtype":"init"}
{"type":"rate_limit_event","rate_limit_info":{"status":"allowed","resetsAt":1800000000,"rateLimitType":"five_hour"}}
{"type":"rate_limit_event","rate_limit_info":{"status":"allowed","resetsAt":1900000000,"rateLimitType":"seven_day"}}
{"type":"result","subtype":"success","is_error":false,"result":"Hi!"}"#;
        assert_eq!(read_print_output(out), (None, Some(1_800_000_000)));
        let out = r#"{"type":"result","is_error":true,"result":"Invalid API key"}"#;
        assert_eq!(
            read_print_output(out),
            (Some("Invalid API key".into()), None)
        );
        assert_eq!(read_print_output("not json"), (None, None));
        let ms = r#"{"type":"rate_limit_event","resetsAt":1800000000000}"#;
        assert_eq!(read_print_output(ms).1, Some(1_800_000_000), "ms read as s");
    }

    #[test]
    fn only_auto_mode_falls_back_and_only_when_the_daemon_could_not_run_it() {
        let report = |reason: &str| failed(reason, "why".into(), None);
        assert!(falls_back(
            true,
            WakeMode::Interactive,
            &report("old_daemon")
        ));
        assert!(falls_back(
            true,
            WakeMode::Interactive,
            &report("no_daemon")
        ));
        assert!(
            !falls_back(false, WakeMode::Interactive, &report("old_daemon")),
            "pinned"
        );
        assert!(!falls_back(true, WakeMode::Interactive, &report("timeout")));
        assert!(
            !falls_back(true, WakeMode::Interactive, &report("daemon_lost")),
            "asked, then lost: it may have gone out already"
        );
        assert!(!falls_back(
            true,
            WakeMode::Interactive,
            &report("no_prompt")
        ));
        assert!(!falls_back(true, WakeMode::Print, &report("old_daemon")));
    }

    #[test]
    fn a_schedule_reads_back_in_words() {
        let s = WakeSchedule {
            enabled: true,
            anchor_min: 330,
            days: wake::WEEKDAYS,
            keep_rolling: true,
            skip_idle_days: Some(3),
            mode: Some(WakeMode::Interactive),
            wake_pc: true,
        };
        assert_eq!(
            describe_schedule(&s),
            "at 05:30, Mon\u{2013}Fri, keep rolling, skip after 3 idle days, wakes the PC, interactive mode"
        );
    }
}
