# Lane foundation: the shared contracts

Branch `release/0.60-foundation`, worktree `C:\Code\bro\.claude\worktrees\lane-060-foundation`, from `origin/release/0.60`. Opus lead, at most 2 Sonnet helpers. Read the plan's Lane rules first; they bind this brief.

Spec: section 3 (decisions 2, 3, 8, 11, 14), section 4 (all), and the "Daemon", "CLI", "TUI" and "Desktop" paragraphs of sections 5 to 13, which name the call sites this lane wires.

## Goal

Land every file that more than one lane would otherwise touch, so that after this PR merges each remaining file in the train has one owner. Concretely: protocol 66, migrations 36 and 37, the shared `nebula_core::studio` readers, the settings rows, the six verbs, the view and overlay registration, the Tauri command registration, the CLI tables, the e2e suite registration, and a placeholder for every lane-owned module that something here calls. With this PR alone, Kete 0.59 behaviour is unchanged: every existing test passes, every new request is answered with `Error { message: "not in this build yet" }` (or an empty list), and every new page renders an empty state.

## Files owned

Create or edit only these.

**Core** (`crates/nebula-core/`)
- `Cargo.toml`: add `toml = "1"` (1.1.8 is already in Cargo.lock through winresource).
- `src/lib.rs`: `pub mod studio; pub mod roles; pub mod usage_ledger;`.
- `src/protocol.rs`: spec 4.1 in full, with tests.
- `src/studio/mod.rs`, `wire.rs`, `autonomy.rs`, `manifest.rs`, `inbox.rs`, `verdicts.rs`, `settings.rs`, `refusals.rs`: complete, with tests.
- Placeholders (module doc comment naming the owning lane, plus any signature listed under Interfaces produced): `src/studio/queue.rs` (queue), `src/studio/board.rs` (board), `src/studio/gauntlet.rs` (gauntlet), `src/studio/inbox_write.rs` (triage), `src/roles/mod.rs` (roles), `src/usage_ledger.rs` (ledger).
- `src/accounts.rs`: only `pub enum Launcher { Person, Automation }` and a `launcher: Launcher` parameter on `resolve_launch` and `pick_for_project`, passed through and ignored. The accounts lane owns the rest.
- `src/profiles.rs`: only `#[serde(default)] pub reserved: bool` on `Profile`, with a round-trip test. The accounts lane owns the rest.

