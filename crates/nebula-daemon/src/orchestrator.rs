//! Privileged commands for the ORCHESTRATOR SESSION, and its provisioning.
//!
//! The CLI wrappers are just another same-user socket client, so prompt text
//! is not part of the trust boundary. Every privileged operation here starts
//! by checking the caller row persisted in SQLite is the orchestrator.
//!
//! There is at most one orchestrator per machine: the `role` column carries
//! the pin (migration 33 backs it with a partial unique index), and every
//! path that grants the role — [`Daemon::start_orchestrator`],
//! [`Daemon::set_orchestrator`] — demotes whichever row held it before.
//! `CreateAgent` cannot grant it at all (see `server.rs`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use nebula_core::{
    Agent, AgentId, AgentKind, AgentRole, AgentStatus, EntityId, Link, OrchestratorSession,
    OutputTail, Project, ProjectId, ReviewTabKind, ServerEvent, SessionRef, SubagentSummary,
    TerminalTab, Worktree, WorktreeId, MAX_CLOUD_PROMPT_BYTES,
};

use crate::registry::{CreateAgentSpec, Daemon};

/// The orchestrator's system prompt (or, for a harness without a
/// system-prompt flag, the opening of its first prompt), injected by role on
/// every cold spawn — see [`launch_rule`].
pub const CLAUDE_ORCHESTRATOR_GUIDANCE: &str = "[kete] You are the pinned ORCHESTRATOR session \
for kete: the one session on this machine that coordinates work across every project, worktree \
and session, so the user does not have to click through them. The daemon refuses the commands \
below unless this session is the persisted orchestrator.\n\n\
## Reading the room\n\
  kete orchestrator list [--json]\n\
Every live session in ATTENTION ORDER, the order the user's own `/` palette uses: \
`needs_feedback` (blocked on a permission or a question: the user's attention first), then \
`running` (mid-turn: leave it be), then `unread` (finished a turn nobody has looked at yet), \
then `idle`, `terminated` and `disconnected`. Each row names its project, branch, harness, \
parent session and open subagents. Use `--json` when you need exact ids; names work everywhere \
an id does, as long as they are unambiguous.\n\
  kete orchestrator read <session> [--lines N]\n\
Recent context: the harness transcript when kete knows it, else the cleaned terminal tail.\n\n\
## Acting\n\
  kete orchestrator send <session> \"<prompt>\" [--no-wait] [--timeout S]\n\
Delivers a follow-up labelled `[from orchestrator]`. A busy target is waited on until idle; \
`--no-wait` refuses instead. You cannot send to yourself.\n\
  kete orchestrator spawn --project <project> [--worktree <branch>] [--base <ref>] \
[--kind claude|codex|...] [--name N] [--model M] [--effort E] [--profile P] \"<task>\"\n\
Starts a worker on a self-contained task. `--worktree <branch>` reuses that checkout or cuts a \
new worktree for it (from `--base`, else the project's base branch): prefer a fresh worktree \
for any change that could collide with work already running on a branch.\n\
  kete orchestrator review <session>... [--tab terminal|diff|history|pr]...\n\
Opens kete's tabbed review modal for the user on one or more sessions.\n\
The everyday commands work here too: `kete sessions`, `kete read <session>`, and \
`kete ask <session> \"<question>\"` (send a question, wait, and print the answer). Inside your \
own turn you may use subagents for research or parallel reading; delegate real changes to \
spawned workers so they show up in kete where the user can see them.\n\n\
## Rules\n\
Tell, then ask: report what you found or did in a line or two, then ask before anything \
destructive or irreversible. Never approve another session's permission prompt unless the user \
explicitly tells you to; surface it instead. Prefer `read` over `ask` when the answer is already \
on screen, and do not interrupt a `running` session unless the user asked you to.\n\n\
## Studio\n\
A repo can run as a studio: its inbox notes are work for you, and while you sit idle kete hands \
you a short `[from kete studio]` note naming new ones and which accounts have room. You decide \
what to do with each; set a note's status when you take it.\n\
  kete studio status [--json]      each studio repo: autonomy, inbox, open notes, what waits on the user\n\
  kete studio queue [--project P]  what the next idle note would list, or why it is holding\n\
  kete studio note \"<text>\" --project P [--to R] [--priority high|normal|low]   file one inbox note\n\
  kete profile pick --json         the account the next spawn should take, and why; pass its name to --profile\n\
  kete profile list --json         every account with its 5-hour and weekly use\n\
  kete usage --csv [--since YYYY-MM-DD]   tokens, estimated cost, tool uses and turns per session per day\n\
`--role <id>` on `kete orchestrator spawn` (and on `kete spawn`) starts the worker with that role \
card from the repo's `.kete/roles.toml`: its model, tools, prompt and permissions.\n\
Each studio repo has an autonomy level. It is the user's to set, never yours:\n\
  A0 Hold: no new sessions in the repo; spawns are refused, so finish or park what is running.\n\
  A1 Plan: spawn and plan; no pushes, pull requests, ships or tags.\n\
  A2 Deliver: push and open pull requests; no tags or releases.\n\
  A3 Release: tags and releases too.\n\
Handover: at 60 percent of your context budget, write a handover note into your workspace (what \
is in flight, what is next, what waits on the user), then run `kete orchestrator renew --from \
<note>`. A fresh orchestrator starts on the note once this turn ends, and this session is \
archived, so end your turn after the command.\n\n\
## Azure DevOps\n\
For a repo whose git remote is on Azure DevOps, `kete ado` does what the desktop app's Tasks and \
Pull requests views do. Your own workspace has no remote, so name the repo: `--project <kete \
project>` (as `kete orchestrator list` names it; `--worktree <branch>` for one of its checkouts) \
goes on every `kete ado` command, before or after the verb. Org, project and repo come from that \
checkout's remote; run without `--project` it uses the checkout you are in. Add `--json` when you \
need exact ids.\n\
  kete ado tasks [--all | --open] [--json]\n\
The user's tasks in the team's current sprint, grouped by parent (`--all`: everyone's open items \
in the sprint; `--open`: everything open assigned to them).\n\
  kete ado start-task <id> [--agent <harness>] [--no-agent] [--branch B] [--base R] \
[--no-assign] [--no-activate] [--prompt P] [--model M] [--effort E] [--profile P]\n\
The desktop's Start task: a new branch (default `<initials>/<slug of the title>`) cut from the \
repo's default branch, a worktree for it, the branch linked to the task, the task assigned to the \
user and moved to In Progress, then an agent in the worktree with the task as its prompt. Run \
from here, that agent is your child and shows in your list. Steps after the worktree that did not \
happen are listed, not fatal.\n\
  kete ado prs [--json]          the user's active PRs and the ones waiting on their review\n\
  kete ado pr <id> [--json]      one PR: reviewers and votes, policies, comment threads\n\
  kete ado pr create [--title T] [--summary S] [--testing T] [--target B] [--task <id> | \
--no-task] [--reviewer <name or email>]... [--no-reviewer] [--publish]\n\
With `--worktree <the worker's branch>` (or `--source <branch>`; one of them is required with `--project`; or run in its worktree): pushes that branch and opens a DRAFT PR (unless `--publish`) with \
the desktop composer's rules: co-author and AI attribution lines are cleaned out of unpushed \
commit messages, a linked task (the branch's own by default) and a reviewer are required unless \
waived, and an existing PR from the branch is reported rather than doubled. It refuses to push the default branch or a branch onto itself.\n\
  kete ado vote <id> approve|approve-with-suggestions|wait|reject|reset\n\
  kete ado reply <id> <thread> \"<text>\"\n\
The everyday flow: `kete ado --project P tasks`, pick a task with the user, `kete ado --project \
P start-task <id>` (the worker gets its own branch and worktree), watch it with `list`/`read`, \
then `kete orchestrator review <worker>` for the user, and once they are happy `kete ado \
--project P --worktree <its branch> pr create`. Voting and replying are the user's voice: only when they ask. Sign-in is never \
interactive: if a command says Azure DevOps sign-in is needed, tell the user to sign in from the \
desktop app or run `az login`; never try to sign in yourself.";

