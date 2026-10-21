//! ROLE CARDS (K3, and K9's permission profiles): the launch overlay a role
//! session gets, its settings file and guard, and `SessionRoles`.
//!
//! Two kinds of session get an overlay: a role session (any profile), and,
//! in a repo with a `studio` entry, a session its orchestrator spawned
//! without a role (the `prompt` profile with no role rules). Every other
//! launch gets an empty one, so its argv is 0.59's.
//!
//! The decision is made once, on the session's first spawn, and kept as
//! its folder `<data dir>/sessions/<agent id>/`: a later resume keeps its
//! restrictions even after the orchestrator that spawned it is gone.

use super::*;
use crate::launch_overlay::LaunchOverlay;
use nebula_core::roles::permissions::{self, ComposeCtx};
use nebula_core::roles::{
    accepted_bypass, bypass_refusal, bypass_settings, Mcp, Permission, RoleCard, RolesFile,
    SETTINGS_FILE,
};
use nebula_core::studio::autonomy::Autonomy;
use nebula_core::studio::wire::SessionRole;

/// The role it launched with, for `SessionRoles` after a restart.
const ROLE_FILE: &str = "role.json";

/// Parsed role cards per project, keyed by the root worktree's HEAD, on
/// `Daemon::roles`, and each role session's `SessionRole`.
#[derive(Debug, Default)]
pub(crate) struct RolesBook {
    files: HashMap<PathBuf, (String, Arc<RolesFile>)>,
    sessions: HashMap<AgentId, SessionRole>,
}

fn session_dir(agent: &AgentId) -> PathBuf {
    nebula_core::roles::session_dir(&agent.to_string())
}

fn role_of(agent: &AgentId, card: &RoleCard) -> SessionRole {
    SessionRole {
        agent: agent.clone(),
        card: card.id.clone(),
        label: card.label.clone(),
        tier: card.tier,
        department: card.department.clone(),
        permission: card.permission.as_str().into(),
        context_budget: card.context_budget,
    }
}

impl Daemon {
    /// What `agent`'s launch adds (flags, prompt, settings file), built on
    /// every spawn and resume (`registry/spawn.rs`). `Err` refuses the
    /// spawn with that reason. The default adds nothing.
    pub fn launch_overlay(
        &self,
        agent: &Agent,
        project: &Project,
    ) -> std::result::Result<LaunchOverlay, String> {
        // A command override (tests) replaces the CLI's argv verbatim and
        // takes no `--settings`: there is nothing to lay an overlay on.
        if std::env::var_os(env::AGENT_CMD).is_some() {
            return Ok(LaunchOverlay::default());
        }
        self.overlay_for(agent, project)
    }

    pub(crate) fn overlay_for(
        &self,
        agent: &Agent,
        project: &Project,
    ) -> std::result::Result<LaunchOverlay, String> {
        let card_id = self.role_card_of(&agent.id);
        let dir = session_dir(&agent.id);
        let studio = nebula_core::studio::settings::for_repo(&project.repo_path);
        if card_id.is_none()
            && !dir.exists()
            && !(studio.is_some() && self.spawned_by_orchestrator(agent))
        {
            return Ok(LaunchOverlay::default());
        }
        let result = self.restricted_overlay(
            agent,
            project,
            card_id.as_deref(),
            studio.map(|s| s.autonomy),
            &dir,
        );
        if let (Err(reason), Some(card)) = (&result, &card_id) {
            let refusal = nebula_core::studio::refusals::Refusal {
                at: nebula_core::clock::now_ms(),
                project: nebula_core::studio::settings::repo_key(&project.repo_path),
                agent: Some(agent.id.to_string()),
                role: Some(card.clone()),
                reason: reason.clone(),
            };
            if let Err(e) = nebula_core::studio::refusals::append(&refusal) {
                tracing::warn!(error = %e, "recording a role refusal failed");
            }
        }
        result
    }

    /// Whether the session that asked for `agent` is the orchestrator.
    fn spawned_by_orchestrator(&self, agent: &Agent) -> bool {
        agent
            .parent
            .as_ref()
            .and_then(|p| self.store.get_agent(p).ok().flatten())
            .is_some_and(|p| p.role.is_orchestrator())
    }

