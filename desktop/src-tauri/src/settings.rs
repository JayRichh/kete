//! Settings: the same config.json the TUI reads, through `nebula-config`.
//! The window renders what [`schema::export`] lays out and sends each
//! change back as one [`schema::Target`]; a write touches only the keys it
//! changes, so a running TUI and this window keep each other's edits. The
//! window's own preferences live under the `desktop` object.

use crate::daemon::{Client, Model};
use crate::Res;
use nebula_config::actions;
use nebula_config::desktop::{DesktopPrefs, SIDEBAR_MAX, SIDEBAR_MIN};
use nebula_config::schema::{self, ProjectRef, Target};
use nebula_config::Config;
use nebula_core::project_file::{self, ProjectCommand};
use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::Arc;
use tauri::State;

fn projects(model: &Model) -> Vec<ProjectRef> {
    let mut list: Vec<_> = model.projects.iter().collect();
    list.sort_by_key(|p| p.sort_order);
    list.into_iter()
        .map(|p| ProjectRef {
            name: p.name.clone(),
            path: p.repo_path.clone(),
        })
        .collect()
}

/// Every tab and row, with the values stored now.
#[tauri::command]
pub fn settings_schema(client: State<'_, Arc<Client>>) -> schema::Schema {
    // `Config::load` also brings the Claude model list up to a hand-edited
    // `claude_models`, which the Agents tab's rows read.
    let _ = Config::load();
    let (path, local) = nebula_config::layer_paths();
    schema_at(&path, &local, &projects(&client.snapshot()))
}

/// The schema of the layers at `path` / `local`, with why they can't be
/// written, if they can't.
pub fn schema_at(
    path: &std::path::Path,
    local: &std::path::Path,
    projects: &[ProjectRef],
) -> schema::Schema {
    let mut out = schema::export(
        &Config::load_at(path, local),
        &DesktopPrefs::load_at(path, local),
        projects,
    );
    out.problem = nebula_config::write_blocker(path, local);
    out
}

/// Why the settings files can't be written (one that can't be parsed),
/// for the window to say at startup; None when they read.
#[tauri::command]
pub fn config_problem() -> Option<String> {
    let (path, local) = nebula_config::layer_paths();
    nebula_config::write_blocker(&path, &local)
}

/// Write one row.
#[tauri::command]
pub fn settings_set(
    client: State<'_, Arc<Client>>,
    target: Target,
    value: serde_json::Value,
) -> Res<()> {
    let target = schema::resolve_target(&target, &projects(&client.snapshot()))?;
    let (path, local) = nebula_config::layer_paths();
    schema::set_in(&path, &local, &target, &value)
}

/// Put every row the window shows back to its default (see
/// [`schema::reset_config`] for what stays).
#[tauri::command]
pub fn settings_reset() -> Res<()> {
    let (path, local) = nebula_config::layer_paths();
    schema::reset_in(&path, &local)
}

/// The window's own preferences, read at startup.
#[tauri::command]
pub fn desktop_prefs() -> DesktopPrefs {
    DesktopPrefs::load()
}

/// The verb registry, and the window's own key overrides from
/// config.json's `keybindings` (key id to chords; an empty list is a
/// deliberate unbind). The window's defaults are compiled in from
/// `actions.gen.ts`; this is what the user changed.
#[derive(Debug, Serialize)]
pub struct ActionsList {
    pub verbs: Vec<actions::Verb>,
    pub overrides: BTreeMap<String, Vec<String>>,
}

#[tauri::command]
pub fn actions_list() -> ActionsList {
    let (path, local) = nebula_config::layer_paths();
    actions_list_at(&path, &local)
}

pub fn actions_list_at(path: &std::path::Path, local: &std::path::Path) -> ActionsList {
    ActionsList {
        verbs: actions::verbs(),
        overrides: actions::desktop_overrides(&Config::load_at(path, local).keybindings),
    }
}

/// Bind one of the window's keys (`chords`, empty to unbind) or put it
/// back to its default (None). Writes only `keybindings`, and only the
/// window's entries in it: the terminal app's stay as they are.
#[tauri::command]
pub fn desktop_key_set(
    id: String,
    chords: Option<Vec<String>>,
) -> Res<BTreeMap<String, Vec<String>>> {
    let (path, local) = nebula_config::layer_paths();
    desktop_key_set_at(&path, &local, &id, chords.as_deref())
}