/// Name a started orchestrator gets when the request names none.
pub const DEFAULT_ORCHESTRATOR_NAME: &str = "orchestrator";

/// The tag every orchestrator follow-up opens with, so the receiving model
/// (and the user reading its pane) knows where the turn came from.
const FROM_ORCHESTRATOR: &str = "[from orchestrator]";

/// How long a waiting `send` waits for a busy target, when the request says 0.
const DEFAULT_TELL_TIMEOUT: Duration = Duration::from_secs(120);

pub type OrchestratorTree = (
    Vec<Project>,
    Vec<Worktree>,
    Vec<Agent>,
    Vec<TerminalTab>,
    Vec<Link>,
);

/// What `StartOrchestrator` asks for.
#[derive(Debug, Clone, Default)]
pub struct StartOrchestratorSpec {
    pub kind: Option<AgentKind>,
    pub custom_harness: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub profile: Option<String>,
    /// None: the dedicated orchestrator workspace (see
    /// [`Daemon::orchestrator_home`]).
    pub project: Option<ProjectId>,
    pub name: Option<String>,
    pub replace: bool,
    /// The ROLE CARD the orchestrator launches with (0.60).
    pub role_card: Option<String>,
    /// The new orchestrator's first prompt: a renewal's handover. Only the
    /// daemon's own renewal sets it (`orchestrator_renew.rs`), so a start
    /// with one is automation's (`accounts::Launcher`).
    pub starting_prompt: Option<String>,
}

