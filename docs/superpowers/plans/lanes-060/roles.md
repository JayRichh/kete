# Lane roles: K3 role cards and K9's permission profiles

Branch `release/0.60-roles`, worktree `C:\Code\bro\.claude\worktrees\lane-060-roles`, from `origin/release/0.60` after the foundation merged. Opus lead, at most 2 Sonnet helpers. Wave 1. Read the plan's Lane rules first; they bind this brief.

Spec: section 7 (K3 and the permission profiles), corrections 3 to 6 and 10, decisions 6, 7, 8, 10 and 13, section 14, section 16 questions 3 and 8. Verify every Claude flag against `claude --help` text and the permission rule syntax against code.claude.com/docs/en/permissions and /permission-modes before building; if a claim in the spec is wrong, say so in the PR and adapt.

## Goal

A session launched with a role card comes up with that role's model, effort, tool set, MCP setting, appended prompt and permission profile, every time it starts or resumes. Allowlist and unattended roles carry the built-in denies, the repo's governance denies and the autonomy denies, and every shell command they run passes `kete _guard`. Unattended launches only happen on accounts Jay accepted himself. `kete roles check` shows all of it before a shift.

## Files owned

- `crates/nebula-core/src/roles/mod.rs`, `card.rs`, `permissions.rs`, `guard.rs` (new; the foundation left `mod.rs` as a placeholder)
- `crates/nebula-daemon/src/registry/roles.rs` (placeholder from the foundation)
- `crates/nebula/src/roles.rs`, `crates/nebula/src/guard.rs` (placeholders from the foundation)
- `crates/nebula-tui/src/quick_prompt.rs`, `crates/nebula-tui/src/event_loop/quick_launch.rs` (the role field and its launch), and the chip builder in `crates/nebula-tui/src/launcher.rs` (the account chip list near launcher.rs:354; no other part of launcher.rs)
- `desktop/src/lib/roles/**` (`RoleChip.svelte` placeholder from the foundation; `RolePicker.svelte`, `roles.svelte.ts` new)
- `desktop/src/lib/QuickPrompt.svelte`, `desktop/src/lib/orchestrator/StartOrchestrator.svelte`
- `desktop/src-tauri/src/roles.rs` (placeholder), `desktop/src-tauri/src/launch.rs`, `desktop/src-tauri/src/orchestrator.rs` (role on create and start)
- `crates/nebula/tests/e2e_windows/roles.rs`
- `docs/roles.md` (new)

## Files linked (read only)

`crates/nebula-daemon/src/launch_overlay.rs`, `registry.rs` (`agent_spawn_command_with`), `registry/spawn.rs` (`statusline_settings`, `insert_settings_flag`), `hooks/installer.rs` (exec-form hook entries from 0.54 B3, `is_nebula_command`), `orchestrator.rs`, `server.rs`; `crates/nebula-core/src/studio/{autonomy,settings,refusals}.rs`; `crates/nebula-core/src/profiles.rs` (profile homes, `use_default`); `crates/nebula/src/profile.rs` (`kete profile login`, the pattern for `accept-bypass`).

## Interfaces consumed

`LaunchOverlay`, `Daemon::launch_overlay` / `session_roles` / `role_card_of` (placeholders you fill), `Store::{set_agent_role_card, agent_role_cards}`, `Autonomy::denies`, `studio::settings::for_repo`, `studio::refusals::append`, `studio::wire::SessionRole`, `ServerEvent::SessionRoles`, the `role_card` fields on `CreateAgent`, `StartOrchestrator`, `OrchestratorSpawnWith`, `SpawnSiblingAgent`, `ipc::{SpawnArgs, StartArgs}.role_card`, `RolesCommand::{Check, AcceptBypass}`, `guard::run(session) -> i32`.

## Interfaces produced