**Daemon** (`crates/nebula-daemon/src/`)
- `launch_overlay.rs` (new, complete): `LaunchOverlay`.
- `registry.rs`: `CreateAgentSpec.role_card`; `agent_spawn_command_with` takes `overlay: &LaunchOverlay` and applies it; `resolve_profile_name` takes and forwards a `Launcher`: `Automation` for orchestrator spawns, renewals and any launch with a role card, `Person` for the rest.
- `registry/spawn.rs`: calls `self.launch_overlay(&agent, &project)` before building argv; on `Err` the spawn fails with that message; passes the overlay; when `overlay.settings_path` is set it replaces the statusline file on `--settings`; stores `role_card` with `store.set_agent_role_card`.
- `registry/status.rs`: on a turn end (the `Finished` arm of `apply_status_effects`) calls `self.ledger_on_turn_end(&agent)`; on PostToolUse calls `self.ledger_on_tool_use(&agent)`.
- `registry/usage.rs`: `on_statusline` also calls `self.ledger_on_statusline(&agent, &reading, &raw)` with the raw statusLine JSON (keep the parsed `serde_json::Value` on the delivery).
- `registry/sessions.rs`: `last_input_at` bookkeeping if the Input path lives here; otherwise in server.rs.
- `server.rs`: dispatch arms for `SetStudio`, `StudioQueue`, `RenewOrchestrator`, `UsageLedger`; `privileged_caller` returns the caller for `RenewOrchestrator { caller: Some(..) }`; `SetStudio` and `RenewOrchestrator { caller: None }` run `check_provisioning_peer` first; the `OrchestratorSpawn`, `OrchestratorSpawnWith`, `SpawnSiblingAgent` and `SpawnSiblingAgentWith` handlers call `self.studio_check_spawn(&project)` first and refuse on `Err`; `SpawnSiblingAgentWith` is handled as `SpawnSiblingAgent` plus its role card; `role_card` threaded from every widened request into its spec; `Input` records `last_input_at`; after the `Snapshot`, a 66 peer gets `SessionRoles { roles: daemon.session_roles() }`.
- `orchestrator.rs`: `StartOrchestratorSpec.role_card` and `.starting_prompt` (passed to `create_agent`), `OrchestratorSpawnSpec.role_card`; `CLAUDE_ORCHESTRATOR_GUIDANCE` gains a "Studio" section teaching `kete studio status`, `kete studio queue`, `kete studio note`, `kete profile pick --json`, `kete usage --csv`, `--role` on spawn, the autonomy levels A0 to A3 (spec section 6 table, one line each), and the 60 percent handover rule with `kete orchestrator renew --from <note>`.
- `lib.rs`: module declarations; the 30 s tick (lib.rs:384-399) calls `studio_queue::tick(&daemon)`.
- `store.rs`: migrations 36 and 37 and the store functions in spec 4.2, with tests (`user_version == MIGRATIONS.len()` still holds; the v0.48 fixture upgrade test still passes).
- `hooks/installer.rs`: `CLAUDE_ALLOW_RULES` gains Bash and PowerShell rules for `kete studio status`, `kete studio queue`, `kete studio note`, `kete usage`, `kete profile list`, `kete profile pick` and `kete orchestrator renew` (never `kete studio autonomy`, `enable` or `disable`).
- Placeholders: `registry/studio_queue.rs`, `registry/studio_policy.rs` (queue), `registry/roles.rs` (roles), `registry/ledger.rs` (ledger), `orchestrator_renew.rs` (renew), each holding the `impl Daemon` methods listed below with neutral bodies.

**CLI** (`crates/nebula/src/`)
- `cli.rs` and `main.rs`: every command in spec sections 5 to 13 (listed below), dispatched to the owning lane's file.
- `profile.rs`: only the new `ProfileOp` variants with placeholder bodies. The accounts lane owns the rest.
- Placeholders: `studio.rs` (queue), `studio_note.rs` (triage), `roles.rs` and `guard.rs` (roles), `usage.rs` (ledger), `orchestrator_renew.rs` (renew). Each prints "not in this build yet" to stderr and exits 1; `guard.rs` exits 0 so a placeholder never blocks a shell command.

**TUI** (`crates/nebula-tui/src/`)
- `lib.rs` (module declarations), `app.rs`, `ui.rs`, `ui/overlay.rs`, `event_loop/overlays.rs`, `event_loop/mouse_modes.rs`, `event_loop/verbs.rs`, `event_loop/orchestrator.rs`, `orchestrator_view.rs`, `ipc.rs` (`role_card` on `SpawnArgs` and `StartArgs`, sent only to a 66 daemon; a sibling spawn with a role sends `SpawnSiblingAgentWith`, without one `SpawnSiblingAgent` as before), `ipc_studio.rs` (new, complete).
- Placeholders: `studio_board.rs`, `ui/studio_board.rs` (board), `gauntlet_view.rs`, `ui/gauntlet_view.rs` (gauntlet), `triage_strip.rs` (triage), `studio_queue_view.rs` (queue), `renew_view.rs` (renew), `ledger_export.rs` (ledger).

**Config** (`crates/nebula-config/`)
- `src/actions.rs`, `src/config.rs`, `src/schema.rs`, `tests/parity.rs`; `desktop/src/lib/actions.gen.ts` regenerated with `KETE_BLESS=1 cargo test -p nebula-config actions_gen`.

