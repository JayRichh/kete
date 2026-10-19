//! TUI user settings. The data model, its load and save, and the settings
//! overlay's tabs and rows live in `nebula_config`, shared with the desktop
//! window; this module re-exports all of it under its old path and adds
//! what needs the terminal UI's own types ([`ConfigExt`]).

pub use nebula_config::*;

/// The settings read as the TUI's own types: a palette, a pane side, a
/// graphics renderer, a dispatch keymap, a preset's side. Methods on
/// [`Config`] once; a trait now `Config` lives in another crate.
pub trait ConfigExt {
    /// `theme` resolved to a palette without asking the machine: a preset
    /// as it is, `auto` and `vscode` as One Dark. The running UI resolves
    /// through its LIVE THEME instead (`crate::appearance::LiveTheme`),
    /// which does ask.
    fn theme(&self) -> crate::theme::Theme;
    /// `session_pane` resolved to the side the LAUNCHER VIEW lays its pane
    /// out on.
    fn pane_side(&self) -> crate::launcher::PaneSide;
    fn graphics_mode(&self) -> crate::graphics::GraphicsMode;
    /// Hotkeys as the event loop dispatches them: defaults with this
    /// config's overrides applied.
    fn keymap(&self) -> crate::keymap::Keymap;
    /// Whether a preset may launch: its harness's Agents tab switch,
    /// enabled and valid.
    fn preset_harness_usable(&self, preset: &crate::agent_presets::AgentPreset) -> bool;
}

impl ConfigExt for Config {
    fn theme(&self) -> crate::theme::Theme {
        crate::theme::resolve(&self.theme, &crate::theme::Machine::default())
    }

    fn pane_side(&self) -> crate::launcher::PaneSide {
        crate::launcher::PaneSide::parse(&self.session_pane)
    }

    fn graphics_mode(&self) -> crate::graphics::GraphicsMode {
        crate::graphics::graphics_mode(self.inline_graphics_label())
    }

    fn keymap(&self) -> crate::keymap::Keymap {
        crate::keymap::Keymap::from_overrides(&self.keybindings)
    }