/// Every orchestrator-spawned worker's knobs, `OrchestratorSpawn[With]`.
#[derive(Debug, Clone)]
pub struct OrchestratorSpawnSpec {
    pub project: ProjectId,
    pub worktree: Option<WorktreeId>,
    pub branch: Option<String>,
    pub base: Option<String>,
    pub name: String,
    pub kind: AgentKind,
    pub custom_harness: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub profile: Option<String>,
    pub starting_prompt: String,
    /// The ROLE CARD the worker launches with (0.60).
    pub role_card: Option<String>,
}

/// Fold the role's guidance into a spawn's launch rule (the PR / issue
/// scope, when the row has one): the orchestrator's guidance leads, so it
/// rides the system-prompt flag where the harness maps one and opens a cold
/// spawn's first prompt where it prepends instead.
pub(crate) fn launch_rule(role: AgentRole, rule: Option<String>) -> Option<String> {
    if !role.is_orchestrator() {
        return rule;
    }
    Some(match rule {
        Some(rule) => format!("{CLAUDE_ORCHESTRATOR_GUIDANCE}\n\n{rule}"),
        None => CLAUDE_ORCHESTRATOR_GUIDANCE.to_string(),
    })
}

impl Daemon {
    pub(crate) fn require_orchestrator(&self, caller: &AgentId) -> Result<Agent> {
        let agent = self
            .store
            .get_agent(caller)?
            .context("orchestrator session not found")?;
        if agent.archived {
            bail!("orchestrator session is archived");
        }
        if agent.role != AgentRole::Orchestrator {
            bail!("this command is only available to the orchestrator session");
        }
        Ok(agent)
    }

    // ---- provisioning ----

    /// Where an orchestrator started without a project runs: a folder of
    /// its own under the data dir, so its hooks, notes and scratch files
    /// never land in one of the user's repos.
    pub fn orchestrator_home(&self) -> PathBuf {
        self.orchestrator_home_override()
            .unwrap_or_else(|| nebula_core::paths::data_dir().join("orchestrator"))
    }

    /// The root worktree an orchestrator lives in: `project`'s, or the
    /// dedicated workspace's — registered as a project named
    /// `orchestrator` (git-initialized) the first time one is started.
    async fn orchestrator_worktree(
        self: &Arc<Self>,
        project: Option<&ProjectId>,
    ) -> Result<Worktree> {
        let project = match project {
            Some(id) => {
                self.store.get_project(id)?.context("project not found")?;
                id.clone()
            }
            None => {
                let home = self.orchestrator_home();
                match self.project_at(&home)? {
                    Some(id) => id,
                    None => {
                        ensure_own_repo(&home).await?;
                        let created = self
                            .add_project(&home, Some(DEFAULT_ORCHESTRATOR_NAME.into()), true)
                            .await
                            .context("set up the orchestrator workspace")?;
                        let EntityId::Project(id) = created else {
                            bail!("the orchestrator workspace did not register as a project");
                        };
                        id
                    }
                }
            }
        };
        self.store
            .load_tree()?
            .1
            .into_iter()
            .filter(|w| w.project_id == project)
            .min_by_key(|w| (!w.is_main, w.sort_order))
            .context("project has no worktrees")
    }

