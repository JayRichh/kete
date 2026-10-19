//! ACCOUNT SPREAD: which profile a launch that names none runs under.
//!
//! `account_spread` in `config.json` (see `nebula-config`'s Sessions tab):
//! - `manual` (default): the harness's default profile, else none — what
//!   kete did before spreads existed.
//! - `failover`: the default until its session (5h) or weekly use reaches
//!   `account_spread_threshold` percent, then the next enabled profile of
//!   that harness under it.
//! - `least_used`: the enabled profile with the least use.
//!
//! A disabled profile is never picked ([`crate::profiles::Profile::disabled`]).
//! An account with no reading yet counts as unused: picking it is how it
//! gets one. One pure function ([`pick`]) serves the daemon (a create
//! with no profile) and both apps (the toast that says what was picked
//! and why).

use serde::Deserialize;

use crate::entities::{Agent, AgentKind};
use crate::profiles::{self, Profile};
use crate::usage::AccountUsage;

/// The `config.json` keys.
pub const SPREAD_KEY: &str = "account_spread";
pub const THRESHOLD_KEY: &str = "account_spread_threshold";

/// The spreads, as config.json spells them.
pub const SPREADS: &[&str] = &["manual", "failover", "least_used"];
/// The thresholds offered, percent.
pub const THRESHOLDS: &[&str] = &["50", "70", "80", "90", "95"];
pub const DEFAULT_THRESHOLD: u8 = 90;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Spread {
    #[default]
    Manual,
    Failover,
    LeastUsed,
}

impl Spread {
    pub fn parse(word: &str) -> Spread {
        match word.trim() {
            "failover" => Spread::Failover,
            "least_used" | "least-used" => Spread::LeastUsed,
            _ => Spread::Manual,
        }
    }

    /// As config.json spells it ([`SPREADS`]).
    pub fn as_str(&self) -> &'static str {
        match self {
            Spread::Manual => SPREADS[0],
            Spread::Failover => SPREADS[1],
            Spread::LeastUsed => SPREADS[2],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Policy {
    pub spread: Spread,
    /// Percent at which `failover` moves on.
    pub threshold: u8,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            spread: Spread::Manual,
            threshold: DEFAULT_THRESHOLD,
        }
    }
}

