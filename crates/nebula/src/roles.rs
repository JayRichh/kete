//! `kete roles` (K3): what each role card gives a session, and accepting
//! bypass mode for an account.

use crate::cli::RolesCommand;
use anyhow::{bail, Context, Result};
use nebula_core::roles::permissions::{self, ComposeCtx};
use nebula_core::roles::{self, Mcp, Permission, RoleCard, RolesFile};
use nebula_core::studio::autonomy::Autonomy;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub fn run(command: RolesCommand) -> Result<()> {
    match command {
        RolesCommand::Check {
            project,
            role,
            json,
        } => check(project.as_deref(), role.as_deref(), json),
        RolesCommand::AcceptBypass { profile } => accept_bypass(&profile),
    }
}

/// The flags a launch with `card` gets, in the order Kete passes them
/// (an explicit `--model` or `--effort` on the launch wins over the card's).
pub fn launch_flags(card: &RoleCard, settings: &Path) -> Vec<String> {
    let mut flags = vec!["--settings".to_string(), settings.display().to_string()];
    if let Some(model) = &card.model {
        flags.extend(["--model".into(), model.clone()]);
    }
    if let Some(effort) = &card.effort {
        flags.extend(["--effort".into(), effort.clone()]);
    }
    if let Some(mode) = card.permission.mode() {
        flags.extend(["--permission-mode".into(), mode.into()]);
    }
    if card.mcp == Mcp::None {
        flags.push("--strict-mcp-config".into());
    }
    if let Some(tools) = &card.tools {
        flags.extend(["--tools".into(), tools.join(",")]);
    }
    flags.push("--append-system-prompt".into());
    flags.push(match &card.prompt {
        Some(p) => format!("<kete guidance, then the role prompt: {} bytes>", p.len()),
        None => "<kete guidance>".into(),
    });
    flags
}

/// One role as `kete roles check` shows it, for a session in the root
/// worktree.
pub fn describe(
    file: &RolesFile,
    card: &RoleCard,
    root: &Path,
    autonomy: Option<Autonomy>,
) -> Value {
    let data_dir = nebula_core::paths::data_dir();
    let config_path = nebula_core::paths::config_path();
    let config_dir = config_path.parent().unwrap_or(&data_dir).to_path_buf();
    let settings_file = roles::session_dir("<agent id>").join(roles::SETTINGS_FILE);
    let rules = permissions::compose(&ComposeCtx {
        card: Some(card),
        governance: &file.governance,
        autonomy,
        worktree: root,
        root_worktree: root,
        data_dir: &data_dir,
        config_dir: &config_dir,
        settings_file: &settings_file,
    });
    let exe = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "kete".into());
    let status_line = std::fs::read_to_string(
        data_dir.join(nebula_daemon::hooks::installer::STATUSLINE_SETTINGS_FILE),
    )
    .ok()
    .and_then(|t| serde_json::from_str::<Value>(&t).ok())
    .and_then(|v| v.get("statusLine").cloned());
    let settings = permissions::settings_json(
        &rules,
        permissions::guard_hook(&exe, "<agent id>"),
        status_line,
        autonomy.map(|level| (root, level)),
    );
    json!({
        "id": card.id,
        "label": card.label,
        "permission": card.permission.as_str(),
        "tier": card.tier,
        "department": card.department,
        "context_budget": card.context_budget,
        "flags": launch_flags(card, &settings_file),
        "settings_file": settings_file.display().to_string(),
        "settings": settings,
    })
}

