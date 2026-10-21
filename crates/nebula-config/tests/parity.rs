//! The desktop window renders every setting the TUI's overlay declares,
//! and writes them back without touching what it did not change.

use nebula_config::desktop::DesktopPrefs;
use nebula_config::schema::{
    self, export, rendered_kinds, set_in, Control, ProjectRef, Target, DESKTOP_EXCLUDED,
    TERMINAL_ONLY,
};
use nebula_config::{all_settings, Config, SettingKind};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn layers() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.json");
    let local = dir.path().join("config.local.json");
    (dir, path, local)
}

fn read(path: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn one_project() -> Vec<ProjectRef> {
    vec![ProjectRef {
        name: "demo".into(),
        path: PathBuf::from("/repo/demo"),
    }]
}

fn declared() -> Vec<SettingKind> {
    all_settings().map(|(_, _, spec)| spec.kind).collect()
}

#[test]
fn every_overlay_row_is_rendered_or_explicitly_excluded() {
    let schema = export(&Config::default(), &DesktopPrefs::default(), &one_project());
    let rendered = rendered_kinds(&schema);
    for kind in declared() {
        let shown = rendered.contains(&kind);
        let excluded = DESKTOP_EXCLUDED.contains(&kind);
        assert!(
            shown ^ excluded,
            "{kind:?}: rendered {shown}, excluded {excluded} (exactly one, please)"
        );
    }
    for kind in TERMINAL_ONLY {
        assert!(
            declared().contains(kind),
            "{kind:?} is listed but no tab declares it"
        );
    }
}

/// A running TUI reloads config.json on its config beat — the values it
/// mirrors into its own state and the hotkeys too — so no row tells the
/// window's user to restart the terminal app, the once-mirrored ones
/// (theme, kanban, worktree layout...) included.
#[test]
fn no_row_carries_a_restart_note() {
    let schema = export(&Config::default(), &DesktopPrefs::default(), &one_project());
    let mut seen = Vec::new();
    for tab in &schema.tabs {
        for group in &tab.groups {
            for row in &group.rows {
                assert_eq!(row.restart, None, "{} in {:?}", row.label, tab.title);
                if let Target::Global { key } = &row.target {
                    seen.push(schema::kind_for_key(key).unwrap());
                }
            }
        }
    }
    for kind in [
        SettingKind::Theme,
        SettingKind::Kanban,
        SettingKind::WorktreeLayout,
        SettingKind::HideDraftPrs,
    ] {
        assert!(seen.contains(&kind), "{kind:?} was checked");
    }
}

#[test]
fn terminal_only_rows_sit_in_the_terminal_group() {
    let schema = export(&Config::default(), &DesktopPrefs::default(), &one_project());
    for tab in &schema.tabs {
        for group in &tab.groups {
            for row in &group.rows {
                if let Target::Global { key } = &row.target {
                    let kind = schema::kind_for_key(key).unwrap();
                    assert_eq!(
                        group.terminal,
                        TERMINAL_ONLY.contains(&kind),
                        "{key} in group {:?}",
                        group.title
                    );
                }
            }
        }
    }
}

/// A new `Config` field must be a settings row, or be named here as one
/// the overlay does not edit (retired keys, maps edited elsewhere).
#[test]
fn every_config_key_is_a_row_or_named_as_not_one() {
    const NOT_ROWS: &[&str] = &[
        // retired, kept for older builds sharing the file
        "git_init_on_create",
        "hide_card_prompt",
        "card_line_changes",
        "skip_session_naming",
        "confirm_on_archive",
        "focus_tint",
        "show_workspaces",
        "hide_projects",
        "hide_worktrees",
        "hide_sessions",
        "hide_root_worktree",
        "recent_prompts",
        "recent_prompts_count",
        "show_key_combos",
        "pr_issue_counts",
        // per-project, per-harness and hand-edited maps
        "projects",
        "keybindings",
        "claude_models",
        "custom_harnesses",
        "harnesses",
        "profiles",
        "browsers",
        // machine-local, written only through the daemon's SetStudio
        "studio",
        // the Agents tab's per-harness rows write these legacy keys
        "claude_model",
        "claude_effort",
        "codex_model",
        "codex_effort",
        "cursor_model",
        "cursor_effort",
        "pi_model",
        "pi_effort",
        "muse_model",
        "muse_effort",
        "opencode_model",
        "claude_enabled",
        "codex_enabled",
        "cursor_enabled",
        "pi_enabled",
        "muse_enabled",
        "opencode_enabled",
    ];
    let rows: BTreeSet<&str> = declared().into_iter().map(SettingKind::key).collect();
    let Value::Object(keys) = serde_json::to_value(Config::default()).unwrap() else {
        unreachable!()
    };
    for key in keys.keys() {
        assert!(
            rows.contains(key.as_str()) || NOT_ROWS.contains(&key.as_str()),
            "config key {key} is neither a settings row nor named in NOT_ROWS"
        );
    }
}

#[test]
fn every_global_row_takes_each_of_its_values() {
    let schema = export(&Config::default(), &DesktopPrefs::default(), &[]);
    for row in schema
        .tabs
        .iter()
        .flat_map(|t| &t.groups)
        .flat_map(|g| &g.rows)
    {
        let Target::Global { key } = &row.target else {
            continue;
        };
        let kind = schema::kind_for_key(key).unwrap();
        let values: Vec<Value> = match &row.control {
            Control::Toggle => vec![json!(true), json!(false)],
            Control::Choice { options, .. } => options.iter().map(|o| json!(o)).collect(),
            Control::Text { .. } => vec![json!("develop"), json!("")],
            Control::Font { .. } => vec![json!("Fira Code"), json!("")],
        };
        for value in values {
            let mut cfg = Config::default();
            schema::set_value(&mut cfg, kind, &value).unwrap_or_else(|e| panic!("{key}: {e}"));
            assert_eq!(schema::get_value(&cfg, kind), value, "{key}");
        }
    }
}

#[test]
fn a_write_from_the_window_changes_one_key_and_keeps_the_rest() {
    let (_dir, path, local) = layers();
    std::fs::write(
        &path,
        r#"{"future_key": {"a": 1}, "editor": "hx", "desktop": {"theme": "dark"}}"#,
    )
    .unwrap();
    std::fs::write(&local, r#"{"session_idle_timeout": "1h"}"#).unwrap();
    let global = |key: &str| Target::Global { key: key.into() };
    set_in(&path, &local, &global("kanban"), &json!(true)).unwrap();
    set_in(
        &path,
        &local,
        &global("session_idle_timeout"),
        &json!("15m"),
    )
    .unwrap();
    set_in(
        &path,
        &local,
        &Target::Desktop { key: "home".into() },
        &json!("board"),
    )
    .unwrap();
    assert_eq!(
        read(&path),
        json!({"future_key": {"a": 1}, "editor": "hx", "kanban": true,
               "desktop": {"theme": "dark", "home": "board", "diff_mode": "side", "startup": "home",
                           "editor_folder": "auto", "editor_file": "auto",
                           "editor_reuse": "off"}})
    );
    assert_eq!(read(&local), json!({"session_idle_timeout": "15m"}));
    assert!(set_in(&path, &local, &global("kanban"), &json!("sideways")).is_err());
    assert!(set_in(&path, &local, &global("no_such_key"), &json!(true)).is_err());
}

#[test]
fn agent_and_project_rows_write_where_the_tui_reads_them() {
    let (_dir, path, local) = layers();
    let agent = |id: &str, field: &str| Target::Agent {
        harness: id.into(),
        field: field.into(),
    };
    set_in(
        &path,
        &local,
        &agent("codex", "model"),
        &json!("gpt-5.6-terra"),
    )
    .unwrap();
    set_in(&path, &local, &agent("codex", "effort"), &json!("high")).unwrap();
    set_in(&path, &local, &agent("pi", "enabled"), &json!(false)).unwrap();
    let project = |field: &str| Target::Project {
        path: "/repo/demo".into(),
        field: field.into(),
    };
    set_in(
        &path,
        &local,
        &project("run_command"),
        &json!(" npm run dev "),
    )
    .unwrap();
    set_in(&path, &local, &project("browser"), &json!("edge")).unwrap();

    let cfg = Config::load_at(&path, &local);
    assert_eq!(cfg.codex_model, "gpt-5.6-terra");
    assert_eq!(cfg.codex_effort, "high");
    assert!(!cfg.pi_enabled);
    let p = cfg.project(Path::new("/repo/demo"));
    assert_eq!(p.run_command, "npm run dev");
    assert_eq!(p.browser, "edge");
    let (model, effort) = cfg.launch_model_effort(nebula_core::AgentKind::Codex, None, None, None);
    assert_eq!(model.as_deref(), Some("gpt-5.6-terra"));
    assert_eq!(effort.as_deref(), Some("high"));
    assert_eq!(
        cfg.launch_model_effort(nebula_core::AgentKind::Claude, None, None, None),
        (None, None),
        "default reaches the daemon as no flag"
    );
}

#[test]
fn the_last_harness_stays_on_and_a_refused_write_writes_nothing() {
    let (_dir, path, local) = layers();
    let ids = nebula_config::agent_kind_names();
    for (i, id) in ids.iter().enumerate() {
        let target = Target::Agent {
            harness: id.clone(),
            field: "enabled".into(),
        };
        let result = set_in(&path, &local, &target, &json!(false));
        assert_eq!(result.is_err(), i == ids.len() - 1, "{id}: {result:?}");
    }
    let cfg = Config::load_at(&path, &local);
    let usable = nebula_core::harness::usable(&cfg.harness_registry()).len();
    assert_eq!(usable, 1, "the refused write left the last one on");
}

#[test]
fn the_agents_tab_lists_custom_harnesses_and_the_quick_prompt_defaults() {
    let mut cfg = Config::default();
    cfg.custom_harnesses
        .push(nebula_core::harness::CustomHarness {
            id: "aider".into(),
            label: "Aider".into(),
            program: "aider".into(),
            enabled: true,
            model: "default".into(),
            model_flag: "--model".into(),
            hooks: None,
        });
    let schema = export(&cfg, &DesktopPrefs::default(), &[]);
    let agents = schema.tabs.iter().find(|t| t.id == "agents").unwrap();
    let titles: Vec<&str> = agents.groups.iter().map(|g| g.title.as_str()).collect();
    assert_eq!(titles.first(), Some(&"Quick prompt"));
    assert!(titles.contains(&"Aider"), "{titles:?}");
    assert_eq!(titles.last(), Some(&"Terminal app"));
}

#[test]
fn reset_puts_rows_back_and_keeps_what_it_does_not_show() {
    let (_dir, path, local) = layers();
    std::fs::write(
        &path,
        r#"{"kanban": true, "claude_model": "opus", "theme": "rose",
            "projects": {"/repo/demo": {"run_command": "make"}},
            "profiles": [{"name": "work", "harness": "claude"}],
            "keybindings": {"quit": "ctrl+q"}, "future_key": 5,
            "desktop": {"theme": "dark", "later": true}}"#,
    )
    .unwrap();
    schema::reset_in(&path, &local).unwrap();
    let cfg = Config::load_at(&path, &local);
    assert!(!cfg.kanban);
    assert_eq!(cfg.claude_model, "default");
    assert_eq!(cfg.theme, Config::default().theme);
    assert_eq!(cfg.project(Path::new("/repo/demo")).run_command, "make");
    assert_eq!(
        cfg.keybindings.get("quit").map(String::as_str),
        Some("ctrl+q")
    );
    let raw = read(&path);
    assert_eq!(raw["future_key"], 5);
    assert_eq!(raw["profiles"][0]["name"], "work");
    assert_eq!(raw["desktop"]["theme"], "system");
    assert_eq!(raw["desktop"]["later"], true);
}

/// Every row of the window's own tab takes each of the values it offers
/// (a font row, a family or empty) and reads it back; the Appearance
/// group holds the theme, the fonts and the sizes, and the Theme row lists
/// an imported theme by its name.
#[test]
fn every_desktop_row_takes_each_of_its_values() {
    let mut prefs = DesktopPrefs::default();
    let palette = nebula_config::look::from_vscode(
        &nebula_config::vscode::ThemeData::default(),
        false,
        "Atom One Dark",
    );
    prefs.import_theme(palette).unwrap();
    let (_dir, path, local) = layers();
    // The import is on disk, so the Theme row's `import:` value is too.
    nebula_config::desktop::DesktopPrefs::update_in(&path, &local, |p| {
        *p = prefs.clone();
        Ok(())
    })
    .unwrap();
    let schema = export(&Config::default(), &prefs, &[]);
    let desktop = schema.tabs.iter().find(|t| t.id == "desktop").unwrap();
    let look = desktop
        .groups
        .iter()
        .find(|g| g.title == "Appearance")
        .expect("an Appearance group");
    let keys: Vec<&str> = look
        .rows
        .iter()
        .map(|r| match &r.target {
            Target::Desktop { key } => key.as_str(),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(
        keys,
        [
            "theme",
            "font_ui",
            "ui_size",
            "font_mono",
            "font_term",
            "term_size",
            "term_line",
            "editor_size"
        ]
    );
    let Control::Choice { options, labels } = &look.rows[0].control else {
        panic!("the theme is a choice");
    };
    assert_eq!(
        options.last().map(String::as_str),
        Some("import:atom-one-dark")
    );
    assert_eq!(labels.last().map(String::as_str), Some("Atom One Dark"));
    for row in desktop.groups.iter().flat_map(|g| &g.rows) {
        let Target::Desktop { key } = &row.target else {
            panic!("the window's tab writes the window's preferences");
        };
        let values: Vec<Value> = match &row.control {
            Control::Choice { options, .. } => options.iter().map(|o| json!(o)).collect(),
            Control::Font { .. } => vec![json!("Fira Code"), json!("")],
            other => panic!("{key}: {other:?}"),
        };
        for value in values {
            set_in(&path, &local, &row.target, &value).unwrap_or_else(|e| panic!("{key}: {e}"));
            assert_eq!(
                DesktopPrefs::load_at(&path, &local).get(key),
                value.as_str().unwrap(),
                "{key}"
            );
        }
    }
    // The window's Reset leaves the imported themes to pick again.
    schema::reset_in(&path, &local).unwrap();
    let after = DesktopPrefs::load_at(&path, &local);
    assert_eq!(after.theme, "system");
    assert_eq!(after.term_size, "13");
    assert!(after.themes.contains_key("atom-one-dark"));
}

#[test]
fn a_project_row_must_name_a_repo_the_daemon_knows() {
    let projects = one_project();
    let row = |path: &str| Target::Project {
        path: path.into(),
        field: "run_command".into(),
    };
    assert_eq!(
        schema::resolve_target(&row("/repo/demo"), &projects).unwrap(),
        row("/repo/demo")
    );
    // A trailing separator maps onto the stored key.
    let resolved = schema::resolve_target(&row("/repo/demo/"), &projects).unwrap();
    assert_eq!(resolved, row("/repo/demo"));
    let err = schema::resolve_target(&row("/repo/other"), &projects).unwrap_err();
    assert!(err.contains("not a repo kete knows"), "{err}");
    let global = Target::Global {
        key: "kanban".into(),
    };
    assert_eq!(schema::resolve_target(&global, &[]).unwrap(), global);
}

/// The account rows are the window's too (its rail and quick prompt pick
/// profiles through the same spread): an Accounts group in Sessions, not
/// under "Terminal app", and the daemon's policy reads what they write.
#[test]
fn the_account_rows_are_shared_and_the_policy_reads_them() {
    let schema = export(&Config::default(), &DesktopPrefs::default(), &[]);
    let sessions = schema.tabs.iter().find(|t| t.title == "Sessions").unwrap();
    let group = sessions
        .groups
        .iter()
        .find(|g| g.title == "Accounts")
        .expect("an Accounts group");
    assert!(!group.terminal);
    let keys: Vec<String> = group
        .rows
        .iter()
        .map(|r| match &r.target {
            Target::Global { key } => key.clone(),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(
        keys,
        [
            "account_spread",
            "account_spread_threshold",
            "usage_statusline"
        ]
    );
    assert_eq!(group.rows[0].value, json!("manual"));
    assert_eq!(group.rows[1].value, json!("90"));
    assert_eq!(group.rows[2].value, json!(true));

    let (_dir, path, local) = layers();
    let global = |key: &str| Target::Global { key: key.into() };
    set_in(&path, &local, &global("account_spread"), &json!("failover")).unwrap();
    set_in(
        &path,
        &local,
        &global("account_spread_threshold"),
        &json!("80"),
    )
    .unwrap();
    let saved = read(&path);
    let policy = nebula_core::accounts::Policy::from_words(
        saved["account_spread"].as_str(),
        saved["account_spread_threshold"].as_str(),
    );
    assert_eq!(policy.spread, nebula_core::accounts::Spread::Failover);
    assert_eq!(policy.threshold, 80);
}

/// The critters are both apps': a Critters group on Appearance, not under
/// "Terminal app", off by default; the window's editor preferences sit in
/// an Editors group on its own tab.
#[test]
fn the_critters_rows_are_shared_and_the_editors_are_the_windows() {
    let schema = export(&Config::default(), &DesktopPrefs::default(), &[]);
    let look = schema
        .tabs
        .iter()
        .find(|t| t.title == "Appearance")
        .unwrap();
    let group = look
        .groups
        .iter()
        .find(|g| g.title == "Critters")
        .expect("a Critters group");
    assert!(!group.terminal);
    let rows: Vec<(String, Value)> = group
        .rows
        .iter()
        .map(|r| match &r.target {
            Target::Global { key } => (key.clone(), r.value.clone()),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(
        rows,
        [
            ("critters".to_string(), json!(false)),
            ("critters_size".to_string(), json!("medium")),
            ("critters_often".to_string(), json!("normal")),
        ]
    );
    let desktop = schema.tabs.iter().find(|t| t.id == "desktop").unwrap();
    let editors = desktop
        .groups
        .iter()
        .find(|g| g.title == "Editors")
        .expect("an Editors group");
    let keys: Vec<(&str, &Value)> = editors
        .rows
        .iter()
        .map(|r| match &r.target {
            Target::Desktop { key } => (key.as_str(), &r.value),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(
        keys,
        [
            ("editor_folder", &json!("auto")),
            ("editor_file", &json!("auto")),
            ("editor_reuse", &json!("off"))
        ]
    );

    let (_dir, path, local) = layers();
    let global = |key: &str| Target::Global { key: key.into() };
    set_in(&path, &local, &global("critters"), &json!(true)).unwrap();
    set_in(&path, &local, &global("critters_size"), &json!("large")).unwrap();
    assert!(set_in(&path, &local, &global("critters_often"), &json!("never")).is_err());
    set_in(
        &path,
        &local,
        &Target::Desktop {
            key: "editor_file".into(),
        },
        &json!("kete"),
    )
    .unwrap();
    let cfg = Config::load_at(&path, &local);
    assert!(cfg.critters);
    assert_eq!(cfg.critters_size_word(), "large");
    assert_eq!(cfg.critters_often_word(), "normal");
    assert_eq!(DesktopPrefs::load_at(&path, &local).editor_file, "kete");
}

/// WHAKAOHO's schedules live in `config.local.json` under `wake`, which
/// `Config` has no field for: a settings write from either app keeps them
/// there and never copies them into `config.json` (which `kete ssh`
/// carries to other machines), and the wake writer keeps every setting.
#[test]
fn the_wake_map_stays_local_through_settings_writes() {
    let (_dir, path, local) = layers();
    std::fs::write(&path, r#"{"editor": "hx"}"#).unwrap();
    let schedule = nebula_core::wake::WakeSchedule {
        enabled: true,
        ..Default::default()
    };
    nebula_core::wake::set_in(&path, &local, "work", Some(&schedule)).unwrap();
    let global = |key: &str| Target::Global { key: key.into() };
    set_in(&path, &local, &global("kanban"), &json!(true)).unwrap();
    Config::update_in(&path, &local, |c| c.editor = "nvim".into()).unwrap();
    assert!(read(&path).get("wake").is_none(), "{}", read(&path));
    assert_eq!(read(&local)["wake"]["work"]["enabled"], true);
    assert_eq!(nebula_core::wake::load_in(&local)["work"], schedule);
    nebula_core::wake::set_in(&path, &local, "work", None).unwrap();
    assert_eq!(read(&path)["editor"], "nvim");
    assert_eq!(read(&path)["kanban"], true);
}

/// 0.60's two rows: Idle note after in General's Studio group, and Usage
/// ledger in Sessions' Usage group, each with its key, choices and
/// default, written through the shared writer.
#[test]
fn the_studio_and_ledger_rows_sit_in_their_tabs_and_groups() {
    let place = |kind: SettingKind| {
        all_settings()
            .find(|(_, _, spec)| spec.kind == kind)
            .map(|(tab, _, spec)| (nebula_config::SETTINGS_TABS[tab].title, spec.group))
            .unwrap()
    };
    assert_eq!(place(SettingKind::StudioIdleAfter), ("General", "Studio"));
    assert_eq!(place(SettingKind::UsageLedger), ("Sessions", "Usage"));
    assert_eq!(SettingKind::StudioIdleAfter.key(), "studio_idle_after");
    assert_eq!(SettingKind::UsageLedger.key(), "usage_ledger");
    for kind in [SettingKind::StudioIdleAfter, SettingKind::UsageLedger] {
        assert_eq!(kind.added_on(), (2026, 10, 21));
    }
    let cfg = Config::default();
    assert_eq!(cfg.studio_idle_after(), std::time::Duration::from_secs(45));
    assert!(cfg.usage_ledger());
    assert_eq!(
        nebula_config::STUDIO_IDLE_AFTERS,
        ["30s", "45s", "60s", "120s"]
    );

    let (_dir, path, local) = layers();
    let global = |key: &str| Target::Global { key: key.into() };
    set_in(&path, &local, &global("studio_idle_after"), &json!("120s")).unwrap();
    set_in(&path, &local, &global("usage_ledger"), &json!(false)).unwrap();
    let cfg = Config::load_at(&path, &local);
    assert_eq!(cfg.studio_idle_after(), std::time::Duration::from_secs(120));
    assert!(!cfg.usage_ledger());
    let odd = Config {
        studio_idle_after: "7s".into(),
        ..Config::default()
    };
    assert_eq!(odd.studio_idle_after_word(), "45s", "a word off the list");
}

/// The studio map lives in `config.local.json` under `studio`, which
/// `Config` has no field for: a settings write from either app keeps it
/// there and never copies it into `config.json`, and the studio writer
/// keeps every setting.
#[test]
fn the_studio_map_stays_local_through_settings_writes() {
    use nebula_core::studio::{settings, Autonomy, StudioSettings};
    let (_dir, path, local) = layers();
    std::fs::write(&path, r#"{"editor": "hx"}"#).unwrap();
    let entry = StudioSettings {
        studio_inbox: "notes/inbox".into(),
        autonomy: Autonomy::A2,
        queue_exclude: Vec::new(),
    };
    let repo = Path::new("/code/my-game");
    settings::write_in(&path, &local, repo, Some(&entry)).unwrap();
    let global = |key: &str| Target::Global { key: key.into() };
    set_in(&path, &local, &global("kanban"), &json!(true)).unwrap();
    Config::update_in(&path, &local, |c| c.editor = "nvim".into()).unwrap();
    assert!(read(&path).get("studio").is_none(), "{}", read(&path));
    assert_eq!(settings::for_repo_in(&local, repo), Some(entry));
    settings::write_in(&path, &local, repo, None).unwrap();
    assert_eq!(read(&path)["editor"], "nvim");
    assert_eq!(read(&path)["kanban"], true);
    assert_eq!(settings::for_repo_in(&local, repo), None);
}