    /// The project whose repo is `dir`, compared canonically (git reports
    /// repo paths in its own spelling, the data dir comes from the env).
    fn project_at(&self, dir: &Path) -> Result<Option<ProjectId>> {
        let Ok(want) = std::fs::canonicalize(dir) else {
            return Ok(None);
        };
        Ok(self
            .store
            .load_tree()?
            .0
            .into_iter()
            .find(|p| std::fs::canonicalize(&p.repo_path).is_ok_and(|have| have == want))
            .map(|p| p.id))
    }

    /// Start the machine's one orchestrator. A live (unarchived) one
    /// already pinned refuses unless `replace`; either way the previous
    /// holder is demoted to a worker — its session keeps running — before
    /// the new row is created with the role, so its very first spawn
    /// carries the orchestrator guidance.
    pub async fn start_orchestrator(
        self: &Arc<Self>,
        spec: StartOrchestratorSpec,
    ) -> Result<EntityId> {
        let _gate = self.provision_gate.lock().await;
        let previous = self.store.orchestrator()?;
        if let Some(current) = previous.as_ref().filter(|a| !a.archived) {
            if !spec.replace {
                bail!(
                    "an orchestrator is already running: {} ({}); replace it to start another",
                    current.name,
                    current.id
                );
            }
        }
        let worktree = self.orchestrator_worktree(spec.project.as_ref()).await?;
        let demoted = self.store.set_orchestrator(None)?;
        let name = spec
            .name
            .map(|n| n.trim().to_string())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| DEFAULT_ORCHESTRATOR_NAME.into());
        // A person starts an orchestrator; a renewal (the daemon handing
        // over on its own) or one with a role card is automation's.
        let launcher = if spec.role_card.is_some() || spec.starting_prompt.is_some() {
            nebula_core::accounts::Launcher::Automation
        } else {
            nebula_core::accounts::Launcher::Person
        };
        let created = self
            .create_agent(CreateAgentSpec {
                parent: None,
                profile: spec.profile,
                worktree: worktree.id,
                name,
                kind: spec.kind.unwrap_or(AgentKind::Claude),
                custom_harness: spec.custom_harness,
                model: spec.model,
                effort: spec.effort,
                auto_title: false,
                cloud_prompt: None,
                starting_prompt: spec.starting_prompt,
                pr_url: None,
                issue_url: None,
                role: AgentRole::Orchestrator,
                role_card: spec.role_card,
                launcher,
            })
            .await;
        match created {
            Ok(id) => {
                for agent in &demoted {
                    let _ = self.broadcast_agent(agent);
                }
                Ok(id)
            }
            Err(e) => {
                // Nothing was started: the pin goes back where it was.
                if let Some(prev) = previous.filter(|_| !demoted.is_empty()) {
                    let _ = self.store.set_orchestrator(Some(&prev.id));
                }
                Err(e)
            }
        }
    }

    /// Pin an existing agent as the orchestrator ("Make orchestrator"),
    /// demoting whoever held the role. A live, idle session is handed the
    /// guidance as its next turn at once; one mid-turn, or not running,
    /// gets it with its next cold spawn's system prompt.
    pub fn set_orchestrator(self: &Arc<Self>, id: &AgentId) -> Result<()> {
        let _gate = self.provision_gate.blocking_lock();
        let agent = self.store.get_agent(id)?.context("agent not found")?;
        if agent.archived {
            bail!("an archived session cannot be the orchestrator; unarchive it first");
        }
        if agent.cloud_session_id.is_some() {
            bail!("a Claude Cloud session cannot be the orchestrator: it has no local CLI");
        }
        let changed = self.store.set_orchestrator(Some(id))?;
        for agent in &changed {
            let _ = self.broadcast_agent(agent);
        }
        // Only a CLI sitting at its prompt: never a dead or disconnected pane,
        // where the text would land in whatever is left of it.
        let at_prompt = matches!(agent.status, AgentStatus::Fresh | AgentStatus::Finished);
        if changed.contains(id) && at_prompt {
            if let Some(session) = self.session(&SessionRef::Agent(id.clone())) {
                let note = format!(
                    "{CLAUDE_ORCHESTRATOR_GUIDANCE}\n\nThe user just made this session the \
                     orchestrator. Acknowledge in one line; do nothing else yet."
                );
                if let Err(e) = crate::session_context::write_prompt(&session, &note) {
                    tracing::warn!(agent = %id, error = %e, "orchestrator guidance delivery failed");
                }
            }
        }
        Ok(())
    }

    /// Demote whichever row holds the role. None held is not an error.
    pub fn clear_orchestrator(self: &Arc<Self>) -> Result<()> {
        let _gate = self.provision_gate.blocking_lock();
        for agent in self.store.set_orchestrator(None)? {
            let _ = self.broadcast_agent(&agent);
        }
        Ok(())
    }

    /// Whether `pid` runs inside a live AGENT session — the PTY child or
    /// any descendant of it. Provisioning requests from such a process are
    /// refused, so a worker cannot mint or become the orchestrator through
    /// its own shell. Terminal sessions are the user's own shells and pass.
    pub fn pid_inside_agent_session(&self, pid: u32) -> bool {
        self.pid_session_owner(pid).is_some()
    }

    /// The live AGENT session `pid` runs inside (its PTY child or any
    /// descendant), if any.
    pub fn pid_session_owner(&self, pid: u32) -> Option<AgentId> {
        let roots: HashMap<u32, AgentId> = self
            .session_pids()
            .into_iter()
            .filter_map(|(sref, pid, _)| match sref {
                SessionRef::Agent(id) => Some((pid, id)),
                SessionRef::Terminal(_) => None,
            })
            .collect();
        if roots.is_empty() {
            return None;
        }
        owning_session(&process_parents(), pid, &roots)
    }

    /// A privileged verb's `caller` is what the client says it is
    /// (`NEBULA_AGENT_ID`), so a worker could claim the orchestrator's id.
    /// When the client's pid is known and it runs inside some agent session,
    /// that session must be the caller's own. A process inside no session
    /// (the user's shell) passes, as does an unknown pid: defence in depth
    /// over a same-user socket, not the trust boundary.
    pub fn check_caller_peer(&self, caller: &AgentId, pid: Option<u32>) -> Result<()> {
        let Some(pid) = pid else {
            return Ok(());
        };
        match self.pid_session_owner(pid) {
            Some(owner) if &owner != caller => bail!(
                "this command runs inside another session ({owner}), not the orchestrator \
                 it names"
            ),
            _ => Ok(()),
        }
    }

    // ---- privileged verbs ----

    pub fn orchestrator_tree(&self, caller: &AgentId) -> Result<OrchestratorTree> {
        self.require_orchestrator(caller)?;
        let (projects, worktrees, mut agents, mut terminals) = self.store.load_tree()?;
        for a in &mut agents {
            a.alive = self.is_alive(&SessionRef::Agent(a.id.clone()));
        }
        for t in &mut terminals {
            t.alive = self.is_alive(&SessionRef::Terminal(t.id.clone()));
        }
        Ok((
            projects,
            worktrees,
            agents,
            terminals,
            self.store.load_links()?,
        ))
    }

    /// Every live session in attention order, for `kete orchestrator list`.
    pub fn orchestrator_sessions(&self, caller: &AgentId) -> Result<Vec<OrchestratorSession>> {
        self.require_orchestrator(caller)?;
        let summaries = self.session_summaries()?;
        let agents: HashMap<AgentId, Agent> = self
            .store
            .load_tree()?
            .2
            .into_iter()
            .map(|a| (a.id.clone(), a))
            .collect();
        let mut subagents: HashMap<AgentId, Vec<SubagentSummary>> = HashMap::new();
        for sub in self.subagent_rows() {
            subagents
                .entry(sub.parent_agent.clone())
                .or_default()
                .push(sub);
        }
        Ok(attention_ordered(summaries, &agents, &mut subagents))
    }

    /// Legacy `OrchestratorSend`: the target by id, waiting as `send` does.
    pub async fn orchestrator_send(
        self: &Arc<Self>,
        caller: &AgentId,
        target: &AgentId,
        message: &str,
    ) -> Result<AgentId> {
        self.orchestrator_tell(caller, target.as_str(), message, Duration::ZERO, true)
            .await
    }

    /// Deliver a labelled follow-up to another agent, by id or unambiguous
    /// fuzzy name. A busy target is waited on until idle when `wait`, else
    /// refused. Returns the agent it went to.
    pub async fn orchestrator_tell(
        self: &Arc<Self>,
        caller: &AgentId,
        target: &str,
        message: &str,
        timeout: Duration,
        wait: bool,
    ) -> Result<AgentId> {
        self.require_orchestrator(caller)?;
        let text = validate_message(message)?;
        let timeout = if timeout.is_zero() {
            DEFAULT_TELL_TIMEOUT
        } else {
            timeout
        };
        let deadline = tokio::time::Instant::now() + timeout;
        let SessionRef::Agent(target_id) = self.resolve_session_target(target)? else {
            bail!("follow-ups can only be sent to agent sessions");
        };
        if &target_id == caller {
            bail!("the orchestrator cannot send to itself; name another session");
        }
        // Subscribed before the status is read, so a turn that ends in
        // between is not missed.
        let mut status_rx = self.events.subscribe();
        let agent = self
            .store
            .get_agent(&target_id)?
            .context("target agent not found")?;
        if agent.archived {
            bail!("target agent is archived");
        }
        if agent.cloud_session_id.is_some() {
            bail!("target agent is a Claude Cloud session; use its cloud message path");
        }
        if !crate::session_context::is_idle(agent.status) {
            if !wait {
                bail!(
                    "{} is {}; retry without --no-wait to queue behind its turn",
                    agent.name,
                    agent.status.as_str()
                );
            }
            crate::session_context::wait_until_idle(&mut status_rx, &target_id, deadline).await?;
        }
        let sref = SessionRef::Agent(target_id.clone());
        let session =
            self.ensure_session(&sref, crate::pty::DEFAULT_COLS, crate::pty::DEFAULT_ROWS)?;
        crate::session_context::write_prompt(&session, &format!("{FROM_ORCHESTRATOR}\n{text}"))?;
        Ok(target_id)
    }

    pub fn orchestrator_read(
        &self,
        caller: &AgentId,
        session: &SessionRef,
        max_bytes: u32,
    ) -> Result<Option<OutputTail>> {
        self.require_orchestrator(caller)?;
        Ok(self.read_output_tail_clean(session, max_bytes))
    }

    /// Transcript-aware recent context, by id or fuzzy name.
    pub fn orchestrator_read_session(
        &self,
        caller: &AgentId,
        target: &str,
        max_lines: u32,
    ) -> Result<crate::session_context::SessionRead> {
        self.require_orchestrator(caller)?;
        self.read_session_context(target, max_lines)
    }

    pub async fn orchestrator_spawn(
        self: &Arc<Self>,
        caller: &AgentId,
        spec: OrchestratorSpawnSpec,
    ) -> Result<EntityId> {
        self.require_orchestrator(caller)?;
        let OrchestratorSpawnSpec {
            project,
            worktree,
            branch,
            base,
            name,
            kind,
            custom_harness,
            model,
            effort,
            profile,
            starting_prompt,
            role_card,
        } = spec;
        let project = &project;
        let worktree = match worktree {
            Some(id) => {
                let wt = self
                    .store
                    .get_worktree(&id)?
                    .context("worktree not found")?;
                if &wt.project_id != project {
                    bail!("worktree does not belong to the requested project");
                }
                wt
            }
            None => {
                if branch.as_ref().is_none_or(|b| b.trim().is_empty()) {
                    if let Some(existing) = self
                        .store
                        .load_tree()?
                        .1
                        .into_iter()
                        .filter(|w| &w.project_id == project)
                        .min_by_key(|w| (!w.is_main, w.sort_order))
                    {
                        existing
                    } else {
                        bail!("project has no worktrees");
                    }
                } else {
                    let branch = slugify_branch(branch.unwrap_or_default().as_str());
                    let branch = if branch.is_empty() {
                        random_branch_name()
                    } else {
                        branch
                    };
                    if let Some(existing) = self
                        .store
                        .load_tree()?
                        .1
                        .into_iter()
                        .find(|w| &w.project_id == project && w.branch == branch)
                    {
                        existing
                    } else {
                        let created = self
                            .create_worktree(project, &branch, base.as_deref())
                            .await?;
                        let EntityId::Worktree(id) = created else {
                            unreachable!("create_worktree returns a worktree id");
                        };
                        self.store
                            .get_worktree(&id)?
                            .context("created worktree not found")?
                    }
                }
            }
        };
        let (_, _, agents, _) = self.store.load_tree()?;
        let taken = agents
            .iter()
            .filter(|a| a.worktree_id == worktree.id)
            .map(|a| a.name.clone())
            .collect::<Vec<_>>();
        let name = if name.trim().is_empty() {
            crate::sibling::sibling_name(&taken)
        } else {
            name
        };
        self.create_agent(CreateAgentSpec {
            parent: Some(caller.clone()),
            profile,
            worktree: worktree.id,
            name,
            kind,
            custom_harness,
            model,
            effort,
            auto_title: true,
            cloud_prompt: None,
            starting_prompt: Some(starting_prompt),
            pr_url: None,
            issue_url: None,
            role: AgentRole::Worker,
            role_card,
            launcher: nebula_core::accounts::Launcher::Automation,
        })
        .await
    }

    pub fn orchestrator_open_review(
        &self,
        caller: &AgentId,
        sessions: Vec<SessionRef>,
        tabs: Vec<ReviewTabKind>,
    ) -> Result<()> {
        self.require_orchestrator(caller)?;
        if sessions.is_empty() {
            bail!("no sessions named for review");
        }
        for session in &sessions {
            match session {
                SessionRef::Agent(id) => {
                    self.store.get_agent(id)?.context("agent not found")?;
                }
                SessionRef::Terminal(id) => {
                    self.store.get_terminal(id)?.context("terminal not found")?;
                }
            }
        }
        self.broadcast(ServerEvent::ReviewOpened {
            opener: caller.clone(),
            sessions,
            tabs: if tabs.is_empty() {
                vec![
                    ReviewTabKind::Terminal,
                    ReviewTabKind::Diff,
                    ReviewTabKind::History,
                    ReviewTabKind::PullRequest,
                ]
            } else {
                tabs
            },
        });
        Ok(())
    }
}