impl Policy {
    /// Read from config.json values as written: an odd value is the default.
    pub fn from_words(spread: Option<&str>, threshold: Option<&str>) -> Policy {
        Policy {
            spread: spread.map(Spread::parse).unwrap_or_default(),
            threshold: threshold
                .and_then(|t| t.trim().trim_end_matches('%').parse::<u8>().ok())
                .filter(|t| (1..=100).contains(t))
                .unwrap_or(DEFAULT_THRESHOLD),
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct PolicyOnly {
    account_spread: Option<String>,
    account_spread_threshold: Option<serde_json::Value>,
}

/// The policy `config.json` (with `config.local.json` over it) sets.
pub fn load_policy() -> Policy {
    let loaded = crate::settings::load::<PolicyOnly>(
        &crate::paths::config_path(),
        &crate::paths::config_local_path(),
    );
    let threshold = loaded.value.account_spread_threshold.map(|v| match v {
        serde_json::Value::String(s) => s,
        other => other.to_string(),
    });
    Policy::from_words(loaded.value.account_spread.as_deref(), threshold.as_deref())
}

/// Why [`pick`] chose what it chose.
#[derive(Debug, Clone, PartialEq)]
pub enum Reason {
    /// The launch named the profile.
    Named,
    /// `manual`: the harness's default profile.
    Default,
    /// No usable profile: the CLI's own login.
    NoProfile,
    /// `failover`: the default (or first) is under the threshold.
    UnderThreshold { percent: Option<f32> },
    /// `failover`: moved past accounts at or over the threshold.
    FailedOver { from: String, percent: f32 },
    /// `failover`: every account is over; the least used one.
    AllOver { percent: f32 },
    /// `least_used`.
    LeastUsed { percent: Option<f32> },
    /// The launch's project names this account its harness's default
    /// (`profiles::PROJECT_DEFAULTS_KEY`).
    ProjectDefault,
}

/// A spread's choice.
#[derive(Debug, Clone, PartialEq)]
pub struct Pick<'a> {
    pub profile: Option<&'a Profile>,
    pub reason: Reason,
}

impl Pick<'_> {
    /// One line for a toast or the status bar: `claude: work (default)`,
    /// `claude: home (failover: work at 92%)`.
    pub fn describe(&self, harness: &str) -> String {
        let who = self
            .profile
            .map_or("its own login".to_string(), |p| p.name.trim().to_string());
        let why = match &self.reason {
            Reason::Named => "picked".to_string(),
            Reason::Default => "default".to_string(),
            Reason::NoProfile => "no profile".to_string(),
            Reason::UnderThreshold { percent } => {
                format!("failover, {} used", crate::usage::pct_text(*percent))
            }
            Reason::FailedOver { from, percent } => {
                format!(
                    "failover: {from} at {}",
                    crate::usage::pct_text(Some(*percent))
                )
            }
            Reason::AllOver { percent } => format!(
                "every account over its threshold; least used, {}",
                crate::usage::pct_text(Some(*percent))
            ),
            Reason::LeastUsed { percent } => {
                format!("least used, {}", crate::usage::pct_text(*percent))
            }
            Reason::ProjectDefault => "project default".to_string(),
        };
        format!("{harness}: {who} ({why})")
    }
}

/// The profile a launch on `harness` naming none runs under. `usage`
/// looks an account's latest reading up by profile name; `now_secs`
/// empties windows past their reset.
pub fn pick<'a>(
    all: &'a [Profile],
    harness: &str,
    policy: Policy,
    now_secs: i64,
    usage: impl Fn(&str) -> Option<AccountUsage>,
) -> Pick<'a> {
    if policy.spread == Spread::Manual {
        return match profiles::default_for(all, harness) {
            Some(p) => Pick {
                profile: Some(p),
                reason: Reason::Default,
            },
            None => Pick {
                profile: None,
                reason: Reason::NoProfile,
            },
        };
    }
    // The default first, then config order.
    let mut candidates = profiles::enabled_for(all, harness);
    if let Some(at) = candidates.iter().position(|p| p.default) {
        let d = candidates.remove(at);
        candidates.insert(0, d);
    }
    if candidates.is_empty() {
        return Pick {
            profile: None,
            reason: Reason::NoProfile,
        };
    }
    let used: Vec<Option<f32>> = candidates
        .iter()
        .map(|p| usage(p.name.trim()).and_then(|u| u.worst_at(now_secs)))
        .collect();
    let least = || {
        let mut best = 0;
        for i in 1..candidates.len() {
            if used[i].unwrap_or(0.0) < used[best].unwrap_or(0.0) {
                best = i;
            }
        }
        best
    };
    match policy.spread {
        Spread::Manual => unreachable!("handled above"),
        Spread::LeastUsed => {
            let i = least();
            Pick {
                profile: Some(candidates[i]),
                reason: Reason::LeastUsed { percent: used[i] },
            }
        }
        Spread::Failover => {
            let limit = policy.threshold as f32;
            match used.iter().position(|u| u.unwrap_or(0.0) < limit) {
                Some(0) => Pick {
                    profile: Some(candidates[0]),
                    reason: Reason::UnderThreshold { percent: used[0] },
                },
                Some(i) => Pick {
                    profile: Some(candidates[i]),
                    reason: Reason::FailedOver {
                        from: candidates[0].name.trim().to_string(),
                        percent: used[0].unwrap_or(0.0),
                    },
                },
                None => {
                    let i = least();
                    Pick {
                        profile: Some(candidates[i]),
                        reason: Reason::AllOver {
                            percent: used[i].unwrap_or(0.0),
                        },
                    }
                }
            }
        }
    }
}

/// The profile a launch on `harness` naming none runs under in a project
/// whose default account for that harness is `project_default`: that
/// profile when it is usable, enabled and an account for `harness`, else
/// the spread's [`pick`]. A project default that is switched off, removed
/// or for another harness falls through rather than refusing the launch:
/// the project's setting is a preference, not a pin.
pub fn pick_for_project<'a>(
    all: &'a [Profile],
    harness: &str,
    project_default: Option<&str>,
    policy: Policy,
    now_secs: i64,
    usage: impl Fn(&str) -> Option<AccountUsage>,
) -> Pick<'a> {
    if let Some(name) = project_default.map(str::trim).filter(|n| !n.is_empty()) {
        if let Some(p) = profiles::enabled_for(all, harness)
            .into_iter()
            .find(|p| p.name.trim() == name)
        {
            return Pick {
                profile: Some(p),
                reason: Reason::ProjectDefault,
            };
        }
    }
    pick(all, harness, policy, now_secs, usage)
}