- `roles::card::{RolesFile { version, governance, roles: BTreeMap<String, RoleCard> }, RoleCard { id, label, model, effort, department, tier, context_budget, prompt, tools, allow, deny, mcp, permission }, Permission { Prompt, Allowlist, Unattended }, Mcp { Default, None }, parse(text) -> Result<RolesFile, RoleError>}` with the limits in spec section 7 (64 roles, 65,536 bytes, 8,192-byte prompt, tier 0 to 3).
- `roles::load_committed(root_worktree: &Path) -> Result<Option<RolesFile>, String>` (`git show HEAD:.kete/roles.toml`, `prompt_file` read the same way).
- `roles::permissions::{ComposeCtx { card, autonomy, worktree, root_worktree, data_dir, config_dir, settings_file }, SessionRules { mode: Option<&'static str>, allow: Vec<String>, deny: Vec<String> }, compose(&ComposeCtx) -> SessionRules, BUILTIN_FORCE_PUSH, posix_rule_path(&Path) -> String}`.
- `roles::guard::{check(command: &str, autonomy: Autonomy) -> Result<(), String>}` and `kete _guard --session <id>` (reads the PreToolUse JSON on stdin, the session's autonomy from its settings file, exits 2 with the reason on stderr, 0 otherwise; never contacts the daemon).
- `Daemon::launch_overlay` (writes `<data dir>/sessions/<agent id>/claude-settings.json`: the statusLine block from `statusline_settings()`, `permissions.allow`, `permissions.deny`, and a PreToolUse hook `{matcher: "Bash|PowerShell"}` running `kete _guard --session <id>` in exec form on Windows), `Daemon::session_roles`, `Daemon::role_card_of` (the renew lane carries the old orchestrator's card with it).
- The bypass acceptance check and its refusal text, and `kete roles accept-bypass <profile>` (refuses inside a Kete session).
- `kete roles check [--project P] [--role R] [--json]`.
- TUI role field and chip; desktop `RoleChip { agent: string }`, `RolePicker { repo: string, value: string | null }`; `roles_list` Tauri command returning `{ id, label, model, permission, tier, department }[]` for a repo.

## Tests

- Unit (`roles::card`): every key; refusals for version 2, an unknown permission, a 8,193-byte prompt, 65 roles, tier 4, an unknown `mcp`; `prompt_file` read from HEAD.
- Unit (`roles::permissions`): each profile's exact `mode`, `allow` and `deny` lists in their exact order; governance made absolute for both worktrees; `C:\Code\wheako\DECISIONS\**` becomes `//c/Code/wheako/DECISIONS/**`; `AskUserQuestion` denied for allowlist and unattended only; autonomy A1, A2, A3 denies included for a role session.
- Unit (`roles::guard`), at least 30 commands: `git push --force`, `git push -f origin x`, `git push origin +main`, `git -C ../x push --force-with-lease`, `GIT_TRACE=1 git push -f`, `cd x && git push --mirror`, `git push origin :main`, PowerShell `git push --force`, `git tag v1` at A1 and A2 refused and at A3 allowed, `gh release create` at A2 refused; and safe ones that must pass: `git push`, `git push -u origin feat`, `git push origin feature-force-fix`, `git status`, `echo "--force"`.
- Daemon unit: an empty roles file leaves the 0.59 argv byte for byte (the foundation pinned it); roles come from HEAD, not an uncommitted edit; a non-Claude harness with a role is refused; an unattended role on a profile without `skipDangerousModePermissionPrompt` is refused, with a refusals line; a role card survives a restart (migration 36) and is re-applied on resume.
- Windows e2e (`e2e_windows/roles.rs`, every flow named `roles_*`): a repo with a committed `.kete/roles.toml`; `kete orchestrator spawn --role runner` gives the fake agent `--tools Read,Write,Bash`, `--permission-mode dontAsk`, `--strict-mcp-config`, `--model haiku` and a `--settings` file whose deny list holds the governance and force-push rules (read the args from the agent log, the file from the data dir); `kete _guard --session <id>` fed a `git push --force` hook payload exits 2; an unattended role on an unaccepted account is refused with the `accept-bypass` command; after `kete kill` and a relaunch the card still shows the role label.
- TUI: the role field in the quick prompt (draw at 80x24, input parity for stepping it); the role chip on a card.
- Desktop vitest for `RolePicker` options and the chip text; the picker hidden for a repo with no roles.

## Checks

`cargo fmt --all`; `cargo clippy -p nebula-core -p nebula-daemon -p nebula-tui -p nebula --all-targets -- -D warnings`; `cargo test -p nebula-core`; `cargo test -p nebula-daemon`; `cargo test -p nebula-tui`; `KETE_E2E_SKIP=1 cargo test -p nebula`; `cargo test -p nebula --test e2e_windows -- roles_`; `cd desktop && npm run check && npm test`; `cd desktop/src-tauri && cargo clippy -- -D warnings`.

## Stop condition

A green PR against `release/0.60`, its body per Lane rule 14, including the exact settings file a runner and an unattended lead get for the e2e fixture, the keys you bound, and the gaps from spec section 14 you confirmed or found.