    fn preset_harness_usable(&self, preset: &crate::agent_presets::AgentPreset) -> bool {
        self.harness_usable(preset.kind, preset.custom_harness.as_deref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_presets::PresetText;
    use nebula_core::AgentKind;
    use std::collections::BTreeSet;
    use std::path::Path;

    #[test]
    fn days_from_civil_counts_from_the_epoch() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
        assert_eq!(
            days_from_civil(2026, 9, 24) - days_from_civil(2026, 8, 22),
            33
        );
    }

    #[test]
    fn a_setting_is_new_for_a_week_from_its_release() {
        let shipped = days_from_civil(2026, 9, 22);
        let kind = SettingKind::BlackBackground;
        assert!(!kind.is_new(shipped - 1), "not new before it shipped");
        assert!(kind.is_new(shipped));
        assert!(kind.is_new(shipped + NEW_SETTING_DAYS - 1));
        assert!(!kind.is_new(shipped + NEW_SETTING_DAYS));
    }

    #[test]
    fn no_setting_ships_in_the_future_of_its_own_code() {
        // A date past today is a typo: the row would miss its `(new)` week.
        for tab in 0..tab_count() {
            for spec in tab_settings(tab) {
                let (y, m, d) = spec.kind.added_on();
                assert!(days_from_civil(y, m, d) <= today_days(), "{}", spec.label);
            }
        }
    }
    use crate::keymap::Keymap;

    /// Config files earlier releases wrote, every value off its default.
    /// Add one per release; see [`config_files_from_earlier_releases_still_load_every_key`].
    const CONFIG_FIXTURES: &[(&str, &str)] = &[
        (
            "0.26.0",
            include_str!("../../nebula-core/fixtures/config-0.26.0.json"),
        ),
        (
            "0.27.0",
            include_str!("../../nebula-core/fixtures/config-0.27.0.json"),
        ),
        (
            "0.28.0",
            include_str!("../../nebula-core/fixtures/config-0.28.0.json"),
        ),
        (
            "0.29.0",
            include_str!("../../nebula-core/fixtures/config-0.29.0.json"),
        ),
        (
            "0.30.0",
            include_str!("../../nebula-core/fixtures/config-0.30.0.json"),
        ),
        (
            "0.31.0",
            include_str!("../../nebula-core/fixtures/config-0.31.0.json"),
        ),
        (
            "0.32.0",
            include_str!("../../nebula-core/fixtures/config-0.32.0.json"),
        ),
        (
            "0.33.0",
            include_str!("../../nebula-core/fixtures/config-0.33.0.json"),
        ),
        (
            "0.34.0",
            include_str!("../../nebula-core/fixtures/config-0.34.0.json"),
        ),
        (
            "0.35.0",
            include_str!("../../nebula-core/fixtures/config-0.35.0.json"),
        ),
        (
            "0.36.0",
            include_str!("../../nebula-core/fixtures/config-0.36.0.json"),
        ),
        (
            "0.37.0",
            include_str!("../../nebula-core/fixtures/config-0.37.0.json"),
        ),
        (
            "0.38.0",
            include_str!("../../nebula-core/fixtures/config-0.38.0.json"),
        ),
        (
            "0.39.0",
            include_str!("../../nebula-core/fixtures/config-0.39.0.json"),
        ),
        (
            "0.40.0",
            include_str!("../../nebula-core/fixtures/config-0.40.0.json"),
        ),
        (
            "0.40.1",
            include_str!("../../nebula-core/fixtures/config-0.40.1.json"),
        ),
        (
            "0.40.2",
            include_str!("../../nebula-core/fixtures/config-0.40.2.json"),
        ),
        (
            "0.41.0",
            include_str!("../../nebula-core/fixtures/config-0.41.0.json"),
        ),
        (
            "0.42.0",
            include_str!("../../nebula-core/fixtures/config-0.42.0.json"),
        ),
        (
            "0.43.0",
            include_str!("../../nebula-core/fixtures/config-0.43.0.json"),
        ),
        (
            "0.44.0",
            include_str!("../../nebula-core/fixtures/config-0.44.0.json"),
        ),
        (
            "0.46.0",
            include_str!("../../nebula-core/fixtures/config-0.46.0.json"),
        ),
        (
            "0.55.0",
            include_str!("../../nebula-core/fixtures/config-0.55.0.json"),
        ),
    ];

    fn read_json_file(path: &Path) -> serde_json::Value {
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    /// Run `f` with the config pinned to an empty temp file, so every
    /// registry read (Agents rows, choice lists, tab lengths) sees a
    /// fresh install — never the dev's own file.
    fn with_empty_config<T>(f: impl FnOnce() -> T) -> T {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(&path, "{}").unwrap();
        with_config_path(path, f)
    }

    /// Where an Agents harness row lives in `cfg`'s own rows, without
    /// loading anything: [`locate_agent`] reads the live file, which a
    /// test's in-memory config may have left behind.
    fn locate_in(cfg: &Config, id: &str, field: HarnessField) -> Option<(usize, usize)> {
        let tab = agents_tab();
        cfg.agent_rows()
            .iter()
            .position(|(row_id, row_field)| row_id == id && *row_field == field)
            .map(|i| (tab, AGENTS_HEAD.len() + i))
    }

    /// The first compatibility rule in docs/configuration.md: a key, once
    /// shipped, keeps its name, its type and its meaning. Every key a
    /// release wrote must still load to exactly the value it wrote — a
    /// rename off the `RENAMED_KEYS` table leaves the key unknown, a type
    /// change leaves it unreadable, and either reads back as something
    /// else here. A key on that table reads through its alias into the
    /// field's new name, and a save writes it back under that name alone.
    #[test]
    fn config_files_from_earlier_releases_still_load_every_key() {
        let current = |key: &str| {
            RENAMED_KEYS
                .iter()
                .find(|(old, _)| *old == key)
                .map_or(key, |(_, new)| *new)
                .to_string()
        };
        for (release, raw) in CONFIG_FIXTURES {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("config.json");
            std::fs::write(&path, raw).unwrap();
            let loaded = load_from(&path);
            assert!(
                loaded.skipped.is_empty(),
                "{release}: keys this build can't read: {:?}",
                loaded.skipped
            );
            let known = serde_json::to_value(&loaded).unwrap();
            let fixture: serde_json::Value = serde_json::from_str(raw).unwrap();
            for (key, value) in fixture.as_object().unwrap() {
                assert_eq!(
                    known.get(current(key)),
                    Some(value),
                    "{release}: `{key}` no longer loads as that release wrote it"
                );
            }
            // A save writes every one of them back unchanged — a renamed
            // key under its new name, the old one gone.
            loaded.save_to(&path).unwrap();
            let saved = read_json_file(&path);
            for (key, value) in fixture.as_object().unwrap() {
                let now = current(key);
                assert_eq!(
                    saved.get(&now),
                    Some(value),
                    "{release}: `{key}` after a save"
                );
                if now != *key {
                    assert_eq!(saved.get(key), None, "{release}: `{key}` after a save");
                }
            }
        }
    }

    /// `n` always launches straight from the picker now, so **Skip
    /// starting prompt** has no row to be edited on — but the key an
    /// earlier release wrote still loads, and is written back unchanged
    /// for the older builds that read it.
    #[test]
    fn skip_session_naming_has_no_row_and_is_written_back_for_older_builds() {
        assert!(SETTINGS_TABS.iter().all(|tab| match &tab.body {
            TabBody::Values(rows) | TabBody::Project(rows) => {
                rows.iter().all(|row| row.label != "Skip starting prompt")
            }
            TabBody::Hotkeys | TabBody::Agents => true,
        }));

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(&path, r#"{"skip_session_naming": true}"#).unwrap();
        let mut cfg = load_from(&path);
        assert!(cfg.skipped.is_empty(), "{:?}", cfg.skipped);
        assert!(cfg.skip_session_naming);

        cfg.black_background = false;
        cfg.save_to(&path).unwrap();
        assert_eq!(read_json_file(&path)["skip_session_naming"], true);
    }

    /// The retired key drives nothing — the **Confirm on archive** row is
    /// `ask_before_archive`'s — and a `true` an earlier release wrote
    /// still loads and is written back unchanged for the older builds
    /// that read it.
    #[test]
    fn confirm_on_archive_is_written_back_for_older_builds() {
        assert!(!Config::default().confirm_on_archive);
        let cfg: Config = serde_json::from_str("{}").unwrap();
        assert!(!cfg.confirm_on_archive);

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(&path, r#"{"confirm_on_archive": true}"#).unwrap();
        let mut cfg = load_from(&path);
        assert!(cfg.skipped.is_empty(), "{:?}", cfg.skipped);
        assert!(cfg.confirm_on_archive);

        let (tab, row) = locate(SettingKind::AskBeforeArchive).unwrap();
        cfg.cycle(tab, row, 0);
        assert!(!cfg.ask_before_archive);
        assert!(cfg.confirm_on_archive, "the row leaves the retired key be");
        cfg.save_to(&path).unwrap();
        assert_eq!(read_json_file(&path)["confirm_on_archive"], true);
    }

    /// CONFIRM ON ARCHIVE starts on — `a` asks before a session is
    /// archived — sits on the Sessions tab, toggles like any bool, and a
    /// config predating the key reads as on: so does one holding the
    /// retired `confirm_on_archive`'s old default, which every file an
    /// earlier build saved carries.
    #[test]
    fn ask_before_archive_is_on_by_default_and_toggles() {
        let mut cfg = Config::default();
        assert!(cfg.ask_before_archive);
        let (tab, row) = locate(SettingKind::AskBeforeArchive).unwrap();
        assert_eq!(SETTINGS_TABS[tab].title, "Sessions");
        assert_eq!(cfg.value_label(SettingKind::AskBeforeArchive), "on");
        cfg.cycle(tab, row, 0);
        assert!(!cfg.ask_before_archive);
        assert_eq!(cfg.value_label(SettingKind::AskBeforeArchive), "off");
        cfg.cycle(tab, row, 1);
        assert!(cfg.ask_before_archive, "←/→ toggle it like Enter does");

        let cfg: Config = serde_json::from_str("{}").unwrap();
        assert!(cfg.ask_before_archive, "a missing key reads as on");
        let cfg: Config = serde_json::from_str(r#"{"confirm_on_archive": false}"#).unwrap();
        assert!(
            cfg.ask_before_archive,
            "the retired key's false is not read"
        );

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let cfg = Config {
            ask_before_archive: false,
            ..Config::default()
        };
        cfg.save_to(&path).unwrap();
        assert_eq!(read_json_file(&path)["ask_before_archive"], false);
        assert!(!load_from(&path).ask_before_archive, "off sticks");
    }

    /// Workspaces are gone, so **Workspaces bar** has no row to be edited
    /// on — but the key an earlier release wrote still loads, and is
    /// written back unchanged for the older builds that read it.
    #[test]
    fn show_workspaces_has_no_row_and_is_written_back_for_older_builds() {
        assert!(SETTINGS_TABS.iter().all(|tab| match &tab.body {
            TabBody::Values(rows) | TabBody::Project(rows) => {
                rows.iter().all(|row| row.label != "Workspaces bar")
            }
            TabBody::Hotkeys | TabBody::Agents => true,
        }));

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(&path, r#"{"show_workspaces": false}"#).unwrap();
        let mut cfg = load_from(&path);
        assert!(cfg.skipped.is_empty(), "{:?}", cfg.skipped);
        assert!(!cfg.show_workspaces);

        cfg.black_background = false;
        cfg.save_to(&path).unwrap();
        assert_eq!(read_json_file(&path)["show_workspaces"], false);
    }

    /// The tint is always on now, so **Focused panel tint** has no row to
    /// be edited on — but the key an earlier release wrote still loads,
    /// and is written back unchanged for the older builds that read it.
    #[test]
    fn focus_tint_has_no_row_and_is_written_back_for_older_builds() {
        assert!(SETTINGS_TABS.iter().all(|tab| match &tab.body {
            TabBody::Values(rows) | TabBody::Project(rows) => {
                rows.iter().all(|row| row.label != "Focused panel tint")
            }
            TabBody::Hotkeys | TabBody::Agents => true,
        }));

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(&path, r#"{"focus_tint": false}"#).unwrap();
        let mut cfg = load_from(&path);
        assert!(cfg.skipped.is_empty(), "{:?}", cfg.skipped);
        assert!(!cfg.focus_tint);

        cfg.black_background = false;
        cfg.save_to(&path).unwrap();
        assert_eq!(read_json_file(&path)["focus_tint"], false);
    }

    /// The root is always listed now, so **Hide root worktree** has no row
    /// to be edited on — but the keys earlier releases wrote still load
    /// and are written back unchanged for the older builds that read
    /// them: the top-level `hide_root_worktree` (one switch for every
    /// project, through 0.27) as a retired field of its own, and the one
    /// in a project's entry (the Project tab's row, through 0.33) as a key
    /// this build doesn't know — which also keeps that entry from being
    /// dropped as all-default. Neither hides anything.
    #[test]
    fn hide_root_worktree_has_no_row_and_is_written_back_for_older_builds() {
        assert!(SETTINGS_TABS.iter().all(|tab| match &tab.body {
            TabBody::Values(rows) | TabBody::Project(rows) => {
                rows.iter().all(|row| row.label != "Hide root worktree")
            }
            TabBody::Hotkeys | TabBody::Agents => true,
        }));

        let demo = Path::new("/tmp/demo");
        let other = Path::new("/tmp/other");
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(
            &path,
            r#"{
              "hide_root_worktree": true,
              "projects": {
                "/tmp/other": { "hide_root_worktree": true, "future_row": "x" }
              }
            }"#,
        )
        .unwrap();
        let mut cfg = load_from(&path);
        assert!(cfg.skipped.is_empty(), "{:?}", cfg.skipped);
        assert!(cfg.hide_root_worktree);
        assert_eq!(
            cfg.project(demo),
            ProjectSettings::default(),
            "no entry: the defaults, whatever the old global key says"
        );
        assert_eq!(
            cfg.project(other).other.get("hide_root_worktree"),
            Some(&serde_json::json!(true)),
            "an entry's key rides along unread"
        );

        // A save writes both back; the entry that only carries the old
        // key is kept for it, not dropped as all-default.
        let kept = cfg.project(other);
        cfg.set_project(other, kept);
        cfg.save_to(&path).unwrap();
        let saved = read_json_file(&path);
        assert_eq!(
            saved["hide_root_worktree"], true,
            "written back for older builds"
        );
        assert_eq!(
            saved["projects"],
            serde_json::json!({
                "/tmp/other": { "hide_root_worktree": true, "future_row": "x" }
            })
        );
    }

    /// A value this build can't read — a newer nebula's, most likely — costs
    /// only its own key, and a save leaves it as stored until the setting is
    /// changed here.
    #[test]
    fn an_unreadable_key_keeps_the_rest_and_outlives_a_save() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(
            &path,
            r#"{"theme": "ocean", "hide_draft_prs": "auto", "animations": false}"#,
        )
        .unwrap();
        let mut cfg = load_from(&path);
        assert_eq!(cfg.theme, "ocean");
        assert!(!cfg.animations);
        assert!(!cfg.hide_draft_prs);
        assert_eq!(cfg.skipped, BTreeSet::from(["hide_draft_prs".to_string()]));

        cfg.black_background = false;
        cfg.save_to(&path).unwrap();
        let saved = read_json_file(&path);
        assert_eq!(saved["hide_draft_prs"], "auto", "left as stored");
        assert_eq!(saved["black_background"], false);
        assert_eq!(saved["theme"], "ocean");

        let (t, r) = locate(SettingKind::HideDraftPrs).unwrap();
        cfg.cycle(t, r, 1);
        cfg.save_to(&path).unwrap();
        assert_eq!(
            read_json_file(&path)["hide_draft_prs"],
            true,
            "changing it here is a real edit"
        );
    }

    /// `config.local.json` wins key by key, and a key it holds is saved back
    /// into it — never copied into the portable `config.json`.
    #[test]
    fn the_local_layer_overrides_and_keeps_its_own_keys() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let local = dir.path().join("config.local.json");
        std::fs::write(&path, r#"{"editor": "nvim", "theme": "ocean"}"#).unwrap();
        std::fs::write(&local, r#"{"editor": "nano"}"#).unwrap();
        let mut cfg = load_from(&path);
        assert_eq!(cfg.editor, "nano");
        assert_eq!(cfg.theme, "ocean");

        cfg.theme = "forest".into();
        cfg.save_to(&path).unwrap();
        assert_eq!(read_json_file(&path)["theme"], "forest");
        assert_eq!(
            read_json_file(&path)["editor"],
            "nvim",
            "the local value stays local"
        );
        assert_eq!(
            read_json_file(&local),
            serde_json::json!({"editor": "nano"})
        );

        cfg.editor = "hx".into();
        cfg.save_to(&path).unwrap();
        assert_eq!(read_json_file(&local)["editor"], "hx");
        assert_eq!(read_json_file(&path)["editor"], "nvim");
    }

    #[test]
    fn reset_removes_the_local_layer_too() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let local = dir.path().join("config.local.json");
        with_config_path(path.clone(), || {
            std::fs::write(&local, r#"{"theme": "rose"}"#).unwrap();
            assert_eq!(Config::load().theme, "rose");
            Config::reset_to_defaults().unwrap();
            assert!(!local.exists());
            assert_eq!(Config::load().theme, Config::default().theme);
        });
    }

    #[test]
    fn ssh_sync_defaults_on_and_toggles_from_the_general_tab() {
        assert!(Config::default().ssh_sync_config);
        let cfg: Config = serde_json::from_str("{}").unwrap();
        assert!(cfg.ssh_sync_config);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let mut cfg = Config::default();
        let (t, r) = locate(SettingKind::SshSyncConfig).unwrap();
        assert_eq!(SETTINGS_TABS[t].title, "General");
        cfg.cycle(t, r, 0);
        assert_eq!(cfg.value_label(SettingKind::SshSyncConfig), "off");
        cfg.save_to(&path).unwrap();
        assert_eq!(read_json_file(&path)["ssh_sync_config"], false);
        assert!(!load_from(&path).ssh_sync_config);
    }

    #[test]
    fn defaults_close_the_finder_on_open() {
        assert!(Config::default().close_finder_on_open);
        let cfg: Config = serde_json::from_str("{}").unwrap();
        assert!(cfg.close_finder_on_open);
        let cfg: Config = serde_json::from_str(r#"{"close_finder_on_open": false}"#).unwrap();
        assert!(!cfg.close_finder_on_open);
        // The overlay toggle round-trips through the saved file.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let mut cfg = Config::default();
        let (t, r) = locate(SettingKind::CloseFinderOnOpen).unwrap();
        cfg.cycle(t, r, 1);
        cfg.save_to(&path).unwrap();
        assert!(!load_from(&path).close_finder_on_open);
    }

    /// The two prewarm keys are daemon-owned but overlay-toggled: on by
    /// default, a missing key reads as on, and
    /// the Sessions-tab rows round-trip through the saved file.
    #[test]
    fn prewarm_toggles_default_on_and_round_trip() {
        let cfg = Config::default();
        assert!(cfg.prewarm_agents && cfg.prewarm_sessions);
        let cfg: Config = serde_json::from_str("{}").unwrap();
        assert!(cfg.prewarm_agents && cfg.prewarm_sessions);
        let cfg: Config =
            serde_json::from_str(r#"{"prewarm_agents": false, "prewarm_sessions": false}"#)
                .unwrap();
        assert!(!cfg.prewarm_agents && !cfg.prewarm_sessions);

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let mut cfg = Config::default();
        let (t, r) = locate(SettingKind::PrewarmAgents).unwrap();
        assert_eq!(SETTINGS_TABS[t].title, "Sessions");
        cfg.cycle(t, r, 0);
        let (t, r) = locate(SettingKind::PrewarmSessions).unwrap();
        assert_eq!(SETTINGS_TABS[t].title, "Sessions");
        cfg.cycle(t, r, 0);
        assert_eq!(cfg.value_label(SettingKind::PrewarmAgents), "off");
        assert_eq!(cfg.value_label(SettingKind::PrewarmSessions), "off");
        cfg.save_to(&path).unwrap();
        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        // Written under the daemon's own key names, since it is the reader.
        assert_eq!(saved["prewarm_agents"], false);
        assert_eq!(saved["prewarm_sessions"], false);
        let loaded = load_from(&path);
        assert!(!loaded.prewarm_agents && !loaded.prewarm_sessions);
    }

    #[test]
    fn defaults_enter_attaches() {
        assert!(Config::default().palette_enter_attaches);
        let cfg: Config = serde_json::from_str("{}").unwrap();
        assert!(cfg.palette_enter_attaches);
        let cfg: Config = serde_json::from_str(r#"{"palette_enter_attaches": false}"#).unwrap();
        assert!(!cfg.palette_enter_attaches);
    }

    #[test]
    fn reset_rewrites_the_file_from_scratch() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        with_config_path(path.clone(), || {
            let mut cfg = Config {
                theme: "midnight".into(),
                animations: false,
                ..Config::default()
            };
            cfg.keybindings.insert("git_diff".into(), "f9".into());
            cfg.save().unwrap();
            // A key the overlay doesn't own survives an ordinary save…
            let mut root: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            root["hand_added_key"] = serde_json::json!(false);
            std::fs::write(&path, serde_json::to_vec_pretty(&root).unwrap()).unwrap();
            Config::load().save().unwrap();
            let raw = std::fs::read_to_string(&path).unwrap();
            assert!(
                raw.contains("hand_added_key"),
                "save() patches, keeping foreign keys:\n{raw}"
            );

            // …but not a reset: the file starts over from an empty object.
            let reset = Config::reset_to_defaults().unwrap();
            assert!(reset.animations);
            assert!(reset.keybindings.is_empty());
            let raw = std::fs::read_to_string(&path).unwrap();
            assert!(
                !raw.contains("hand_added_key"),
                "foreign key survived:\n{raw}"
            );
            let loaded = Config::load();
            assert_eq!(loaded.theme, Config::default().theme);
            assert!(loaded.animations);
            assert!(loaded.keybindings.is_empty());
        });
    }

    /// GIT INIT NEW PROJECTS: retired with every project a repository.
    /// No tab shows the row; the key still loads and is written back as
    /// stored for an older daemon sharing the file.
    #[test]
    fn git_init_on_create_is_retired_but_still_round_trips() {
        assert!(SETTINGS_TABS.iter().all(|tab| match tab.body {
            TabBody::Values(rows) | TabBody::Project(rows) => {
                rows.iter().all(|r| r.label != "git init new projects")
            }
            _ => true,
        }));
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(&path, r#"{"git_init_on_create": false}"#).unwrap();
        let cfg = load_from(&path);
        assert!(cfg.palette_enter_attaches);
        assert!(!cfg.git_init_on_create);
        cfg.save_to(&path).unwrap();
        assert_eq!(read_json_file(&path)["git_init_on_create"], false);
    }

    #[test]
    fn worktree_base_branch_is_a_typed_row_that_defaults_to_auto() {
        assert_eq!(Config::default().worktree_base_branch, "");
        let cfg: Config = serde_json::from_str("{}").unwrap();
        assert_eq!(cfg.worktree_base_branch, "");
        assert_eq!(
            cfg.value_label(SettingKind::WorktreeBaseBranch),
            AUTO_CHOICE
        );
        assert!(SettingKind::WorktreeBaseBranch.is_text());

        let (tab, row) = locate(SettingKind::WorktreeBaseBranch).unwrap();
        assert_eq!(
            SETTINGS_TABS[tab].title, "General",
            "sits on the General tab"
        );
        // Enter / ←/→ on a typed row change nothing; the prompt does.
        let mut cfg = Config::default();
        for delta in [0, 1, -1] {
            cfg.cycle(tab, row, delta);
            assert_eq!(cfg.worktree_base_branch, "");
        }

        assert_eq!(cfg.text_value(SettingKind::WorktreeBaseBranch), "");
        assert!(cfg.set_text(SettingKind::WorktreeBaseBranch, "  master "));
        assert_eq!(cfg.worktree_base_branch, "master", "trimmed");
        assert_eq!(cfg.value_label(SettingKind::WorktreeBaseBranch), "master");
        assert_eq!(cfg.text_value(SettingKind::WorktreeBaseBranch), "master");
        assert_eq!(
            spec_for(SettingKind::WorktreeBaseBranch).map(|s| s.label),
            Some("Worktree base branch")
        );
        assert!(
            !cfg.set_text(SettingKind::Editor, "nvim"),
            "a cycled row is not a typed one"
        );
        assert_eq!(cfg.editor, "vim");

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        cfg.save_to(&path).unwrap();
        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(saved["worktree_base_branch"], "master");
        assert_eq!(load_from(&path).worktree_base_branch, "master");

        // Empty is the way back to auto, and is stored as "", not "auto".
        assert!(cfg.set_text(SettingKind::WorktreeBaseBranch, "   "));
        cfg.save_to(&path).unwrap();
        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(saved["worktree_base_branch"], "");
        assert_eq!(
            load_from(&path).value_label(SettingKind::WorktreeBaseBranch),
            AUTO_CHOICE
        );
    }

    #[test]
    fn done_sound_defaults_to_bell_cycles_persists_and_resolves() {
        let mut cfg = Config::default();
        assert_eq!(cfg.done_sound, "Glass");
        // A config predating the key dings too.
        let old: Config = serde_json::from_str("{}").unwrap();
        assert_eq!(old.done_sound, "Glass");

        let (tab, row) = locate(SettingKind::DoneSound).unwrap();
        cfg.cycle(tab, row, -1);
        assert_eq!(cfg.done_sound, "bell");
        cfg.cycle(tab, row, -1);
        assert_eq!(cfg.done_sound, "off");
        cfg.cycle(tab, row, -1);
        assert_eq!(cfg.done_sound, "Basso", "the list wraps");
        cfg.cycle(tab, row, 0);
        assert_eq!(cfg.done_sound, "off");
        cfg.cycle(tab, row, 1);
        cfg.cycle(tab, row, 1);
        assert_eq!(cfg.done_sound, "Glass");
        assert_eq!(cfg.value_label(SettingKind::DoneSound), "Glass");

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        cfg.save_to(&path).unwrap();
        assert_eq!(load_from(&path).done_sound, "Glass");

        // Silence, the bell, and every reason a name falls back to it.
        assert_eq!(resolve_sound("off", false, true), None);
        assert_eq!(resolve_sound("OFF", false, true), None);
        assert_eq!(resolve_sound("", false, true), None);
        assert_eq!(resolve_sound("bell", false, true), Some(Sound::Bell));
        assert_eq!(
            resolve_sound("Glass", true, true),
            Some(Sound::Bell),
            "over ssh afplay would ring the remote box"
        );
        assert_eq!(
            resolve_sound("Glass", false, false),
            Some(Sound::Bell),
            "no system sounds off macOS"
        );
        assert_eq!(resolve_sound("NoSuchSound", false, true), Some(Sound::Bell));
        assert_eq!(
            resolve_sound("../etc/passwd", false, true),
            Some(Sound::Bell)
        );
        #[cfg(target_os = "macos")]
        assert_eq!(
            resolve_sound("Glass", false, true),
            Some(Sound::File(
                Path::new(nebula_config::MACOS_SOUNDS_DIR).join("Glass.aiff")
            ))
        );
    }

    #[test]
    fn feedback_sound_defaults_to_sosumi_cycles_persists_and_resolves() {
        let mut cfg = Config::default();
        assert_eq!(cfg.feedback_sound, "Sosumi");
        assert_ne!(
            cfg.feedback_sound, cfg.done_sound,
            "red and green must sound different"
        );
        // A config predating the key rings too.
        let old: Config = serde_json::from_str("{}").unwrap();
        assert_eq!(old.feedback_sound, "Sosumi");
        // …and one that only ever set the done sound keeps it.
        let old: Config = serde_json::from_str(r#"{"done_sound": "Ping"}"#).unwrap();
        assert_eq!(old.done_sound, "Ping");
        assert_eq!(old.feedback_sound, "Sosumi");

        // Its row sits right after the done sound on the Sessions tab.
        let (tab, row) = locate(SettingKind::FeedbackSound).unwrap();
        assert_eq!(locate(SettingKind::DoneSound).unwrap(), (tab, row - 1));
        assert_eq!(SETTINGS_TABS[tab].title, "Sessions");
        cfg.cycle(tab, row, 1);
        assert_eq!(cfg.feedback_sound, "Basso");
        cfg.cycle(tab, row, 1);
        assert_eq!(cfg.feedback_sound, "off", "the list wraps");
        cfg.cycle(tab, row, -1);
        cfg.cycle(tab, row, -1);
        assert_eq!(cfg.feedback_sound, "Sosumi");
        assert_eq!(cfg.value_label(SettingKind::FeedbackSound), "Sosumi");
        assert_eq!(cfg.done_sound, "Glass", "the done sound is its own row");

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        cfg.feedback_sound = "off".into();
        cfg.save_to(&path).unwrap();
        let loaded = load_from(&path);
        assert_eq!(loaded.feedback_sound, "off");
        assert_eq!(loaded.feedback_sound(), None, "off is silence for both");
        assert_eq!(loaded.done_sound, "Glass");

        // The same resolution as the done sound: the bell over ssh and off
        // macOS, silence for off.
        assert_eq!(resolve_sound("Sosumi", true, true), Some(Sound::Bell));
        assert_eq!(resolve_sound("Sosumi", false, false), Some(Sound::Bell));
        #[cfg(target_os = "macos")]
        assert_eq!(
            resolve_sound("Sosumi", false, true),
            Some(Sound::File(
                Path::new(nebula_config::MACOS_SOUNDS_DIR).join("Sosumi.aiff")
            ))
        );
    }

    #[test]
    fn cycle_toggles_bools_and_walks_session_idle_timeout() {
        let mut cfg = Config::default();
        let (t, r) = locate(SettingKind::PaletteEnterAttaches).unwrap();
        assert!(cfg.palette_enter_attaches);
        cfg.cycle(t, r, 0);
        assert!(!cfg.palette_enter_attaches);
        cfg.cycle(t, r, 1);
        assert!(cfg.palette_enter_attaches);

        assert_eq!(cfg.session_idle_timeout, "5m");
        let (t, r) = locate(SettingKind::SessionIdleTimeout).unwrap();
        cfg.cycle(t, r, 0);
        assert_eq!(cfg.session_idle_timeout, "15m");
        cfg.cycle(t, r, -1);
        assert_eq!(cfg.session_idle_timeout, "5m");
        cfg.cycle(t, r, -1);
        assert_eq!(cfg.session_idle_timeout, "1m");
    }

    /// DELETE EMPTIED WORKTREE starts off — the last card's delete asks
    /// before the checkout goes — sits on the Sessions tab, toggles like
    /// any bool, and a config predating the key reads as off.
    #[test]
    fn delete_empty_worktree_is_off_by_default_and_toggles() {
        let mut cfg = Config::default();
        assert!(!cfg.delete_empty_worktree);
        let (tab, row) = locate(SettingKind::DeleteEmptyWorktree).unwrap();
        assert_eq!(SETTINGS_TABS[tab].title, "Sessions");
        assert_eq!(cfg.value_label(SettingKind::DeleteEmptyWorktree), "off");
        cfg.cycle(tab, row, 0);
        assert!(cfg.delete_empty_worktree);
        assert_eq!(cfg.value_label(SettingKind::DeleteEmptyWorktree), "on");
        cfg.cycle(tab, row, -1);
        assert!(!cfg.delete_empty_worktree, "←/→ toggle it like Enter does");

        let cfg: Config = serde_json::from_str("{}").unwrap();
        assert!(!cfg.delete_empty_worktree, "a missing key reads as off");
        let cfg: Config = serde_json::from_str(r#"{"delete_empty_worktree": true}"#).unwrap();
        assert!(cfg.delete_empty_worktree);
    }

    /// AUTO-CLEAN MERGED WORKTREES is opt-in, sits on the Sessions tab, and
    /// persists under its own key.
    #[test]
    fn auto_cleanup_merged_is_off_by_default_and_toggles() {
        let mut cfg = Config::default();
        assert!(!cfg.auto_cleanup_merged);
        let (tab, row) = locate(SettingKind::AutoCleanupMerged).unwrap();
        assert_eq!(SETTINGS_TABS[tab].title, "Sessions");
        assert_eq!(cfg.value_label(SettingKind::AutoCleanupMerged), "off");
        cfg.cycle(tab, row, 0);
        assert!(cfg.auto_cleanup_merged);
        assert_eq!(cfg.value_label(SettingKind::AutoCleanupMerged), "on");
        cfg.cycle(tab, row, -1);
        assert!(!cfg.auto_cleanup_merged, "←/→ toggle it like Enter does");

        let cfg: Config = serde_json::from_str("{}").unwrap();
        assert!(!cfg.auto_cleanup_merged, "a missing key reads as off");
        let cfg: Config = serde_json::from_str(r#"{"auto_cleanup_merged": true}"#).unwrap();
        assert!(cfg.auto_cleanup_merged);
    }

    /// SHOW ALL WORKTREES starts on — every checkout gets a band — sits
    /// on the Sessions tab beside the delete it changes, toggles like any
    /// bool, and a config predating the key reads as on.
    #[test]
    fn show_all_worktrees_is_on_by_default_and_toggles() {
        let mut cfg = Config::default();
        assert!(cfg.show_all_worktrees);
        let (tab, row) = locate(SettingKind::ShowAllWorktrees).unwrap();
        assert_eq!(SETTINGS_TABS[tab].title, "Sessions");
        assert_eq!(cfg.value_label(SettingKind::ShowAllWorktrees), "on");
        cfg.cycle(tab, row, 0);
        assert!(!cfg.show_all_worktrees);
        assert_eq!(cfg.value_label(SettingKind::ShowAllWorktrees), "off");
        cfg.cycle(tab, row, 1);
        assert!(cfg.show_all_worktrees, "←/→ toggle it like Enter does");

        let cfg: Config = serde_json::from_str("{}").unwrap();
        assert!(cfg.show_all_worktrees, "a missing key reads as on");
        let cfg: Config = serde_json::from_str(r#"{"show_all_worktrees": false}"#).unwrap();
        assert!(!cfg.show_all_worktrees);
    }

    #[test]
    fn editor_defaults_cycles_and_persists() {
        let mut cfg = Config::default();
        assert_eq!(cfg.editor, "vim");
        let (tab, row) = locate(SettingKind::Editor).unwrap();
        cfg.cycle(tab, row, 1);
        assert_eq!(cfg.editor, "nvim");
        cfg.cycle(tab, row, -1);
        assert_eq!(cfg.editor, "vim");
        // Hand-edited commands the picker doesn't list cycle from the start.
        cfg.editor = "kak".into();
        cfg.cycle(tab, row, 1);
        assert_eq!(cfg.editor, "nvim");

        cfg.editor = "nvim".into();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        cfg.save_to(&path).unwrap();
        assert_eq!(load_from(&path).editor, "nvim");
        // A config predating the key keeps vim.
        let cfg: Config = serde_json::from_str("{}").unwrap();
        assert_eq!(cfg.editor, "vim");
    }

    #[test]
    fn editor_resolution_prefers_env_then_setting_then_vim() {
        assert_eq!(resolve_editor(Some("hx"), "nvim"), "hx");
        assert_eq!(resolve_editor(Some("  "), "nvim"), "nvim");
        assert_eq!(resolve_editor(None, " nvim "), "nvim");
        assert_eq!(resolve_editor(None, ""), "vim");
    }

    #[test]
    fn session_idle_timeout_cycles_and_persists() {
        let mut cfg = Config::default();
        assert_eq!(cfg.session_idle_timeout, "5m");
        let (tab, row) = locate(SettingKind::SessionIdleTimeout).unwrap();
        cfg.cycle(tab, row, 1);
        assert_eq!(cfg.session_idle_timeout, "15m");
        cfg.cycle(tab, row, -2);
        assert_eq!(cfg.session_idle_timeout, "1m");
        cfg.cycle(tab, row, -1);
        assert_eq!(cfg.session_idle_timeout, "off");

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        cfg.save_to(&path).unwrap();
        assert_eq!(load_from(&path).session_idle_timeout, "off");
    }

    #[test]
    fn theme_cycles_through_presets_and_resolves() {
        let mut cfg = Config::default();
        // Kete Workshop dark out of the box, on every OS.
        assert_eq!(cfg.theme, crate::theme::DEFAULT_THEME);
        assert_eq!(
            cfg.theme(),
            crate::theme::Theme::by_name(crate::theme::DEFAULT_THEME)
        );
        cfg.theme = "default".into();
        assert_eq!(cfg.theme(), crate::theme::Theme::default());
        let (tab, theme_row) = locate(SettingKind::Theme).unwrap();
        cfg.cycle(tab, theme_row, 1);
        assert_eq!(cfg.theme, "ocean");
        assert_ne!(cfg.theme(), crate::theme::Theme::default());
        cfg.cycle(tab, theme_row, -1);
        assert_eq!(cfg.theme, "default");
        // Unknown names (hand-edited config) cycle from the start and
        // resolve to the default palette rather than erroring.
        cfg.theme = "sparkle".into();
        assert_eq!(cfg.theme(), crate::theme::Theme::default());
    }

    #[test]
    fn animations_default_on_toggle_and_persist() {
        let mut cfg = Config::default();
        assert!(cfg.animations);
        let (tab, row) = locate(SettingKind::Animations).unwrap();
        cfg.cycle(tab, row, 0);
        assert!(!cfg.animations);

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        cfg.save_to(&path).unwrap();
        assert!(!load_from(&path).animations);
        // A config predating the key keeps animations on.
        let cfg: Config = serde_json::from_str("{}").unwrap();
        assert!(cfg.animations);
    }

    /// DRAFT PULL REQUESTS: an Appearance row that reads `shown` / `hidden`
    /// like the panel rows beside it, shown by default so a config that
    /// predates the key keeps every draft on screen, and persisted under
    /// `hide_draft_prs`.
    #[test]
    fn draft_pull_requests_default_shown_toggle_on_the_appearance_tab_and_persist() {
        let mut cfg = Config::default();
        assert!(!cfg.hide_draft_prs, "drafts stay on screen until asked");
        assert_eq!(cfg.value_label(SettingKind::HideDraftPrs), "shown");

        let (tab, row) = locate(SettingKind::HideDraftPrs).unwrap();
        assert_eq!(SETTINGS_TABS[tab].title, "Appearance");
        cfg.cycle(tab, row, 0);
        assert!(cfg.hide_draft_prs);
        assert_eq!(cfg.value_label(SettingKind::HideDraftPrs), "hidden");
        cfg.cycle(tab, row, 1);
        assert!(!cfg.hide_draft_prs, "←/→ toggle a bool like Enter does");
        cfg.cycle(tab, row, 0);

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        cfg.save_to(&path).unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains(r#""hide_draft_prs": true"#), "{raw}");
        assert!(load_from(&path).hide_draft_prs);

        let legacy: Config = serde_json::from_str("{}").unwrap();
        assert!(!legacy.hide_draft_prs);
    }

    #[test]
    fn source_control_graph_defaults_off_and_toggles_on_the_appearance_tab() {
        let mut cfg = Config::default();
        assert!(!cfg.source_control_graph);
        assert_eq!(cfg.value_label(SettingKind::SourceControlGraph), "off");

        let (tab, row) = locate(SettingKind::SourceControlGraph).unwrap();
        assert_eq!(SETTINGS_TABS[tab].title, "Appearance");
        cfg.cycle(tab, row, 0);
        assert!(cfg.source_control_graph);
        assert_eq!(cfg.value_label(SettingKind::SourceControlGraph), "on");
        cfg.cycle(tab, row, 1);
        assert!(!cfg.source_control_graph);
    }

    /// CARD PROMPT: retired with every card carrying its last prompt. The
    /// key an older build wrote (`hide_card_prompt`, off by default) still
    /// loads to what it wrote and is written back as stored, but no tab
    /// shows it any more.
    #[test]
    fn card_prompt_is_retired_but_still_round_trips() {
        assert!(
            !Config::default().hide_card_prompt,
            "the default an older build reads"
        );
        let cfg: Config = serde_json::from_str(r#"{"hide_card_prompt": true}"#).unwrap();
        assert!(cfg.hide_card_prompt, "loaded to what an older build wrote");

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        cfg.save_to(&path).unwrap();
        assert!(load_from(&path).hide_card_prompt, "written back as stored");

        assert!(
            SETTINGS_TABS.iter().all(|t| match t.body {
                TabBody::Values(rows) => rows.iter().all(|r| r.label != "Card prompt"),
                _ => true,
            }),
            "no tab shows the row"
        );
    }

    /// CARD ISSUE NUMBER: an Appearance row that reads `on` / `off`, on
    /// by default (a config that predates the key too), and persisted
    /// under `card_issue_number`.
    #[test]
    fn card_issue_number_default_on_toggle_on_the_appearance_tab_and_persist() {
        let mut cfg = Config::default();
        assert!(cfg.card_issue_number, "on by default");
        assert_eq!(cfg.value_label(SettingKind::CardIssueNumber), "on");

        let (tab, row) = locate(SettingKind::CardIssueNumber).unwrap();
        assert_eq!(SETTINGS_TABS[tab].title, "Appearance");
        cfg.cycle(tab, row, 0);
        assert!(!cfg.card_issue_number);
        assert_eq!(cfg.value_label(SettingKind::CardIssueNumber), "off");
        cfg.cycle(tab, row, 0);
        assert!(cfg.card_issue_number);

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        cfg.save_to(&path).unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains(r#""card_issue_number": true"#), "{raw}");
        assert!(load_from(&path).card_issue_number);

        let legacy: Config = serde_json::from_str("{}").unwrap();
        assert!(legacy.card_issue_number);
    }

    /// HIGHLIGHT CURRENT CARD: an Appearance row that reads `on` / `off`,
    /// on by default (a config that predates the key too), and persisted
    /// under `highlight_current_card`.
    #[test]
    fn highlight_current_card_default_on_toggle_on_the_appearance_tab_and_persist() {
        let mut cfg = Config::default();
        assert!(cfg.highlight_current_card, "on by default");
        assert_eq!(cfg.value_label(SettingKind::HighlightCurrentCard), "on");

        let (tab, row) = locate(SettingKind::HighlightCurrentCard).unwrap();
        assert_eq!(SETTINGS_TABS[tab].title, "Appearance");
        cfg.cycle(tab, row, 0);
        assert!(!cfg.highlight_current_card);
        assert_eq!(cfg.value_label(SettingKind::HighlightCurrentCard), "off");

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        cfg.save_to(&path).unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains(r#""highlight_current_card": false"#), "{raw}");
        assert!(!load_from(&path).highlight_current_card);

        let legacy: Config = serde_json::from_str("{}").unwrap();
        assert!(legacy.highlight_current_card);
    }

    /// EXPAND ALL WORKTREES: an Appearance row under **Worktree layout**
    /// that reads `on` / `off`, off by default (a config that predates the
    /// key too), and persisted under `expand_all_worktrees`.
    #[test]
    fn expand_all_worktrees_is_off_by_default_toggles_and_persists() {
        let mut cfg = Config::default();
        assert!(!cfg.expand_all_worktrees, "off by default");
        assert_eq!(cfg.value_label(SettingKind::ExpandAllWorktrees), "off");

        let (tab, row) = locate(SettingKind::ExpandAllWorktrees).unwrap();
        assert_eq!(SETTINGS_TABS[tab].title, "Appearance");
        assert_eq!(
            locate(SettingKind::WorktreeLayout),
            Some((tab, row - 2)),
            "under the layout it opens"
        );
        assert_eq!(
            locate(SettingKind::InlineGraphics),
            Some((tab, row - 1)),
            "after the graphics row"
        );
        cfg.cycle(tab, row, 0);
        assert!(cfg.expand_all_worktrees);
        assert_eq!(cfg.value_label(SettingKind::ExpandAllWorktrees), "on");

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        cfg.save_to(&path).unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains(r#""expand_all_worktrees": true"#), "{raw}");
        assert!(load_from(&path).expand_all_worktrees);

        cfg.cycle(tab, row, 0);
        assert!(!cfg.expand_all_worktrees);

        let legacy: Config = serde_json::from_str("{}").unwrap();
        assert!(!legacy.expand_all_worktrees, "a missing key reads as off");
    }

    #[test]
    fn inline_graphics_defaults_to_auto_cycles_and_persists() {
        let mut cfg = Config::default();
        assert_eq!(cfg.inline_graphics_label(), "auto");
        assert_eq!(cfg.value_label(SettingKind::InlineGraphics), "auto");
        assert_eq!(
            cfg.graphics_mode(),
            crate::graphics::GraphicsMode::Halfblocks
        );

        let (tab, row) = locate(SettingKind::InlineGraphics).unwrap();
        assert_eq!(SETTINGS_TABS[tab].title, "Appearance");
        cfg.cycle(tab, row, 1);
        assert_eq!(cfg.inline_graphics, "kitty");
        cfg.cycle(tab, row, -1);
        assert_eq!(cfg.inline_graphics, "auto");
        cfg.inline_graphics = "off".into();
        assert_eq!(cfg.graphics_mode(), crate::graphics::GraphicsMode::Off);

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        cfg.save_to(&path).unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains(r#""inline_graphics": "off""#), "{raw}");
        assert_eq!(load_from(&path).inline_graphics, "off");
    }

    /// CARD LINE COUNTS: retired with every card counting its lines. The
    /// key an older build wrote (`card_line_changes`, off by default) still
    /// loads to what it wrote and is written back as stored, but no tab
    /// shows it any more — Appearance carries later source-control settings.
    #[test]
    fn card_line_counts_is_retired_but_still_round_trips() {
        assert!(
            !Config::default().card_line_changes,
            "the default an older build reads"
        );
        let cfg: Config = serde_json::from_str(r#"{"card_line_changes": true}"#).unwrap();
        assert!(cfg.card_line_changes, "loaded to what an older build wrote");

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        cfg.save_to(&path).unwrap();
        assert!(load_from(&path).card_line_changes, "written back as stored");

        let appearance = SETTINGS_TABS
            .iter()
            .position(|t| t.title == "Appearance")
            .unwrap();
        let TabBody::Values(rows) = SETTINGS_TABS[appearance].body else {
            panic!("Appearance lists switches");
        };
        assert!(
            rows.iter().all(|r| r.label != "Card line counts"),
            "no row edits it"
        );
        assert!(
            rows.iter()
                .any(|r| r.kind == SettingKind::SourceControlGraph),
            "Appearance carries SOURCE CONTROL GRAPH"
        );
        assert_eq!(
            rows.last().map(|r| r.kind),
            Some(SettingKind::CrittersOften),
            "Appearance ends on the CRITTERS rows"
        );
    }

    /// The BLACK BACKGROUND: on out of the box, toggled off from its
    /// Appearance row (the terminal's own background shows), persisted
    /// under `black_background`; a config predating the key turns it on,
    /// and a saved `false` is honoured.
    #[test]
    fn black_background_default_on_toggle_and_persist() {
        let mut cfg = Config::default();
        assert!(cfg.black_background);
        let (tab, row) = locate(SettingKind::BlackBackground).unwrap();
        assert_eq!(SETTINGS_TABS[tab].title, "Appearance");
        cfg.cycle(tab, row, 0);
        assert!(!cfg.black_background);

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        cfg.save_to(&path).unwrap();
        assert!(!load_from(&path).black_background);
        let raw: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(raw.get("black_background"), Some(&serde_json::json!(false)));

        let older: Config = serde_json::from_str("{}").unwrap();
        assert!(
            older.black_background,
            "a config predating the key turns it on"
        );
    }

    /// HIDE CARD MARKS: off out of the box, toggled on from its
    /// Appearance row, persisted under `hide_card_marks`; a config
    /// predating the key keeps the marks, and one written under the old
    /// `hide_terminal_glyphs` key still reads.
    #[test]
    fn hide_card_marks_default_off_toggle_and_persist() {
        let mut cfg = Config::default();
        assert!(!cfg.hide_card_marks);
        let (tab, row) = locate(SettingKind::HideCardMarks).unwrap();
        assert_eq!(SETTINGS_TABS[tab].title, "Appearance");
        assert_eq!(cfg.value_label(SettingKind::HideCardMarks), "shown");
        cfg.cycle(tab, row, 0);
        assert!(cfg.hide_card_marks);
        assert_eq!(cfg.value_label(SettingKind::HideCardMarks), "hidden");

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        cfg.save_to(&path).unwrap();
        assert!(load_from(&path).hide_card_marks);
        let raw: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(raw.get("hide_card_marks"), Some(&serde_json::json!(true)));
        assert_eq!(raw.get("hide_terminal_glyphs"), None);

        let older: Config = serde_json::from_str("{}").unwrap();
        assert!(
            !older.hide_card_marks,
            "a config predating the key keeps the marks"
        );
        let renamed: Config = serde_json::from_str(r#"{"hide_terminal_glyphs":true}"#).unwrap();
        assert!(renamed.hide_card_marks, "the old key still reads");
    }

    /// A v0.39.0 file says `hide_terminal_glyphs`; the first save of a
    /// build that renamed it patched `hide_card_marks` in beside it. That
    /// pair once loaded as every default, so the settings overlay (which
    /// reads the file each frame) showed defaults whatever was chosen, and
    /// each save wrote those defaults back. Every other value must survive
    /// the pair, a toggle must read back, and the save must drop the old key.
    #[test]
    fn a_file_holding_both_card_marks_keys_keeps_its_settings_and_heals() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(
            &path,
            r#"{"theme": "ocean", "worktree_layout": "list", "hide_card_marks": true, "hide_terminal_glyphs": false}"#,
        )
        .unwrap();

        let mut cfg = load_from(&path);
        assert_eq!(cfg.theme, "ocean", "the clash costs no other value");
        assert!(cfg.list_layout());
        assert!(cfg.hide_card_marks, "the new key wins over the old one");

        let (tab, row) = locate(SettingKind::Animations).unwrap();
        cfg.cycle(tab, row, 0);
        cfg.save_to(&path).unwrap();
        let reread = load_from(&path);
        assert!(!reread.animations, "the toggle reads back");
        assert_eq!(reread.theme, "ocean");
        assert!(reread.hide_card_marks);
        let raw: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(raw.get("hide_terminal_glyphs"), None, "{raw}");

        // Upgrading straight from v0.39.0: the old key alone carries over
        // under its new name.
        std::fs::write(&path, r#"{"hide_terminal_glyphs": true}"#).unwrap();
        load_from(&path).save_to(&path).unwrap();
        let raw: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(raw.get("hide_card_marks"), Some(&serde_json::json!(true)));
        assert_eq!(raw.get("hide_terminal_glyphs"), None, "{raw}");

        // An old key the local layer holds moves to its new name there.
        let local = dir.path().join("config.local.json");
        std::fs::write(&local, r#"{"hide_terminal_glyphs": true}"#).unwrap();
        std::fs::write(&path, "{}").unwrap();
        load_from(&path).save_to(&path).unwrap();
        let held: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&local).unwrap()).unwrap();
        assert_eq!(held, serde_json::json!({"hide_card_marks": true}));
        assert!(load_from(&path).hide_card_marks);
    }

    /// The **Session pane**: down the right out of the box, cycled from
    /// its Appearance row to the bottom and back, persisted under
    /// `session_pane`. A config predating the key, or holding a word off
    /// the list — `left`, which older builds offered, included — reads as
    /// the right.
    #[test]
    fn session_pane_defaults_to_the_right_cycles_and_persists() {
        use crate::launcher::PaneSide;
        let mut cfg = Config::default();
        assert_eq!(cfg.pane_side(), PaneSide::Right);
        let (tab, row) = locate(SettingKind::SessionPane).unwrap();
        assert_eq!(SETTINGS_TABS[tab].title, "Appearance");
        assert_eq!(cfg.value_label(SettingKind::SessionPane), "right");
        cfg.cycle(tab, row, 0);
        assert_eq!(cfg.pane_side(), PaneSide::Bottom);
        assert_eq!(cfg.value_label(SettingKind::SessionPane), "bottom");
        cfg.cycle(tab, row, 1);
        assert_eq!(cfg.pane_side(), PaneSide::Right, "and round again");
        cfg.cycle(tab, row, -1);
        assert_eq!(cfg.pane_side(), PaneSide::Bottom, "either arrow walks it");

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        cfg.save_to(&path).unwrap();
        assert_eq!(load_from(&path).pane_side(), PaneSide::Bottom);
        let raw: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(raw.get("session_pane"), Some(&serde_json::json!("bottom")));

        let older: Config = serde_json::from_str("{}").unwrap();
        assert_eq!(older.pane_side(), PaneSide::Right, "predating the key");
        let mut odd: Config = serde_json::from_str(r#"{"session_pane": "top"}"#).unwrap();
        assert_eq!(odd.pane_side(), PaneSide::Right, "a word off the list");
        odd.cycle(tab, row, 0);
        assert_eq!(odd.pane_side(), PaneSide::Bottom, "steps on from the right");
        let left: Config = serde_json::from_str(r#"{"session_pane": "left"}"#).unwrap();
        assert_eq!(left.pane_side(), PaneSide::Right, "the retired left side");
    }

    /// The **Worktree layout**: the cards out of the box, cycled from its
    /// Appearance row to the compact list, on to the nested layout, then
    /// columns, and round to the cards again, persisted under `worktree_layout`. A
    /// config predating the key, or holding a word off the list, reads as
    /// the cards.
    #[test]
    fn worktree_layout_defaults_to_cards_cycles_and_persists() {
        let mut cfg = Config::default();
        assert!(!cfg.list_layout());
        assert!(!cfg.columns_layout());
        let (tab, row) = locate(SettingKind::WorktreeLayout).unwrap();
        assert_eq!(SETTINGS_TABS[tab].title, "Appearance");
        assert_eq!(cfg.value_label(SettingKind::WorktreeLayout), "cards");
        cfg.cycle(tab, row, 1);
        assert!(cfg.list_layout());
        assert_eq!(cfg.value_label(SettingKind::WorktreeLayout), "list");
        cfg.cycle(tab, row, -1);
        assert!(!cfg.list_layout(), "and back");
        cfg.cycle(tab, row, 0);
        assert!(cfg.list_layout(), "Enter steps it on too");

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        cfg.save_to(&path).unwrap();
        assert!(load_from(&path).list_layout());
        let raw: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(raw.get("worktree_layout"), Some(&serde_json::json!("list")));

        let older: Config = serde_json::from_str("{}").unwrap();
        assert!(!older.list_layout(), "predating the key");
        let odd: Config = serde_json::from_str(r#"{"worktree_layout": "grid"}"#).unwrap();
        assert!(!odd.list_layout(), "a word off the list");
        assert_eq!(odd.value_label(SettingKind::WorktreeLayout), "cards");

        // The third and fourth choices: one step on from the list reaches
        // nested, another reaches columns, and one more wraps to cards.
        let mut cfg = Config::default();
        cfg.cycle(tab, row, 1);
        cfg.cycle(tab, row, 1);
        assert!(cfg.nested_layout() && !cfg.list_layout() && !cfg.columns_layout());
        assert_eq!(cfg.value_label(SettingKind::WorktreeLayout), "nested");
        cfg.save_to(&path).unwrap();
        assert!(load_from(&path).nested_layout());
        cfg.cycle(tab, row, 1);
        assert!(cfg.columns_layout());
        assert_eq!(cfg.value_label(SettingKind::WorktreeLayout), "columns");
        cfg.save_to(&path).unwrap();
        assert!(load_from(&path).columns_layout());
        cfg.cycle(tab, row, 1);
        assert_eq!(cfg.value_label(SettingKind::WorktreeLayout), "cards");
        cfg.cycle(tab, row, -1);
        assert!(cfg.columns_layout(), "and back from the cards");
    }

    /// The QUICK PROMPT's focus toggle: off unless the user turns it on,
    /// and persisted under its own key (a missed `obj.insert` would let the
    /// row toggle on screen and read back off on the next launch).
    #[test]
    fn quick_prompt_focus_toggles_off_by_default_and_persists() {
        let mut cfg = Config::default();
        assert!(
            !cfg.quick_prompt_focus,
            "a quick prompt stays out of the way"
        );
        assert_eq!(cfg.value_label(SettingKind::QuickPromptFocus), "off");

        let (tab, row) = locate(SettingKind::QuickPromptFocus).unwrap();
        cfg.cycle(tab, row, 0);
        assert!(cfg.quick_prompt_focus);
        assert_eq!(cfg.value_label(SettingKind::QuickPromptFocus), "on");

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        cfg.save_to(&path).unwrap();
        assert!(load_from(&path).quick_prompt_focus);

        // A config predating the key reads as off.
        let cfg: Config = serde_json::from_str("{}").unwrap();
        assert!(!cfg.quick_prompt_focus);
    }

    /// FOLLOW NEW SESSION starts on, sits under the QUICK PROMPT's Focus
    /// row on the Agents tab, toggles like any bool and persists under its
    /// own key; a config predating the key reads as on.
    #[test]
    fn confirm_drag_move_is_on_by_default_and_persists() {
        let mut cfg = Config::default();
        assert!(cfg.confirm_drag_move, "a drop asks before it moves");
        let (tab, row) = locate(SettingKind::ConfirmDragMove).unwrap();
        assert_eq!(SETTINGS_TABS[tab].title, "Sessions");
        cfg.cycle(tab, row, 0);
        assert_eq!(cfg.value_label(SettingKind::ConfirmDragMove), "off");

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        cfg.save_to(&path).unwrap();
        assert!(!load_from(&path).confirm_drag_move, "off survives a save");
        let cfg: Config = serde_json::from_str("{}").unwrap();
        assert!(cfg.confirm_drag_move, "a missing key reads as on");
    }

    #[test]
    fn follow_new_session_is_on_by_default_and_persists() {
        let mut cfg = Config::default();
        assert!(cfg.follow_new_session, "a launch lands on its new card");
        assert_eq!(cfg.value_label(SettingKind::FollowNewSession), "on");
        let (tab, row) = locate(SettingKind::FollowNewSession).unwrap();
        assert_eq!(SETTINGS_TABS[tab].title, "Agents");
        assert_eq!(
            locate(SettingKind::QuickPromptFocus),
            Some((tab, row - 1)),
            "beside the Focus row that outranks it"
        );

        cfg.cycle(tab, row, 0);
        assert!(!cfg.follow_new_session);
        assert_eq!(cfg.value_label(SettingKind::FollowNewSession), "off");

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        cfg.save_to(&path).unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains(r#""follow_new_session": false"#), "{raw}");
        assert!(!load_from(&path).follow_new_session, "off survives a save");

        let cfg: Config = serde_json::from_str("{}").unwrap();
        assert!(cfg.follow_new_session, "a missing key reads as on");
    }

    /// **Run command** on the Project tab: a typed row (Enter prompts,
    /// ←/→ and cycle change nothing) kept per repo path as `run_command`,
    /// shown as `.nebula.json` while empty — the file decides then — and
    /// left out of the file while empty, so an entry with nothing else in
    /// it is dropped rather than written.
    #[test]
    fn run_command_is_a_typed_project_row_kept_per_repo_path() {
        let demo = Path::new("/tmp/demo");
        let other = Path::new("/tmp/other");
        let mut cfg = Config::default();
        assert!(SettingKind::RunCommand.is_text());
        assert!(SettingKind::RunCommand.is_project());
        let (tab, row) = locate(SettingKind::RunCommand).unwrap();
        assert_eq!(tab, project_tab());
        assert_eq!(row, 0, "the first row of the tab");
        assert_eq!(cfg.project(demo).run_command, "");
        assert_eq!(
            cfg.project(demo).value_label(SettingKind::RunCommand),
            PROJECT_FILE_CHOICE
        );
        assert_eq!(cfg.value_label(SettingKind::RunCommand), ".kete.json");
        assert_eq!(cfg.project_text_value(demo, SettingKind::RunCommand), "");

        // The tab's cycle never touches a typed row.
        for delta in [0, 1, -1] {
            cfg.cycle(tab, row, delta);
        }
        assert!(cfg.projects.is_empty());
        assert!(
            !cfg.set_text(SettingKind::RunCommand, "npm run dev"),
            "not a top-level row"
        );
        assert!(
            !cfg.set_project_text(demo, SettingKind::Theme, "dark"),
            "not a project row"
        );
        assert!(cfg.projects.is_empty());

        assert!(cfg.set_project_text(demo, SettingKind::RunCommand, "  npm run dev "));
        assert_eq!(cfg.project(demo).run_command, "npm run dev", "trimmed");
        assert_eq!(
            cfg.project(demo).value_label(SettingKind::RunCommand),
            "npm run dev"
        );
        assert_eq!(
            cfg.project_text_value(demo, SettingKind::RunCommand),
            "npm run dev"
        );
        assert_eq!(
            cfg.project(other).value_label(SettingKind::RunCommand),
            PROJECT_FILE_CHOICE,
            "one project's, not every project's"
        );
        assert_eq!(
            cfg.project(demo).open_command,
            "",
            "open's key is untouched"
        );

        // Persisted in the project's entry, under its path, beside no key
        // it did not set.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        assert!(cfg.set_project_text(other, SettingKind::OpenCommand, "open x"));
        cfg.save_to(&path).unwrap();
        assert_eq!(
            read_json_file(&path)["projects"],
            serde_json::json!({
                "/tmp/demo": { "run_command": "npm run dev" },
                "/tmp/other": { "open_command": "open x" },
            })
        );
        let loaded = load_from(&path);
        assert_eq!(loaded.project(demo).run_command, "npm run dev");
        assert_eq!(loaded.project(other).run_command, "");

        // Empty is the way back to the file — and drops the entry.
        assert!(cfg.set_project_text(demo, SettingKind::RunCommand, "   "));
        assert_eq!(cfg.projects.keys().collect::<Vec<_>>(), [other]);
        cfg.save_to(&path).unwrap();
        assert_eq!(
            read_json_file(&path)["projects"],
            serde_json::json!({ "/tmp/other": { "open_command": "open x" } })
        );
    }

    /// **Open command** on the Project tab: the same typed per-project row
    /// as Run command, right under it, kept as `open_command` — what
    /// `Shift+Enter` / `Shift+O` runs before looking at `.nebula.json`.
    /// Its own key, so setting it leaves `run_command` alone; empty drops
    /// it from the entry.
    #[test]
    fn open_command_is_a_typed_project_row_under_run_command() {
        let demo = Path::new("/tmp/demo");
        let mut cfg = Config::default();
        assert!(SettingKind::OpenCommand.is_text());
        assert!(SettingKind::OpenCommand.is_project());
        let (tab, row) = locate(SettingKind::OpenCommand).unwrap();
        assert_eq!(tab, project_tab());
        assert_eq!(
            row,
            locate(SettingKind::RunCommand).unwrap().1 + 1,
            "right under Run command"
        );
        assert_eq!(cfg.project(demo).open_command, "");
        assert_eq!(
            cfg.project(demo).value_label(SettingKind::OpenCommand),
            PROJECT_FILE_CHOICE
        );
        assert_eq!(cfg.value_label(SettingKind::OpenCommand), ".kete.json");

        // A typed row: cycling its tab row changes nothing, and the
        // top-level setter is not its.
        cfg.cycle(tab, row, 0);
        assert!(cfg.projects.is_empty());
        assert!(!cfg.set_text(SettingKind::OpenCommand, "open x"));

        assert!(cfg.set_project_text(
            demo,
            SettingKind::OpenCommand,
            "  open http://localhost:3000 "
        ));
        assert_eq!(
            cfg.project(demo).open_command,
            "open http://localhost:3000",
            "trimmed"
        );
        assert_eq!(cfg.project(demo).run_command, "", "run's key is untouched");
        assert_eq!(
            cfg.project_text_value(demo, SettingKind::OpenCommand),
            "open http://localhost:3000"
        );

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        cfg.save_to(&path).unwrap();
        assert_eq!(
            read_json_file(&path)["projects"],
            serde_json::json!({
                "/tmp/demo": { "open_command": "open http://localhost:3000" },
            })
        );
        assert_eq!(
            load_from(&path).project(demo).open_command,
            "open http://localhost:3000"
        );

        // Empty is the way back to the file — and drops the entry.
        assert!(cfg.set_project_text(demo, SettingKind::OpenCommand, " "));
        assert!(cfg.projects.is_empty());
    }

    /// The Project tab: one line naming the project, then its rows, every
    /// one of them a project row — and no project row anywhere else.
    #[test]
    fn the_project_tab_names_the_project_then_lists_its_rows() {
        let tab = project_tab();
        assert_eq!(SETTINGS_TABS[tab].title, "Project");
        assert!(tab < hotkeys_tab());
        // No profiles, so no Default account rows: the declared ones alone.
        let (rows, len) = with_empty_config(|| (settings_rows(tab), tab_len(tab)));
        assert_eq!(rows[0], SettingsRow::Project);
        assert_eq!(
            rows[1..],
            (0..len).map(SettingsRow::Setting).collect::<Vec<_>>()[..]
        );
        for (t, _, spec) in all_settings() {
            assert_eq!(
                spec.kind.is_project(),
                t == tab,
                "{:?} sits on {}",
                spec.kind,
                SETTINGS_TABS[t].title
            );
        }
    }

    /// The KEY COMBO DISPLAY is always on now, so **Key combo display**
    /// has no row to be edited on — but the key an earlier release wrote
    /// still loads, and is written back unchanged for the older builds
    /// that read it.
    #[test]
    fn show_key_combos_has_no_row_and_is_written_back_for_older_builds() {
        assert!(SETTINGS_TABS.iter().all(|tab| match &tab.body {
            TabBody::Values(rows) | TabBody::Project(rows) => {
                rows.iter().all(|row| row.label != "Key combo display")
            }
            TabBody::Hotkeys | TabBody::Agents => true,
        }));

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(&path, r#"{"show_key_combos": true}"#).unwrap();
        let mut cfg = load_from(&path);
        assert!(cfg.skipped.is_empty(), "{:?}", cfg.skipped);
        assert!(cfg.show_key_combos);

        cfg.remember_harness = true;
        cfg.save_to(&path).unwrap();
        assert_eq!(read_json_file(&path)["show_key_combos"], true);

        let cfg: Config = serde_json::from_str("{}").unwrap();
        assert!(
            !cfg.show_key_combos,
            "unknown to a config written before it"
        );
    }

    /// REMEMBER HARNESS: an Experimental switch, off by default, a plain
    /// toggle persisted under `remember_harness`, unknown to a config
    /// written before it (which reads as off).
    #[test]
    fn remember_harness_is_off_by_default_on_the_experimental_tab_and_persists() {
        let mut cfg = Config::default();
        assert!(!cfg.remember_harness, "a pick is one session's by default");
        assert_eq!(cfg.value_label(SettingKind::RememberHarness), "off");
        assert_eq!(
            cfg.remembered_kind(),
            None,
            "off: the picker starts on its first row"
        );

        let (tab, row) = locate(SettingKind::RememberHarness).unwrap();
        assert_eq!(SETTINGS_TABS[tab].title, "Experimental");
        assert_eq!(tab + 1, hotkeys_tab(), "Hotkeys stays last");
        assert_eq!(
            row, 0,
            "the tab's first row, the KEY COMBO DISPLAY being always on"
        );
        cfg.cycle(tab, row, 0);
        assert!(cfg.remember_harness);
        assert_eq!(cfg.value_label(SettingKind::RememberHarness), "on");
        assert_eq!(cfg.remembered_kind(), Some(AgentKind::Claude));
        cfg.cycle(tab, row, 1);
        assert!(!cfg.remember_harness, "either arrow toggles it back");
        cfg.cycle(tab, row, -1);
        assert!(cfg.remember_harness);

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        cfg.save_to(&path).unwrap();
        assert!(load_from(&path).remember_harness);

        let cfg: Config = serde_json::from_str("{}").unwrap();
        assert!(!cfg.remember_harness);
    }

    /// PRESET TEXT: a Sessions row after the sounds, `prefix` by default,
    /// cycling the three sides and persisted under `preset_text`; a hand
    /// edit off the list reads as the default, and `both` as the long
    /// label.
    #[test]
    fn preset_text_is_prefix_by_default_on_the_sessions_tab_and_cycles_the_sides() {
        let mut cfg = Config::default();
        assert_eq!(
            cfg.preset_text(),
            PresetText::Prefix,
            "one box, before the task"
        );
        assert_eq!(cfg.value_label(SettingKind::PresetText), "prefix");
        assert_eq!(
            PRESET_TEXTS.to_vec(),
            PresetText::ALL.map(PresetText::as_str).to_vec(),
            "the row cycles every side, in the enum's order"
        );

        let (tab, row) = locate(SettingKind::PresetText).unwrap();
        assert_eq!(SETTINGS_TABS[tab].title, "Sessions");
        let (sound_tab, sound_row) = locate(SettingKind::FeedbackSound).unwrap();
        assert_eq!((sound_tab, sound_row + 1), (tab, row), "after the sounds");
        cfg.cycle(tab, row, 1);
        assert_eq!(cfg.preset_text(), PresetText::Postfix);
        cfg.cycle(tab, row, 1);
        assert_eq!(cfg.preset_text(), PresetText::Both);
        assert_eq!(cfg.value_label(SettingKind::PresetText), "prefix & postfix");
        cfg.cycle(tab, row, 1);
        assert_eq!(cfg.preset_text(), PresetText::Prefix, "wraps");
        cfg.cycle(tab, row, -1);
        assert_eq!(cfg.preset_text(), PresetText::Both, "← steps back");
        cfg.cycle(tab, row, 0);
        assert_eq!(cfg.preset_text(), PresetText::Prefix, "Enter steps on");

        cfg.cycle(tab, row, -1);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        cfg.save_to(&path).unwrap();
        assert_eq!(load_from(&path).preset_text, "prefix & postfix");
        assert_eq!(load_from(&path).preset_text(), PresetText::Both);

        let cfg: Config = serde_json::from_str("{}").unwrap();
        assert_eq!(
            cfg.preset_text(),
            PresetText::Prefix,
            "unknown to an older file"
        );
        let cfg: Config = serde_json::from_str(r#"{"preset_text": "both"}"#).unwrap();
        assert_eq!(cfg.preset_text(), PresetText::Both);
        assert_eq!(cfg.value_label(SettingKind::PresetText), "prefix & postfix");
        let mut cfg: Config = serde_json::from_str(r#"{"preset_text": "suffix"}"#).unwrap();
        assert_eq!(
            cfg.preset_text(),
            PresetText::Prefix,
            "an unknown value is the default"
        );
        cfg.cycle(tab, row, 1);
        assert_eq!(
            cfg.preset_text(),
            PresetText::Postfix,
            "and cycles on from it"
        );
    }

    /// PR & ISSUE COUNTS: retired with the header always counting. The
    /// key an older build wrote (`pr_issue_counts`, on by default) still
    /// loads to what it wrote and is written back as stored, but no tab
    /// shows it any more — Experimental ends on REMEMBER HARNESS.
    #[test]
    fn pr_issue_counts_is_retired_but_still_round_trips() {
        let cfg = Config::default();
        assert!(cfg.pr_issue_counts, "the default an older build reads");
        let cfg: Config = serde_json::from_str(r#"{"pr_issue_counts": false}"#).unwrap();
        assert!(!cfg.pr_issue_counts, "loaded to what an older build wrote");

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        cfg.save_to(&path).unwrap();
        assert!(!load_from(&path).pr_issue_counts, "written back as stored");

        let experimental = SETTINGS_TABS
            .iter()
            .position(|t| t.title == "Experimental")
            .unwrap();
        let TabBody::Values(rows) = SETTINGS_TABS[experimental].body else {
            panic!("Experimental lists switches");
        };
        assert!(
            rows.iter().all(|r| r.label != "PR & issue counts"),
            "no row edits it"
        );
        assert_eq!(
            rows.last().map(|r| r.kind),
            Some(SettingKind::RememberHarness),
            "Experimental ends on REMEMBER HARNESS again"
        );
    }

    /// What a remembered launch writes: the harness into the QUICK
    /// PROMPT's `quick_prompt_kind`, a picked model or effort into that
    /// harness's own rows — and nothing at all while the switch is off,
    /// or when the pick already is the default.
    #[test]
    fn remember_launch_writes_the_agents_tab_rows_only_while_on() {
        let mut cfg = Config::default();
        assert!(
            !cfg.remember_launch(AgentKind::Codex, None, Some("gpt-5.5"), Some("high")),
            "off: nothing moves"
        );
        assert_eq!(cfg.quick_prompt_kind(), AgentKind::Claude);
        assert_eq!(cfg.codex_model, DEFAULT_CHOICE);
        assert_eq!(cfg.codex_effort, DEFAULT_CHOICE);

        cfg.remember_harness = true;
        // The harness alone: the rows it did not drill into stay put.
        assert!(cfg.remember_launch(AgentKind::Codex, None, None, None));
        assert_eq!(cfg.quick_prompt_kind(), AgentKind::Codex);
        assert_eq!(cfg.remembered_kind(), Some(AgentKind::Codex));
        assert_eq!(cfg.codex_model, DEFAULT_CHOICE, "no model was picked");
        assert_eq!(cfg.codex_effort, DEFAULT_CHOICE);
        assert!(
            !cfg.remember_launch(AgentKind::Codex, None, None, None),
            "the same pick again changes nothing, so nothing is saved"
        );

        // A model and effort drilled into land on that harness's rows.
        assert!(cfg.remember_launch(AgentKind::Claude, None, Some("opus"), Some("high")));
        assert_eq!(cfg.quick_prompt_kind(), AgentKind::Claude);
        assert_eq!(
            cfg.default_model(AgentKind::Claude).as_deref(),
            Some("opus")
        );
        assert_eq!(
            cfg.default_effort(AgentKind::Claude).as_deref(),
            Some("high")
        );
        assert_eq!(
            cfg.codex_model, DEFAULT_CHOICE,
            "another harness's rows are its own"
        );
        // The explicit "default" row is a pick too: back to no flag.
        assert!(cfg.remember_launch(AgentKind::Claude, None, Some("default"), None));
        assert_eq!(cfg.claude_model, DEFAULT_CHOICE);
        assert_eq!(
            cfg.default_effort(AgentKind::Claude).as_deref(),
            Some("high")
        );
        // A blank pick is no pick.
        assert!(!cfg.remember_launch(AgentKind::Claude, None, Some("  "), Some("")));

        // Cursor: a family picked without an effort refits the stored one
        // to what the family ships, as the AGENTS TAB's own cycle does.
        cfg.cursor_effort = "high".into();
        let family_without_efforts = crate::cursor_catalogue::models()
            .iter()
            .find(|m| {
                !m.eq_ignore_ascii_case(DEFAULT_CHOICE)
                    && effort_choices(AgentKind::Cursor, Some(m), None).is_empty()
            })
            .copied()
            .expect("the seed catalogue ships a family with no effort variants");
        assert!(cfg.remember_launch(AgentKind::Cursor, None, Some(family_without_efforts), None));
        assert_eq!(cfg.cursor_model, family_without_efforts);
        assert_eq!(
            cfg.default_effort(AgentKind::Cursor),
            None,
            "refitted, never refused"
        );

        // Round trip: what was remembered is what loads.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        cfg.save_to(&path).unwrap();
        let loaded = load_from(&path);
        assert_eq!(loaded.quick_prompt_kind(), AgentKind::Cursor);
        assert_eq!(loaded.claude_effort, "high");
        assert_eq!(loaded.cursor_model, family_without_efforts);
    }

    /// The QUICK PROMPT's harness: one name, cycled over every AGENT KIND,
    /// read back through the fallback that steps around a harness switched
    /// off since it was chosen.
    #[test]
    fn quick_prompt_kind_cycles_every_harness_and_persists() {
        let names: Vec<String> = AgentKind::ALL
            .iter()
            .filter(|k| **k != AgentKind::Custom)
            .map(|k| k.as_str().to_string())
            .collect();
        assert_eq!(agent_kind_names(), names, "one choice per kind");

        let mut cfg = Config::default();
        assert_eq!(cfg.quick_prompt_kind(), AgentKind::Claude);
        let (tab, row) = locate(SettingKind::QuickPromptKind).unwrap();
        cfg.cycle(tab, row, 1);
        assert_eq!(cfg.value_label(SettingKind::QuickPromptKind), "codex");
        assert_eq!(cfg.quick_prompt_kind(), AgentKind::Codex);
        cfg.cycle(tab, row, -1);
        assert_eq!(cfg.quick_prompt_kind(), AgentKind::Claude);

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        cfg.cycle(tab, row, 2);
        cfg.save_to(&path).unwrap();
        assert_eq!(load_from(&path).quick_prompt_kind(), AgentKind::Cursor);

        // A config predating the key, and a name nothing parses, both read
        // as Claude rather than refusing to launch.
        let cfg: Config = serde_json::from_str("{}").unwrap();
        assert_eq!(cfg.quick_prompt_kind(), AgentKind::Claude);
        let cfg: Config = serde_json::from_str(r#"{"quick_prompt_kind":"gemini"}"#).unwrap();
        assert_eq!(cfg.quick_prompt_kind(), AgentKind::Claude);

        // The chosen harness switched off on the AGENTS TAB steps on to the
        // first one still enabled.
        let cfg: Config = serde_json::from_str(
            r#"{"quick_prompt_kind":"codex","codex_enabled":false,"claude_enabled":false}"#,
        )
        .unwrap();
        assert_eq!(cfg.quick_prompt_kind(), AgentKind::Cursor);
    }

    #[test]
    fn harness_toggles_default_on_and_persist() {
        let mut cfg = Config::default();
        assert!(cfg.claude_enabled && cfg.codex_enabled && cfg.cursor_enabled);
        assert!(cfg.pi_enabled && cfg.muse_enabled && cfg.opencode_enabled);
        let builtin: Vec<AgentKind> = AgentKind::ALL
            .into_iter()
            .filter(|kind| *kind != AgentKind::Custom)
            .collect();
        assert_eq!(cfg.enabled_kinds(), builtin);
        assert!(
            !cfg.kind_enabled(AgentKind::Custom),
            "a bare Custom kind is never enabled: entries gate themselves"
        );

        let (tab, row) = locate_in(&cfg, "codex", HarnessField::Enabled).unwrap();
        cfg.cycle(tab, row, 0);
        assert!(!cfg.codex_enabled);
        assert!(!cfg.kind_enabled(AgentKind::Codex));
        assert_eq!(
            cfg.enabled_kinds(),
            vec![
                AgentKind::Claude,
                AgentKind::Cursor,
                AgentKind::Pi,
                AgentKind::Muse,
                AgentKind::Grok,
                AgentKind::OpenCode,
                AgentKind::Copilot
            ],
            "the disabled kind drops out, order kept"
        );
        assert!(
            !cfg.enabled_kinds().contains(&AgentKind::Custom),
            "a bare Custom kind never lists"
        );
        // ←/→ toggle a bool just like Enter does.
        cfg.cycle(tab, row, -1);
        assert!(cfg.codex_enabled);
        cfg.cycle(tab, row, 1);
        assert!(!cfg.codex_enabled);

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        cfg.save_to(&path).unwrap();
        let loaded = load_from(&path);
        assert!(loaded.claude_enabled);
        assert!(!loaded.codex_enabled);
        assert!(loaded.cursor_enabled);
        // A config predating the keys offers every built-in harness (a
        // bare Custom kind never lists — entries come from the registry).
        let cfg: Config = serde_json::from_str("{}").unwrap();
        assert_eq!(cfg.enabled_kinds().len(), AgentKind::ALL.len() - 1);

        // Every kind off is representable (a hand edit), and reads as empty.
        let cfg: Config = serde_json::from_str(
            r#"{"claude_enabled":false,"codex_enabled":false,"cursor_enabled":false,"pi_enabled":false,"muse_enabled":false,"opencode_enabled":false,"harnesses":{"grok":{"enabled":false},"copilot":{"enabled":false}}}"#,
        )
        .unwrap();
        assert!(cfg.enabled_kinds().is_empty());
    }

    #[test]
    fn grok_is_builtin_and_settings_persist_without_legacy_keys() {
        let mut cfg: Config = serde_json::from_str("{}").unwrap();
        assert!(cfg.offered_harnesses().contains(&(AgentKind::Grok, None)));
        assert_eq!(AgentKind::parse("grok"), Some(AgentKind::Grok));
        assert_eq!(AgentKind::Grok.cli_program(), "grok");
        cfg.set_harness_enabled("grok", false);
        cfg.set_harness_model("grok", "model-id".into());
        cfg.set_harness_effort("grok", "high".into());
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        cfg.save_to(&path).unwrap();
        let loaded = load_from(&path);
        assert!(!loaded.kind_enabled(AgentKind::Grok));
        assert_eq!(
            loaded.default_model(AgentKind::Grok).as_deref(),
            Some("model-id")
        );
        assert_eq!(
            loaded.default_effort(AgentKind::Grok).as_deref(),
            Some("high")
        );
        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(saved["harnesses"]["grok"]["enabled"], false);
        assert_eq!(saved["harnesses"]["grok"]["model_default"], "model-id");
        assert_eq!(saved["harnesses"]["grok"]["effort_default"], "high");
    }

    #[test]
    fn offered_harnesses_list_usable_entries_in_registry_order() {
        let cfg = Config::default();
        assert_eq!(
            cfg.offered_harnesses(),
            vec![
                (AgentKind::Claude, None),
                (AgentKind::Codex, None),
                (AgentKind::Cursor, None),
                (AgentKind::Pi, None),
                (AgentKind::Muse, None),
                (AgentKind::Grok, None),
                (AgentKind::OpenCode, None),
                (AgentKind::Copilot, None),
            ]
        );

        // Disabled and broken entries are out; the tab still lists them
        // (with their reason) so they can be fixed.
        let cfg: Config = serde_json::from_str(
            r#"{"custom_harnesses": [
                {"id": "agy", "program": "agy"},
                {"id": "off", "program": "off", "enabled": false},
                {"id": "broken", "program": ""}
            ]}"#,
        )
        .unwrap();
        let offered = cfg.offered_harnesses();
        assert_eq!(offered.len(), 9);
        assert_eq!(offered[8], (AgentKind::Custom, Some("agy".into())));
        let rows = cfg.agent_rows();
        assert!(rows.contains(&("off".to_string(), HarnessField::Enabled)));
        assert!(rows.contains(&("broken".to_string(), HarnessField::Enabled)));
        assert!(
            cfg.agent_hint("broken", HarnessField::Enabled)
                .contains("broken:"),
            "the tab names the reason"
        );

        // Under the hide switch an entry survives only when its program
        // is on PATH, built-ins and customs alike.
        let dir = tempfile::tempdir().unwrap();
        // Windows finds a program through PATHEXT, as a spawn does: a bare
        // extensionless file is not one there.
        let on_disk = if cfg!(windows) { "agy.exe" } else { "agy" };
        std::fs::write(dir.path().join(on_disk), "").unwrap();
        // Looked up on a PATH of the temp dir alone, never by pointing the
        // process's PATH there: every other test thread spawning git reads
        // that one, and found no git while it pointed here.
        let hiding = Config {
            hide_uninstalled_harnesses: true,
            ..serde_json::from_str::<Config>(
                r#"{"custom_harnesses": [
                    {"id": "agy", "program": "agy"},
                    {"id": "missing", "program": "definitely-not-on-path"}
                ]}"#,
            )
            .unwrap()
        };
        let offered = hiding.offered_harnesses_on(dir.path().as_os_str());
        assert_eq!(offered.len(), 1);
        assert_eq!(offered[0], (AgentKind::Custom, Some("agy".into())));
    }

    /// Safety: one broken `harnesses` entry never takes the rest down.
    /// The picker hides it, the tab shows its reason, and every other
    /// harness launches exactly as before.
    #[test]
    fn a_broken_harness_entry_isolates_itself() {
        let cfg: Config = serde_json::from_str(
            r#"{"harnesses": {
                "codex": {"program": ""},
                "agy": {"program": "agy", "resume_flag": "--resume"}
            }}"#,
        )
        .unwrap();
        let offered = cfg.offered_harnesses();
        assert!(
            !offered.iter().any(|(kind, _)| *kind == AgentKind::Codex),
            "the broken built-in hides"
        );
        assert!(
            offered.contains(&(AgentKind::Custom, Some("agy".into()))),
            "the valid newcomer offers"
        );
        assert_eq!(cfg.default_model(AgentKind::Claude), None);
        assert_eq!(cfg.default_model(AgentKind::Codex), None);
        assert!(cfg
            .agent_hint("codex", HarnessField::Enabled)
            .contains("broken:"));
        // The entry still resolves for reads (placeholder-free), while
        // launches refuse it with the reason.
        let codex = cfg.effective_harness_by_id("codex");
        assert!(codex.problem().is_some());
        assert_eq!(
            nebula_core::harness::resolve(&cfg.harness_registry(), AgentKind::Codex, None)
                .unwrap_err(),
            "harness `codex` has no program"
        );
    }

    /// Safety: a `harnesses` map that fails to parse costs only its own
    /// key — every other setting keeps its value, and the registry reads
    /// as a fresh install.
    #[test]
    fn an_unreadable_harnesses_map_costs_only_that_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(
            &path,
            r#"{"harnesses": {"claude": {"enabled": "yes"}}, "theme": "ocean"}"#,
        )
        .unwrap();
        let cfg = load_from(&path);
        assert_eq!(cfg.theme, "ocean");
        assert_eq!(cfg.skipped, BTreeSet::from(["harnesses".to_string()]));
        assert!(cfg.harness_registry().iter().all(|entry| entry.enabled));
    }

    #[test]
    fn model_effort_defaults_resolve_and_cycle() {
        let mut cfg = Config::default();
        // "default" everywhere → no flags for any kind.
        assert_eq!(cfg.default_model(AgentKind::Claude), None);
        assert_eq!(cfg.default_effort(AgentKind::Claude), None);
        assert_eq!(cfg.default_model(AgentKind::Codex), None);
        assert_eq!(cfg.default_effort(AgentKind::Codex), None);

        cfg.claude_model = "opus".into();
        cfg.codex_effort = "high".into();
        assert_eq!(
            cfg.default_model(AgentKind::Claude).as_deref(),
            Some("opus")
        );
        assert_eq!(cfg.default_effort(AgentKind::Claude), None);
        assert_eq!(cfg.default_model(AgentKind::Codex), None);
        assert_eq!(
            cfg.default_effort(AgentKind::Codex).as_deref(),
            Some("high")
        );
        // Pi passes both through like Claude: a pattern and a thinking level.
        assert_eq!(cfg.default_model(AgentKind::Pi), None);
        assert_eq!(cfg.default_effort(AgentKind::Pi), None);
        cfg.pi_model = "sonnet".into();
        cfg.pi_effort = "xhigh".into();
        assert_eq!(cfg.default_model(AgentKind::Pi).as_deref(), Some("sonnet"));
        assert_eq!(cfg.default_effort(AgentKind::Pi).as_deref(), Some("xhigh"));
        // Muse passes --model through verbatim; effort is reserved and
        // never sent, whatever the file holds.
        assert_eq!(cfg.default_model(AgentKind::Muse), None);
        assert_eq!(cfg.default_effort(AgentKind::Muse), None);
        cfg.muse_model = "spark".into();
        cfg.muse_effort = "high".into();
        assert_eq!(cfg.default_model(AgentKind::Muse).as_deref(), Some("spark"));
        assert_eq!(cfg.default_effort(AgentKind::Muse), None);
        assert_eq!(AgentKind::parse("muse"), Some(AgentKind::Muse));
        assert_eq!(AgentKind::Muse.cli_program(), "muse");
        // OpenCode passes a `provider/model` id through verbatim and has
        // no effort key at all: no Effort row to locate.
        assert_eq!(cfg.default_model(AgentKind::OpenCode), None);
        assert_eq!(cfg.default_effort(AgentKind::OpenCode), None);
        cfg.opencode_model = "anthropic/claude-sonnet-5".into();
        assert_eq!(
            cfg.default_model(AgentKind::OpenCode).as_deref(),
            Some("anthropic/claude-sonnet-5")
        );
        assert_eq!(cfg.default_effort(AgentKind::OpenCode), None);
        assert_eq!(AgentKind::parse("opencode"), Some(AgentKind::OpenCode));
        assert_eq!(AgentKind::OpenCode.cli_program(), "opencode");
        assert!(locate_in(&cfg, "opencode", HarnessField::Model).is_some());
        assert!(
            locate_in(&cfg, "opencode", HarnessField::Effort).is_none(),
            "no Effort row"
        );
        let (tab, row) = locate_in(&cfg, "pi", HarnessField::Effort).unwrap();
        cfg.cycle(tab, row, 1);
        assert_eq!(cfg.agent_value("pi", HarnessField::Effort), "max");
        cfg.cycle(tab, row, 1);
        assert_eq!(
            cfg.agent_value("pi", HarnessField::Effort),
            DEFAULT_CHOICE,
            "wraps"
        );
        // Cursor: the family is the model; the effort only counts when
        // that family ships it.
        assert_eq!(cfg.default_model(AgentKind::Cursor), None);
        assert_eq!(cfg.default_effort(AgentKind::Cursor), None);
        cfg.cursor_effort = "high".into();
        assert_eq!(
            cfg.default_effort(AgentKind::Cursor),
            None,
            "no family, no suffix to join"
        );
        cfg.cursor_model = "claude-opus-5".into();
        assert_eq!(
            cfg.default_model(AgentKind::Cursor).as_deref(),
            Some("claude-opus-5")
        );
        assert_eq!(
            cfg.default_effort(AgentKind::Cursor).as_deref(),
            Some("high")
        );
        cfg.cursor_effort = "max".into();
        assert_eq!(
            cfg.default_effort(AgentKind::Cursor).as_deref(),
            Some("high"),
            "Opus 5 has no max variant and no bare id: its fallback launches"
        );
        cfg.cursor_model = "gpt-5.3-codex".into();
        assert_eq!(
            cfg.default_effort(AgentKind::Cursor),
            None,
            "a family with a bare id: default really is no suffix"
        );
        cfg.cursor_effort = "high-fast".into();
        assert_eq!(
            cfg.default_effort(AgentKind::Cursor).as_deref(),
            Some("high-fast")
        );

        // The settings rows walk the same choice lists the submenus show.
        let (tab, row) = locate_in(&cfg, "claude", HarnessField::Model).unwrap();
        cfg.claude_model = "default".into();
        cfg.cycle(tab, row, 1);
        assert_eq!(cfg.claude_model, "fable");
        cfg.cycle(tab, row, -1);
        assert_eq!(cfg.claude_model, "default");
        let (tab, row) = locate_in(&cfg, "codex", HarnessField::Effort).unwrap();
        cfg.cycle(tab, row, 0);
        assert_eq!(
            cfg.codex_effort, "xhigh",
            "activate steps forward from high"
        );
    }

    #[test]
    fn cursor_settings_rows_follow_the_family() {
        let mut cfg = Config::default();
        let (tab, model_row) = locate_in(&cfg, "cursor", HarnessField::Model).unwrap();
        let (_, effort_row) = locate_in(&cfg, "cursor", HarnessField::Effort).unwrap();
        // No family: the effort row is n/a and does not cycle.
        assert_eq!(cfg.agent_value("cursor", HarnessField::Effort), "n/a");
        cfg.cycle(tab, effort_row, 1);
        assert_eq!(cfg.cursor_effort, "default");
        // default → auto (still no efforts) → claude-fable-5, which has no
        // bare id, so the effort lands on its fallback at once.
        cfg.cycle(tab, model_row, 1);
        assert_eq!(cfg.cursor_model, "auto");
        assert_eq!(cfg.agent_value("cursor", HarnessField::Effort), "n/a");
        cfg.cycle(tab, model_row, 1);
        assert_eq!(cfg.cursor_model, "claude-fable-5");
        assert_eq!(cfg.cursor_effort, "high");
        cfg.cycle(tab, effort_row, 1);
        assert_eq!(cfg.cursor_effort, "xhigh");
        cfg.cycle(tab, effort_row, 1);
        assert_eq!(cfg.cursor_effort, "max");
        // fable-5-thinking has max; Opus 5 doesn't → back to its fallback.
        cfg.cycle(tab, model_row, 1);
        assert_eq!(cfg.cursor_model, "claude-fable-5-thinking");
        assert_eq!(cfg.cursor_effort, "max", "a shared effort survives");
        cfg.cycle(tab, model_row, 1);
        assert_eq!(cfg.cursor_model, "claude-opus-5");
        assert_eq!(cfg.cursor_effort, "high");
        cfg.cycle(tab, effort_row, 1);
        assert_eq!(cfg.cursor_effort, "high-fast", "-fast rides in the effort");
        // A family with a bare id offers default again, and ← wraps onto
        // its last fast variant.
        cfg.cursor_model = "gpt-5.3-codex".into();
        cfg.cursor_effort = "default".into();
        cfg.cycle(tab, effort_row, -1);
        assert_eq!(cfg.cursor_effort, "xhigh-fast");
    }

    #[test]
    fn fit_effort_resolves_cursor_pairs() {
        let cursor = nebula_core::harness::builtin("cursor").unwrap();
        let fit = |m: Option<&str>, e: Option<&str>| fit_effort_in(&cursor, m, e.map(String::from));
        assert_eq!(fit(None, Some("high")), None, "no family, nothing to join");
        assert_eq!(fit(Some("default"), Some("high")), None);
        assert_eq!(
            fit(Some("auto"), Some("high")),
            None,
            "auto has no variants"
        );
        assert_eq!(fit(Some("nope"), Some("high")), None);
        assert_eq!(fit(Some("gpt-5.3-codex"), None), None, "bare id exists");
        assert_eq!(fit(Some("gpt-5.3-codex"), Some("bogus")), None);
        assert_eq!(
            fit(Some("gpt-5.3-codex"), Some("fast")).as_deref(),
            Some("fast")
        );
        assert_eq!(fit(Some("claude-fable-5"), None).as_deref(), Some("high"));
        assert_eq!(
            fit(Some("claude-fable-5"), Some("default")).as_deref(),
            Some("high")
        );
        assert_eq!(
            fit(Some("claude-fable-5"), Some("MAX ")).as_deref(),
            Some("max")
        );
        assert_eq!(
            fit(Some("gpt-5.5"), Some("xhigh")).as_deref(),
            Some("high"),
            "spelled extra-high there"
        );
        assert_eq!(
            fit(Some("gpt-5.5"), Some("extra-high-fast")).as_deref(),
            Some("extra-high-fast")
        );
        let codex = nebula_core::harness::builtin("codex").unwrap();
        assert_eq!(
            fit_effort_in(&codex, None, Some("high".into())).as_deref(),
            Some("high"),
            "claude/codex pass through"
        );
    }

    /// `claude_models` is hand-edited only: empty by default, written back
    /// as `[]` so the key is discoverable, and read back verbatim — a
    /// Bedrock id or an org's full model name survives the round trip.
    #[test]
    fn claude_models_key_round_trips_and_defaults_empty() {
        assert!(Config::default().claude_models.is_empty());
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        Config::default().save_to(&path).unwrap();
        let raw: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(raw["claude_models"], serde_json::json!([]));

        std::fs::write(
            &path,
            r#"{"claude_models": ["claude-sonnet-5", "us.anthropic.claude-opus-5-v1:0"]}"#,
        )
        .unwrap();
        let cfg = load_from(&path);
        assert_eq!(
            cfg.claude_models,
            vec![
                "claude-sonnet-5".to_string(),
                "us.anthropic.claude-opus-5-v1:0".to_string()
            ]
        );
        cfg.save_to(&path).unwrap();
        assert_eq!(load_from(&path).claude_models, cfg.claude_models);
    }

    #[test]
    fn save_persists_model_effort_keys() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let cfg = Config {
            claude_model: "sonnet".into(),
            codex_effort: "xhigh".into(),
            cursor_model: "gpt-5.6-sol".into(),
            cursor_effort: "none".into(),
            ..Config::default()
        };
        cfg.save_to(&path).unwrap();
        let reread = load_from(&path);
        assert_eq!(reread.claude_model, "sonnet");
        assert_eq!(reread.claude_effort, "default");
        assert_eq!(reread.codex_model, "default");
        assert_eq!(reread.codex_effort, "xhigh");
        assert_eq!(reread.cursor_model, "gpt-5.6-sol");
        assert_eq!(reread.cursor_effort, "none");
    }

    #[test]
    fn save_patches_known_keys_and_keeps_unknown_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(
            &path,
            r#"{
  "git_init_on_create": false,
  "future_daemon_flag": true,
  "session_idle_timeout": "15m"
}
"#,
        )
        .unwrap();

        let mut cfg = load_from(&path);
        assert!(!cfg.git_init_on_create);
        assert_eq!(cfg.session_idle_timeout, "15m");
        cfg.palette_enter_attaches = false;
        cfg.git_init_on_create = true;
        cfg.session_idle_timeout = "1h".into();
        cfg.save_to(&path).unwrap();

        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(saved["palette_enter_attaches"], false);
        assert_eq!(saved["git_init_on_create"], true);
        assert_eq!(saved["session_idle_timeout"], "1h");
        assert_eq!(saved["future_daemon_flag"], true);
    }

    #[test]
    fn tabs_cover_every_setting_once_and_rows_match() {
        with_empty_config(|| {
            // Every SettingKind appears exactly once across the tabs.
            let mut kinds: Vec<SettingKind> = all_settings().map(|(_, _, s)| s.kind).collect();
            let total = kinds.len();
            kinds.sort_by_key(|k| format!("{k:?}"));
            kinds.dedup();
            assert_eq!(kinds.len(), total, "a kind repeats across tabs");

            // Each tab's rows walk its own index space, in order.
            for (t, tab) in SETTINGS_TABS.iter().enumerate() {
                let indices: Vec<usize> = settings_rows(t)
                    .into_iter()
                    .filter_map(|row| row.index())
                    .collect();
                assert_eq!(
                    indices,
                    (0..tab_len(t)).collect::<Vec<_>>(),
                    "{} rows",
                    tab.title
                );
            }

            // A value tab carries headers exactly when its rows name
            // groups; Hotkeys and Agents always do.
            for (t, tab) in SETTINGS_TABS.iter().enumerate() {
                let headers = settings_rows(t)
                    .into_iter()
                    .filter(|row| matches!(row, SettingsRow::Header(_)))
                    .count();
                match tab.body {
                    TabBody::Values(settings) | TabBody::Project(settings) => {
                        let grouped = settings.iter().any(|s| !s.group.is_empty());
                        assert_eq!(headers > 0, grouped, "{}", tab.title);
                    }
                    TabBody::Hotkeys => assert!(headers > 0, "hotkeys tab groups its rows"),
                    TabBody::Agents => assert!(headers > 0, "agents tab groups its rows"),
                }
            }
        });
    }

    #[test]
    fn agents_tab_groups_its_rows_per_harness() {
        with_empty_config(|| {
            let tab = agents_tab();
            assert_eq!(SETTINGS_TABS[tab].title, "Agents");
            let cfg = Config::load();

            // Read the rows back the way the screen shows them: a header,
            // then the labels under it, with a blank between sections.
            let mut sections: Vec<(String, Vec<String>)> = Vec::new();
            for row in settings_rows(tab) {
                match row {
                    SettingsRow::Header(title) => sections.push((title, Vec::new())),
                    SettingsRow::Setting(i) => {
                        let label = match AGENTS_HEAD.get(i) {
                            Some(spec) => spec.label.to_string(),
                            None => cfg
                                .agent_row(i)
                                .map(|(_, field)| field.label().to_string())
                                .expect("every Agents row resolves"),
                        };
                        sections
                            .last_mut()
                            .expect("every Agents row sits under a header")
                            .1
                            .push(label);
                    }
                    SettingsRow::Blank => assert!(!sections.is_empty(), "no leading blank"),
                    SettingsRow::Project | SettingsRow::Hotkey(_) => unreachable!(),
                }
            }
            assert_eq!(
                sections,
                vec![
                    (
                        "Quick prompt".to_string(),
                        vec![
                            "Agent".to_string(),
                            "Focus".to_string(),
                            "Follow new".to_string(),
                            "New worktree".to_string(),
                            "Hide missing CLIs".to_string()
                        ]
                    ),
                    (
                        "Claude".to_string(),
                        vec![
                            "Enabled".to_string(),
                            "Model".to_string(),
                            "Effort".to_string()
                        ]
                    ),
                    (
                        "Codex".to_string(),
                        vec![
                            "Enabled".to_string(),
                            "Model".to_string(),
                            "Effort".to_string()
                        ]
                    ),
                    (
                        "Cursor".to_string(),
                        vec![
                            "Enabled".to_string(),
                            "Model".to_string(),
                            "Effort".to_string()
                        ]
                    ),
                    (
                        "Pi".to_string(),
                        vec![
                            "Enabled".to_string(),
                            "Model".to_string(),
                            "Effort".to_string()
                        ]
                    ),
                    (
                        "Muse".to_string(),
                        vec![
                            "Enabled".to_string(),
                            "Model".to_string(),
                            "Effort".to_string()
                        ]
                    ),
                    (
                        "Grok Build".to_string(),
                        vec![
                            "Enabled".to_string(),
                            "Model".to_string(),
                            "Effort".to_string()
                        ]
                    ),
                    (
                        "OpenCode".to_string(),
                        vec!["Enabled".to_string(), "Model".to_string()]
                    ),
                    (
                        "Copilot".to_string(),
                        vec![
                            "Enabled".to_string(),
                            "Model".to_string(),
                            "Effort".to_string()
                        ]
                    ),
                ]
            );

            // One blank line separates the sections and nothing else does.
            let blanks = settings_rows(tab)
                .into_iter()
                .filter(|row| *row == SettingsRow::Blank)
                .count();
            assert_eq!(blanks, sections.len() - 1);
        });
    }

    /// A new CLI in the registry grows its own Agents section — model and
    /// effort rows included — with no code change, and the picker offers
    /// it in the same order.
    #[test]
    fn agents_tab_grows_a_section_per_registry_entry() {
        with_empty_config(|| {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("config.json");
            std::fs::write(
                &path,
                r#"{"harnesses": {
                    "agy": {
                        "program": "agy",
                        "model_default": "big-1",
                        "effort_flag": "--effort",
                        "efforts": ["low", "high"],
                        "resume_flag": "--resume",
                        "hooks": "claude"
                    }
                }}"#,
            )
            .unwrap();
            with_config_path(path, || {
                let tab = agents_tab();
                let cfg = Config::load();
                let sections: Vec<String> = settings_rows(tab)
                    .into_iter()
                    .filter_map(|row| match row {
                        SettingsRow::Header(title) => Some(title),
                        _ => None,
                    })
                    .collect();
                assert_eq!(
                    sections,
                    vec![
                        "Quick prompt",
                        "Claude",
                        "Codex",
                        "Cursor",
                        "Pi",
                        "Muse",
                        "Grok Build",
                        "OpenCode",
                        "Copilot",
                        "agy"
                    ]
                );
                let (_, model_row) =
                    locate_agent("agy", HarnessField::Model).expect("the newcomer locates");
                assert_eq!(cfg.agent_value("agy", HarnessField::Model), "big-1");
                let (_, effort_row) =
                    locate_agent("agy", HarnessField::Effort).expect("its effort row shows");
                assert_eq!(cfg.agent_value("agy", HarnessField::Effort), "default");
                assert_eq!(tab_len(tab), AGENTS_HEAD.len() + cfg.agent_rows().len());
                // ... and the picker offers it after the built-ins.
                let offered = cfg.offered_harnesses();
                assert_eq!(
                    offered.last(),
                    Some(&(AgentKind::Custom, Some("agy".to_string())))
                );
                // Cycling its rows edits the map entry a save persists:
                // an off-list hand edit steps onto the list, effort
                // steps forward through its rows.
                let mut cfg = cfg;
                cfg.cycle(tab, model_row, 0);
                assert_eq!(
                    cfg.harnesses["agy"].model_default.as_deref(),
                    Some("default"),
                    "an off-list hand edit steps onto the offered rows"
                );
                cfg.cycle(tab, effort_row, 1);
                assert_eq!(cfg.harnesses["agy"].effort_default.as_deref(), Some("low"));
            });
        });
    }

    #[test]
    fn every_tab_holds_something() {
        with_empty_config(|| {
            assert!(tab_count() >= 2);
            for (t, tab) in SETTINGS_TABS.iter().enumerate() {
                assert!(tab_len(t) > 0, "{} is empty", tab.title);
                assert!(!tab.title.is_empty());
            }
            assert_eq!(tab_len(hotkeys_tab()), crate::keymap::ACTIONS.len());
        });
    }

    #[test]
    fn keybindings_round_trip_through_the_config_file() {
        let mut cfg = Config::default();
        assert!(cfg.keybindings.is_empty(), "no overrides out of the box");
        let mut keymap = cfg.keymap();
        let quit = crate::keymap::index_of(crate::keymap::Action::Quit).unwrap();
        keymap.bind(quit, crate::keymap::KeyChord::parse("f9").unwrap(), false);
        cfg.keybindings = keymap.overrides();

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        cfg.save_to(&path).unwrap();
        let reloaded = load_from(&path);
        assert_eq!(
            reloaded.keybindings.get("quit").map(String::as_str),
            Some("f9")
        );
        assert_eq!(
            reloaded.keymap().lookup(
                crate::keymap::Scope::Global,
                &crate::keymap::KeyChord::parse("f9").unwrap()
            ),
            Some(crate::keymap::Action::Quit)
        );
        // A config predating the key still gets the full default keymap.
        let old: Config = serde_json::from_str("{}").unwrap();
        assert_eq!(
            old.keymap().label(crate::keymap::Action::Quit),
            Keymap::default().label(crate::keymap::Action::Quit)
        );
    }

    #[test]
    fn a_hotkey_save_keeps_the_desktop_windows_keys() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(
            &path,
            r#"{"keybindings": {"quit": "ctrl+q", "desktop.palette": "ctrl+j", "desktop.help": "f2, ,"}}"#,
        )
        .unwrap();
        let mut cfg = load_from(&path);
        // The desktop entries mean nothing to the TUI's keymap.
        let mut keymap = cfg.keymap();
        assert_eq!(keymap.overrides().len(), 1);
        let help = crate::keymap::index_of(crate::keymap::Action::Help).unwrap();
        keymap.bind(help, crate::keymap::KeyChord::parse("f9").unwrap(), false);
        nebula_config::actions::merge_tui_overrides(&mut cfg.keybindings, keymap.overrides());
        cfg.save_to(&path).unwrap();
        let reloaded = load_from(&path);
        assert_eq!(reloaded.keybindings["quit"], "ctrl+q");
        assert_eq!(reloaded.keybindings["desktop.palette"], "ctrl+j");
        assert_eq!(reloaded.keybindings["desktop.help"], "f2, ,");
        assert!(reloaded.keybindings["help"].contains("f9"));
    }

    #[test]
    fn save_creates_file_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("config.json");
        Config::default().save_to(&path).unwrap();
        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(saved["palette_enter_attaches"], true);
        assert_eq!(saved["git_init_on_create"], true);
        assert_eq!(saved["session_idle_timeout"], "5m");
    }
}

/// Byte-for-byte pins on what a save writes, so moving `Config` between
/// crates (or touching its save path) can't change a byte of either layer
/// unnoticed. `KETE_BLESS=1` rewrites the goldens.
#[cfg(test)]
mod golden {
    use super::*;
    use std::path::Path;

    const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/src/testdata/config_golden");

    fn check(name: &str, actual: &str) {
        let path = Path::new(DIR).join(name);
        if std::env::var_os("KETE_BLESS").is_some() {
            std::fs::write(&path, actual).unwrap();
            return;
        }
        let want = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{}: {e} (run with KETE_BLESS=1)", path.display()));
        assert_eq!(actual, want.replace("\r\n", "\n"), "{name} drifted");
    }

    #[test]
    fn a_default_save_writes_these_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        Config::default().save_to(&path).unwrap();
        check("default.json", &std::fs::read_to_string(&path).unwrap());
    }

    #[test]
    fn a_patched_save_writes_these_bytes_in_both_layers() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let local = dir.path().join("config.local.json");
        std::fs::write(
            &path,
            r#"{"future_key": {"nested": [1, 2]}, "hide_terminal_glyphs": true, "editor": "nvim",
               "projects": {"/repo/a": {"run_command": "npm run dev", "mystery": 7}}}"#,
        )
        .unwrap();
        std::fs::write(&local, r#"{"theme": "ocean", "local_only": "x"}"#).unwrap();
        let mut cfg = load_from(&path);
        cfg.kanban = true;
        cfg.claude_model = "opus".into();
        cfg.theme = "forest".into();
        cfg.set_project_text(
            Path::new("/repo/b"),
            SettingKind::OpenCommand,
            "open http://x",
        );
        cfg.save_to(&path).unwrap();
        check("patched.json", &std::fs::read_to_string(&path).unwrap());
        check(
            "patched.local.json",
            &std::fs::read_to_string(&local).unwrap(),
        );
    }

    #[test]
    fn a_reset_writes_these_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(&path, r#"{"future_key": 1, "kanban": true}"#).unwrap();
        let cfg = with_config_path(path.clone(), Config::reset_to_defaults).unwrap();
        assert!(!cfg.kanban);
        check("reset.json", &std::fs::read_to_string(&path).unwrap());
    }
}

/// The test guard survived the move: `Config` lives in another crate now,
/// where `cfg(test)` is false while these tests run, so the guard rides on
/// that crate's `test-support` feature instead. A save with no path
/// override must still refuse rather than write the developer's config.
#[cfg(test)]
mod guard {
    use super::*;

    #[test]
    #[should_panic(expected = "without a path override")]
    fn a_save_without_a_path_override_panics() {
        let _ = Config::default().save();
    }

    #[test]
    #[should_panic(expected = "without a path override")]
    fn a_reset_without_a_path_override_panics() {
        let _ = Config::reset_to_defaults();
    }
}

#[cfg(test)]
mod shared_words {
    use super::*;
    use crate::launcher::PaneSide;

    #[test]
    fn the_shared_pane_words_are_the_pane_sides() {
        assert_eq!(
            choices::PANE_SIDES,
            [
                PaneSide::default().as_str(),
                PaneSide::default().other().as_str()
            ]
        );
        for word in ["right", "bottom", " bottom ", "left", "", "BOTTOM"] {
            let cfg = Config {
                session_pane: word.into(),
                ..Config::default()
            };
            assert_eq!(cfg.pane_side_word(), cfg.pane_side().as_str(), "{word:?}");
        }
    }
}

/// What the desktop window writes, the TUI reads back as it was meant.
#[cfg(test)]
mod desktop_round_trip {
    use super::*;
    use nebula_config::desktop::DesktopPrefs;
    use nebula_config::schema::{self, Target};
    use serde_json::{json, Value};
    use std::path::Path;

    const STORED: &str = r#"{
  "future_key": {"from": "a newer build"},
  "theme": "ocean",
  "kanban": true,
  "codex_model": "gpt-5.6-terra",
  "worktree_base_branch": "develop",
  "projects": {"/repo/demo": {"run_command": "make dev", "mystery": 1}},
  "keybindings": {"quit": "ctrl+q"},
  "desktop": {"theme": "dark"}
}"#;

    fn as_json(cfg: &Config) -> Value {
        serde_json::to_value(cfg).unwrap()
    }

    #[test]
    fn writing_every_row_back_as_it_is_changes_nothing_the_tui_reads() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let local = dir.path().join("config.local.json");
        std::fs::write(&path, STORED).unwrap();
        std::fs::write(&local, r#"{"editor": "hx", "machine_only": 1}"#).unwrap();
        let before_bytes = std::fs::read_to_string(&path).unwrap();
        let original = with_config_path(path.clone(), Config::load);
        let projects = [schema::ProjectRef {
            name: "demo".into(),
            path: "/repo/demo".into(),
        }];
        let exported = schema::export(&original, &DesktopPrefs::load_at(&path, &local), &projects);
        for row in exported
            .tabs
            .iter()
            .flat_map(|t| &t.groups)
            .flat_map(|g| &g.rows)
        {
            if matches!(row.target, Target::Desktop { .. }) {
                continue;
            }
            // An effort with nothing to offer shows n/a and is not written.
            if row.value == json!("n/a") {
                continue;
            }
            schema::set_in(&path, &local, &row.target, &row.value)
                .unwrap_or_else(|e| panic!("{:?}: {e}", row.target));
        }
        let reread = with_config_path(path.clone(), Config::load);
        assert_eq!(as_json(&reread), as_json(&original));
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            before_bytes,
            "no-op writes leave the file as it was"
        );
    }

    #[test]
    fn a_desktop_change_reads_back_in_the_tui_as_that_change_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let local = dir.path().join("config.local.json");
        std::fs::write(&path, STORED).unwrap();
        let mut expected = with_config_path(path.clone(), Config::load);

        let global = |key: &str| Target::Global { key: key.into() };
        schema::set_in(&path, &local, &global("kanban"), &json!(false)).unwrap();
        schema::set_in(
            &path,
            &local,
            &global("session_idle_timeout"),
            &json!("30m"),
        )
        .unwrap();
        schema::set_in(
            &path,
            &local,
            &Target::Agent {
                harness: "claude".into(),
                field: "model".into(),
            },
            &json!("opus"),
        )
        .unwrap();
        schema::set_in(
            &path,
            &local,
            &Target::Project {
                path: "/repo/demo".into(),
                field: "open_command".into(),
            },
            &json!("open http://localhost:3000"),
        )
        .unwrap();
        schema::set_in(
            &path,
            &local,
            &Target::Desktop {
                key: "theme".into(),
            },
            &json!("light"),
        )
        .unwrap();

        expected.kanban = false;
        expected.session_idle_timeout = "30m".into();
        expected.claude_model = "opus".into();
        expected.set_project_text(
            Path::new("/repo/demo"),
            SettingKind::OpenCommand,
            "open http://localhost:3000",
        );
        let reread = with_config_path(path.clone(), Config::load);
        assert_eq!(as_json(&reread), as_json(&expected));
        let raw: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(raw["future_key"], json!({"from": "a newer build"}));
        assert_eq!(raw["projects"]["/repo/demo"]["mystery"], 1);
        assert_eq!(raw["desktop"]["theme"], "light");
        assert_eq!(raw["theme"], "ocean", "the window's theme is not the TUI's");
    }
}