**Desktop**
- `desktop/package.json`: `"e2e:run": "node --test --test-concurrency=1 e2e/desktop.e2e.mjs e2e/studio.e2e.mjs e2e/gauntlet.e2e.mjs e2e/triage.e2e.mjs e2e/renew.e2e.mjs"`.
- `desktop/src-tauri/src/main.rs` (modules and `invoke_handler` entries), `daemon.rs` (state for the new events), `studio_common.rs` (new, complete).
- `desktop/src/App.svelte`, `src/lib/api.ts`, `src/lib/commands.ts`, `src/lib/store.svelte.ts`, `src/lib/shell/{history.svelte.ts,Rail.svelte,SidebarRail.svelte,Breadcrumb.svelte}`, `src/lib/studio/{StudioView.svelte,types.ts}`.
- One-line mounts: `<RoleChip agent={id} />` in `src/lib/Board.svelte` (card), `src/lib/overview/SessionTree.svelte` (row) and `src/lib/Session.svelte` (header); `<SpendLine agent={id} />` in `src/lib/SessionRail.svelte` (details).
- Placeholders (render nothing, or an empty state naming the page): `src/lib/studio/queue/QueueHeader.svelte` and `autonomy.ts` (queue); `src/lib/studio/board/Board.svelte` (board); `src/lib/gauntlet/GauntletView.svelte` (gauntlet); `src/lib/triage/TriageStrip.svelte` and `focus.ts` (triage); `src/lib/orchestrator/RenewNotice.svelte`, `RenewForm.svelte` and `renew.ts` (renew); `src/lib/roles/RoleChip.svelte` (roles); `src/lib/ledger/SpendLine.svelte` and `export.ts` (ledger); `src-tauri/src/{studio_queue,studio_board,gauntlet,triage,roles,renew,ledger}.rs` (each command returns `Err("not in this build yet".into())`); `desktop/e2e/{studio,gauntlet,triage,renew}.e2e.mjs` (a header comment naming the owner, no tests).

**Tests**: `crates/nebula/tests/e2e_windows/main.rs` runs `flows::TESTS` and then `accounts::TESTS`, `queue::TESTS`, `roles::TESTS`, `renew::TESTS`, `ledger::TESTS`, `triage::TESTS` (filter, `--exact` and `--list` cover all of them); placeholders `e2e_windows/{accounts,queue,roles,renew,ledger,triage}.rs`, each `pub const TESTS: &[(&str, fn())] = &[];`.

**Docs and scripts**: `docs/studio.md` (new: what studio mode is, `.kete/studio.toml`, the inbox note format, the autonomy table, the files Kete keeps, and links to studio-queue.md, studio-board.md, gauntlet.md, triage.md, roles.md, usage-ledger.md); `docs/commands.md` (every new command and flag, from the spec); `docs/configuration.md` (`studio_idle_after`, `usage_ledger`, the `studio` map, `profiles[].reserved`, the new data-dir files); `ARCHITECTURE.md` (a "Studio path" paragraph: repo files read through `nebula_core::studio`, the queue tick, role overlays, the ledger at turn end); `scripts/linux-check.sh` (`set -euo pipefail; cargo fmt --all -- --check; cargo clippy --workspace --all-targets --locked -- -D warnings`).

## Files linked (read, never edit)

`crates/nebula-daemon/src/session_context.rs`, `orchestrator/tests.rs`, `registry/wake.rs` (the 0.54 pattern for a daemon feature with its own module), `crates/nebula-core/src/wake/` (the pattern for a core feature directory), `crates/nebula-tui/src/wake_tab.rs` and `view_jobs.rs`, `desktop/src/lib/wake/` and `desktop/src-tauri/src/wake.rs`, `crates/nebula/tests/e2e_windows/{harness,agent,flows}.rs`.

## Interfaces consumed

