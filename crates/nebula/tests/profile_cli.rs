//! `kete profile list --json`, `pick`, `reserve` and `unreserve` against
//! throwaway data and runtime dirs: no daemon runs, so a pick's load is its
//! holds alone, and the home dir points into the temp dir so no real login
//! is read.

use nebula_core::env::{
    CLAUDE_CONFIG_DIR, CODEX_HOME, CONFIG_FILE, DATA_DIR, IMPORT_BUNDLE, RUNTIME_DIR,
};
use nebula_core::usage::{AccountUsage, UsageSource, UsageWindow, WindowKind};
use serde_json::{json, Value};
use std::path::Path;
use std::process::{Command, Output};

struct Dirs {
    root: tempfile::TempDir,
}

impl Dirs {
    fn new(config: Value) -> Dirs {
        let root = tempfile::tempdir().unwrap();
        for dir in ["data", "rt", "home"] {
            std::fs::create_dir_all(root.path().join(dir)).unwrap();
        }
        let dirs = Dirs { root };
        std::fs::write(dirs.config(), config.to_string()).unwrap();
        dirs
    }

    fn data(&self) -> std::path::PathBuf {
        self.root.path().join("data")
    }

    fn config(&self) -> std::path::PathBuf {
        self.data().join("config.json")
    }

    fn kete(&self, args: &[&str]) -> Output {
        let home = self.root.path().join("home");
        Command::new(env!("CARGO_BIN_EXE_kete"))
            .args(args)
            .env(DATA_DIR, self.data())
            .env(RUNTIME_DIR, self.root.path().join("rt"))
            .env("USERPROFILE", &home)
            .env("HOME", &home)
            .env_remove(CONFIG_FILE)
            .env_remove(IMPORT_BUNDLE)
            .env_remove(CLAUDE_CONFIG_DIR)
            .env_remove(CODEX_HOME)
            .output()
            .expect("failed to run kete")
    }

    fn json(&self, args: &[&str]) -> (Option<i32>, Value) {
        let out = self.kete(args);
        let text = String::from_utf8_lossy(&out.stdout);
        let value = serde_json::from_str(&text).unwrap_or_else(|e| {
            panic!(
                "{e}: {text}\nstderr: {}",
                String::from_utf8_lossy(&out.stderr)
            )
        });
        (out.status.code(), value)
    }

    /// A reading for `account`: `session` percent on the 5-hour window,
    /// `weekly` on the weekly, both resetting in an hour.
    fn reading(&self, account: &str, session: f32, weekly: f32) {
        let now = nebula_core::clock::now_secs() as i64;
        let window = |kind, percent| UsageWindow {
            kind,
            label: None,
            percent,
            resets_at: Some(now + 3600),
            read_at: now,
            estimated: false,
        };
        let usage = AccountUsage {
            account: account.into(),
            harness: "claude".into(),
            windows: vec![
                window(WindowKind::Session, session),
                window(WindowKind::Weekly, weekly),
            ],
            source: UsageSource::Statusline,
            at: now * 1000,
            plan: None,
        };
        nebula_core::usage::write_account_in(&self.data().join("profiles"), &usage).unwrap();
    }
}

fn claude(name: &str, extra: Value) -> Value {
    let mut entry = json!({ "name": name, "harness": "claude" });
    entry
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    entry
}

fn holds_file(data: &Path) -> std::path::PathBuf {
    data.join("profiles").join(".picks.json")
}

#[test]
fn list_json_names_every_account_with_its_windows_and_flags() {
    let dirs = Dirs::new(json!({
        "account_spread": "least_used",
        "profiles": [
            claude("jay", json!({ "default": true, "reserved": true })),
            claude("max2", json!({ "disabled": true })),
        ]
    }));
    dirs.reading("jay", 20.0, 5.0);
    dirs.reading("@claude", 1.0, 1.0);
    let (code, out) = dirs.json(&["profile", "list", "--json"]);
    assert_eq!(code, Some(0));
    assert_eq!(out["spread"], "least_used");
    assert_eq!(out["threshold"], 90);
    let accounts = out["accounts"].as_array().unwrap();
    assert_eq!(accounts.len(), 3, "{out}");
    let jay = &accounts[0];
    assert_eq!(jay["name"], "jay");
    assert_eq!(jay["harness"], "claude");
    assert_eq!(jay["kind"], "profile");
    assert_eq!(jay["default"], true);
    assert_eq!(jay["reserved"], true);
    assert_eq!(jay["disabled"], false);
    assert_eq!(jay["signed_in"], false);
    assert_eq!(jay["email"], Value::Null);
    assert_eq!(jay["live_sessions"], 0);
    assert_eq!(jay["holds"], 0);
    let windows = jay["windows"].as_array().unwrap();
    assert_eq!(windows[0]["kind"], "session");
    assert_eq!(windows[0]["percent"], 20.0);
    assert_eq!(windows[1]["kind"], "weekly");
    for key in ["resets_at", "estimated", "read_at"] {
        assert!(windows[0].get(key).is_some(), "{key} missing: {jay}");
    }
    assert_eq!(accounts[1]["disabled"], true);
    assert_eq!(accounts[1]["windows"], json!([]));
    let own = &accounts[2];
    assert_eq!(
        (&own["name"], &own["kind"], &own["reserved"]),
        (&json!("@claude"), &json!("own_login"), &json!(false))
    );
}

