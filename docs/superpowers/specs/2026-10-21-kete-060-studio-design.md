# Kete 0.60: the studio train (K1 to K9)

Date 2026-10-21. Base: `main` at 0.59.0 (a668819, protocol 65, migration 35). Plan: [docs/superpowers/plans/2026-10-21-kete-060-plan.md](../plans/2026-10-21-kete-060-plan.md). Lane briefs: [docs/superpowers/plans/lanes-060/](../plans/lanes-060/).

Source: the WHEKO studio design (`C:\Code\wheako\docs\superpowers\specs\2026-10-21-studio-os-and-gauntlet-design.md`, sections 4, 8, 9, 10 and 13) and decision 0121 (Proposed). Those documents describe one always-on studio of agents on Jay's PC, with the Kete orchestrator as its studio manager. Section 10 lists nine Kete changes, K1 to K9. This spec decides every one of them up front so the lanes can run in parallel. The main thread merges them into `release/0.60` and releases once at the end, the shape Jay set for 0.54.

**Process note.** Jay's standing Kete authorisation (plan, scope, deliver and release through 0.60 without check-ins) still holds. Every open question in studio design section 13 that a K item depends on is listed with the default this train takes. Section 16 names the two answers that would change the plan.

**Kete stays a public, general tool.** Nothing in the code names WHEKO, its paths or its departments. A repo opts in with two files under `<repo>/.kete/` and one machine-local setting. WHEKO's values appear only in Appendix A, as the config Jay's PC uses.

## 1. Scope

| # | Feature | Lane (branch) | Protocol 66 | Migration | New settings |
|---|---|---|---|---|---|
| K1 | `kete profile list --json`, `kete profile pick [--dry-run]`, reserved profiles | accounts (`release/0.60-accounts`) | none | none | profile field `reserved` |
| K2 | Queue runner: per-project `studio_inbox` and `autonomy` (A0 to A3, default A1), idle notes to the orchestrator | queue (`release/0.60-queue`) | `SetStudio`, `StudioQueue`, `StudioQueueState` | none | `studio` map (machine-local), `studio_idle_after` |
| K3 | Role cards from `<repo>/.kete/roles.toml` applied at launch | roles (`release/0.60-roles`) | `role_card` on three requests, `SpawnSiblingAgentWith`, `SessionRoles` | 36 | none (repo file) |
| K4 | Studio board page, desktop and TUI, read-only from repo files | board (`release/0.60-board`) | none | none | repo file `.kete/studio.toml` |
| K5 | Gauntlet view, read-only from verdict files, links into the Studio app | gauntlet (`release/0.60-gauntlet`) | none | none | `.kete/studio.toml` `kinds`, `links` |
| K6 | Triage desk: inbox note writer, triage role, strip under the orchestrator view | triage (`release/0.60-triage`) | none | none | none |
| K7 | Usage ledger per session per day, `kete usage --csv` | ledger (`release/0.60-ledger`) | `UsageLedger`, `UsageLedgerRows`, `SessionSpend` | 37 | `usage_ledger` |
| K8 | Register `C:\Code\wheako` and `C:\Code\wheko-web` | **not a lane**: two `kete add` commands after install | none | none | none |
| K9 | `kete orchestrator renew --from <note>`, run by the daemon; unattended and allowlist permission profiles per role | renew (`release/0.60-renew`) for renewal; roles (`release/0.60-roles`) for the permission profiles | `RenewOrchestrator`, `OrchestratorRenewal` | none | role field `permission` |

Two more lanes carry no K item: **foundation** (`release/0.60-foundation`) lands every shared contract first, and **visual-qa** (`release/0.60-visual-qa`) checks every new surface last.

K8 is a one-time step on Jay's PC, not code. 0.57 already registers linked worktrees of a registered repo on the stamp tick (about every 10 s), so the lane worktrees under `C:\Code\wheako-<lane>` appear on their own once `C:\Code\wheako` is added. It is step 7 of the plan's install note.

## 2. Corrections to the studio design (verified 2026-10-21)

These replace what studio design section 10 assumed. Each was checked against the code at a668819, `claude --help` of the installed Claude Code 2.1.304, or code.claude.com.

1. **K1 lives in the CLI and core, not nebula-config.** `kete profile` is `crates/nebula/src/profile.rs`; the spread is `nebula_core::accounts::pick` (accounts.rs:174) with `Pick::describe` (:141); profiles are `nebula_core::profiles::Profile` (profiles.rs:41). `kete profile list` has no `--json` and there is no `pick` subcommand today (cli.rs:1182).
2. **"Disable Jay's profile for lanes" would also take it off Jay's own launches.** A disabled profile is never picked for any launch that names none, his own included (docs/profiles.md, enable / disable). K1 adds `reserved` instead: still the default for launches a person starts, never picked for launches the orchestrator or a script starts.
3. **`--allowedTools` cannot limit a tier.** It pre-approves calls; `--disallowedTools` denies them; `--tools <list>` sets the built-in tool set itself (`claude --help`, 2.1.304). In `bypassPermissions` mode allow rules have no effect at all, while deny rules block in every mode (code.claude.com/docs/en/permission-modes, read 2026-10-21). K3 therefore puts a tier's tools in `--tools` and every rule in a generated settings file.
4. **Bypass mode needs a one-time acceptance per account.** The first interactive session in `bypassPermissions` shows a dialog; accepting sets `skipDangerousModePermissionPrompt: true` in that account's user `settings.json` (same page). Each Kete profile has its own `CLAUDE_CONFIG_DIR`, so each account accepts once. Kete never writes that key: it refuses an unattended launch on an account that has not accepted, and gives Jay a command that opens the dialog for him to answer.
5. **`AskUserQuestion` still prompts under bypass** (permission-modes, "Actions no mode auto-approves"). An unattended session that calls it waits for a person, so allowlist and unattended profiles deny it and role prompts say to write `pending Jay: A | B` into the inbox instead.
6. **Deny rules do not catch everything.** Read and Edit rules cover Claude's file tools, recognised shell file commands and redirects, not a Python or Node script that opens files itself (code.claude.com/docs/en/permissions). Bash patterns that constrain arguments are fragile (same page). The governance and force-push denials are defence in depth, backed by a Kete guard hook on every shell command (decision 13), and section 14 lists the gaps.
7. **The statusLine carries no running token total.** `context_window.total_input_tokens` and `total_output_tokens` are the tokens in the context window from the latest response; `cost.total_cost_usd` is a list-price estimate that accumulates across the session and resets on `/clear` (code.claude.com/docs/en/statusline, read 2026-10-21). K7 takes token totals from the session transcript's per-message `usage` and the cost estimate from the statusLine.
8. **A session cannot start an orchestrator.** `StartOrchestrator` is refused when the client runs inside a live agent session (`check_provisioning_peer`, server.rs:959; docs/orchestrator.md:179-184). Renewal is a new orchestrator verb that the daemon carries out after the caller's turn ends.
9. **The idle note cannot ride the UserPromptSubmit hook.** That channel only fires when a prompt is submitted (hooks/mod.rs:697-730). An idle orchestrator is reached through `session_context::write_prompt` (session_context.rs:211), the path `orchestrator_tell` uses.
10. **The system prompt is recorded once per conversation.** `--system-prompt-snapshot` defaults to on: the prompt rendered on the first request, `--append-system-prompt` included, is reused on every resume until a compaction (`claude --help`). A role card edit therefore reaches new sessions only. The docs say so.
11. **Branch names cannot nest under `release/0.60`.** Git refuses `release/0.60/<lane>` while `release/0.60` exists (tested 2026-10-21: "cannot lock ref"). Lanes use `release/0.60-<lane>`.
12. **K6's "quick prompt preset" is the `triage` role card.** An agent preset carries a prefix, a postfix, a model and an effort; the desk also needs a tool set, a permission profile and a stable system prompt, which only a role card carries. The strip launches the desk with that card.
13. **`--tools <tools...>` is variadic.** Claude's option parser keeps taking values after a variadic flag until the next option, so a trailing task placed straight after the tool list would be read as one more tool name. Kete always emits `--tools <list>` immediately before `--append-system-prompt`, and a test pins that order.
14. **`SpawnSiblingAgent` cannot take another trailing field.** Its last field, `profile`, is left off the frame when None (protocol 58), and frames are positional: a trailing `role_card` sent with no profile would be read as the profile. The role goes in a new variant, `SpawnSiblingAgentWith`, instead.