pub fn desktop_key_set_at(
    path: &std::path::Path,
    local: &std::path::Path,
    id: &str,
    chords: Option<&[String]>,
) -> Res<BTreeMap<String, Vec<String>>> {
    // Check it on what is stored first, so a refused one writes nothing.
    let mut trial = Config::load_at(path, local).keybindings;
    actions::set_desktop_override(&mut trial, id, chords)?;
    let mut outcome = Ok(());
    let cfg = Config::update_in(path, local, |cfg| {
        outcome = actions::set_desktop_override(&mut cfg.keybindings, id, chords)
    })
    .map_err(|e| e.to_string())?;
    outcome?;
    Ok(actions::desktop_overrides(&cfg.keybindings))
}

/// The overview groups the user has folded, saved whole.
#[tauri::command]
pub fn desktop_set_collapsed(keys: Vec<String>) -> Res<()> {
    DesktopPrefs::update(|p| {
        p.collapsed = keys;
        Ok(())
    })
    .map(|_| ())
}

/// The sidebar's width and fold, saved together. A width outside the
/// limits is clamped; 0 is the default width.
#[tauri::command]
pub fn desktop_set_sidebar(width: u16, collapsed: bool) -> Res<()> {
    DesktopPrefs::update(|p| {
        p.sidebar_width = if width == 0 {
            0
        } else {
            width.clamp(SIDEBAR_MIN, SIDEBAR_MAX)
        };
        p.sidebar_collapsed = collapsed;
        Ok(())
    })
    .map(|_| ())
}

/// The run command each worktree's Run button would start, by worktree
/// id: the project's own `run_command` setting, else the checkout's
/// `.kete.json` `run` — the order the daemon's StartRun reads them in.
/// Worktrees with neither are left out.
#[tauri::command]
pub fn run_commands(client: State<'_, Arc<Client>>) -> BTreeMap<String, String> {
    run_commands_for(&client.snapshot(), &Config::load())
}

pub fn run_commands_for(model: &Model, cfg: &Config) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for w in &model.worktrees {
        let main = model
            .projects
            .iter()
            .find(|p| p.id == w.project_id)
            .map_or(w.path.as_path(), |p| p.repo_path.as_path());
        let configured = cfg.project(main).run_command.trim().to_string();
        let command = if configured.is_empty() {
            project_file::lookup(&w.path, main, ProjectCommand::Run)
                .ok()
                .flatten()
        } else {
            Some(configured)
        };
        if let Some(command) = command.filter(|c| !c.trim().is_empty()) {
            out.insert(w.id.0.clone(), command);
        }
    }
    out
}

/// A harness the quick prompt offers.
#[derive(Debug, Serialize, PartialEq)]
pub struct HarnessInfo {
    pub id: String,
    pub label: String,
    /// The model and effort pickers' lists and defaults, as the Agents tab.
    pub choices: schema::LaunchChoices,
}

/// What the New session picker offers, as the TUI's: enabled, valid
/// entries of the effective registry (built-ins with the `harnesses` map
/// and legacy keys applied, then custom harnesses), less any missing from
/// PATH while `hide_uninstalled_harnesses` is on.
pub fn offered(cfg: &Config) -> Vec<HarnessInfo> {
    cfg.offered_harnesses()
        .into_iter()
        .map(|(kind, custom)| {
            let descriptor = cfg.effective_harness(kind, custom.as_deref());
            HarnessInfo {
                choices: schema::launch_choices(cfg, &descriptor.id),
                id: descriptor.id.clone(),
                label: descriptor.display_label().to_string(),
            }
        })
        .collect()
}

/// A create's harness id as the daemon wants it: a built-in kind, or
/// `Custom` with the entry's id.
pub fn resolve_kind(cfg: &Config, id: &str) -> Res<(nebula_core::AgentKind, Option<String>)> {
    use nebula_core::AgentKind;
    if let Some(kind) = AgentKind::parse(id).filter(|k| *k != AgentKind::Custom) {
        return Ok((kind, None));
    }
    if cfg.harness_registry().iter().any(|h| h.id == id) {
        return Ok((AgentKind::Custom, Some(id.to_string())));
    }
    Err(format!("unknown harness {id}"))
}

/// The model and effort a create passes for `id`, as the TUI resolves
/// them: what the caller picked, else the Agents tab's defaults ("default"
/// = no flag).
pub fn launch_args(
    cfg: &Config,
    id: &str,
    model: Option<String>,
    effort: Option<String>,
) -> Res<LaunchArgs> {
    let (kind, custom) = resolve_kind(cfg, id)?;
    let (model, effort) = cfg.launch_model_effort(kind, custom.as_deref(), model, effort);
    Ok(LaunchArgs {
        kind,
        custom,
        model,
        effort,
    })
}