#[test]
fn pick_json_names_the_account_and_why_and_holds_it() {
    let dirs = Dirs::new(json!({
        "profiles": [
            claude("jay", json!({ "default": true, "reserved": true })),
            claude("max2", json!({})),
            claude("max3", json!({})),
        ]
    }));
    dirs.reading("jay", 0.0, 0.0);
    dirs.reading("max2", 70.0, 10.0);
    dirs.reading("max3", 20.0, 40.0);
    let (code, out) = dirs.json(&["profile", "pick", "--json"]);
    assert_eq!(code, Some(0), "{out}");
    assert_eq!(out["harness"], "claude");
    assert_eq!(out["profile"], "max3", "manual reads as least used: {out}");
    assert_eq!(out["reason"], "least_used");
    assert_eq!(out["line"], "claude: max3 (least used, 40%)");
    assert!(out["held_until"].as_i64().unwrap() > 0);
    let rows = out["candidates"].as_array().unwrap();
    let row = |name: &str| rows.iter().find(|r| r["name"] == name).unwrap().clone();
    assert_eq!(row("jay")["eligible"], false);
    assert_eq!(row("jay")["why"], "reserved");
    assert_eq!(row("max3")["why"], "picked");
    assert_eq!(row("max3")["percent"], 40.0);
    assert_eq!(row("max2")["why"], "eligible");
    for key in ["live_sessions", "holds"] {
        assert!(row("max2").get(key).is_some(), "{key}");
    }
    assert!(holds_file(&dirs.data()).is_file());
    // The plain answer is the name alone, for `--profile $(kete profile pick)`.
    let out = dirs.kete(&["profile", "pick"]);
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "max3");
}

#[test]
fn pick_exits_3_when_no_account_is_eligible() {
    let dirs = Dirs::new(json!({
        "profiles": [
            claude("jay", json!({ "default": true, "reserved": true })),
            claude("off", json!({ "disabled": true })),
        ]
    }));
    let (code, out) = dirs.json(&["profile", "pick", "--json"]);
    assert_eq!(code, Some(3));
    assert_eq!(out["profile"], Value::Null);
    assert_eq!(out["reason"], "none");
    assert_eq!(out["held_until"], Value::Null);
    let out = dirs.kete(&["profile", "pick"]);
    assert_eq!(out.status.code(), Some(3));
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("no claude account is eligible"));
    assert!(!holds_file(&dirs.data()).exists());
}

#[test]
fn a_dry_run_holds_nothing_and_shows_every_candidate() {
    let dirs = Dirs::new(json!({
        "profiles": [
            claude("jay", json!({ "default": true, "reserved": true })),
            claude("max2", json!({})),
        ]
    }));
    let out = dirs.kete(&["profile", "pick", "--dry-run"]);
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("jay") && text.contains("reserved"), "{text}");
    assert!(text.contains("max2") && text.contains("picked"), "{text}");
    assert!(text.contains("nothing held"), "{text}");
    let (_, out) = dirs.json(&["profile", "pick", "--dry-run", "--json"]);
    assert_eq!(out["held_until"], Value::Null);
    assert!(
        !holds_file(&dirs.data()).exists(),
        "a dry run holds nothing"
    );
}

#[test]
fn two_picks_in_a_row_spread_when_the_readings_tie() {
    let dirs = Dirs::new(json!({
        "profiles": [claude("max2", json!({})), claude("max3", json!({}))]
    }));
    dirs.reading("max2", 21.0, 0.0);
    dirs.reading("max3", 28.0, 0.0);
    let first = dirs.kete(&["profile", "pick"]);
    let second = dirs.kete(&["profile", "pick"]);
    let names = [&first, &second].map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
    assert_eq!(names, ["max2".to_string(), "max3".to_string()]);
    let (_, out) = dirs.json(&["profile", "list", "--json"]);
    let holds: Vec<&Value> = out["accounts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| &a["holds"])
        .collect();
    assert_eq!(holds, [&json!(1), &json!(1)]);
}

#[test]
fn reserve_and_unreserve_round_trip_in_config_json() {
    let dirs = Dirs::new(json!({
        "theme": "rose",
        "profiles": [claude("jay", json!({ "default": true }))]
    }));
    let read = || -> Value {
        serde_json::from_str(&std::fs::read_to_string(dirs.config()).unwrap()).unwrap()
    };
    let out = dirs.kete(&["profile", "reserve", "jay"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(read()["profiles"][0]["reserved"], true);
    assert_eq!(read()["theme"], "rose");
    let out = dirs.kete(&["profile", "list"]);
    assert!(String::from_utf8_lossy(&out.stdout).contains("jay * (yours)"));
    let out = dirs.kete(&["profile", "unreserve", "jay"]);
    assert!(out.status.success());
    assert!(read()["profiles"][0].get("reserved").is_none());
    let out = dirs.kete(&["profile", "reserve", "nope"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("no profile named `nope`"));
}