## 3. Engineering decisions

Recorded so lanes do not relitigate them.

1. **One release, 0.60.0.** Lanes merge into `release/0.60` as their PRs go green; one gate and one release at the end.
2. **Foundation first, then one owner per file.** The foundation lane lands every shared file: protocol, dispatch arms, CLI and verb tables, settings rows, migrations, view and overlay registration, Tauri command registration, e2e suite registration, and a placeholder for every lane-owned module it wires. After it merges, every file in the 0.60 change set has exactly one owning lane. A lane that finds it needs a shared file stops and reports; the main thread makes that change on `release/0.60`.
3. **Protocol 66, once.** `PROTOCOL_VERSION` 65 to 66 with one gate constant, `STUDIO_PROTOCOL = 66`. `MIN_COMPATIBLE_PROTOCOL` stays 53. So: no new field on any struct an older peer decodes (`Agent`, `ContextUsage`, `AccountUsage`, `Snapshot`); new data travels in new events that `decodable_by` sends only to peers at 66 or later; new optional request fields go last with `#[serde(default, skip_serializing_if = "Option::is_none")]` and a client sends them only to a daemon that said 66 at Hello (the protocol 58 `SpawnSiblingAgent.profile` pattern), and only on a request whose current last field is always encoded (correction 14).
4. **Machine-local control, repo-local description.** What a repo *is* (inbox folder, departments, verdict folders, roles) lives in `<repo>/.kete/studio.toml` and `<repo>/.kete/roles.toml`, committed. What Jay *allows* (whether the queue runs, the autonomy level, exclusions) lives in `config.local.json` under `studio`, keyed by repo root, never exported and never sent to remotes, like `wake` since 0.54. A repo file can never raise autonomy.
5. **Only a person changes autonomy.** `SetStudio` goes through the daemon, which refuses it from a client inside a live agent session (the `check_provisioning_peer` process walk). The apps and `kete studio` from Jay's own shell pass.
6. **Roles are read from committed history.** The daemon reads `.kete/roles.toml` with `git show HEAD:.kete/roles.toml` in the project's root worktree, never a lane worktree's working copy. An uncommitted edit changes nothing; a committed one changes new sessions only (correction 10).
7. **Roles apply to Claude sessions only in 0.60.** A role on another harness is refused with the reason. `--tools`, `--permission-mode`, `--strict-mcp-config` and settings files are Claude flags.
8. **One generated settings file per role session.** `<data dir>/sessions/<agent id>/claude-settings.json` holds the statusLine block Kete already passes, the session's permission rules and the guard hook. It replaces the shared `claude-statusline.json` on that session's `--settings`. Rules never go on the command line (Windows caps a command line at 32,767 characters).
9. **Kete feeds the manager; the manager decides.** The queue runner never spawns anything. It writes one short `[from kete studio]` note into an idle orchestrator naming new inbox notes and account headroom. Spawning, routing and merging stay the orchestrator's work, inside the autonomy level.
10. **Autonomy is enforced where Kete can enforce it.** A0 refuses orchestrator and sibling spawns into the repo. A1 and A2 add deny rules to every session the orchestrator spawns there and every role session: A1 denies push, pull request, ship and tag commands; A2 denies tag and release commands; A3 adds none. Sessions Jay starts himself without a role are never restricted.
11. **Read-only pages read files, not the daemon.** The board and the gauntlet view read the repo's files through `nebula_core::studio` in the desktop backend and in TUI background jobs (the `view_jobs` pattern). The daemon only supplies what it already has: sessions, statuses, contexts, roles, spend.
12. **Verdict writes stay in the Studio app** (studio design question 6, default). K5 links out; it has no approve, split or baseline buttons.
13. **Force-push is refused twice.** Deny rules in the session's settings, plus `kete _guard`, a PreToolUse command hook for Bash and PowerShell that parses the command and exits 2 on any force-push form. The guard needs no daemon, so a hung daemon cannot let a push through.
14. **The ledger is SQLite.** Migration 37 adds `session_spend`, one row per session per local day. The daemon updates it at every turn end; `kete usage` asks the daemon. No file rotation.
15. **Tests never run real Claude.** The Windows e2e fake agent (`crates/nebula/tests/e2e_windows/agent.rs`) stands in for every session. CLI flags are checked against `claude --help` text only.

## 4. Shared contracts (the foundation lane lands these)

### 4.1 Protocol 66

`crates/nebula-core/src/protocol.rs`: `PROTOCOL_VERSION = 66`, `MIN_COMPATIBLE_PROTOCOL = 53`, and

```rust
/// The first protocol whose daemon takes the 0.60 studio requests and the
/// trailing `role_card` fields, and whose peers decode the 0.60 events.
pub const STUDIO_PROTOCOL: u32 = 66;
```

New `ClientRequest` variants, appended after `ClearEndedSubagents` in this order:

```rust
// -- protocol 66
SetStudio { req_id: u64, project: ProjectId, studio: Option<crate::studio::StudioSettings> },
StudioQueue { req_id: u64, project: Option<ProjectId>, run: bool },
RenewOrchestrator { req_id: u64, caller: Option<AgentId>, note: PathBuf, profile: Option<String>,
                    model: Option<String>, keep_old: bool, now: bool },
UsageLedger { req_id: u64, since: Option<String>, until: Option<String>, project: Option<ProjectId> },
```

`SetStudio` with `studio: None` switches the repo's studio off and is answered with `Ack`. `StudioQueue` with `run: false` reports what the runner would do now and changes nothing; `run: true` runs one tick at once. `since` and `until` are local dates, `YYYY-MM-DD`, inclusive.

New trailing field, `#[serde(default, skip_serializing_if = "Option::is_none")] role_card: Option<String>`, on `CreateAgent` (last field today: `profile`, always encoded), `StartOrchestrator` (`replace`, always encoded) and `OrchestratorSpawnWith` (`starting_prompt`, always encoded). With `role_card: None` each frame is the 65 frame byte for byte.

`SpawnSiblingAgent` keeps its 58 shape (correction 14). A sibling spawn with a role uses a new variant appended after `UsageLedger`, every field always encoded:

```rust
SpawnSiblingAgentWith { req_id: u64, id: AgentId, kind: Option<AgentKind>, starting_prompt: String,
                        worktree: Option<String>, base: Option<String>, profile: Option<String>,
                        role_card: String },
```

A client sends it only to a 66 daemon and only when a role is named; without one it sends `SpawnSiblingAgent` as before.

New `ServerEvent` variants, appended in this order, all gated `peer_version >= STUDIO_PROTOCOL` in `decodable_by`:

```rust
StudioQueueState { req_id: u64, rows: Vec<crate::studio::wire::StudioQueueRow> },
OrchestratorRenewal { req_id: Option<u64>, old: AgentId, new: Option<AgentId>, state: crate::studio::wire::RenewState },
SessionRoles { roles: Vec<crate::studio::wire::SessionRole> },
SessionSpend { rows: Vec<crate::studio::wire::SpendRow> },
UsageLedgerRows { req_id: u64, rows: Vec<crate::studio::wire::LedgerRow> },
```

Wire types, `crates/nebula-core/src/studio/wire.rs` (new structs, so positional encoding is safe):