/// The account a launch on `harness` runs under, in the one order every
/// launch path keeps: the profile it names (`asked`; it must exist and be
/// for `harness`, switched off or not), else the project's default account
/// for the harness, else the ACCOUNT SPREAD's pick, which under `manual` is
/// the harness's default profile, else none (the CLI's own login). `Err`
/// says why a named profile cannot be used.
pub fn resolve_launch<'a>(
    all: &'a [Profile],
    harness: &str,
    asked: Option<&str>,
    project_default: Option<&str>,
    policy: Policy,
    now_secs: i64,
    usage: impl Fn(&str) -> Option<AccountUsage>,
) -> Result<Pick<'a>, String> {
    if asked.map(str::trim).is_some_and(|a| !a.is_empty()) {
        return profiles::resolve(all, harness, asked).map(|profile| Pick {
            profile,
            reason: Reason::Named,
        });
    }
    Ok(pick_for_project(
        all,
        harness,
        project_default,
        policy,
        now_secs,
        usage,
    ))
}

/// Whether an account has a login, as far as its files say: credentials
/// were found, not that they still work. The existence of a credentials
/// file (and, for Claude on macOS, where the keychain holds it, the
/// account entry in `.claude.json`) is what counts; Claude's
/// `.credentials.json` is also checked for its subscription entry and that
/// entry's refresh-token expiry ([`LoginCheck::expired`]). No token or
/// other secret is ever read out or kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LoginState {
    LoggedIn,
    NeedsLogin,
    /// A harness kete knows no sign-in files for.
    Unknown,
}

/// The config dir `harness` keeps its login in under `profile` (its home
/// dir, or the CLI's default for an adopted login or no profile).
fn login_dir(
    profile: Option<&Profile>,
    harness: &str,
    root: &std::path::Path,
) -> Option<std::path::PathBuf> {
    use crate::env;
    let var = match harness.trim() {
        "claude" => env::CLAUDE_CONFIG_DIR,
        "codex" => env::CODEX_HOME,
        "copilot" => profiles::GH_CONFIG_DIR,
        _ => return None,
    };
    if let Some(dir) = profile.and_then(|p| p.dir_for(root, var)) {
        return Some(dir);
    }
    if let Some(dir) = env::non_empty(var) {
        return Some(std::path::PathBuf::from(dir));
    }
    let home = env::home_dir()?;
    Some(match harness.trim() {
        "claude" => home.join(".claude"),
        "codex" => home.join(".codex"),
        _ => {
            // gh: %APPDATA%\GitHub CLI on Windows, ~/.config/gh elsewhere.
            #[cfg(windows)]
            {
                std::path::PathBuf::from(std::env::var_os("APPDATA")?).join("GitHub CLI")
            }
            #[cfg(not(windows))]
            {
                home.join(".config").join("gh")
            }
        }
    })
}

/// [`LoginState`] with why: `expired` when the credentials are there but
/// say their login has run out (Claude's refresh token past its
/// `refreshTokenExpiresAt`), which reads as [`LoginState::NeedsLogin`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoginCheck {
    pub state: LoginState,
    pub expired: bool,
}

/// What Claude's `.credentials.json` says about its subscription login.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClaudeCredentials {
    /// No file.
    Missing,
    /// A `claudeAiOauth` entry, its refresh token still good (or with no
    /// expiry written).
    Present,
    /// A `claudeAiOauth` entry whose refresh token has expired.
    Expired,
    /// The file holds no subscription login (only MCP servers' tokens).
    NoLogin,
}

/// Only these two fields are taken from the file: serde passes over every
/// token in it without keeping one.
#[derive(Deserialize)]
struct CredentialsFile {
    #[serde(rename = "claudeAiOauth", default)]
    oauth: Option<OauthEntry>,
}