#[derive(Debug, PartialEq)]
pub struct LaunchArgs {
    pub kind: nebula_core::AgentKind,
    pub custom: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use nebula_core::{AgentKind, Project, ProjectId, Worktree, WorktreeId};
    use serde_json::json;
    use std::path::Path;

    fn cfg_from(json: serde_json::Value) -> (tempfile::TempDir, Config) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(&path, json.to_string()).unwrap();
        let cfg = Config::load_at(&path, &dir.path().join("config.local.json"));
        (dir, cfg)
    }

    #[test]
    fn a_settings_file_that_cannot_be_parsed_is_named_and_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let local = dir.path().join("config.local.json");
        let broken = "\u{feff}{\"theme\": \"rose\",}";
        std::fs::write(&path, broken).unwrap();
        let out = schema_at(&path, &local, &[]);
        assert!(out.problem.as_deref().unwrap().contains("can't be parsed"));
        let target = Target::Global {
            key: "kanban".into(),
        };
        let err = schema::set_in(&path, &local, &target, &serde_json::json!(true)).unwrap_err();
        assert!(err.contains("nothing was written"), "{err}");
        assert!(schema::reset_in(&path, &local).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), broken);

        std::fs::write(&path, "\u{feff}{\"kanban\": false}").unwrap();
        assert_eq!(schema_at(&path, &local, &[]).problem, None);
        schema::set_in(&path, &local, &target, &serde_json::json!(true)).unwrap();
    }

    #[test]
    fn a_create_passes_the_configured_model_and_effort() {
        let (_d, cfg) = cfg_from(json!({"claude_model": "opus", "claude_effort": "high"}));
        let args = launch_args(&cfg, "claude", None, None).unwrap();
        assert_eq!(args.kind, AgentKind::Claude);
        assert_eq!(args.model.as_deref(), Some("opus"));
        assert_eq!(args.effort.as_deref(), Some("high"));
        // "default" is no flag at all.
        let (_d, cfg) = cfg_from(json!({}));
        let args = launch_args(&cfg, "codex", None, None).unwrap();
        assert_eq!((args.model, args.effort), (None, None));
        // A pick beats the default.
        let (_d, cfg) = cfg_from(json!({"claude_model": "opus"}));
        let args = launch_args(&cfg, "claude", Some("sonnet".into()), None).unwrap();
        assert_eq!(args.model.as_deref(), Some("sonnet"));
    }

    #[test]
    fn a_custom_harness_launches_as_custom_with_its_id() {
        let (_d, cfg) = cfg_from(json!({"custom_harnesses": [
            {"id": "aider", "program": "aider", "model": "gpt-x", "model_flag": "--model"}
        ]}));
        let args = launch_args(&cfg, "aider", None, None).unwrap();
        assert_eq!(args.kind, AgentKind::Custom);
        assert_eq!(args.custom.as_deref(), Some("aider"));
        assert_eq!(args.model.as_deref(), Some("gpt-x"));
        assert!(launch_args(&cfg, "nope", None, None).is_err());
    }

    #[test]
    fn the_offered_harnesses_follow_the_agents_tab() {
        let (_d, cfg) = cfg_from(json!({
            "codex_enabled": false,
            "harnesses": {"cursor": {"enabled": false, "label": "Cursor CLI"}},
            "custom_harnesses": [{"id": "aider", "label": "Aider", "program": "aider", "model": "default", "model_flag": "--model"}]
        }));
        let ids: Vec<String> = offered(&cfg).into_iter().map(|h| h.id).collect();
        assert!(ids.contains(&"claude".to_string()));
        assert!(!ids.contains(&"codex".to_string()), "{ids:?}");
        assert!(!ids.contains(&"cursor".to_string()), "{ids:?}");
        assert!(ids.contains(&"aider".to_string()), "{ids:?}");
    }

    #[test]
    fn each_offered_harness_carries_its_model_and_effort_choices() {
        let (_d, cfg) = cfg_from(json!({"claude_model": "opus", "claude_effort": "high"}));
        let claude = offered(&cfg)
            .into_iter()
            .find(|h| h.id == "claude")
            .unwrap();
        assert_eq!(claude.choices.model, "opus");
        assert_eq!(claude.choices.effort, "high");
        assert_eq!(claude.choices.models[0], "default");
        assert!(claude.choices.efforts["opus"].contains(&"high".to_string()));
    }

    #[test]
    fn hide_missing_clis_drops_a_harness_whose_program_is_nowhere() {
        let (_d, cfg) = cfg_from(json!({
            "hide_uninstalled_harnesses": true,
            "custom_harnesses": [{"id": "ghost", "program": "kete-no-such-cli-xyz", "model": "default", "model_flag": "--model"}]
        }));
        let ids: Vec<String> = offered(&cfg).into_iter().map(|h| h.id).collect();
        assert!(!ids.contains(&"ghost".to_string()), "{ids:?}");
    }

    #[test]
    fn a_run_command_comes_from_the_setting_then_the_project_file() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        let wt = dir.path().join("repo-wt");
        let bare = dir.path().join("bare");
        for d in [&repo, &wt, &bare] {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(wt.join(".kete.json"), r#"{"run": "npm run dev"}"#).unwrap();
        let model = Model {
            projects: vec![
                Project {
                    id: ProjectId("p1".into()),
                    name: "repo".into(),
                    repo_path: repo.clone(),
                    sort_order: 0,
                },
                Project {
                    id: ProjectId("p2".into()),
                    name: "bare".into(),
                    repo_path: bare.clone(),
                    sort_order: 1,
                },
            ],
            worktrees: vec![
                worktree("w1", "p1", &repo),
                worktree("w2", "p1", &wt),
                worktree("w3", "p2", &bare),
            ],
            ..Model::default()
        };
        let none = run_commands_for(&model, &Config::default());
        assert_eq!(none.get("w2").map(String::as_str), Some("npm run dev"));
        assert!(!none.contains_key("w1") && !none.contains_key("w3"));

        let mut cfg = Config::default();
        cfg.set_project_text(&repo, nebula_config::SettingKind::RunCommand, "cargo run");
        let set = run_commands_for(&model, &cfg);
        assert_eq!(set.get("w1").map(String::as_str), Some("cargo run"));
        assert_eq!(set.get("w2").map(String::as_str), Some("cargo run"));
    }

    fn worktree(id: &str, project: &str, path: &Path) -> Worktree {
        Worktree {
            id: WorktreeId(id.into()),
            project_id: ProjectId(project.into()),
            path: path.to_path_buf(),
            branch: "main".into(),
            is_main: true,
            sort_order: 0,
        }
    }

    #[test]
    fn a_desktop_key_writes_beside_the_terminal_apps() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let local = dir.path().join("config.local.json");
        std::fs::write(
            &path,
            json!({"keybindings": {"quit": "ctrl+q"}, "theme": "x"}).to_string(),
        )
        .unwrap();
        let over = desktop_key_set_at(&path, &local, "palette", Some(&["ctrl+j".into()])).unwrap();
        assert_eq!(over["palette"], vec!["ctrl+j"]);
        assert!(desktop_key_set_at(&path, &local, "list_move", Some(&["x".into()])).is_err());
        let stored: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(stored["keybindings"]["quit"], "ctrl+q");
        assert_eq!(stored["keybindings"]["desktop.palette"], "ctrl+j");
        assert_eq!(
            actions_list_at(&path, &local).overrides["palette"],
            vec!["ctrl+j"]
        );
        desktop_key_set_at(&path, &local, "palette", None).unwrap();
        assert!(actions_list_at(&path, &local).overrides.is_empty());
    }
}