```rust
pub struct StudioQueueRow { pub project: ProjectId, pub name: String, pub autonomy: Autonomy,
    pub inbox: String, pub open: u32, pub to_jay: u32, pub unannounced: u32,
    pub next: Vec<QueueNote>, pub held: Option<String>, pub last_note_at: Option<i64> }
pub struct QueueNote { pub id: String, pub title: String, pub to: String, pub priority: String, pub path: String }
pub enum RenewState { Scheduled, Done, Failed { reason: String } }
pub struct SessionRole { pub agent: AgentId, pub card: String, pub label: String, pub tier: u8,
    pub department: Option<String>, pub permission: String, pub context_budget: Option<u64> }
pub struct SpendRow { pub agent: AgentId, pub day: String, pub input_tokens: u64, pub output_tokens: u64,
    pub cache_read_tokens: u64, pub cache_write_tokens: u64, pub cost_usd: f64, pub tool_uses: u32, pub turns: u32 }
pub struct LedgerRow { pub spend: SpendRow, pub session: String, pub project: String, pub branch: Option<String>,
    pub role_card: Option<String>, pub profile: Option<String>, pub model: Option<String>,
    pub status: AgentStatus, pub pr_url: Option<String> }
```

`SessionRoles` is pushed after the `Snapshot` to a 66 peer and again whenever a role session is created or archived. `SessionSpend` carries today's row for one session and is pushed at each of its turn ends. `OrchestratorRenewal` answers `RenewOrchestrator` with `Scheduled`, then is broadcast with `req_id: None` on `Done` or `Failed`.

Protocol tests follow the existing pattern (protocol.rs:1460-1593): a 65 frame of each widened request decodes on 66 with `role_card: None`; a request without a role card encodes to the 65 frame byte for byte; every combination of None and Some across each widened request's optional fields round-trips to the same values (so no field can land in another's slot); `SpawnSiblingAgent` with and without `profile` is unchanged from 58; every new event is refused by `decodable_by(65)`; `MIN_COMPATIBLE_PROTOCOL` is still 53.

### 4.2 SQLite

Appended to `MIGRATIONS` in `crates/nebula-daemon/src/store.rs`:

- **36:** `ALTER TABLE agents ADD COLUMN role_card TEXT;` (K3: a resume or restart re-applies the same card).
- **37:** `CREATE TABLE session_spend (agent_id TEXT NOT NULL, day TEXT NOT NULL, project TEXT, branch TEXT, session TEXT, model TEXT, profile TEXT, role_card TEXT, input_tokens INTEGER NOT NULL DEFAULT 0, output_tokens INTEGER NOT NULL DEFAULT 0, cache_read_tokens INTEGER NOT NULL DEFAULT 0, cache_write_tokens INTEGER NOT NULL DEFAULT 0, cost_usd REAL NOT NULL DEFAULT 0, tool_uses INTEGER NOT NULL DEFAULT 0, turns INTEGER NOT NULL DEFAULT 0, first_at INTEGER NOT NULL, last_at INTEGER NOT NULL, PRIMARY KEY (agent_id, day));` No foreign key: the ledger outlives a deleted session, and `project`, `branch` and `session` are copied in so an old row still says what it was.

Store functions (foundation, with unit tests): `set_agent_role_card(&self, id: &AgentId, card: Option<&str>) -> Result<()>`, `agent_role_cards(&self) -> Result<Vec<(AgentId, String)>>`, `add_spend(&self, d: &SpendDelta) -> Result<SpendRow>` (upsert, adding the delta to the row for `(agent, day)`), `spend_rows(&self, since: Option<&str>, until: Option<&str>, project: Option<&ProjectId>) -> Result<Vec<LedgerRow>>`. `SpendDelta` is a daemon struct in store.rs with the table's columns.

### 4.3 Files and settings

| Where | Key or file | Owner lane | Meaning |
|---|---|---|---|
| `config.local.json` | `studio` (map keyed by repo root) | foundation (reader, writer), queue (behaviour) | `{ "studio_inbox": "<repo-relative dir>", "autonomy": "A0" to "A3", "queue_exclude": [ids or about values] }`. Absent entry: studio off for that repo. In `NOT_ROWS`; written only through `SetStudio`. |
| `config.json` row | `studio_idle_after` | foundation (row), queue (behaviour) | How long the orchestrator must sit idle before an idle note: `30s`, `45s` (default), `60s`, `120s`. Settings, General, group "Studio". |
| `config.json` row | `usage_ledger` | foundation (row), ledger (behaviour) | `on` (default) or `off`. Settings, Sessions, group "Usage". Off keeps existing rows and records nothing new. |
| `config.json` `profiles[]` | `reserved` | foundation (field), accounts (behaviour) | `true`: never picked for a launch the orchestrator or a script starts. Default `false`. |
| `<repo>/.kete/studio.toml` | manifest | foundation (reader) | Section 4.4. |
| `<repo>/.kete/roles.toml` | role cards | roles | Section 7. |
| `<data dir>/sessions/<agent id>/claude-settings.json` | per-session settings | roles | Decision 8. Removed when the session is deleted. |
| `<data dir>/studio/queue-state.json` | announced notes, last idle note | queue | Survives a daemon restart, so a restart never repeats a note. |
| `<data dir>/studio/refusals.jsonl` | refused unattended launches and A0 spawns | roles, queue (each appends through `studio::refusals::append`, foundation) | One line per refusal; the board's What needs you strip reads the last 20. |
| `<data dir>/orchestrator-renewals.jsonl` | renewal log | renew | One line per renewal: at, old, new, note path, state. |
| `<data dir>/profiles/.picks.json` | pick holds | accounts | Holds from `kete profile pick`, each 600 s. |

### 4.4 `.kete/studio.toml`

```toml
version = 1
inbox = "notes/inbox"                 # repo-relative; the default for studio_inbox
departments = "docs/DEPARTMENTS.md"   # first markdown table, first column, one department per row
signoffs = "reviews/signoffs"         # <signoffs>/<kind>/<id>.json verdict files
evidence = "reviews/evidence"         # EV folders a verdict's `evidence` field names
studio_url = "http://127.0.0.1:5173"  # a local review app the pages link into; optional
recipients = ["orchestrator", "manager"]  # `to:` values announced to the orchestrator

[kinds]   # verdict folder -> department slug, for the board's last-gauntlet cell
art = "art"

[links]   # verdict folder -> route on studio_url; {id} is replaced
art = "/art?id={id}"
```

Every key is optional except `version`. A department slug is its label lowercased with each run of other characters turned into one `-` ("Art and 3D" is `art-and-3d`). `nebula_core::studio::manifest::load(repo_root) -> Result<Option<StudioManifest>, String>` reads it (at most 65,536 bytes; a parse error names the line). TOML comes from the `toml` crate, already in Cargo.lock at 1.1.8 through `winresource`.

### 4.5 Inbox notes

`nebula_core::studio::inbox` (foundation) reads Markdown notes with YAML-style frontmatter, the shape the Studio app writes today:

```
---
id: 2026-10-21-storm-sky-banding     # optional; default: file name without .md
status: open                          # new, open, resolved, done, closed, rejected
to: orchestrator                      # a recipient, a department slug, or jay
about: World and weather
priority: high                        # high, normal (default), low
title: Storm sky banding at dusk
at: 2026-10-21T05:52:13Z
by: jay
---
Body text.
```

Only the first 4,096 bytes of a note are read; a folder lists at most 2,000 notes; a file over 1 MiB is skipped. `read_dir(dir) -> Vec<InboxNote>`, `parse(path, text) -> Option<InboxNote>`, and `eligible(notes, recipients, exclude) -> Vec<&InboxNote>` (status new or open, `to` set, not excluded, ordered by priority, then `at` oldest first). The writer and its duplicate check belong to K6 (`inbox_write`).

### 4.6 Verdict files