#[derive(Deserialize)]
struct OauthEntry {
    /// Milliseconds since the epoch.
    #[serde(rename = "refreshTokenExpiresAt", default)]
    refresh_expires_at: Option<i64>,
}

fn claude_credentials(path: &std::path::Path, now_ms: i64) -> ClaudeCredentials {
    let Ok(text) = std::fs::read_to_string(path) else {
        return ClaudeCredentials::Missing;
    };
    match serde_json::from_str::<CredentialsFile>(&text) {
        Ok(CredentialsFile { oauth: None }) => ClaudeCredentials::NoLogin,
        Ok(CredentialsFile {
            oauth: Some(OauthEntry {
                refresh_expires_at: Some(at),
            }),
        }) if at > 0 && at <= now_ms => ClaudeCredentials::Expired,
        Ok(_) => ClaudeCredentials::Present,
        // A shape this build does not know: the file's being there counts,
        // as it always did.
        Err(_) => ClaudeCredentials::Present,
    }
}

/// [`LoginState`] of `profile` (None = the harness's own default login).
pub fn login_state_in(
    profile: Option<&Profile>,
    harness: &str,
    root: &std::path::Path,
) -> LoginState {
    login_check_in(profile, harness, root, crate::clock::now_ms()).state
}

/// [`login_state_in`], with whether the credentials found have expired.
pub fn login_check_in(
    profile: Option<&Profile>,
    harness: &str,
    root: &std::path::Path,
    now_ms: i64,
) -> LoginCheck {
    let Some(dir) = login_dir(profile, harness, root) else {
        return LoginCheck {
            state: LoginState::Unknown,
            expired: false,
        };
    };
    let mut expired = false;
    let found = match harness.trim() {
        "claude" => {
            let creds = claude_credentials(&dir.join(".credentials.json"), now_ms);
            expired = creds == ClaudeCredentials::Expired;
            creds == ClaudeCredentials::Present
                || (!expired && {
                    // `.claude.json` sits in the config dir for a moved one,
                    // beside `~/.claude` for the default.
                    let json = if profile.is_some_and(|p| !p.use_default)
                        || crate::env::non_empty(crate::env::CLAUDE_CONFIG_DIR).is_some()
                    {
                        dir.join(".claude.json")
                    } else {
                        dir.with_file_name(".claude.json")
                    };
                    std::fs::read_to_string(json)
                        .ok()
                        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
                        .is_some_and(|v| v.get("oauthAccount").is_some_and(|a| a.is_object()))
                })
        }
        "codex" => dir.join("auth.json").is_file(),
        _ => dir.join("hosts.yml").is_file(),
    };
    LoginCheck {
        state: if found {
            LoginState::LoggedIn
        } else {
            LoginState::NeedsLogin
        },
        expired,
    }
}

/// [`login_check_in`] under the profiles root, now.
pub fn login_check(profile: Option<&Profile>, harness: &str) -> LoginCheck {
    login_check_in(
        profile,
        harness,
        &profiles::profiles_root(),
        crate::clock::now_ms(),
    )
}

/// The command that signs an account in, for the user to run: `kete
/// profile login <name>` for a profile (an adopted login too: it runs the
/// CLI's sign-in under the profile's environment), the CLI's own sign-in
/// (`claude auth login --claudeai`) for a harness's own login. None for a
/// harness kete knows no sign-in for.
pub fn login_command(profile: Option<&Profile>, harness: &str) -> Option<String> {
    let args = profiles::login_args(harness)?;
    Some(match profile {
        Some(p) => format!("kete profile login {}", p.name.trim()),
        None => format!("{} {}", harness.trim(), args.join(" ")),
    })
}

/// The harness a session runs: a custom harness's id, else its kind.
pub fn session_harness(agent: &Agent) -> String {
    match (&agent.kind, agent.custom_harness.as_deref()) {
        (AgentKind::Custom, Some(id)) => id.to_string(),
        (kind, _) => kind.as_str().to_string(),
    }
}

/// The account a session counts against: its profile's name, or its
/// harness's own login (`@claude`) — the key usage readings are kept
/// under ([`crate::usage::account_key`]).
pub fn session_account(agent: &Agent) -> String {
    crate::usage::account_key(agent.profile.as_deref(), &session_harness(agent))
}