/// The demo's settings: what the screenshot harness (`desktop/demo`)
/// renders, exported from the real schema so the screenshots show the
/// real rows. Run with `cargo test -- --ignored write_demo_settings`.
#[cfg(test)]
mod demo {
    use nebula_config::desktop::DesktopPrefs;
    use nebula_config::schema::{export, ProjectRef};
    use nebula_config::Config;

    #[test]
    #[ignore = "writes desktop/demo/settings-schema.json"]
    fn write_demo_settings() {
        let mut cfg = Config {
            claude_model: "opus".into(),
            claude_effort: "high".into(),
            session_idle_timeout: "15m".into(),
            ..Config::default()
        };
        cfg.set_project_text(
            std::path::Path::new(r"C:\demo\ledger-api"),
            nebula_config::SettingKind::RunCommand,
            "dotnet watch run",
        );
        let projects = [
            ProjectRef {
                name: "ledger-api".into(),
                path: r"C:\demo\ledger-api".into(),
            },
            ProjectRef {
                name: "storefront-web".into(),
                path: r"C:\demo\storefront-web".into(),
            },
        ];
        let schema = export(&cfg, &DesktopPrefs::default(), &projects);
        let out = concat!(env!("CARGO_MANIFEST_DIR"), "/../demo/settings-schema.json");
        let mut json = serde_json::to_string_pretty(&schema).unwrap();
        json.push('\n');
        std::fs::write(out, json).unwrap();
    }
}