`nebula_core::studio::verdicts` (foundation) reads `<signoffs>/<kind>/<id>.json` leniently: `Verdict { id, kind, verdict, by, at, note, tags, evidence, reconciled: Option<PathBuf>, path, modified }`. Unknown fields are ignored; a missing `id` is the file stem; `<id>-reconciled.json` is linked to `<id>` rather than listed twice. `read_kind(dir, kind)`, `kinds(signoffs_dir)` and `newest(signoffs_dir, kind)`. At most 5,000 files per kind are read.

### 4.7 Verbs, views, overlays

Appended to `ACTIONS` after `ask_aside` (107 becomes 113), unbound by default and in the unbound allowlist:

| Id | Applies to | Does |
|---|---|---|
| `studio_board` | Global, Repo | Opens the studio board (on that repo) |
| `gauntlet_view` | Global, Repo | Opens the gauntlet view |
| `triage_note` | Global | Focuses the triage strip (desktop), or opens the triage line (TUI) |
| `studio_autonomy` | Repo | Opens the autonomy control |
| `renew_orchestrator` | Session (the orchestrator only) | Opens the renew form |
| `usage_csv` | Global | Exports the usage ledger to `<data dir>/exports/usage-<date>.csv` and shows the file |

`MENUS`: Repo gains `studio_board`, `gauntlet_view`, `studio_autonomy`; Session gains `renew_orchestrator`; Global gains `studio_board`, `gauntlet_view`, `triage_note`, `usage_csv`. Rows are state-aware: the studio rows show only for a repo with a manifest or a `studio` entry, `renew_orchestrator` only on the orchestrator, `triage_note` only while an orchestrator or a triage desk exists. `actions.gen.ts` is regenerated (`KETE_BLESS=1 cargo test -p nebula-config actions_gen`).

Desktop `View` gains `'studio' | 'gauntlet'`. The rail gains Studio and Gauntlet entries, shown once any repo has a manifest or a `studio` entry. TUI `Overlay` gains `StudioBoard(crate::studio_board::BoardView)` and `Gauntlet(crate::gauntlet_view::GauntletView)`; `PromptKind` gains `RenewNote`.

## 5. K1: accounts for scripts

**User story.** As the studio manager I run `kete profile pick --json` before each spawn and pass the answer to `--profile`, so lanes spread over the accounts with headroom and never land on Jay's own account. As Jay I run `kete profile list --json` to see every account's windows in one read.

**Data model.** `Profile` gains `#[serde(default)] reserved: bool` (the foundation adds the field, so the queue lane can name reserved accounts in its idle note; this lane gives it meaning). `nebula_core::accounts` gains `enum Launcher { Person, Automation }` (foundation adds the enum and the parameter) and `pick_for(all, harness, project_default, policy, launcher, now_secs, usage, load) -> Pick`. For `Launcher::Automation`, reserved and disabled profiles are never candidates; `manual` spread is read as `least_used` (manual would hand every lane the default account, which is usually the reserved one); a project default that is reserved is skipped. `load` is a closure giving live sessions plus holds per profile. Ranking for `least_used`: usage in 10-point bands (the higher of the 5-hour and weekly percent), then load, then name. `Reason` gains `Reserved` for the `--dry-run` table.

**Protocol.** None. `kete profile list` and `pick` read the usage cache (`usage::read_all_in`) and count live sessions with the existing `ListSessions`; with no daemon, load is holds only.

**Daemon.** `resolve_profile_name` (registry.rs:1060) passes `Launcher::Automation` for sessions created through `OrchestratorSpawn`, `OrchestratorSpawnWith` and renewal, and for any launch that carries a role card (the triage desk included); it passes `Launcher::Person` for every other launch, which is every launch Jay makes from the apps without a role. The foundation adds the parameter and the call sites; this lane writes the rule.

**CLI.**
- `kete profile list [--json]`: JSON `{ spread, threshold, accounts: [{ name, harness, kind ("profile" or "own_login"), default, disabled, reserved, signed_in, email, windows: [{ kind, percent, resets_at, estimated, read_at }], live_sessions, holds }] }`.
- `kete profile pick [--harness claude] [--project P] [--dry-run] [--json]`: prints the profile name (plain) or `{ harness, profile, reason, line, candidates: [{ name, eligible, why, percent, live_sessions, holds }], held_until }`. Without `--dry-run` it records a 600 s hold in `<data dir>/profiles/.picks.json`, so three picks in a row spread before the readings move; `--dry-run` records nothing and prints the candidate table. Exit code 3 when no profile is eligible.
- `kete profile reserve <name>` and `kete profile unreserve <name>`.

**TUI.** The accounts overlay marks a reserved account `(yours)` and toggles it with a key the lane picks from the overlay's free keys; the account menu gains Reserve for me / Stop reserving.

**Desktop.** AccountsView rows gain a Reserve for me switch with the tooltip "Kept for your own sessions; the orchestrator and scripts never pick it". The row's account chip `claude@max2` (since 0.46) is how a spawned session shows its pick; no new chip and no protocol change.

**Settings keys.** `profiles[].reserved`.

**Tests.** Unit: `pick_for` with reserved defaults, all reserved (exit 3), manual read as least_used for automation, the 10-point band and load tie-break, holds expiring after 600 s, `Launcher::Person` unchanged from 0.59 (the existing `pick` tests stay green). CLI: `list --json` and `pick --json` shapes in `crates/nebula/tests/` against an isolated `NEBULA_DATA_DIR`. Windows e2e (`e2e_windows/accounts.rs`): two fake profiles with usage files at 20 and 70 percent, the lower one reserved; `kete orchestrator spawn` with no profile lands on the unreserved one and the card shows `claude@<it>`.

**Docs.** docs/profiles.md: Reserved accounts, scripted picks, the JSON shapes.

**Open question.** Studio design question 1 (account pool). Default: Kete ships the flag and reserves nothing itself; the install note asks Jay to run `kete profile reserve jayrich` (the recommendation). If Jay wants his account in the pool, he skips that step; no code changes.

## 6. K2: studio inbox, autonomy and idle notes

**User story.** As Jay I point Kete at a repo's inbox folder and set how much the studio may do. When the orchestrator goes idle and I am not typing to it, Kete tells it what is new in the inbox, so the studio keeps moving between my visits, and when I set A0 nothing new starts.

**What a level change reaches.** A0 stops idle notes and refuses new spawns at once. Sessions already running keep going: their deny rules are the ones they started with, but `kete _guard` reads the repo's live autonomy on every shell command (section 7), so a lowered level stops their pushes and tags from the next command. Stopping a running session stays Jay's own action.

**Autonomy levels** (studio design 4.4, decision 0121):

| Level | Kete enforces |
|---|---|
| A0 Hold | No idle notes. `OrchestratorSpawn`, `OrchestratorSpawnWith`, `SpawnSiblingAgent` and `SpawnSiblingAgentWith` into the repo are refused: "autonomy A0 (Hold) for <repo>". `denies()` returns A1's list, which the guard applies to sessions already running. |
| A1 Plan (default) | Idle notes. Spawned and role sessions in the repo get A1 denies: `git push`, `gh pr`, `gh release`, `git tag`, `kete ship`, `kete ado pr` (Bash and PowerShell forms). |
| A2 Deliver | Idle notes. A2 denies: `git tag`, `gh release`, `git push --tags`, `git push` of `refs/tags/`. |
| A3 Release | Idle notes. No autonomy denies. |

The table lives in `nebula_core::studio::autonomy::Autonomy::denies` (foundation). Governance and force-push denials (section 7) apply at every level.

**Data model.** `config.local.json` `studio` entry (section 4.3). `<data dir>/studio/queue-state.json`: `{ announced: { "<repo>": { "<note id>": <file mtime> } }, last_note_at: <epoch>, notes_this_hour: [<epoch>, ...] }`.

**Protocol.** `SetStudio`, `StudioQueue`, `StudioQueueState` (section 4.1).