    fn restricted_overlay(
        &self,
        agent: &Agent,
        project: &Project,
        card_id: Option<&str>,
        autonomy: Option<Autonomy>,
        dir: &Path,
    ) -> std::result::Result<LaunchOverlay, String> {
        if agent.kind != AgentKind::Claude {
            return Err(format!(
                "role cards apply to Claude sessions only: this session runs {}",
                agent.kind.as_str()
            ));
        }
        // A permissions flag on the Claude harness (a user override) rides
        // every launch ahead of `--permission-mode`, and would skip every
        // prompt on an account that never accepted bypass mode.
        let harness = super::resolve_harness(agent.kind, agent.custom_harness.as_deref())
            .map_err(|e| e.to_string())?;
        if let Some(flag) = harness
            .permissions_flag
            .as_deref()
            .filter(|f| !f.trim().is_empty())
        {
            return Err(format!(
                "the {} harness passes `{flag}`, which skips Claude's permission checks: clear its permissions flag in Settings to launch role or studio sessions",
                harness.id
            ));
        }
        let root = &project.repo_path;
        let file = self.roles_file(root)?;
        let card = match card_id {
            Some(id) => Some(
                file.as_ref()
                    .and_then(|f| f.roles.get(id))
                    .cloned()
                    .ok_or_else(|| match &file {
                        Some(_) => format!(
                            "role `{id}` is not in {} at HEAD",
                            nebula_core::roles::ROLES_PATH
                        ),
                        None => format!(
                            "role `{id}`: {} has no committed {}",
                            project.name,
                            nebula_core::roles::ROLES_PATH
                        ),
                    })?,
            ),
            None => None,
        };
        if card
            .as_ref()
            .is_some_and(|c| c.permission == Permission::Unattended)
        {
            let profiles = nebula_core::profiles::load();
            let profile = agent
                .profile
                .as_deref()
                .and_then(|name| profiles.iter().find(|p| p.name.trim() == name));
            let accepted = bypass_settings(profile).is_some_and(|s| accepted_bypass(&s));
            if !accepted {
                return Err(bypass_refusal(agent.profile.as_deref()));
            }
        }
        let worktree = self
            .store
            .get_worktree(&agent.worktree_id)
            .map_err(|e| e.to_string())?
            .map(|w| w.path)
            .unwrap_or_else(|| root.clone());
        let data_dir = nebula_core::paths::data_dir();
        let config_path = nebula_core::paths::config_path();
        let config_dir = config_path.parent().unwrap_or(&data_dir).to_path_buf();
        let settings_file = dir.join(SETTINGS_FILE);
        let governance = file
            .as_ref()
            .map(|f| f.governance.clone())
            .unwrap_or_default();
        let rules = permissions::compose(&ComposeCtx {
            card: card.as_ref(),
            governance: &governance,
            autonomy,
            worktree: &worktree,
            root_worktree: root,
            data_dir: &data_dir,
            config_dir: &config_dir,
            settings_file: &settings_file,
        });
        let status_line = super::spawn::statusline_settings()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
            .and_then(|v| v.get("statusLine").cloned());
        let exe = std::env::current_exe()
            .map(|p| {
                let path = p.display().to_string();
                path.strip_prefix(r"\\?\").unwrap_or(&path).to_string()
            })
            .unwrap_or_else(|_| "kete".into());
        let settings = permissions::settings_json(
            &rules,
            permissions::guard_hook(&exe, &agent.id.to_string()),
            status_line,
            autonomy.map(|level| (root.as_path(), level)),
        );
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("could not create {}: {e}", dir.display()))?;
        let text = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?;
        nebula_core::settings::write_atomic(&settings_file, text.as_bytes())
            .map_err(|e| format!("could not write {}: {e}", settings_file.display()))?;
        if let Some(card) = &card {
            let role = role_of(&agent.id, card);
            if let Ok(text) = serde_json::to_string(&role) {
                let _ = nebula_core::settings::write_atomic(&dir.join(ROLE_FILE), text.as_bytes());
            }
            self.roles.lock().sessions.insert(agent.id.clone(), role);
        }
        Ok(LaunchOverlay {
            append_prompt: card.as_ref().and_then(|c| c.prompt.clone()),
            tools: card.as_ref().and_then(|c| c.tools.clone()),
            permission_mode: rules.mode.map(str::to_string),
            settings_path: Some(settings_file),
            strict_mcp: card.as_ref().is_some_and(|c| c.mcp == Mcp::None),
            model: card.as_ref().and_then(|c| c.model.clone()),
            effort: card.as_ref().and_then(|c| c.effort.clone()),
        })
    }

    /// The roles file at `root`'s HEAD, re-read only when HEAD moved.
    fn roles_file(&self, root: &Path) -> std::result::Result<Option<Arc<RolesFile>>, String> {
        let head = nebula_core::roles::head(root);
        if let Some(head) = &head {
            if let Some((at, file)) = self.roles.lock().files.get(root) {
                if at == head {
                    return Ok(Some(file.clone()));
                }
            }
        }
        let Some(file) = nebula_core::roles::load_committed(root)? else {
            self.roles.lock().files.remove(root);
            return Ok(None);
        };
        let file = Arc::new(file);
        if let Some(head) = head {
            self.roles
                .lock()
                .files
                .insert(root.to_path_buf(), (head, file.clone()));
        }
        Ok(Some(file))
    }