/// Make `dir` a git repo of its own: created when missing, and `git init`ed
/// unless it is already a repo's top level. A data dir inside another repo
/// (a dotfiles checkout of the home dir) would otherwise hand
/// `add_project` the ENCLOSING repo, registered as "orchestrator".
async fn ensure_own_repo(dir: &Path) -> Result<()> {
    tokio::fs::create_dir_all(dir)
        .await
        .with_context(|| format!("create {}", dir.display()))?;
    let own = std::fs::canonicalize(dir).ok();
    let top = crate::git::repo_toplevel(dir)
        .await
        .ok()
        .and_then(|t| std::fs::canonicalize(t).ok());
    if top.is_none() || top != own {
        crate::git::init(dir).await?;
    }
    Ok(())
}

/// The attention tier a session sorts into: nebula-core's, what the user's
/// `/` palette uses, so the orchestrator triages in the order the user
/// would. A terminal sorts after every session.
fn attention_tier(agent: Option<&Agent>) -> u8 {
    agent.map_or(4, |a| nebula_core::board::attention(a).rank())
}

/// `kete sessions` rows in attention order, each grown by role, parent,
/// subagents and its triage state.
fn attention_ordered(
    summaries: Vec<nebula_core::SessionSummary>,
    agents: &HashMap<AgentId, Agent>,
    subagents: &mut HashMap<AgentId, Vec<SubagentSummary>>,
) -> Vec<OrchestratorSession> {
    let mut rows: Vec<OrchestratorSession> = summaries
        .into_iter()
        .map(|summary| {
            let agent = match &summary.session {
                SessionRef::Agent(id) => agents.get(id),
                SessionRef::Terminal(_) => None,
            };
            let state = match agent {
                Some(a) if nebula_core::board::is_unread(a) => "unread".to_string(),
                _ => summary.status.clone(),
            };
            OrchestratorSession {
                attention: 0,
                state,
                role: agent.map(|a| a.role).unwrap_or_default(),
                parent: agent.and_then(|a| a.parent.clone()),
                status_changed_at: agent.map(|a| a.status_changed_at).unwrap_or(0),
                subagents: agent
                    .and_then(|a| subagents.remove(&a.id))
                    .unwrap_or_default(),
                summary,
            }
        })
        .collect();
    rows.sort_by_key(|row| {
        let agent = match &row.summary.session {
            SessionRef::Agent(id) => agents.get(id),
            SessionRef::Terminal(_) => None,
        };
        (
            attention_tier(agent),
            std::cmp::Reverse(row.status_changed_at),
            row.summary.name.clone(),
        )
    });
    for (i, row) in rows.iter_mut().enumerate() {
        row.attention = i as u32;
    }
    rows
}