**Daemon** (`crates/nebula-daemon/src/registry/studio_queue.rs`, `studio_policy.rs`). The 30 s tick (lib.rs:384-399) calls `studio_queue::tick`. A tick writes an idle note only when every condition holds:

1. at least one repo has a `studio` entry with autonomy A1 or higher, and its inbox has eligible notes not yet announced at their current mtime;
2. an orchestrator exists and its status is `Finished` or `Fresh` (never `Running` or `NeedsFeedback`), unchanged for at least `studio_idle_after` (default 45 s);
3. no client has sent `Input` to the orchestrator for 120 s (the foundation tracks `last_input_at` per session in the Input path);
4. fewer than 6 idle notes in the last 60 minutes.

The note goes in through `session_context::write_prompt`, at most 1,200 bytes, at most 5 notes listed, highest priority first:

```
[from kete studio] 14:05 · wheako A2 · 3 new inbox notes
1. high · world-and-weather · notes/inbox/2026-10-21-storm-sky.md · Storm sky banding at dusk
2. normal · orchestrator · notes/inbox/2026-10-21-ledger.md · Shift ledger
Accounts: max2 5h 34% (resets 3:10 pm); jayrich reserved.
Act within A2 (Deliver). Set each note's status when you take it.
```

A note is announced once per mtime: editing it re-announces it; an open note left alone is never repeated. `queue_exclude` drops notes by id or `about`. **Context nudge:** when the orchestrator's context is at or over 60 percent of its role card's `context_budget` (or of its model window without a card), the note ends with "Context 61% of 400k: write your handover note, then run `kete orchestrator renew --from <note>`." and the nudge goes out alone, under the same conditions, at most once per 30 minutes. `Daemon::studio_check_spawn(&self, project) -> Result<(), String>` (in `studio_policy.rs`) is the A0 refusal, called from the spawn handlers the foundation wires. `SetStudio` is refused inside agent sessions (decision 5).

**CLI** (`crates/nebula/src/studio.rs`).
- `kete studio enable <project> [--inbox DIR]`: writes the entry with autonomy A1 and the manifest's `inbox` unless `--inbox` names one.
- `kete studio disable <project>`.
- `kete studio status [--json]`: per repo the autonomy, the inbox, open and to-Jay counts, the next notes, why the runner is holding (`held`), the last idle note.
- `kete studio autonomy <project> [A0|A1|A2|A3]`: prints, or sets.
- `kete studio exclude <project> <id-or-about>` and `kete studio include <project> <id-or-about>`: edit `queue_exclude`.
- `kete studio queue [--project P] [--now] [--json]`: what the runner would send now; `--now` sends it if the conditions hold.

**TUI.** The board overlay's header line (drawn by `studio_queue_view::header_line`) shows the autonomy level and the runner's state ("idle note at 14:05", "holding: you typed 40 s ago"). Keys in the board step autonomy behind a confirm when raising to A2 or A3.

**Desktop.** The board's header (`lib/studio/queue/QueueHeader.svelte`): an autonomy control (four segments A0 to A3 with their names, raising past A1 asks first), the runner state line, and "Send now". The control calls `SetStudio` through the backend.

**Settings keys.** `studio` (machine-local), `studio_idle_after`.