`ClientRequest::ClearEndedSubagents` (the current tail), `ServerEvent::decodable_by`, `ServerEvent::Ack`, `ServerEvent::Error`, `Daemon::check_provisioning_peer`, `Daemon::check_caller_peer`, `Daemon::require_orchestrator`, `Daemon::start_orchestrator`, `Store::set_orchestrator`, `settings::write_atomic`, the config lock in nebula-core `settings.rs`, `paths::data_dir`, `usage::StatuslineReading`, `hooks::installer::CLAUDE_ALLOW_RULES`.

## Interfaces produced (exact names; lanes code against these)

**Protocol** (spec 4.1 and corrections 13 and 14): `STUDIO_PROTOCOL`; `ClientRequest::{SetStudio, StudioQueue, RenewOrchestrator, UsageLedger, SpawnSiblingAgentWith}` in that order; `role_card` on `CreateAgent`, `StartOrchestrator`, `OrchestratorSpawnWith` (never on `SpawnSiblingAgent`, whose skip-serialized `profile` makes a trailing field ambiguous); `ServerEvent::{StudioQueueState, OrchestratorRenewal, SessionRoles, SessionSpend, UsageLedgerRows}`.

**`nebula_core::studio`**
- `wire::{StudioQueueRow, QueueNote, RenewState, SessionRole, SpendRow, LedgerRow}` (spec 4.1).
- `autonomy::Autonomy { A0, A1, A2, A3 }`: `Default` is `A1`; `parse(&str) -> Option<Autonomy>`; `as_str()` ("A0"); `label()` ("Hold", "Plan", "Deliver", "Release"); `allows_spawn()` (false for A0 only); `denies() -> &'static [&'static str]` (Claude permission rule strings, Bash and PowerShell forms, spec section 6 table; A0 returns A1's list).
- `manifest::{StudioManifest, MANIFEST_PATH = ".kete/studio.toml", load(repo_root) -> Result<Option<StudioManifest>, String>, slug(label) -> String}`. Fields: `version, inbox, departments, signoffs, evidence, studio_url, recipients, kinds: BTreeMap<String, String>, links: BTreeMap<String, String>`.
- `inbox::{InboxNote { id, path, title, status, to, about, priority, at, by }, NoteStatus { New, Open, Resolved, Done, Closed, Rejected, Other(String) }, Priority { High, Normal, Low }, read_dir(dir) -> Vec<InboxNote>, parse(path, text) -> Option<InboxNote>, eligible<'a>(notes: &'a [InboxNote], recipients: &[String], exclude: &[String]) -> Vec<&'a InboxNote>}`. A note with no `title` takes its first heading, else its first body line, else its id.
- `verdicts::{Verdict { id, kind, verdict, by, at, note, tags, evidence, reconciled, path, modified }, kinds(signoffs) -> Vec<String>, read_kind(signoffs, kind) -> Vec<Verdict>, newest(signoffs, kind) -> Option<Verdict>}`.
- `settings::{StudioSettings { studio_inbox: String, autonomy: Autonomy, queue_exclude: Vec<String> }, load_all() -> BTreeMap<PathBuf, StudioSettings>, for_repo(repo_root) -> Option<StudioSettings>, write(repo_root, Option<&StudioSettings>) -> Result<(), String>}`. `write` touches only the `studio` key of `config.local.json`, under the config lock, tmp-then-rename; it never writes `config.json`.
- `refusals::{Refusal { at, project, agent, role, reason }, append(&Refusal) -> Result<(), String>, read_last(n) -> Vec<Refusal>}` at `<data dir>/studio/refusals.jsonl`, at most 1 MiB (the oldest half is dropped past that).

**`nebula_core::accounts::Launcher { Person, Automation }`**, and `nebula_core::profiles::Profile.reserved`.

**Daemon**
- `launch_overlay::LaunchOverlay { append_prompt: Option<String>, tools: Option<Vec<String>>, permission_mode: Option<String>, settings_path: Option<PathBuf>, strict_mcp: bool, model: Option<String>, effort: Option<String> }` with `Default` and `is_empty()`. Applied in this argv order: resume, permissions flag, model (the launch's own, else the overlay's), effort (likewise), `--permission-mode`, `--strict-mcp-config`, `--tools <comma-joined list>`, then `--append-system-prompt` with `append_prompt` after Kete's guidance and before the PR or issue rule, then the trailing prompt. `--tools` is variadic (spec correction 13), so it must be followed directly by `--append-system-prompt`; when an overlay sets `tools`, the system prompt flag is always emitted.
- Placeholder methods on `Daemon` (the owning lane fills them; signatures fixed here):
  - roles: `launch_overlay(&self, agent: &Agent, project: &Project) -> Result<LaunchOverlay, String>` (placeholder `Ok(LaunchOverlay::default())`), `session_roles(&self) -> Vec<SessionRole>` (empty), `role_card_of(&self, agent: &AgentId) -> Option<String>` (reads the store).
  - queue: `studio_queue::tick(daemon: &Arc<Daemon>)` (no-op), `studio_set(&self, project: &ProjectId, studio: Option<StudioSettings>) -> Result<(), String>`, `studio_queue(self: &Arc<Self>, project: Option<&ProjectId>, run: bool) -> Result<Vec<StudioQueueRow>, String>`, `studio_check_spawn(&self, project: &ProjectId) -> Result<(), String>` (placeholder `Ok(())`), `last_input_at(&self, agent: &AgentId) -> Option<Instant>` (complete, foundation).
  - ledger: `ledger_on_statusline(&self, agent: &AgentId, reading: &StatuslineReading, raw: &serde_json::Value)`, `ledger_on_tool_use(&self, agent: &AgentId)`, `ledger_on_turn_end(&self, agent: &AgentId)` (no-ops), `usage_ledger(&self, since: Option<&str>, until: Option<&str>, project: Option<&ProjectId>) -> Result<Vec<LedgerRow>, String>` (calls `store.spend_rows`).
  - renew: `renew_orchestrator(self: &Arc<Self>, req: RenewRequest) -> Result<RenewState, String>` with `RenewRequest { caller: Option<AgentId>, note: PathBuf, profile: Option<String>, model: Option<String>, keep_old: bool, now: bool }` (placeholder `Err`).
- Store: `set_agent_role_card`, `agent_role_cards`, `add_spend(&SpendDelta) -> Result<SpendRow>`, `spend_rows`, and `SpendDelta { agent, day, project, branch, session, model, profile, role_card, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, cost_usd, tool_uses, turns, at }`.

**CLI** (`cli.rs` and `main.rs`), dispatched to:
- `profile::run_profile(ProfileOp::List { json } | ProfileOp::Pick { harness, project, dry_run, json } | ProfileOp::Reserve(name) | ProfileOp::Unreserve(name))`.
- `studio::run(StudioCommand)` for `Enable { project, inbox }`, `Disable { project }`, `Status { json }`, `Autonomy { project, level: Option<String> }`, `Exclude { project, value }`, `Include { project, value }`, `Queue { project, now, json }`; `studio_note::run(NoteArgs { text, project, to, about, priority, triage, json })` for `kete studio note`.
- `roles::run(RolesCommand)` for `Check { project, role, json }`, `AcceptBypass { profile }`; `guard::run(session: String) -> i32` for the hidden `_guard --session <id>`.
- `usage::run(UsageArgs { since, until, project, by, csv })`.
- `orchestrator_renew::run(RenewArgs { from, profile, pick, model, keep_old, now, json })` for `kete orchestrator renew`.
- `--role <id>` on `kete spawn` (sent as `SpawnSiblingAgentWith`), `kete orchestrator spawn` and `kete orchestrator start` (carried as `role_card`).

**TUI**
- `ipc_studio::{set_studio(project, studio) -> Result<()>, studio_queue(project, run) -> Result<Vec<StudioQueueRow>>, renew_orchestrator(req: RenewRequestArgs) -> Result<RenewState>, usage_ledger(since, until, project) -> Result<Vec<LedgerRow>>}`; each refuses a daemon below `STUDIO_PROTOCOL` with "needs the 0.60 daemon: run `kete kill` and reopen".
- `Overlay::StudioBoard(crate::studio_board::BoardView)`, `Overlay::Gauntlet(crate::gauntlet_view::GauntletView)`, `PromptKind::RenewNote`.
- `App.session_roles: HashMap<AgentId, SessionRole>` (replaced on `SessionRoles`), `App.session_spend: HashMap<AgentId, SpendRow>` (upserted on `SessionSpend`); `OrchestratorRenewal` calls `crate::renew_view::on_renewal(app, req_id, old, new, state)`.
- Placeholder signatures the dispatch calls: `studio_board::{open(app, repo: Option<ProjectId>), handle_key, handle_mouse}`, `ui::studio_board::draw(f, app, area)`; `gauntlet_view::{open(app, repo), open_on(app, repo, kind: &str, id: &str), handle_key, handle_mouse}`, `ui::gauntlet_view::draw`; `triage_strip::{rows(app) -> u16, open(app), handle_key(app, key) -> bool, draw(f, app, area)}`; `studio_queue_view::{header_line(app, repo) -> Line<'static>, open_autonomy(app, repo), handle_key(app, key) -> bool}`; `renew_view::{open(app), submit(app, text), on_renewal(..)}`; `ledger_export::run(app)`.
- `orchestrator_view::split` carves `triage_strip::rows(app)` rows at the bottom; in chip mode `t` calls `triage_strip::open(app)`.
- Verb dispatch: `studio_board` to `studio_board::open`, `gauntlet_view` to `gauntlet_view::open`, `triage_note` to `triage_strip::open`, `studio_autonomy` to `studio_queue_view::open_autonomy`, `renew_orchestrator` to `renew_view::open`, `usage_csv` to `ledger_export::run`.

**Config**: action ids `studio_board`, `gauntlet_view`, `triage_note`, `studio_autonomy`, `renew_orchestrator`, `usage_csv` (`ACTIONS.len() == 113`, `ACTIONS[106..]` updated); `MENUS` rows per spec 4.7; `SettingKind::StudioIdleAfter` (key `studio_idle_after`, General tab, group "Studio", choices `30s`, `45s`, `60s`, `120s`, default `45s`) and `SettingKind::UsageLedger` (key `usage_ledger`, Sessions tab, group "Usage", `on` or `off`, default `on`), both with `added_on` 2026-10-21 and getters `Config::studio_idle_after() -> Duration` and `Config::usage_ledger() -> bool`; `"studio"` in `NOT_ROWS`.

**Desktop**
- Tauri commands registered: `studio_common::studio_repos` (complete: every project with `{ repo, name, manifest: bool, enabled: bool, autonomy }`), `studio_queue::{studio_queue, studio_set}`, `studio_board::studio_board`, `gauntlet::{gauntlet_list, gauntlet_evidence, gauntlet_open_studio}`, `triage::{triage_submit, triage_tail}`, `roles::roles_list`, `renew::orchestrator_renew`, `ledger::{usage_ledger, usage_export}`.
- `api.ts`: `studioRepos`, `studioQueue`, `studioSet`, `studioBoard`, `gauntletList`, `gauntletEvidence`, `gauntletOpenStudio`, `triageSubmit`, `triageTail`, `rolesList`, `orchestratorRenew`, `usageLedger`, `usageExport`; their types in `src/lib/studio/types.ts` (TS mirrors of the wire structs).
- `View` gains `'studio'` and `'gauntlet'`; `place.item` on `'gauntlet'` is `<kind>/<id>` and opens that verdict. Rail and SidebarRail entries `data-rail="studio"` and `data-rail="gauntlet"`, shown when `app.studioRepos.length > 0`; Breadcrumb labels "Studio" and "Gauntlet".
- `store.svelte.ts`: `app.roles: Record<string, SessionRole>`, `app.spend: Record<string, SpendRow>`, `app.studioRepos: StudioRepo[]` (refreshed with the projects list).
- `daemon.rs` emits the Tauri event `orchestrator-renewal` with `{ old, new, state }`.
- `StudioView.svelte`: a repo picker over `app.studioRepos`, then `<QueueHeader repo />` and `<Board repo />`.
- `App.svelte`: routes for both views; `<TriageStrip />` inside the orchestrator view's right pane under the session (App.svelte:440-451); `<RenewNotice />` beside `<ReviewNotices />`.
- `commands.ts` verb table: `studio_board` and `gauntlet_view` navigate; `triage_note` calls `focusTriage()` (`lib/triage/focus.ts`); `studio_autonomy` calls `openAutonomy(repo)` (`lib/studio/queue/autonomy.ts`); `renew_orchestrator` calls `openRenew()` (`lib/orchestrator/renew.ts`); `usage_csv` calls `exportUsage()` (`lib/ledger/export.ts`).

## Tests

- Protocol (spec 4.1 list), in protocol.rs's test module.
- Store: migration count; `set_agent_role_card` round trip; `add_spend` upserts and sums two deltas for one `(agent, day)`; `spend_rows` filters by date range and project; the v0.48 fixture upgrade still passes.
- `studio::*`: manifest (every key, a missing file is `Ok(None)`, a bad line is named, 65,537 bytes refused); slug ("Art and 3D", "UX and UI", "Web, community, release"); inbox (the existing Studio frontmatter shape with no id or priority, a 1 MiB file skipped, the 4,096-byte cap, ordering, exclusion by id and by about); verdicts (lenient fields, a reconciled pair, newest); settings (writes only `studio`, keeps every other key of config.local.json, never creates config.json); refusals (append, read_last, the 1 MiB trim); autonomy (each level's denies, exactly, Bash and PowerShell forms).
- Protocol combinations: for `CreateAgent`, `StartOrchestrator` and `OrchestratorSpawnWith`, every mix of None and Some across their optional fields plus `role_card` round-trips to the same values; `SpawnSiblingAgent` with and without `profile` encodes exactly as at 58; `SpawnSiblingAgentWith` with `profile: None` keeps `role_card` in its own slot.
- Daemon: `agent_spawn_command_with` with `LaunchOverlay::default()` gives the 0.59 argv byte for byte (pin today's argv for a plain Claude launch, a resume, a PR launch and an orchestrator start before changing anything); a full overlay gives the flags in the documented order; with `tools` set and a trailing task, the element after the tool list is `--append-system-prompt` and the task is the last element; `SetStudio` and `RenewOrchestrator { caller: None }` refused from inside a session (the peer fixture in `orchestrator/tests.rs`); `role_card` stored on create and present after a restart.
- Config: `the_tui_table_keeps_its_ids` at 113; `actions_gen_ts_is_current`; parity with `studio` in `NOT_ROWS`; the two new rows in their tabs and groups.
- TUI: a 66 daemon's `SessionRoles` and `SessionSpend` land in the maps; a 65 daemon is never sent `role_card` (the protocol 58 test pattern in ipc.rs).
- Desktop: `npm run check` and `npm test` green; one vitest that `View` includes both new views and the rail hides them with no studio repos.

## Checks

`cargo fmt --all`; `cargo clippy -p nebula-core -p nebula-config -p nebula-daemon -p nebula-tui -p nebula --all-targets -- -D warnings` (one command); `cargo test -p nebula-core`, `-p nebula-config`, `-p nebula-daemon`, `-p nebula-tui`, then `KETE_E2E_SKIP=1 cargo test -p nebula` (one at a time); `cargo test -p nebula --test e2e_windows` once (this lane changes the suite's main); `cd desktop && npm run check && npm test`; `cd desktop/src-tauri && cargo clippy -- -D warnings && cargo test`.

## Stop condition

A green PR against `release/0.60`. The PR body lists every placeholder with its owning lane, so the main thread can check after the train that none is left.