/// Whether `pid` is one of `roots` or descends from one, walking `parents`
/// (pid → ppid) upward. Bounded, so a cycle in a racy snapshot ends.
#[cfg(test)]
fn descends_from(
    parents: &HashMap<u32, u32>,
    pid: u32,
    roots: &std::collections::HashSet<u32>,
) -> bool {
    let roots: HashMap<u32, ()> = roots.iter().map(|r| (*r, ())).collect();
    owning_session(parents, pid, &roots).is_some()
}

/// The root (and what it maps to) `pid` is or descends from, walking
/// `parents` (pid → ppid) upward. Bounded, so a cycle in a racy snapshot
/// ends.
fn owning_session<T: Clone>(
    parents: &HashMap<u32, u32>,
    pid: u32,
    roots: &HashMap<u32, T>,
) -> Option<T> {
    let mut at = pid;
    for _ in 0..128 {
        if let Some(owner) = roots.get(&at) {
            return Some(owner.clone());
        }
        match parents.get(&at) {
            Some(&parent) if parent != at && parent != 0 => at = parent,
            _ => return None,
        }
    }
    None
}

/// The machine's process table as pid → ppid; empty when it can't be read.
/// Windows keeps a child's ppid after its parent exits, and pids are
/// reused, so a long-dead ancestor's pid can now name an agent session's
/// shell. A link whose "parent" was created after the child is such a stale
/// one and is dropped; one whose times can't be read is kept.
#[cfg(windows)]
fn process_parents() -> HashMap<u32, u32> {
    use nebula_core::platform::process_created_at;
    let mut created: HashMap<u32, Option<u64>> = HashMap::new();
    let mut at = |pid: u32| {
        *created
            .entry(pid)
            .or_insert_with(|| process_created_at(pid))
    };
    nebula_core::platform::process_table()
        .into_iter()
        .filter(|row| match (at(row.ppid), at(row.pid)) {
            (Some(parent), Some(child)) => parent <= child,
            _ => true,
        })
        .map(|row| (row.pid, row.ppid))
        .collect()
}

