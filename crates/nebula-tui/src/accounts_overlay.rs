//! The ACCOUNTS OVERLAY (`⇧U`): every account kete can launch under —
//! each PROFILE, and each harness's own login that has a usage reading
//! (`@claude`) — with its plan usage as two block bars (session 5h and
//! weekly; a Copilot account's monthly allowances), when the windows reset,
//! where the numbers came from, and whether it is signed in. `Space` (or
//! `e`) switches a profile on or off, `d` (or `*`) makes it its harness's
//! default; both write config.json in place (`nebula_core::profiles`),
//! which the config beat then reloads everywhere. A login is never run
//! from here: an account that needs one shows the `kete profile login`
//! command to type.

use std::path::PathBuf;

use nebula_core::accounts::LoginState;
use nebula_core::profiles::{self, Profile};
use nebula_core::usage::{self, AccountUsage, ContextUsage};
use ratatui::layout::Rect;

/// One row of the overlay.
#[derive(Debug, Clone, PartialEq)]
pub struct AccountRow {
    /// The account key: the profile's name, or `@<harness>`.
    pub key: String,
    pub harness: String,
    /// None for a harness's own login.
    pub profile: Option<Profile>,
    pub usage: Option<AccountUsage>,
    pub login: LoginState,
    /// The harness's default profile.
    pub default: bool,
}

impl AccountRow {
    /// What the row is called: the profile's name, or `claude (own login)`.
    pub fn label(&self) -> String {
        match &self.profile {
            Some(p) => p.name.trim().to_string(),
            None => format!("{} (own login)", self.harness),
        }
    }

    pub fn disabled(&self) -> bool {
        self.profile.as_ref().is_some_and(|p| p.disabled)
    }

    /// The login hint for an account that needs one.
    pub fn login_text(&self) -> String {
        match self.login {
            LoginState::LoggedIn => "signed in".into(),
            LoginState::NeedsLogin => match self.profile.as_ref().and_then(profiles::login_hint) {
                Some(hint) => format!("needs login: {hint}"),
                None => "needs login".into(),
            },
            LoginState::Unknown => String::new(),
        }
    }
}

/// The rows for `all` profiles and the `usage` readings: profiles in
/// config order, then each own-login account that has a reading.
pub fn build_rows(
    all: &[Profile],
    usage: &[AccountUsage],
    login: impl Fn(Option<&Profile>, &str) -> LoginState,
) -> Vec<AccountRow> {
    let reading = |key: &str| usage.iter().find(|u| u.account == key).cloned();
    let mut rows: Vec<AccountRow> = all
        .iter()
        .filter(|p| p.problem().is_none())
        .map(|p| AccountRow {
            key: p.name.trim().to_string(),
            harness: p.harness.trim().to_string(),
            profile: Some(p.clone()),
            usage: reading(p.name.trim()),
            login: login(Some(p), p.harness.trim()),
            default: profiles::default_for(all, &p.harness).is_some_and(|d| d.name == p.name),
        })
        .collect();
    for u in usage {
        if let Some(harness) = usage::default_login_harness(&u.account) {
            rows.push(AccountRow {
                key: u.account.clone(),
                harness: harness.to_string(),
                profile: None,
                usage: Some(u.clone()),
                login: login(None, harness),
                default: false,
            });
        }
    }
    rows
}

/// The overlay's state.
#[derive(Debug, Clone, PartialEq)]
pub struct AccountsView {
    pub rows: Vec<AccountRow>,
    pub cursor: usize,
    /// The settings file edits go to.
    pub config: PathBuf,
    /// Drawn rect, for click-outside.
    pub area: Rect,
}

impl AccountsView {
    pub fn new(rows: Vec<AccountRow>, config: PathBuf) -> Self {
        Self {
            rows,
            cursor: 0,
            config,
            area: Rect::default(),
        }
    }

    pub fn selected(&self) -> Option<&AccountRow> {
        self.rows.get(self.cursor)
    }