/// Every unarchived session by the account it runs under, in the order
/// given: what an accounts list counts per row.
pub fn sessions_by_account<'a>(
    agents: impl IntoIterator<Item = &'a Agent>,
) -> std::collections::BTreeMap<String, Vec<&'a Agent>> {
    let mut out: std::collections::BTreeMap<String, Vec<&'a Agent>> = Default::default();
    for agent in agents.into_iter().filter(|a| !a.archived) {
        out.entry(session_account(agent)).or_default().push(agent);
    }
    out
}

/// The harnesses whose own login an accounts list shows a row for, sorted:
/// each one in use — a reading kept under `@<harness>`, or an unarchived
/// session of a harness with a known sign-in running with no profile —
/// whatever profiles that harness has. A profile adopting the default
/// login (`use_default`) keeps its own row: its sessions and readings go
/// under its name, not `@<harness>`.
pub fn own_login_harnesses<'a>(
    reading_keys: impl IntoIterator<Item = &'a str>,
    agents: impl IntoIterator<Item = &'a Agent>,
) -> Vec<String> {
    let mut out: Vec<String> = reading_keys
        .into_iter()
        .filter_map(crate::usage::default_login_harness)
        .map(str::to_string)
        .chain(
            agents
                .into_iter()
                .filter(|a| !a.archived && a.profile.as_deref().is_none_or(|p| p.trim().is_empty()))
                .map(session_harness)
                .filter(|h| profiles::login_args(h).is_some()),
        )
        .collect();
    out.sort();
    out.dedup();
    out
}

/// [`login_state_in`] under the profiles root.
pub fn login_state(profile: Option<&Profile>, harness: &str) -> LoginState {
    login_state_in(profile, harness, &profiles::profiles_root())
}

/// [`pick`] against the kept readings, now.
pub fn pick_now<'a>(all: &'a [Profile], harness: &str, policy: Policy) -> Pick<'a> {
    pick(
        all,
        harness,
        policy,
        crate::clock::now_secs() as i64,
        crate::usage::read_account,
    )
}