#[cfg(unix)]
fn process_parents() -> HashMap<u32, u32> {
    let table = std::process::Command::new("ps")
        .args(["-axo", "pid=,ppid="])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();
    table
        .lines()
        .filter_map(|line| {
            let mut cols = line.split_whitespace();
            Some((cols.next()?.parse().ok()?, cols.next()?.parse().ok()?))
        })
        .collect()
}

fn validate_message(message: &str) -> Result<&str> {
    let text = message.trim();
    if text.is_empty() {
        bail!("message is empty");
    }
    if text.contains('\0') {
        bail!("message cannot contain NUL bytes");
    }
    if text.len() > MAX_CLOUD_PROMPT_BYTES {
        bail!(
            "message is too long (max {} KiB)",
            MAX_CLOUD_PROMPT_BYTES / 1024
        );
    }
    Ok(text)
}

fn slugify_branch(raw: &str) -> String {
    raw.trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

fn random_branch_name() -> String {
    format!("orchestrator-{}", nebula_core::clock::now_ms())
}

#[cfg(test)]
mod tests;

/// The 0.60 Studio section of the guidance (orchestrator/tests.rs holds the
/// rest of its checks).
#[cfg(test)]
mod studio_guidance_tests {
    use super::CLAUDE_ORCHESTRATOR_GUIDANCE as GUIDANCE;

    #[test]
    fn the_guidance_teaches_the_studio_commands_autonomy_and_the_handover() {
        for needle in [
            "## Studio",
            "kete studio status",
            "kete studio queue",
            "kete studio note",
            "kete profile pick --json",
            "kete usage --csv",
            "--role <id>",
            "A0 Hold",
            "A1 Plan",
            "A2 Deliver",
            "A3 Release",
            "60 percent",
            "kete orchestrator renew --from",
        ] {
            assert!(GUIDANCE.contains(needle), "guidance lacks {needle}");
        }
        // Autonomy is the user's: the guidance never teaches the verbs that
        // change it.
        for never in [
            "kete studio autonomy",
            "kete studio enable",
            "kete studio disable",
        ] {
            assert!(!GUIDANCE.contains(never), "guidance teaches {never}");
        }
    }
}