fn check(project: Option<&str>, role: Option<&str>, as_json: bool) -> Result<()> {
    let root = project_root(project)?;
    let file = roles::load_committed(&root)
        .map_err(anyhow::Error::msg)?
        .with_context(|| format!("{} has no committed {}", root.display(), roles::ROLES_PATH))?;
    let cards: Vec<&RoleCard> = match role {
        Some(id) => vec![file
            .roles
            .get(id)
            .with_context(|| format!("role `{id}` is not in {} at HEAD", roles::ROLES_PATH))?],
        None => file.roles.values().collect(),
    };
    let autonomy = nebula_core::studio::settings::for_repo(&root).map(|s| s.autonomy);
    let described: Vec<Value> = cards
        .iter()
        .map(|c| describe(&file, c, &root, autonomy))
        .collect();
    let unattended = cards.iter().any(|c| c.permission == Permission::Unattended);
    let accounts = accounts_bypass();
    if as_json {
        let out = json!({
            "repo": root.display().to_string(),
            "autonomy": autonomy.map(Autonomy::as_str),
            "governance": file.governance,
            "roles": described,
            "bypass_accepted": accounts.iter().map(|(n, ok)| json!({ "account": n, "accepted": ok })).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }
    println!(
        "{} at HEAD, autonomy {}",
        root.display(),
        autonomy.map_or("none (no studio entry)".to_string(), |a| a.to_string())
    );
    for role in &described {
        println!();
        println!(
            "{}  {}  {}  tier {}",
            role["id"].as_str().unwrap_or_default(),
            role["label"].as_str().unwrap_or_default(),
            role["permission"].as_str().unwrap_or_default(),
            role["tier"]
        );
        let flags: Vec<&str> = role["flags"]
            .as_array()
            .map(|a| a.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        println!("  flags: {}", flags.join(" "));
        println!(
            "  settings file {}:",
            role["settings_file"].as_str().unwrap_or_default()
        );
        for line in serde_json::to_string_pretty(&role["settings"])?.lines() {
            println!("    {line}");
        }
    }
    if unattended {
        println!();
        println!("Bypass mode accepted:");
        for (name, ok) in &accounts {
            println!("  {name}: {}", if *ok { "yes" } else { "no" });
        }
    }
    Ok(())
}

/// Each Claude account and whether it accepted bypass mode; Claude's own
/// login first, as `claude`.
fn accounts_bypass() -> Vec<(String, bool)> {
    let own = roles::bypass_settings(None).is_some_and(|p| roles::accepted_bypass(&p));
    let mut out = vec![("claude".to_string(), own)];
    for p in nebula_core::profiles::load() {
        if p.harness.trim() == "claude" {
            let ok = roles::bypass_settings(Some(&p)).is_some_and(|s| roles::accepted_bypass(&s));
            out.push((p.name.trim().to_string(), ok));
        }
    }
    out
}

/// The root worktree `project` names (an id, a name, or a path inside
/// one of its checkouts), or the one the current folder is in.
fn project_root(project: Option<&str>) -> Result<PathBuf> {
    let cwd = std::env::current_dir()?;
    let query = project
        .map(str::to_string)
        .unwrap_or_else(|| cwd.display().to_string());
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let (projects, worktrees) = rt.block_on(fetch_projects())?;
    let q = query.trim();
    if let Some(p) = projects
        .iter()
        .find(|p| p.id.0 == q || p.name.eq_ignore_ascii_case(q))
    {
        return Ok(p.repo_path.clone());
    }
    let want = normal(Path::new(q));
    let inside = |dir: &Path| {
        let dir = normal(dir);
        want == dir || want.starts_with(&format!("{dir}/"))
    };
    projects
        .iter()
        .find(|p| {
            inside(&p.repo_path)
                || worktrees
                    .iter()
                    .any(|w| w.project_id == p.id && inside(&w.path))
        })
        .map(|p| p.repo_path.clone())
        .with_context(|| {
            format!("no kete project named `{q}` or holding that path: `kete add` adds one")
        })
}

/// A path for comparing: canonical when it exists, `/` separators, and on
/// Windows case folded.
fn normal(p: &Path) -> String {
    let p = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let text = nebula_core::studio::settings::repo_key(&p);
    if cfg!(windows) {
        text.to_lowercase()
    } else {
        text
    }
}

async fn fetch_projects() -> Result<(Vec<nebula_core::Project>, Vec<nebula_core::Worktree>)> {
    use nebula_core::codec::{read_frame, write_frame};
    use nebula_core::{ClientRequest, ServerEvent};
    let mut conn = nebula_tui::ipc::connect_or_spawn().await?;
    write_frame(&mut conn.stream, &ClientRequest::Subscribe).await?;
    loop {
        match read_frame::<ServerEvent, _>(&mut conn.stream).await? {
            Some(ServerEvent::Snapshot {
                projects,
                worktrees,
                ..
            }) => return Ok((projects, worktrees)),
            Some(_) => continue,
            None => bail!("the daemon closed the connection before sending its projects"),
        }
    }
}

/// Run Claude in bypass mode under `name`'s account in this terminal, so
/// its one-time dialog is answered by the person at the keyboard.
fn accept_bypass(name: &str) -> Result<()> {
    if nebula_core::env::non_empty(nebula_core::env::AGENT_ID).is_some() {
        bail!("kete roles accept-bypass runs in your own terminal, not inside a kete session");
    }
    let all = nebula_core::profiles::load();
    let Some(profile) = all.iter().find(|p| p.name.trim() == name.trim()) else {
        bail!("no profile named `{name}`: kete profile list shows them");
    };
    if profile.harness.trim() != "claude" {
        bail!(
            "`{name}` is a {} account: bypass mode is Claude's",
            profile.harness
        );
    }
    let settings = roles::bypass_settings(Some(profile));
    if settings.as_deref().is_some_and(roles::accepted_bypass) {
        println!("account {name} has already accepted bypass mode");
        return Ok(());
    }
    let registry = nebula_tui::config::Config::load().harness_registry();
    let program = registry
        .iter()
        .find(|h| h.id == "claude")
        .map(|h| h.program.trim().to_string())
        .filter(|p| !p.is_empty())
        .unwrap_or_else(|| "claude".into());
    profile.prepare(None).context("prepare the profile home")?;
    let env = profile.launch_env();
    let args = vec!["--permission-mode".to_string(), "bypassPermissions".into()];
    eprintln!(
        "kete: starting {program} in bypass mode as `{name}`. Answer its dialog yourself, then type /exit."
    );
    let mut command = command_for(&program, &args)?;
    for var in env.removals() {
        command.env_remove(var);
    }
    command.envs(env.set.iter().map(|(k, v)| (k, v)));
    let status = command
        .status()
        .with_context(|| format!("run {program}: is it installed?"))?;
    if settings.as_deref().is_some_and(roles::accepted_bypass) {
        println!("account {name} accepted bypass mode: its unattended roles can run");
        Ok(())
    } else if status.success() {
        bail!("account {name} still has not accepted bypass mode")
    } else {
        bail!("{program} exited with {status}")
    }
}

/// `program args…` as a process: on Windows resolved through PATH and
/// PATHEXT, a `.cmd` or `.ps1` under its host, as a session spawn does.
#[cfg(windows)]
fn command_for(program: &str, args: &[String]) -> Result<std::process::Command> {
    let launch = nebula_core::shell::launch(program, args).map_err(anyhow::Error::msg)?;
    let mut command = std::process::Command::new(&launch.program);
    command
        .args(&launch.args)
        .envs(launch.env.iter().map(|(k, v)| (k, v)));
    Ok(command)
}

#[cfg(unix)]
fn command_for(program: &str, args: &[String]) -> Result<std::process::Command> {
    let mut command = std::process::Command::new(program);
    command.args(args);
    Ok(command)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_follow_the_launch_order() {
        let file = roles::parse(
            "version = 1\n[roles.runner]\nmodel = \"haiku\"\neffort = \"low\"\nprompt = \"go\"\ntools = [\"Read\", \"Bash\"]\nmcp = \"none\"\npermission = \"allowlist\"\n",
        )
        .unwrap();
        let flags = launch_flags(&file.roles["runner"], Path::new("s.json"));
        assert_eq!(
            flags,
            [
                "--settings",
                "s.json",
                "--model",
                "haiku",
                "--effort",
                "low",
                "--permission-mode",
                "dontAsk",
                "--strict-mcp-config",
                "--tools",
                "Read,Bash",
                "--append-system-prompt",
                "<kete guidance, then the role prompt: 2 bytes>",
            ]
        );
        let bare = roles::parse("version = 1\n[roles.x]\n").unwrap();
        assert_eq!(
            launch_flags(&bare.roles["x"], Path::new("s.json")),
            [
                "--settings",
                "s.json",
                "--append-system-prompt",
                "<kete guidance>"
            ]
        );
    }

    #[test]
    fn a_description_holds_the_settings_file_as_written() {
        let file = roles::parse(
            "version = 1\ngovernance = [\"DECISIONS/**\"]\n[roles.lead]\npermission = \"unattended\"\nallow = [\"Read\"]\n",
        )
        .unwrap();
        let root = Path::new("C:/Code/game");
        let v = describe(&file, &file.roles["lead"], root, Some(Autonomy::A2));
        assert_eq!(v["permission"], "unattended");
        let deny = v["settings"]["permissions"]["deny"].as_array().unwrap();
        assert!(deny.contains(&json!("Edit(//c/Code/game/DECISIONS/**)")));
        assert!(deny.contains(&json!("Bash(git tag *)")));
        assert!(deny.contains(&json!("AskUserQuestion")));
        assert_eq!(v["settings"]["permissions"]["allow"], json!([]));
        assert_eq!(v["settings"]["env"][permissions::ENV_STUDIO_AUTONOMY], "A2");
    }
}