/// [`resolve_launch`] under the policy `config.json` sets, against the
/// kept readings, now.
pub fn resolve_launch_now<'a>(
    all: &'a [Profile],
    harness: &str,
    asked: Option<&str>,
    project_default: Option<&str>,
) -> Result<Pick<'a>, String> {
    resolve_launch(
        all,
        harness,
        asked,
        project_default,
        load_policy(),
        crate::clock::now_secs() as i64,
        crate::usage::read_account,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::{UsageSource, UsageWindow, WindowKind};
    use std::collections::BTreeMap;

    fn profile(name: &str, default: bool) -> Profile {
        Profile {
            name: name.into(),
            harness: "claude".into(),
            env: BTreeMap::new(),
            default,
            use_default: false,
            disabled: false,
        }
    }

    fn usage(map: &[(&str, f32, f32)]) -> impl Fn(&str) -> Option<AccountUsage> {
        let map: Vec<(String, f32, f32)> = map
            .iter()
            .map(|(n, s, w)| (n.to_string(), *s, *w))
            .collect();
        move |name: &str| {
            map.iter()
                .find(|(n, _, _)| n == name)
                .map(|(n, s, w)| AccountUsage {
                    account: n.clone(),
                    harness: "claude".into(),
                    windows: vec![
                        UsageWindow {
                            kind: WindowKind::Session,
                            label: None,
                            percent: *s,
                            resets_at: None,
                            read_at: 1,
                            estimated: false,
                        },
                        UsageWindow {
                            kind: WindowKind::Weekly,
                            label: None,
                            percent: *w,
                            resets_at: Some(1000),
                            read_at: 1,
                            estimated: false,
                        },
                    ],
                    source: UsageSource::Statusline,
                    at: 0,
                    plan: None,
                })
        }
    }

    fn policy(spread: Spread) -> Policy {
        Policy {
            spread,
            threshold: 90,
        }
    }

    #[test]
    fn manual_is_todays_behaviour() {
        let all = vec![profile("a", false), profile("b", true)];
        let p = pick(
            &all,
            "claude",
            policy(Spread::Manual),
            0,
            usage(&[("b", 99.0, 99.0)]),
        );
        assert_eq!(
            p.profile.unwrap().name,
            "b",
            "the default, whatever its use"
        );
        assert_eq!(p.reason, Reason::Default);
        let none = vec![profile("a", false)];
        let p = pick(&none, "claude", policy(Spread::Manual), 0, usage(&[]));
        assert_eq!(p.profile, None, "no default flagged: no profile");
        assert_eq!(p.reason, Reason::NoProfile);
        assert_eq!(p.describe("claude"), "claude: its own login (no profile)");
    }

    #[test]
    fn failover_moves_on_at_the_threshold_and_skips_disabled() {
        let mut off = profile("off", false);
        off.disabled = true;
        let all = vec![
            profile("a", false),
            off,
            profile("main", true),
            profile("c", false),
        ];
        let u = usage(&[
            ("main", 20.0, 95.0),
            ("off", 0.0, 0.0),
            ("a", 91.0, 10.0),
            ("c", 5.0, 5.0),
        ]);
        let p = pick(&all, "claude", policy(Spread::Failover), 0, &u);
        assert_eq!(
            p.profile.unwrap().name,
            "c",
            "main over on weekly, a over on 5h"
        );
        assert_eq!(
            p.reason,
            Reason::FailedOver {
                from: "main".into(),
                percent: 95.0
            }
        );
        assert_eq!(p.describe("claude"), "claude: c (failover: main at 95%)");
        // Under the threshold: the default stays.
        let u = usage(&[("main", 89.0, 10.0)]);
        let p = pick(&all, "claude", policy(Spread::Failover), 0, &u);
        assert_eq!(p.profile.unwrap().name, "main");
        // A window past its reset no longer counts.
        let u = usage(&[("main", 0.0, 99.0)]);
        let p = pick(&all, "claude", policy(Spread::Failover), 2000, &u);
        assert_eq!(p.profile.unwrap().name, "main");
    }

    /// The order every launch keeps: named > project default > spread >
    /// harness default > own login.
    #[test]
    fn a_launch_takes_the_named_then_the_project_then_the_spread_then_the_default() {
        let mut off = profile("off", false);
        off.disabled = true;
        let mut codex = profile("gpt", false);
        codex.harness = "codex".into();
        let all = vec![
            profile("main", true),
            profile("proj", false),
            profile("spare", false),
            off,
            codex,
        ];
        let u = usage(&[("main", 99.0, 99.0), ("proj", 50.0, 50.0)]);
        let manual = policy(Spread::Manual);
        let launch = |asked, project, policy| {
            resolve_launch(&all, "claude", asked, project, policy, 0, &u)
                .map(|p| (p.profile.map(|p| p.name.clone()), p.reason))
        };
        // Named wins over everything, a switched-off one too.
        assert_eq!(
            launch(Some("off"), Some("proj"), manual).unwrap(),
            (Some("off".into()), Reason::Named)
        );
        assert!(launch(Some("gone"), Some("proj"), manual).is_err());
        assert!(
            launch(Some("gpt"), None, manual).is_err(),
            "named on another harness"
        );
        // The project's default beats the spread and the harness default.
        assert_eq!(
            launch(None, Some("proj"), policy(Spread::LeastUsed)).unwrap(),
            (Some("proj".into()), Reason::ProjectDefault)
        );
        assert_eq!(
            launch(None, Some("proj"), manual).unwrap(),
            (Some("proj".into()), Reason::ProjectDefault)
        );
        // A project default that cannot run falls through, never refuses.
        for unusable in ["off", "gone", "gpt", "  "] {
            assert_eq!(
                launch(None, Some(unusable), manual).unwrap(),
                (Some("main".into()), Reason::Default),
                "{unusable}"
            );
        }
        // No project default: the spread, else the harness default.
        assert_eq!(
            launch(None, None, policy(Spread::LeastUsed)).unwrap().0,
            Some("spare".into())
        );
        assert_eq!(
            launch(None, None, manual).unwrap(),
            (Some("main".into()), Reason::Default)
        );
        let p = pick_for_project(&all, "claude", Some("proj"), manual, 0, &u);
        assert_eq!(p.describe("claude"), "claude: proj (project default)");
    }

    /// A reading with no reset time that has aged past its window no longer
    /// holds an account back: failover comes back to the default.
    #[test]
    fn failover_forgets_an_expired_reading() {
        let all = vec![profile("main", true), profile("b", false)];
        let u = usage(&[("main", 99.0, 99.0)]);
        let p = pick(&all, "claude", policy(Spread::Failover), 1 + 5 * 3600, &u);
        assert_eq!(p.profile.unwrap().name, "main");
        let p = pick(&all, "claude", policy(Spread::Failover), 2, &u);
        assert_eq!(p.profile.unwrap().name, "b", "fresh: still over");
    }

    #[test]
    fn failover_with_every_account_over_takes_the_least_used() {
        let all = vec![profile("a", true), profile("b", false)];
        let u = usage(&[("a", 99.0, 0.0), ("b", 92.0, 0.0)]);
        let p = pick(&all, "claude", policy(Spread::Failover), 0, &u);
        assert_eq!(p.profile.unwrap().name, "b");
        assert_eq!(p.reason, Reason::AllOver { percent: 92.0 });
    }

    #[test]
    fn least_used_prefers_the_unmeasured_and_then_the_default_on_ties() {
        let all = vec![profile("a", false), profile("b", true), profile("c", false)];
        let u = usage(&[("a", 10.0, 30.0), ("b", 40.0, 0.0)]);
        let p = pick(&all, "claude", policy(Spread::LeastUsed), 0, &u);
        assert_eq!(
            p.profile.unwrap().name,
            "c",
            "no reading yet counts as unused"
        );
        let u = usage(&[("a", 0.0, 0.0), ("b", 0.0, 0.0), ("c", 0.0, 0.0)]);
        let p = pick(&all, "claude", policy(Spread::LeastUsed), 0, &u);
        assert_eq!(p.profile.unwrap().name, "b");
        let p = pick(&[], "claude", policy(Spread::LeastUsed), 0, &u);
        assert_eq!(p.profile, None);
    }

    #[test]
    fn login_state_looks_only_for_the_credentials_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let work = profile("work", false);
        assert_eq!(
            login_state_in(Some(&work), "claude", root),
            LoginState::NeedsLogin
        );
        let claude = root.join("work").join("claude");
        std::fs::create_dir_all(&claude).unwrap();
        std::fs::write(
            claude.join(".claude.json"),
            r#"{"hasCompletedOnboarding":true}"#,
        )
        .unwrap();
        assert_eq!(
            login_state_in(Some(&work), "claude", root),
            LoginState::NeedsLogin
        );
        std::fs::write(claude.join(".claude.json"), r#"{"oauthAccount":{}}"#).unwrap();
        assert_eq!(
            login_state_in(Some(&work), "claude", root),
            LoginState::LoggedIn
        );
        let mut codex = profile("cx", false);
        codex.harness = "codex".into();
        assert_eq!(
            login_state_in(Some(&codex), "codex", root),
            LoginState::NeedsLogin
        );
        std::fs::create_dir_all(root.join("cx").join("codex")).unwrap();
        std::fs::write(root.join("cx").join("codex").join("auth.json"), "{}").unwrap();
        assert_eq!(
            login_state_in(Some(&codex), "codex", root),
            LoginState::LoggedIn
        );
        assert_eq!(login_state_in(None, "aider", root), LoginState::Unknown);
    }

    /// Claude's `.credentials.json`: a subscription entry counts, MCP
    /// servers' tokens alone do not, and a refresh token past its expiry
    /// reads as needing a login, marked expired. A file of a shape this
    /// build does not know still counts by being there.
    #[test]
    fn claude_credentials_count_by_their_subscription_entry_and_its_expiry() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let work = profile("work", false);
        let claude = root.join("work").join("claude");
        std::fs::create_dir_all(&claude).unwrap();
        let creds = claude.join(".credentials.json");
        let check = |now: i64| login_check_in(Some(&work), "claude", root, now);
        std::fs::write(&creds, r#"{"mcpOAuth":{"x":{"accessToken":"t"}}}"#).unwrap();
        assert_eq!(
            check(0),
            LoginCheck {
                state: LoginState::NeedsLogin,
                expired: false
            },
            "MCP tokens are not a login"
        );
        std::fs::write(
            &creds,
            r#"{"claudeAiOauth":{"accessToken":"t","expiresAt":10,"refreshTokenExpiresAt":5000}}"#,
        )
        .unwrap();
        assert_eq!(
            check(100).state,
            LoginState::LoggedIn,
            "an access token past expiresAt is refreshed: still signed in"
        );
        assert_eq!(
            check(5000),
            LoginCheck {
                state: LoginState::NeedsLogin,
                expired: true
            }
        );
        // An expired login is not rescued by `.claude.json`'s account entry.
        std::fs::write(claude.join(".claude.json"), r#"{"oauthAccount":{}}"#).unwrap();
        assert!(check(6000).expired);
        assert_eq!(check(6000).state, LoginState::NeedsLogin);
        std::fs::write(&creds, r#"{"claudeAiOauth":{}}"#).unwrap();
        assert_eq!(
            check(i64::MAX).state,
            LoginState::LoggedIn,
            "no expiry written"
        );
        std::fs::write(&creds, "not json").unwrap();
        assert_eq!(check(0).state, LoginState::LoggedIn);
    }

    #[test]
    fn every_account_with_a_known_sign_in_has_a_login_command() {
        let mut adopted = profile("main", false);
        adopted.use_default = true;
        assert_eq!(
            login_command(Some(&adopted), "claude").as_deref(),
            Some("kete profile login main"),
            "an adopted login signs in through kete too"
        );
        assert_eq!(
            login_command(None, "claude").as_deref(),
            Some("claude auth login --claudeai")
        );
        assert_eq!(login_command(None, "codex").as_deref(), Some("codex login"));
        assert_eq!(login_command(None, "aider"), None);
    }

    fn agent(id: &str, kind: AgentKind, profile: Option<&str>, archived: bool) -> Agent {
        Agent {
            id: crate::AgentId(id.into()),
            worktree_id: crate::WorktreeId("w".into()),
            name: id.into(),
            status: crate::AgentStatus::Fresh,
            archived,
            archived_at: 0,
            unseen: false,
            status_changed_at: 0,
            kind,
            custom_harness: None,
            model: None,
            effort: None,
            session_id: None,
            cloud_session_id: None,
            issue_url: None,
            role: crate::entities::AgentRole::Worker,
            sort_order: 0,
            alive: false,
            recent_prompts: Vec::new(),
            profile: profile.map(str::to_string),
            parent: None,
        }
    }

    #[test]
    fn sessions_group_by_their_account_and_archived_ones_drop_out() {
        let agents = [
            agent("a", AgentKind::Claude, Some("work"), false),
            agent("b", AgentKind::Claude, None, false),
            agent("c", AgentKind::Claude, Some("work"), false),
            agent("d", AgentKind::Claude, Some("work"), true),
            agent("e", AgentKind::Codex, Some(" "), false),
        ];
        let by = sessions_by_account(&agents);
        let ids = |key: &str| -> Vec<&str> { by[key].iter().map(|a| a.id.0.as_str()).collect() };
        assert_eq!(ids("work"), ["a", "c"]);
        assert_eq!(ids("@claude"), ["b"]);
        assert_eq!(ids("@codex"), ["e"], "a blank profile is none");
        assert_eq!(by.len(), 3);
    }

    #[test]
    fn an_own_login_row_shows_while_that_login_is_in_use() {
        let agents = [
            agent("a", AgentKind::Claude, Some("work"), false),
            agent("b", AgentKind::Codex, None, false),
            agent("c", AgentKind::Copilot, None, true),
            agent("d", AgentKind::Cursor, None, false),
        ];
        assert_eq!(
            own_login_harnesses(["work", "@claude", "@claude"], &agents),
            ["claude", "codex"],
            "a reading, or a live session with no profile; not an archived \
             one, nor a harness kete knows no sign-in for"
        );
        assert!(own_login_harnesses([], &agents[..1]).is_empty());
    }

    #[test]
    fn policy_words_parse_and_odd_ones_read_as_the_default() {
        assert_eq!(
            Policy::from_words(Some("failover"), Some("80")),
            Policy {
                spread: Spread::Failover,
                threshold: 80
            }
        );
        assert_eq!(
            Policy::from_words(Some("nope"), Some("200")),
            Policy::default()
        );
        assert_eq!(Policy::from_words(None, Some("95%")).threshold, 95);
        assert_eq!(Spread::parse("least_used"), Spread::LeastUsed);
    }
}