    pub fn step(&mut self, delta: i32) {
        if self.rows.is_empty() {
            self.cursor = 0;
            return;
        }
        let n = self.rows.len() as i32;
        self.cursor = (self.cursor as i32 + delta).clamp(0, n - 1) as usize;
    }

    /// Switch the selected profile on or off in the settings file. The
    /// message for the footer, or why not.
    pub fn toggle_enabled(&mut self) -> Result<String, String> {
        let row = self.selected().ok_or("no account selected")?;
        let Some(profile) = &row.profile else {
            return Err(format!(
                "{} is the CLI's own login: add it as a profile to switch it off (kete profile add <name> --harness {} --use-default)",
                row.label(),
                row.harness
            ));
        };
        let name = profile.name.trim().to_string();
        let on = profile.disabled;
        profiles::set_enabled_in_file(&self.config, &name, on)?;
        if let Some(Some(p)) = self.rows.get_mut(self.cursor).map(|r| r.profile.as_mut()) {
            p.disabled = !on;
        }
        Ok(if on {
            format!("{name} is on")
        } else {
            format!("{name} is off: launches that pick no profile skip it")
        })
    }

    /// Make the selected profile its harness's default.
    pub fn set_default(&mut self) -> Result<String, String> {
        let row = self.selected().ok_or("no account selected")?;
        let Some(profile) = &row.profile else {
            return Err(format!("{} is not a profile", row.label()));
        };
        let (name, harness) = (profile.name.trim().to_string(), row.harness.clone());
        profiles::set_default_in_file(&self.config, &name)?;
        for r in &mut self.rows {
            if r.harness == harness && r.profile.is_some() {
                r.default = r.key == name;
            }
        }
        Ok(format!("{name} is now the {harness} default"))
    }
}

/// `ctx 42%` (with `⟳2` after two compactions) for a session card; None
/// when nothing is known.
pub fn context_label(ctx: &ContextUsage) -> Option<String> {
    let p = ctx.percent?;
    let mut out = format!("ctx {}", usage::pct_text(Some(p)));
    if ctx.compactions > 0 {
        out.push_str(&format!(" \u{27f3}{}", ctx.compactions));
    }
    Some(out)
}

/// The QUICK PROMPT's profile field: `work ▰▰▱▱ 21% ▰▱▱▱ 3%`, or the
/// name alone when no reading is known.
pub fn field_text(name: &str, reading: Option<&AccountUsage>, now: i64) -> String {
    let Some(u) = reading else {
        return name.to_string();
    };
    let mut out = name.to_string();
    for w in u.bars() {
        let p = w.map(|w| w.percent_at(now));
        out.push(' ');
        out.push_str(&usage::block_bar(p, 4));
        out.push(' ');
        out.push_str(&usage::pct_text(p));
    }
    out
}

/// The worst tone of a reading's two bars.
pub fn reading_tone(reading: Option<&AccountUsage>, now: i64) -> usage::Tone {
    reading
        .and_then(|u| u.worst_at(now))
        .map_or(usage::Tone::Ok, usage::tone)
}

