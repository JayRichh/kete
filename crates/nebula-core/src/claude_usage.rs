//! A Claude account's plan usage, asked of Anthropic directly: the same
//! `GET /api/oauth/usage` Claude Code's own `/usage` view makes, with the
//! login Claude keeps in `<config dir>/.credentials.json`.
//!
//! The statusLine feed (`usage::claude_statusline`) only moves while a
//! session of the account is drawing a status line and can reach the
//! daemon; a background job draws none, and a reading that stops arriving
//! leaves the meter on its last number for hours. Asking the account is
//! right per profile too: each profile's config dir holds its own login.

use crate::usage::{UsageWindow, WindowKind};
use serde_json::Value;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const ENDPOINT: &str = "https://api.anthropic.com/api/oauth/usage";
const FETCH_TIMEOUT: Duration = Duration::from_secs(15);

/// The access token of the login in `config_dir`, unless it has run out
/// (Claude renews it when a session next starts; an expired one is not
/// worth a request).
pub fn access_token(config_dir: &Path, now_ms: i64) -> Option<String> {
    let text = std::fs::read_to_string(config_dir.join(".credentials.json")).ok()?;
    token_from(&serde_json::from_str(&text).ok()?, now_ms)
}

fn token_from(creds: &Value, now_ms: i64) -> Option<String> {
    let oauth = creds.get("claudeAiOauth")?;
    if oauth.get("expiresAt").and_then(Value::as_i64)? <= now_ms {
        return None;
    }
    oauth
        .get("accessToken")
        .and_then(Value::as_str)
        .filter(|t| !t.is_empty())
        .map(str::to_string)
}

/// The email the login in `config_dir` belongs to. A profile keeps
/// `.claude.json` in its config dir; the default login keeps it beside
/// `~/.claude`, not inside.
pub fn account_email(config_dir: &Path) -> Option<String> {
    let inside = config_dir.join(".claude.json");
    let beside: Option<PathBuf> = config_dir.parent().map(|p| p.join(".claude.json"));
    [Some(inside), beside]
        .into_iter()
        .flatten()
        .find_map(|file| {
            email_from(&serde_json::from_str(&std::fs::read_to_string(file).ok()?).ok()?)
        })
}

fn email_from(json: &Value) -> Option<String> {
    json.get("oauthAccount")?
        .get("emailAddress")?
        .as_str()
        .filter(|e| !e.is_empty())
        .map(str::to_string)
}

/// The windows of a usage reply: the 5-hour session, the week, and a
/// model's own week (`seven_day_opus`, `seven_day_sonnet`) when it has one.
/// `utilization` is a percent already; a null window is one the plan lacks.
pub fn windows(body: &Value, now_secs: i64) -> Vec<UsageWindow> {
    [
        ("five_hour", WindowKind::Session, None),
        ("seven_day", WindowKind::Weekly, None),
        ("seven_day_opus", WindowKind::ModelWeekly, Some("Opus")),
        ("seven_day_sonnet", WindowKind::ModelWeekly, Some("Sonnet")),
    ]
    .into_iter()
    .filter_map(|(field, kind, label)| {
        let w = body.get(field).filter(|w| !w.is_null())?;
        Some(UsageWindow {
            kind,
            label: label.map(str::to_string),
            percent: w.get("utilization")?.as_f64()?.max(0.0) as f32,
            resets_at: w
                .get("resets_at")
                .and_then(Value::as_str)
                .and_then(epoch_secs),
            read_at: now_secs,
            estimated: false,
        })
    })
    .collect()
}

/// Epoch seconds of an RFC 3339 time (`2026-10-19T10:09:59.89+00:00`).
pub fn epoch_secs(text: &str) -> Option<i64> {
    let (date, rest) = text.split_once('T')?;
    let mut d = date.split('-').map(|n| n.parse::<i64>().ok());
    let (y, m, day) = (d.next()??, d.next()??, d.next()??);
    let clock_end = rest.find(['Z', '+', '-']).unwrap_or(rest.len());
    let (clock, zone) = rest.split_at(clock_end);
    let mut c = clock.split(':');
    let hour: i64 = c.next()?.parse().ok()?;
    let minute: i64 = c.next()?.parse().ok()?;
    let second: f64 = c.next().unwrap_or("0").parse().ok()?;
    let offset = match zone.as_bytes().first() {
        Some(b'+') | Some(b'-') => {
            let sign = if zone.starts_with('-') { -1 } else { 1 };
            let (h, m) = zone[1..].split_once(':')?;
            sign * (h.parse::<i64>().ok()? * 3600 + m.parse::<i64>().ok()? * 60)
        }
        _ => 0,
    };
    // Days from civil (Howard Hinnant).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + hour * 3600 + minute * 60 + second as i64 - offset)
}