    /// Every live role session, for `ServerEvent::SessionRoles`. Never runs
    /// git: a session not launched since this daemon started reads the
    /// role its last launch wrote.
    pub fn session_roles(&self) -> Vec<SessionRole> {
        let Ok(cards) = self.store.agent_role_cards() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for (id, card) in cards {
            if !self
                .store
                .get_agent(&id)
                .ok()
                .flatten()
                .is_some_and(|a| !a.archived)
            {
                continue;
            }
            let known = self.roles.lock().sessions.get(&id).cloned();
            let role = known
                .or_else(|| {
                    let text = std::fs::read_to_string(session_dir(&id).join(ROLE_FILE)).ok()?;
                    let role: SessionRole = serde_json::from_str(&text).ok()?;
                    self.roles.lock().sessions.insert(id.clone(), role.clone());
                    Some(role)
                })
                .unwrap_or(SessionRole {
                    agent: id.clone(),
                    label: card.clone(),
                    card,
                    tier: nebula_core::roles::card::DEFAULT_TIER,
                    department: None,
                    permission: Permission::Prompt.as_str().into(),
                    context_budget: None,
                });
            out.push(role);
        }
        out
    }

    /// The role card `agent` launched with (migration 36).
    pub fn role_card_of(&self, agent: &AgentId) -> Option<String> {
        self.store
            .agent_role_cards()
            .ok()?
            .into_iter()
            .find(|(id, _)| id == agent)
            .map(|(_, card)| card)
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::EnvGuard;
    use super::*;
    use nebula_core::AgentRole;
    use std::process::Command;

    fn git(dir: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .output()
            .unwrap()
            .status
            .success();
        assert!(ok, "git {args:?}");
    }

    const ROLES: &str = r#"version = 1
governance = ["DECISIONS/**"]
[roles.runner]
label = "Runner"
model = "haiku"
effort = "low"
tier = 3
prompt = "Run the checks."
tools = ["Read", "Write", "Bash"]
allow = ["Bash(npm test)"]
mcp = "none"
permission = "allowlist"
[roles.lead]
permission = "unattended"
"#;

    struct Fixture {
        daemon: Arc<Daemon>,
        project: Project,
        worktree: Worktree,
        data: tempfile::TempDir,
        _repo: tempfile::TempDir,
        _env: EnvGuard,
    }

    /// A daemon on a file database in a fresh data dir, and a committed
    /// repo holding `roles` (when given).
    fn fixture(roles: Option<&str>) -> Fixture {
        let data = tempfile::tempdir().unwrap();
        let env = EnvGuard::set_all(&[
            (env::DATA_DIR, data.path().to_str().unwrap()),
            (env::CONFIG_FILE, ""),
        ]);
        let repo = tempfile::tempdir().unwrap();
        git(repo.path(), &["init", "-q"]);
        std::fs::write(repo.path().join("README.md"), "x").unwrap();
        if let Some(roles) = roles {
            std::fs::create_dir_all(repo.path().join(".kete")).unwrap();
            std::fs::write(repo.path().join(".kete/roles.toml"), roles).unwrap();
        }
        git(repo.path(), &["add", "-A"]);
        git(repo.path(), &["commit", "-qm", "one"]);
        let daemon = open(data.path());
        let project = Project {
            id: ProjectId::generate(),
            name: "game".into(),
            repo_path: repo.path().to_path_buf(),
            sort_order: 0,
        };
        daemon.store.insert_project(&project).unwrap();
        let worktree = Worktree {
            id: WorktreeId::generate(),
            project_id: project.id.clone(),
            path: repo.path().to_path_buf(),
            branch: "main".into(),
            is_main: true,
            sort_order: 0,
        };
        daemon.store.insert_worktree(&worktree).unwrap();
        Fixture {
            daemon,
            project,
            worktree,
            data,
            _repo: repo,
            _env: env,
        }
    }

    fn open(data: &Path) -> Arc<Daemon> {
        Daemon::new(
            Arc::new(Store::open(&data.join("nebula.db")).unwrap()),
            HookEnv {
                port: 0,
                token: String::new(),
            },
        )
    }

    /// A stored session in the fixture's worktree.
    fn agent_with(
        f: &Fixture,
        id: &str,
        card: Option<&str>,
        role: AgentRole,
        parent: Option<&str>,
    ) -> Agent {
        let agent = Agent {
            parent: parent.map(|p| AgentId(p.into())),
            profile: None,
            id: AgentId(id.into()),
            worktree_id: f.worktree.id.clone(),
            name: id.into(),
            status: AgentStatus::Fresh,
            archived: false,
            archived_at: 0,
            unseen: false,
            kind: AgentKind::Claude,
            custom_harness: None,
            model: None,
            effort: None,
            session_id: None,
            cloud_session_id: None,
            sort_order: 0,
            status_changed_at: 0,
            alive: false,
            issue_url: None,
            role,
            recent_prompts: Vec::new(),
        };
        f.daemon.store.insert_agent(&agent).unwrap();
        f.daemon.store.set_agent_role_card(&agent.id, card).unwrap();
        agent
    }

    fn agent(f: &Fixture, id: &str, card: Option<&str>) -> Agent {
        agent_with(f, id, card, AgentRole::Worker, None)
    }

    fn settings(path: &Path) -> serde_json::Value {
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn a_session_without_a_role_gets_nothing() {
        let f = fixture(Some("version = 1\n"));
        let a = agent(&f, "plain", None);
        assert_eq!(
            f.daemon.overlay_for(&a, &f.project),
            Ok(LaunchOverlay::default())
        );
        assert!(!session_dir(&a.id).exists());
        assert!(f.daemon.session_roles().is_empty());
    }

    #[test]
    fn a_role_session_gets_its_flags_prompt_and_settings_file() {
        let f = fixture(Some(ROLES));
        let a = agent(&f, "r1", Some("runner"));
        let overlay = f.daemon.overlay_for(&a, &f.project).unwrap();
        let path = session_dir(&a.id).join(SETTINGS_FILE);
        assert_eq!(
            overlay,
            LaunchOverlay {
                append_prompt: Some("Run the checks.".into()),
                tools: Some(vec!["Read".into(), "Write".into(), "Bash".into()]),
                permission_mode: Some("dontAsk".into()),
                settings_path: Some(path.clone()),
                strict_mcp: true,
                model: Some("haiku".into()),
                effort: Some("low".into()),
            }
        );
        let v = settings(&path);
        let deny: Vec<&str> = v["permissions"]["deny"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|r| r.as_str())
            .collect();
        assert!(deny.contains(&"Bash(git push --force*)"));
        assert!(deny.contains(&"PowerShell(git push * +*)"));
        assert!(deny.contains(&"AskUserQuestion"));
        let gov = format!(
            "Edit({}/DECISIONS/**)",
            permissions::posix_rule_path(&f.project.repo_path)
        );
        assert!(deny.contains(&gov.as_str()), "{deny:?}");
        assert_eq!(
            v["permissions"]["allow"],
            serde_json::json!(["Bash(npm test)"])
        );
        assert_eq!(v["hooks"]["PreToolUse"][0]["matcher"], "Bash|PowerShell");
        assert!(v.get("env").is_none(), "no studio entry, no level");
        let roles = f.daemon.session_roles();
        assert_eq!(roles.len(), 1);
        assert_eq!((roles[0].label.as_str(), roles[0].tier), ("Runner", 3));
        assert_eq!(roles[0].permission, "allowlist");
    }

    #[test]
    fn roles_come_from_head_not_the_working_copy() {
        let f = fixture(Some(ROLES));
        std::fs::write(
            f.project.repo_path.join(".kete/roles.toml"),
            ROLES.replace("model = \"haiku\"", "model = \"opus\""),
        )
        .unwrap();
        let a = agent(&f, "r1", Some("runner"));
        let overlay = f.daemon.overlay_for(&a, &f.project).unwrap();
        assert_eq!(overlay.model.as_deref(), Some("haiku"));
        // Committed, it reaches the next launch.
        git(&f.project.repo_path, &["commit", "-qam", "opus"]);
        let overlay = f.daemon.overlay_for(&a, &f.project).unwrap();
        assert_eq!(overlay.model.as_deref(), Some("opus"));
    }

    #[test]
    fn unknown_roles_and_other_harnesses_are_refused() {
        let f = fixture(Some(ROLES));
        let a = agent(&f, "x1", Some("nobody"));
        let why = f.daemon.overlay_for(&a, &f.project).unwrap_err();
        assert!(why.contains("`nobody` is not in"), "{why}");
        let mut codex = agent(&f, "c1", Some("runner"));
        codex.kind = AgentKind::Codex;
        let why = f.daemon.overlay_for(&codex, &f.project).unwrap_err();
        assert!(why.contains("Claude sessions only"), "{why}");
        drop(f);
        let none = fixture(None);
        let a = agent(&none, "n1", Some("runner"));
        assert!(none
            .daemon
            .overlay_for(&a, &none.project)
            .unwrap_err()
            .contains("has no committed"));
    }

    #[test]
    fn a_harness_that_skips_permission_checks_is_refused() {
        let f = fixture(Some(ROLES));
        std::fs::write(
            f.data.path().join("config.json"),
            r#"{"harnesses": {"claude": {"permissions_flag": "--dangerously-skip-permissions"}}}"#,
        )
        .unwrap();
        let a = agent(&f, "r1", Some("runner"));
        let why = f.daemon.overlay_for(&a, &f.project).unwrap_err();
        assert!(why.contains("--dangerously-skip-permissions"), "{why}");
    }

    #[test]
    fn unattended_needs_the_account_to_have_accepted_bypass() {
        let f = fixture(Some(ROLES));
        std::fs::write(
            f.data.path().join("config.json"),
            r#"{"profiles": [{"name": "max2", "harness": "claude"}]}"#,
        )
        .unwrap();
        let mut a = agent(&f, "u1", Some("lead"));
        a.profile = Some("max2".into());
        let why = f.daemon.overlay_for(&a, &f.project).unwrap_err();
        assert_eq!(why, bypass_refusal(Some("max2")));
        assert!(why.contains("kete roles accept-bypass max2"), "{why}");
        let logged = nebula_core::studio::refusals::read_last(5);
        assert_eq!(logged.len(), 1);
        assert_eq!(logged[0].role.as_deref(), Some("lead"));
        assert_eq!(logged[0].reason, why);
        // Accepted in the account's own settings, the launch goes ahead.
        let home = f.data.path().join("profiles/max2/claude");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(
            home.join("settings.json"),
            r#"{"skipDangerousModePermissionPrompt": true}"#,
        )
        .unwrap();
        let overlay = f.daemon.overlay_for(&a, &f.project).unwrap();
        assert_eq!(
            overlay.permission_mode.as_deref(),
            Some("bypassPermissions")
        );
        let v = settings(overlay.settings_path.as_ref().unwrap());
        assert_eq!(v["permissions"]["allow"], serde_json::json!([]));
    }

    #[test]
    fn an_orchestrator_spawn_in_a_studio_repo_is_restricted_for_good() {
        let f = fixture(None);
        nebula_core::studio::settings::write(
            &f.project.repo_path,
            Some(&nebula_core::studio::StudioSettings {
                studio_inbox: "inbox".into(),
                autonomy: Autonomy::A1,
                queue_exclude: Vec::new(),
            }),
        )
        .unwrap();
        agent_with(&f, "o1", None, AgentRole::Orchestrator, None);
        let mut child = agent_with(&f, "k1", None, AgentRole::Worker, Some("o1"));
        let mine = agent(&f, "j1", None);
        assert_eq!(
            f.daemon.overlay_for(&mine, &f.project),
            Ok(LaunchOverlay::default()),
            "Jay's own sessions are never restricted"
        );
        let overlay = f.daemon.overlay_for(&child, &f.project).unwrap();
        assert_eq!(overlay.permission_mode, None);
        assert!(overlay.tools.is_none() && overlay.append_prompt.is_none());
        let v = settings(overlay.settings_path.as_ref().unwrap());
        let deny = v["permissions"]["deny"].as_array().unwrap();
        assert!(deny.contains(&serde_json::json!("Bash(git push *)")));
        assert!(!deny.contains(&serde_json::json!("AskUserQuestion")));
        assert_eq!(v["env"][permissions::ENV_STUDIO_AUTONOMY], "A1");
        // Its parent gone, a resume keeps the restrictions.
        child.parent = None;
        assert!(f
            .daemon
            .overlay_for(&child, &f.project)
            .unwrap()
            .settings_path
            .is_some());
    }

    #[test]
    fn a_role_survives_a_restart_and_is_reapplied_on_resume() {
        let f = fixture(Some(ROLES));
        let mut a = agent(&f, "r1", Some("runner"));
        let first = f.daemon.overlay_for(&a, &f.project).unwrap();
        let reopened = open(f.data.path());
        let roles = reopened.session_roles();
        assert_eq!(roles.len(), 1, "read back without a launch");
        assert_eq!(roles[0].label, "Runner");
        a.session_id = Some("sid".into());
        assert_eq!(reopened.overlay_for(&a, &f.project).unwrap(), first);
        // Archived, it is no longer a live role session.
        reopened.store.set_agent_archived(&a.id, true).unwrap();
        assert!(reopened.session_roles().is_empty());
    }
}