/// `5m ago`, `2h ago` — how old a reading is.
pub fn age(at_ms: i64, now_ms: i64) -> String {
    let secs = ((now_ms - at_ms) / 1000).max(0);
    match secs {
        0..=59 => "just now".into(),
        60..=3599 => format!("{}m ago", secs / 60),
        3600..=86_399 => format!("{}h ago", secs / 3600),
        _ => format!("{}d ago", secs / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nebula_core::usage::{UsageSource, UsageWindow, WindowKind};
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

    fn reading(key: &str, s: f32, w: f32) -> AccountUsage {
        AccountUsage {
            account: key.into(),
            harness: "claude".into(),
            windows: vec![
                UsageWindow {
                    kind: WindowKind::Session,
                    label: None,
                    percent: s,
                    resets_at: None,
                },
                UsageWindow {
                    kind: WindowKind::Weekly,
                    label: None,
                    percent: w,
                    resets_at: None,
                },
            ],
            source: UsageSource::Statusline,
            at: 0,
            plan: None,
        }
    }

    #[test]
    fn rows_are_profiles_then_own_logins_with_readings() {
        let all = vec![profile("work", true), profile("home", false)];
        let usage = vec![reading("work", 10.0, 20.0), reading("@claude", 5.0, 1.0)];
        let rows = build_rows(&all, &usage, |p, _| match p {
            Some(p) if p.name == "home" => LoginState::NeedsLogin,
            _ => LoginState::LoggedIn,
        });
        let keys: Vec<&str> = rows.iter().map(|r| r.key.as_str()).collect();
        assert_eq!(keys, ["work", "home", "@claude"]);
        assert!(rows[0].default && !rows[1].default);
        assert!(rows[0].usage.is_some() && rows[1].usage.is_none());
        assert_eq!(rows[2].label(), "claude (own login)");
        assert_eq!(rows[1].login_text(), "needs login: kete profile login home");
        assert_eq!(rows[0].login_text(), "signed in");
        // Jay's case: no profiles, one own-login reading.
        let rows = build_rows(&[], &[reading("@claude", 1.0, 1.0)], |_, _| {
            LoginState::LoggedIn
        });
        assert_eq!(rows.len(), 1);
    }

    #[test]
    fn toggle_and_default_write_the_settings_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(
            &path,
            r#"{"theme":"x","profiles":[{"name":"work","harness":"claude","default":true},{"name":"home","harness":"claude"}]}"#,
        )
        .unwrap();
        let all = vec![profile("work", true), profile("home", false)];
        let mut view = AccountsView::new(
            build_rows(&all, &[reading("@claude", 0.0, 0.0)], |_, _| {
                LoginState::LoggedIn
            }),
            path.clone(),
        );
        view.step(1);
        assert!(view.toggle_enabled().unwrap().contains("off"));
        assert!(view.rows[1].disabled());
        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(saved["profiles"][1]["disabled"], true);
        assert_eq!(saved["theme"], "x", "other keys kept");
        assert!(view.toggle_enabled().unwrap().contains("on"));
        view.set_default().unwrap();
        assert!(view.rows[1].default && !view.rows[0].default);
        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(saved["profiles"][1]["default"], true);
        assert!(saved["profiles"][0].get("default").is_none());
        assert!(saved["profiles"][1].get("disabled").is_none());
        // The own-login row is not a profile.
        view.step(5);
        assert_eq!(view.cursor, 2, "clamped");
        assert!(view.toggle_enabled().is_err());
        assert!(view.set_default().is_err());
    }

    #[test]
    fn field_and_card_labels() {
        assert_eq!(field_text("work", None, 0), "work");
        assert_eq!(
            field_text("work", Some(&reading("work", 50.0, 10.0)), 0),
            "work \u{25b0}\u{25b0}\u{25b1}\u{25b1} 50% \u{25b0}\u{25b1}\u{25b1}\u{25b1} 10%"
        );
        assert_eq!(
            reading_tone(Some(&reading("w", 81.0, 0.0)), 0),
            usage::Tone::Warn
        );
        assert_eq!(reading_tone(None, 0), usage::Tone::Ok);
        let ctx = ContextUsage {
            percent: Some(42.4),
            tokens: None,
            window: None,
            compactions: 2,
            last_compact_at: None,
            source: UsageSource::Statusline,
            at: 0,
        };
        assert_eq!(context_label(&ctx).as_deref(), Some("ctx 42% \u{27f3}2"));
        assert_eq!(
            context_label(&ContextUsage {
                percent: None,
                ..ctx
            }),
            None
        );
        assert_eq!(age(0, 30_000), "just now");
        assert_eq!(age(0, 3 * 3_600_000), "3h ago");
    }
}