/// Ask Anthropic for the usage of the login in `config_dir`. Blocking, a
/// few seconds at most; None for no login, an expired one, or any failure.
/// `curl` carries it (the token goes in on stdin, never on a command line).
pub fn fetch(config_dir: &Path, now_ms: i64) -> Option<Vec<UsageWindow>> {
    let token = access_token(config_dir, now_ms)?;
    let mut cmd = Command::new("curl");
    cmd.args(["-sS", "-m", "12", "-K", "-", ENDPOINT])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    crate::platform::no_window(&mut cmd);
    let mut child = cmd.spawn().ok()?;
    child
        .stdin
        .take()?
        .write_all(
            format!(
                "header = \"Authorization: Bearer {token}\"\n\
                 header = \"anthropic-beta: oauth-2025-04-20\"\n\
                 header = \"User-Agent: kete\"\n"
            )
            .as_bytes(),
        )
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut out = Vec::new();
        let _ = stdout.read_to_end(&mut out);
        out
    });
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(Some(_)) | Err(_) => return None,
            Ok(None) if started.elapsed() > FETCH_TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
        }
    }
    let body: Value = serde_json::from_slice(&reader.join().ok()?).ok()?;
    let found = windows(&body, now_ms / 1000);
    (!found.is_empty()).then_some(found)
}

/// A window coming back to the front asks for a fresh reading at once
/// rather than within the minute: it drops this file in the data dir (the
/// desktop and the daemon share it, so no protocol request is needed) and
/// the daemon's next two-second look takes it.
pub fn nudge() {
    let _ = std::fs::write(nudge_path(), b"");
}

/// Whether a nudge was waiting; it is consumed.
pub fn take_nudge() -> bool {
    std::fs::remove_file(nudge_path()).is_ok()
}

fn nudge_path() -> PathBuf {
    crate::paths::data_dir().join("usage-poll-now")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real reply (trimmed): the account Jay's orchestrator runs under,
    /// read while the statusLine feed still said 7 percent.
    const REPLY: &str = r#"{"five_hour":{"utilization":49.0,"resets_at":"2026-10-19T10:09:59.892648+00:00","limit_dollars":null},
        "seven_day":{"utilization":57.0,"resets_at":"2026-10-20T19:59:59.892667+00:00"},
        "seven_day_oauth_apps":null,"seven_day_opus":null,"seven_day_sonnet":{"utilization":3.5,"resets_at":null}}"#;

    #[test]
    fn a_reply_becomes_the_session_week_and_model_windows() {
        let w = windows(&serde_json::from_str(REPLY).unwrap(), 1_792_390_000);
        assert_eq!(w.len(), 3);
        assert_eq!((w[0].kind, w[0].percent), (WindowKind::Session, 49.0));
        assert_eq!(w[0].resets_at, Some(1_792_404_599));
        assert_eq!((w[1].kind, w[1].percent), (WindowKind::Weekly, 57.0));
        assert_eq!(w[2].label.as_deref(), Some("Sonnet"));
        assert_eq!(w[2].resets_at, None);
        assert!(w.iter().all(|w| w.read_at == 1_792_390_000));
    }

    #[test]
    fn rfc3339_times_read_with_their_offset() {
        assert_eq!(epoch_secs("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(
            epoch_secs("2026-10-19T10:09:59.5+00:00"),
            Some(1_792_404_599)
        );
        assert_eq!(epoch_secs("2026-10-19T12:09:59+02:00"), Some(1_792_404_599));
        assert_eq!(epoch_secs("2026-10-19T08:09:59-02:00"), Some(1_792_404_599));
        assert_eq!(epoch_secs("garbage"), None);
    }

    #[test]
    fn an_expired_or_missing_login_gives_no_token() {
        let creds = serde_json::json!({"claudeAiOauth": {"accessToken": "t", "expiresAt": 2000}});
        assert_eq!(token_from(&creds, 1000).as_deref(), Some("t"));
        assert_eq!(token_from(&creds, 2000), None);
        assert_eq!(token_from(&serde_json::json!({}), 0), None);
    }

    #[test]
    fn the_email_comes_from_the_profiles_claude_json_or_the_one_beside_the_default_dir() {
        let root = tempfile::tempdir().unwrap();
        let profile = root.path().join("work").join("claude");
        std::fs::create_dir_all(&profile).unwrap();
        let json = r#"{"oauthAccount":{"emailAddress":"a@b.co"}}"#;
        std::fs::write(profile.join(".claude.json"), json).unwrap();
        assert_eq!(account_email(&profile).as_deref(), Some("a@b.co"));
        let home = root.path().join("home");
        std::fs::create_dir_all(home.join(".claude")).unwrap();
        std::fs::write(home.join(".claude.json"), json).unwrap();
        assert_eq!(
            account_email(&home.join(".claude")).as_deref(),
            Some("a@b.co")
        );
        assert_eq!(account_email(&root.path().join("none")), None);
    }
}