**Tests.** Unit (core `studio::queue`): eligibility and ordering; mtime re-announce; exclude by id and about; every holding reason; the 6 per hour cap; the 1,200-byte cap; the context nudge threshold and its 30-minute repeat. Daemon unit: `SetStudio` refused from inside a session (the orchestrator tests' peer fixture). Windows e2e (`e2e_windows/queue.rs`): a repo with three notes, `kete studio enable`, an idle fake orchestrator; `kete studio queue --now` puts `[from kete studio]` into the orchestrator's pane (the fake agent's echo log); a second `--now` sends nothing; editing one note re-announces it; `kete studio autonomy <repo> A0` then `kete orchestrator spawn --project <repo>` is refused with the A0 reason.

**Docs.** docs/studio-queue.md (new); docs/orchestrator.md's "Follow-ups" line about idle-note injection becomes a link (the renew lane owns that file and makes the edit).

**Open questions.** Question 2 (autonomy default): A1 ships as the default and nothing raises it; Jay raises wheako with `kete studio autonomy wheako A2` once he has watched a shift. Question 7 (paused lanes): Jay's `queue_exclude` lists library-sweep-2, mech-econ-residue and artux-residue until he rules.

## 7. K3: role cards (and K9's permission profiles)

**User story.** As the studio manager I spawn `--role runner` and the session comes up on Haiku with only Read, Write and Bash, the runner's prompt appended, no MCP servers, and permission to run only the repo's named scripts. As Jay I read `kete roles check` and see exactly what every role gets before a shift.

**`.kete/roles.toml`.**

```toml
version = 1

# Repo-relative paths no allowlist or unattended session may edit.
governance = ["docs/decisions/**", "docs/QUESTIONS.md", ".kete/**"]

[roles.runner]
label = "Runner"                 # chip text; default: the id
model = "haiku"                  # alias or full model id; an explicit --model on the launch wins
effort = "low"                   # low, medium, high, xhigh, max
department = "audio"             # board column slug; optional
tier = 3                         # 0 to 3, shown on the board; default 2
context_budget = 100000          # tokens; the board warns at 60 and 90 percent
prompt_file = ".kete/roles/runner.md"   # or prompt = "..."; at most 8,192 bytes
tools = ["Read", "Write", "Bash"]       # the whole built-in tool set (--tools); omitted keeps Claude's default
allow = ["Bash(uv run tools/check.py*)"]  # pre-approved calls; for allowlist, the only calls that run
deny = ["WebFetch", "WebSearch"]          # more denials
mcp = "none"                     # none (--strict-mcp-config, no servers) or default
permission = "allowlist"         # prompt (default), allowlist or unattended
```

At most 64 roles and 65,536 bytes. A parse error refuses the launch and names the line; Kete never starts a role session without its restrictions.

**Permission profiles** (K9's "unattended permission profile per role"):

| Profile | Claude flags | Rules in the session settings file |
|---|---|---|
| `prompt` | none added (Claude's own starting mode) | allow = role `allow`; deny = built-in + governance + autonomy + role `deny` |
| `allowlist` | `--permission-mode dontAsk` | allow = role `allow`; deny = built-in + governance + autonomy + role `deny` + `AskUserQuestion` |
| `unattended` | `--permission-mode bypassPermissions` | deny = built-in + governance + autonomy + role `deny` + `AskUserQuestion` (allow rules have no effect in this mode) |

Governance denies apply when the repo has a roles file; autonomy denies when the repo has a `studio` entry. Every role session gets a settings file and the guard, whatever its profile.

**Sessions the orchestrator spawns without a role.** In a repo with a `studio` entry, a session created through `OrchestratorSpawn` or `OrchestratorSpawnWith` with no role card is treated as the `prompt` profile with no role rules: it gets a settings file with the built-in, governance and autonomy denies and the guard (decision 10). Outside studio repos, and for every session Jay starts himself without a role, nothing changes from 0.59.

**Built-in denies** (a roles file cannot remove them):
- Force-push, as Bash and PowerShell rules: `git push --force*`, `git push -f*`, `git push * --force*`, `git push * -f*`, `git push --force-with-lease*`, `git push * --force-with-lease*`, `git push * +*`, `git push --mirror*`, `git push * --mirror*`.
- Edit and Write on Kete's data and config folders (`//<data dir>/**`, `//<config dir>/**`, in the POSIX form Claude matches on Windows, `//c/Users/...`), on `.claude/**` and `.kete/**` in the session's worktree and in the project's root worktree, and on the session's own settings file.
- Governance paths from `roles.toml`, made absolute for the session's worktree and for the root worktree.

**The guard.** `kete _guard --session <id>` is a PreToolUse command hook (matcher `Bash|PowerShell`, exec form on Windows as 0.54's B3 does) in the session settings file. It reads the hook JSON on stdin, splits the command on `&&`, `||`, `;` and pipes, finds every `git` invocation (through `-C <dir>`, `-c k=v` and environment prefixes), and exits 2 with a reason for: any force flag, a `+` refspec, `--mirror`, deleting the remote default branch, and the autonomy level's denied commands. It reads the repo's live autonomy from the `studio` map in `config.local.json` (which sessions are denied from editing), falling back to the level written in its settings file, so it needs no daemon and a lowered level applies from the next command.

**Bypass acceptance.** Before an `unattended` launch the daemon reads the account's user settings (`<profile home>/claude/settings.json`, or `~/.claude/settings.json` for an adopted login). Without `skipDangerousModePermissionPrompt: true` it refuses: "account max2 has not accepted bypass mode: run `kete roles accept-bypass max2` in your own terminal". `kete roles accept-bypass <profile>` runs `claude --permission-mode bypassPermissions` under that profile in the current terminal, as `kete profile login` runs a sign-in; Jay answers Claude's own dialog. It refuses to run inside a Kete session.

**Data model.** Migration 36 `agents.role_card`. The daemon keeps the parsed card per project in memory, keyed by the root worktree's HEAD, re-read when HEAD moves.

**Protocol.** `role_card` on `CreateAgent`, `StartOrchestrator`, `OrchestratorSpawnWith`; `SpawnSiblingAgentWith`; `SessionRoles` (section 4.1).

**Daemon** (`crates/nebula-daemon/src/registry/roles.rs`). `Daemon::launch_overlay(&self, agent, project) -> Result<LaunchOverlay, String>` returns the flags (`--tools`, `--permission-mode`, `--strict-mcp-config`, model and effort when the launch named none), the role prompt for the system prompt join, and writes the session settings file. The foundation's argv builder applies a `LaunchOverlay` (an empty one changes nothing, by test). The role prompt goes after Kete's worktree guidance and before the PR or issue rule, in that fixed order every time, so the cached prefix is stable. Unattended and allowlist refusals append a line to `<data dir>/studio/refusals.jsonl`.

**CLI** (`crates/nebula/src/roles.rs`, `guard.rs`). `kete roles check [--project P] [--role R] [--json]` prints each role's argv additions and settings file as they would be written, from the committed roles file; `kete roles accept-bypass <profile>`; `kete _guard` (hidden). `--role <id>` on `kete spawn`, `kete orchestrator spawn` and `kete orchestrator start`.

**TUI.** Cards and the pane header show the role label beside the account chip (`runner · claude@max2 haiku`). The quick prompt's details row gains a role field when the repo has roles, stepped like the profile field.

**Desktop.** `RoleChip` on session rows, board cards and the session header; a role picker in the quick prompt and the Start orchestrator form, shown when the repo has roles, with each role's model and permission in the option text.

**Tests.** Unit (core `roles`): parse and refuse (bad version, unknown permission, prompt over 8,192 bytes, 65 roles); compose for each profile (rule lists exact, order exact); Windows paths to `//c/...` rules; the guard on 30 commands (force forms, `-C`, environment prefixes, chains, PowerShell, safe pushes that must pass, tag pushes per autonomy). Daemon unit: an empty overlay leaves the 0.59 argv byte for byte; roles read from HEAD, not the working copy; bypass acceptance refusal. Windows e2e (`e2e_windows/roles.rs`): a repo with a committed roles file; `kete orchestrator spawn --role runner` gives the fake agent `--tools Read,Write,Bash`, `--permission-mode dontAsk`, `--strict-mcp-config`, `--model haiku` and a `--settings` file whose deny list holds the governance and force-push rules; `kete _guard` fed a `git push --force` hook payload exits 2; an `unattended` role on an account without acceptance is refused with the command to run; the card shows the role label after a daemon restart (migration 36); at A1, `kete orchestrator spawn` with no role in a studio repo gets a settings file denying `git push`, and the guard refuses `git push` for it.

**Docs.** docs/roles.md (new): the file, the profiles, the built-in denies, the guard, acceptance, what deny rules do not catch, and that a card edit reaches new sessions only.

**Open questions.** Question 3 (department lead tiering): Kete is tier-agnostic; Jay's roles file (Appendix A) takes section 4.1's recommendation of four Opus departments. Question 8 (unattended mode): bypass with deny rules, the recommendation, is the `unattended` profile; a repo opts in per role.

## 8. K4: studio board

**User story.** As Jay I open one page and see each department's queue, its lead, its runners, its last gauntlet verdict, what is waiting on me and what it cost today, without a second copy of anything.

**Data.** `nebula_core::studio::board::build(repo_root, manifest, settings, sessions, roles, spend, refusals, now) -> Board`: departments from the manifest's `departments` file; for each, open notes `to` that slug, the lead (a live session whose role card has that department and tier 1 or 2), runners (tier 3), the newest verdict from `verdicts::newest` for the kinds mapped to it, notes `to: jay` about it, sessions needing feedback, and today's spend summed from `SessionSpend`. Sessions with no department go in an "Unassigned" column. A What needs you strip sits above the columns: notes to Jay, sessions needing feedback, the last 20 refusals, a failed renewal.

**Protocol.** None; the board reads `SessionRoles` and `SessionSpend` that clients already hold.

**Desktop** (`lib/studio/board/`). Departments as columns 240 px wide in a horizontal scroll (2 visible at 640 px, 5 at 1280 px). Each column: queue count and the top 3 notes (title, priority, age), the lead row (state dot with its glyph, model, account, context against budget, amber at 60 percent, red at 90), runner chips, the last verdict (word, id, age), blocked-on-Jay count, today's tokens and estimated USD. A click on a note opens it in Kete's file view; on a session, opens the session; on a verdict, opens the gauntlet view on it. Refreshes every 10 s while visible and on window focus.

**TUI** (`studio_board.rs`, `ui/studio_board.rs`). One row per department (80 columns hold name, queue, lead dot, runners, last verdict, blocked, spend); Enter expands a department to its notes and sessions; Enter on a session attaches it. Background reads every 10 s while open.

**Tests.** Unit: department parsing from a markdown table (slugs, a missing file, a table with no rows); board assembly from fixtures; the Unassigned column; budget colours at 59, 60, 89 and 90 percent. TUI draw tests at 80x24 and 120x40 and input parity for row activation. Desktop vitest for the column model; desktop e2e (`e2e/studio.e2e.mjs`): a fixture repo with a manifest and notes opens the board, shows its columns, and a note click opens the file.

**Docs.** docs/studio-board.md (new).

**Open question.** None of its own. Its verdict cell follows question 6's default (decision 12).

## 9. K5: gauntlet view

**User story.** As Jay I see every verdict by kind, newest first, with its evidence one click away, and jump into the Studio app to rule on one.

**Data.** `nebula_core::studio::gauntlet::list(signoffs, evidence, kinds) -> Vec<KindVerdicts>` on `verdicts::read_kind`, adding the evidence folder's listing (at most 200 entries) and the reconciled link. Verdict words map to tones: `approve`, `approved`, `pass` read as done; `reject`, `rejected`, `regenerate`, `fail` as failed; `split`, `third-reviewer` as needs you; any other word plain, shown as written.

**Protocol.** None.

**Desktop** (`lib/gauntlet/`). Tabs per kind, a filter by verdict word and by age, rows with id, verdict, by, at, note; a row opens a detail with the note, tags, the reconciled file and the evidence listing (Open in Kete, Show in Explorer). Open in Studio builds `studio_url + links[kind]` with `{id}` replaced and opens it in the browser; with no `studio_url` the button is absent. No write actions (decision 12).

**TUI** (`gauntlet_view.rs`, `ui/gauntlet_view.rs`). Kind tabs, rows, Enter for the detail, `o` opens the Studio link.

**Tests.** Unit: lenient parsing (missing fields, unknown fields, a reconciled pair, a non-JSON file skipped); tone mapping; link building. TUI draw tests at 80x24 and 120x40; desktop vitest; desktop e2e (`e2e/gauntlet.e2e.mjs`): a fixture signoffs folder lists, filters and shows evidence.

**Docs.** docs/gauntlet.md (new).

**Open question.** Question 6 (verdict surface). Default: read-only with links, as recommended. Studio design section 10's "split-to-third-reviewer action" and "baseline approval button that writes the signoff" are not built this train. If Jay rules that Kete writes verdicts, that is a new lane (section 16).

## 10. K6: triage desk

**User story.** As Jay I type one line under the orchestrator ("the storm sky bands at dusk"), press Enter, and within one turn it is a filed inbox note with a department, a priority and a route, and I see the route in a line.

**Data** (`nebula_core::studio::inbox_write`). `write(dir, &NewNote { text, to, about, priority, by }, now) -> Result<Written, Duplicate>`: id `YYYY-MM-DD-<slug of the first 6 words>`, frontmatter `status: new`, `to: triage` unless given, `by: jay`, `at` now in UTC; written to a temp file and renamed. `Duplicate` when the id exists or an open note's title shares its first 5 normalised words (lowercase, letters and digits); it names the existing note.

**The preset.** The studio design's "quick prompt preset" is the `triage` role card (correction 12).

**Flow.** The note is written, then sent as one turn to the repo's triage desk: the live session with role card `triage`, started on first use in the repo's root worktree through `CreateAgent { role_card: Some("triage") }` (name "triage desk") when the repo's roles file defines `triage`, else the orchestrator, labelled `[triage]`. The turn asks the desk to set `to`, `about`, `priority` and `status: open` in the note and to answer in one line. The strip shows that line once the turn ends (read back through `ReadSession`), waiting up to 120 s; past that it says the note is filed and the desk is busy. A desk at or over 60 percent context is left and a fresh one started for the next note.

**CLI** (`crates/nebula/src/studio_note.rs`). `kete studio note "<text>" [--project P] [--to R] [--about A] [--priority high|normal|low] [--triage] [--json]`; exit 3 on a duplicate.

**Desktop** (`lib/triage/TriageStrip.svelte`, mounted by the foundation under the orchestrator's session pane). A one-line field, a target chip (Triage desk or Orchestrator), the repo it files into, the last route line and the last 3 notes filed (links). `triage_note` focuses it.

**TUI** (`triage_strip.rs`). In the orchestrator view's chip mode (`^q`), `t` opens a one-line triage field on the bottom row; Enter files and sends; the row then shows the route line until the next key.

**Tests.** Unit: id and slug, the duplicate rules, atomic write, frontmatter round trip through the foundation's `inbox::parse`. Windows e2e (`e2e_windows/triage.rs`): `kete studio note "storm sky bands at dusk" --triage --project <repo>` files the note and the fake desk's turn receives its path; the same line again is a duplicate (exit 3). TUI draw test of the strip at 80x24; desktop e2e (`e2e/triage.e2e.mjs`): type in the strip, the note file appears.

**Docs.** docs/triage.md (new).

**Open question.** None in section 13; the desk's model is section 4.1's Sonnet, set in Jay's roles file.

## 11. K7: usage ledger

**User story.** As the studio manager I run `kete usage --csv --since 2026-10-21` at each shift change and commit it as the studio's usage ledger, so the QA advisor can report cost per confirmed defect. As Jay I see what each lane cost today on the board.

**Sources** (correction 7). Per turn: input, output, cache-read and cache-write tokens summed from the transcript's assistant messages since the last turn end (deduplicated by message id; subagent transcripts included when Claude writes them beside the session's); the change in `cost.total_cost_usd` since the last turn end (a drop, after `/clear`, counts the new value whole); PostToolUse hooks counted during the turn; one turn. Rows are per local day.

**Data model.** Migration 37 `session_spend` (section 4.2). `nebula_core::usage_ledger`: `TranscriptTally` (an incremental reader keeping its byte offset per transcript), CSV writing.

**Protocol.** `UsageLedger`, `UsageLedgerRows`, `SessionSpend` (section 4.1).

**Daemon** (`registry/ledger.rs`). `Daemon::ledger_on_statusline` keeps the latest cost per session; `ledger_on_tool_use` counts; `ledger_on_turn_end` builds the delta, calls `store.add_spend`, and broadcasts `SessionSpend`. `usage_ledger = off` skips all three. The foundation wires the three calls.

**CLI** (`crates/nebula/src/usage.rs`). `kete usage [--since YYYY-MM-DD] [--until YYYY-MM-DD] [--project P] [--by session|day|role] [--csv]`; the default is today's table by session. CSV columns: `date,project,branch,session,role,account,model,input_tokens,output_tokens,cache_read_tokens,cache_write_tokens,cost_usd_estimate,tool_uses,turns,status,pr_url`. The cost column says estimate because it is Claude Code's list-price figure, not a bill.

**TUI.** `usage_csv` writes the export and flashes its path. Cards stay as they are.

**Desktop.** `SpendLine` in the session details rail ("today: 412k tokens, est. US$3.10, 58 tool uses, 9 turns"); the board's spend cell; `usage_csv` exports and shows the file in Explorer.

**Settings keys.** `usage_ledger`.

**Tests.** Unit: transcript tally on a synthetic transcript (duplicates by message id, a truncated last line, a subagent file); cost deltas across `/clear`; CSV escaping. Store unit tests from the foundation. Windows e2e (`e2e_windows/ledger.rs`): the fake agent gains a `usage:<in>,<out>,<cost>` step that appends an assistant line to its transcript and posts a statusLine; two turns give one row with both turns summed; `kete usage --csv` prints it; with `usage_ledger` off, nothing new is recorded.

**Docs.** docs/usage-ledger.md (new).

**Open question.** None in section 13; section 8's budget accounting is the requirement.

## 12. K8: registering the repos

Not a lane. After installing 0.60 on Jay's PC: `kete add C:\Code\wheako` and `kete add C:\Code\wheko-web`. Linked worktrees of both register on their own (0.57). Depends on question 4 (which shift change installs 0.60); default: between shifts, on Jay's word.

## 13. K9: orchestrator renewal

**User story.** As the studio manager, at 60 percent of my context budget I write a handover note and run `kete orchestrator renew --from <note>`; when my turn ends the daemon starts a fresh orchestrator on the same project with my note as its first prompt, moves the role to it and archives me, so the studio carries on across usage windows with no person in the loop.

**Protocol.** `RenewOrchestrator`, `OrchestratorRenewal` (section 4.1).

**Daemon** (`crates/nebula-daemon/src/orchestrator_renew.rs`).
1. **Who may ask.** With `caller: Some(id)`: `check_caller_peer` and `require_orchestrator` (only the orchestrator renews itself). With `caller: None`: `check_provisioning_peer` (Jay's own shell or the apps; never another session).
2. **The note.** A file inside the orchestrator's workspace (`<data dir>/orchestrator`) or inside a registered project, UTF-8, 1 to 65,536 bytes. Anything else is refused before anything changes.
3. **When.** The reply is `Scheduled`. The swap waits for the old session's turn to end (its `Finished` status), at most 300 s; `now: true` (or a caller that is not mid-turn) swaps at once.
4. **The swap.** Under the provisioning gate, `start_orchestrator` with `replace: true` demotes the old session and creates the new one already holding the role, so its cold spawn carries the orchestrator guidance (`launch_rule` adds it only to a session created as the orchestrator, and a partial unique index allows one orchestrator row). The new session takes the old one's project, harness, model and effort (or `model`), role card, and profile (or `profile`, which may come from `kete profile pick`), with the starting prompt `[from kete renew] You are the orchestrator, renewed from <old name> at <time>. Your handover note is <path>; it is quoted below. Read it, then carry on.` followed by the note. `StartOrchestratorSpec` gains `starting_prompt` (foundation). If the new session sends no hook within 60 s, it is stopped and archived, the old session is re-pinned with `Store::set_orchestrator` directly (not `Daemon::set_orchestrator`, which would type the guidance into it as a turn; its own system prompt already holds it), and the state is `Failed`.
5. **The old session.** Archived and its process stopped, unless `keep_old`, which leaves it running as a worker.
6. **The record.** A line in `<data dir>/orchestrator-renewals.jsonl`; `OrchestratorRenewal` broadcast to every 66 peer.

**CLI** (`crates/nebula/src/orchestrator_renew.rs`). `kete orchestrator renew --from <note> [--profile P | --pick] [--model M] [--keep-old] [--now] [--json]`. `--pick` runs K1's automation pick. The orchestrator guidance (foundation) teaches the command and the 60 percent rule.

**TUI.** On `Done` the orchestrator view follows the new session and a flash names the note; on `Failed` the flash gives the reason. `renew_orchestrator` opens a prompt for the note path.

**Desktop.** `RenewNotice.svelte` shows "Orchestrator renewed from <note>" with Open note; `RenewForm.svelte` (from the session menu) takes the note path, an account (offering the pick), Keep old session, and Renew now.

**Unattended permission profile per role.** Delivered by the roles lane (section 7).

**Tests.** Daemon unit: who may ask (the orchestrator, a worker refused, Jay's shell, a session's shell refused); note path checks (outside, too big, missing, not UTF-8); the swap waits for turn end and times out at 300 s; rollback when the new session never reports. Windows e2e (`e2e_windows/renew.rs`): a fake orchestrator; `kete orchestrator renew --from <workspace>/handover.md --now` from outside sessions starts a new orchestrator whose first prompt (agent log) holds the note text; `kete orchestrator status` names the new session; the old one is archived; `--keep-old` keeps it as a worker.

**Docs.** docs/orchestrator.md: Renewal section, the permission model lines for the new verb, and the Follow-ups list updated for idle notes (K2) and renewal.

**Open questions.** Question 8 (unattended mode) for the permission profiles, as in section 7. Question 4 (install timing): renewal is new in 0.60, so the manager cannot renew until 0.60 is installed; the install's `kete kill` stops it and Jay restarts it by hand once.

## 14. Security model

The socket stays same-user; that is the real boundary (docs/orchestrator.md, Permission model). 0.60 adds:

- **Autonomy and studio settings** change only through `SetStudio`, refused from inside agent sessions. Gap: a Kete terminal tab counts as Jay's own shell (the existing exemption); a script that writes `config.local.json` directly bypasses the daemon, and the Edit and Write denials on Kete's config folder catch only Claude's file tools and recognised shell writes.
- **Roles** come from committed history in the root worktree, and `.kete/**` is denied to allowlist and unattended sessions. Gap: an agent with push rights at A2 could commit a roles change and have the manager merge it; the governance list should name `.kete/**` (Jay's does), which makes a roles change a governance change in decision 0121's sense.
- **Force-push** is denied by rules and by the guard. Gap: a push from inside a script (`python -c "subprocess.run(['git','push','-f'])"`) passes both; branch protection on the remote is the boundary that holds, and the install note asks Jay to keep it on for `main` in wheako and wheko-web.
- **Unattended** runs only on accounts Jay accepted himself, only for roles a committed roles file marks `unattended`, and never with `AskUserQuestion`.
- **The guard** needs no daemon and reads only the session's settings file and the `studio` map in `config.local.json`, both of which the session is denied from editing.

## 15. Non-goals

- Kete writing verdicts (question 6 default).
- A full chat surface; K6 is the compact form (studio design section 10).
- Kete spawning work on its own; the queue runner only informs the orchestrator.
- Role cards for Codex, Copilot, Cursor and the other harnesses.
- Enforcing a runner's input-size cap; the card's `context_budget` is shown and warned on, and the runner's own prompt refuses oversized work.
- A file watcher; every new page polls at 10 s while visible, like the 0.56 git stamps.

## 16. Questions for Jay

Studio design section 13, with the default this train takes:

| Q | Topic | Default taken | Changes the plan? |
|---|---|---|---|
| 1 | Reserve Jay's account | Ship `reserved`; install note says `kete profile reserve jayrich` | No: config only |
| 2 | Autonomy default | A1 | No: `kete studio autonomy wheako A2` when he chooses |
| 3 | Lead model tiering | Section 4.1 (four Opus departments) in his roles file | No: roles file only |
| 4 | Which shift change installs 0.60 | Between shifts, on his word | No: timing only |
| 5 | Local VLM as a gauntlet worker | No Kete feature depends on it | No |
| 6 | Verdict surface | Read-only, links into Studio | **Yes.** "Kete writes verdicts" adds a verdict-write lane (approve, split to third reviewer, baseline approval, the `CulturalCheckMissing` refusal) and a STUDIO-SPEC amendment; K5 gains write actions and an e2e for the refusal |
| 7 | Paused lanes | Excluded through `queue_exclude` | No: config only |
| 8 | Unattended mode | Bypass with deny rules and the guard | **Yes.** "Prompt per write with Jay on call" replaces the `unattended` profile with `acceptEdits` plus a needs-you notification path to Jay, and the shift test changes from "no prompt" to "prompts answered" |

Jay's own actions, not questions: accept bypass mode once per account he lets run unattended (`kete roles accept-bypass <account>`), and keep branch protection on `main`.

## Appendix A: Jay's studio on this PC (example config, not code)

`C:\Code\wheako\.kete\studio.toml` (written in wheako by its W2 lane, not by Kete):

```toml
version = 1
inbox = "Design/Studio/inbox"
departments = "Design/Production/DEPARTMENTS.md"
signoffs = "Design/Reviews/signoffs"
evidence = "Design/Reviews/evidence"
studio_url = "http://127.0.0.1:5173"
recipients = ["orchestrator", "manager"]

[kinds]
art = "art-and-3d"
sound = "audio"
story = "production-and-tooling"

[links]
art = "/art"
sound = "/sound"
story = "/stories"
```

`C:\Code\wheako\.kete\roles.toml`, the shape only (wheako's lane writes the prompts):

```toml
version = 1
governance = ["DECISIONS/**", "Design/Production/ROOTS-STANDARD.md", "Design/Production/BASICS-CHECKLIST.md",
  "Design/Production/GAUNTLET-REVIEWS.md", "Design/Production/REVIEW-CADENCE.md", "Design/Production/MILESTONES.md",
  "Design/Analysis/GAPS-FOR-1.0.md", "Design/Analysis/SYSTEM-COVERAGE.md", "Design/Analysis/FEATURE-MATRIX.md",
  "Design/Mechanics/SYSTEMS-MAP.md", "Design/QUESTIONS.md", "Design/Studio/briefs/**", ".kete/**"]

[roles.manager]
model = "opus"
tier = 1
context_budget = 400000
permission = "unattended"

[roles.lead-gameplay]
model = "opus"
department = "gameplay-systems"
tier = 1
context_budget = 400000
permission = "unattended"

[roles.lead-audio]
model = "sonnet"
department = "audio"
tier = 2
context_budget = 200000
permission = "unattended"

[roles.runner]
model = "haiku"
tier = 3
context_budget = 100000
tools = ["Read", "Write", "Bash"]
mcp = "none"
permission = "allowlist"

[roles.triage]
model = "sonnet"
tier = 2
context_budget = 200000
tools = ["Read", "Edit", "Grep", "Glob"]
mcp = "none"
permission = "allowlist"
```

The `config.local.json` entry that `kete studio enable wheako` writes, with question 7's exclusions added by `kete studio exclude`:

```json
{ "studio": { "C:/Code/wheako": { "studio_inbox": "Design/Studio/inbox", "autonomy": "A1",
  "queue_exclude": ["2026-10-19-session-ledger", "library-sweep-2", "mech-econ-residue", "artux-residue"] } } }
```
