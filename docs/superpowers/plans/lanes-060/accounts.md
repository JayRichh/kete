# Lane accounts: K1, accounts for scripts

Branch `release/0.60-accounts`, worktree `C:\Code\bro\.claude\worktrees\lane-060-accounts`, from `origin/release/0.60` after the foundation merged. Opus lead, at most 2 Sonnet helpers. Wave 1. Read the plan's Lane rules first; they bind this brief.

Spec: section 5 (K1), corrections 1 and 2, section 4.3 (`reserved`, `.picks.json`), section 16 question 1.

## Goal

The studio manager picks an account from a script, the pick spreads over accounts with headroom, and Jay's own account is never picked for the orchestrator's launches while staying the default for his own. `kete profile list --json`, `kete profile pick [--dry-run]`, `kete profile reserve` and `unreserve`, the automation rule in the daemon's spread, and Reserve for me in both apps.

## Files owned

- `crates/nebula-core/src/accounts.rs` (the foundation added `Launcher` and the parameter; the rule is yours)
- `crates/nebula-core/src/profiles.rs` (the foundation added `reserved`; `set_reserved` and its use are yours)
- `crates/nebula-core/src/accounts/holds.rs` (new; or a `holds` module inside accounts.rs, your choice, said in the PR)
- `crates/nebula/src/profile.rs`
- `crates/nebula/tests/profile_cli.rs` (new)
- `crates/nebula-tui/src/accounts_overlay.rs`, `crates/nebula-tui/src/ui/accounts_view.rs`, `crates/nebula-tui/src/event_loop/accounts.rs`
- `desktop/src/lib/accounts/**`
- `desktop/src-tauri/src/accounts.rs`, `desktop/src-tauri/src/profiles.rs`
- `crates/nebula/tests/e2e_windows/accounts.rs`
- `docs/profiles.md`

## Files linked (read only)

`crates/nebula-daemon/src/registry.rs` (`resolve_profile_name`, which the foundation wired to pass `Launcher`), `crates/nebula-core/src/usage.rs` (`read_all_in`, `read_account_in`), `crates/nebula-tui/src/ipc.rs` (`ListSessions` client), `crates/nebula-tui/src/event_loop/quick_launch.rs` and `desktop/src-tauri/src/launch.rs` (`Launcher::Person` callers; their behaviour must not change), `crates/nebula/tests/e2e_windows/harness.rs`.

## Interfaces consumed

`nebula_core::accounts::{Launcher, Policy, Spread, Reason, Pick, pick, resolve_launch, pick_for_project}`, `nebula_core::profiles::{Profile, load, profiles_root}`, `nebula_core::usage::{read_all_in, AccountUsage}`, `ClientRequest::ListSessions`, the `ProfileOp` variants the foundation added (`List { json }`, `Pick { harness, project, dry_run, json }`, `Reserve(String)`, `Unreserve(String)`).

## Interfaces produced

- `accounts::pick_for(all, harness, project_default, policy, launcher: Launcher, now_secs, usage, load: impl Fn(&str) -> u32) -> Pick`, used by `resolve_launch` for `Launcher::Automation`; `Launcher::Person` keeps 0.59's result exactly.
- Automation rules: reserved and disabled profiles are never candidates; `manual` is read as `least_used`; a reserved project default is skipped; ranking is the higher of 5-hour and weekly percent in 10-point bands, then `load`, then name. `Reason::Reserved` for the dry-run table.
- `accounts::holds::{HOLD_SECS = 600, add(profile, now), active(now) -> BTreeMap<String, u32>}` on `<data dir>/profiles/.picks.json`, written tmp-then-rename.
- `profiles::set_reserved(name, bool) -> Result<(), String>` (through the existing config writer, refusing an unknown name).
- CLI per spec section 5, with the JSON shapes there, exit code 3 when nothing is eligible. The renew lane's `--pick` calls `pick_for` with `Launcher::Automation`; keep the signature.
- TUI: `(yours)` on a reserved account row, the toggle key (pick a free key in the accounts overlay and name it in the PR), Reserve for me / Stop reserving in the account menu.
- Desktop: the Reserve for me switch on AccountsView rows and the account menu rows, with the tooltip from the spec.

## Tests

- Unit (accounts.rs): the existing `pick` tests unchanged; `pick_for` with a reserved default; every profile reserved (no pick); manual read as least_used for automation; the 10-point band (20 and 28 percent tie on load, 20 and 31 do not); load breaks a tie; a reserved project default skipped; `Launcher::Person` unchanged on the same fixtures.
- Unit (holds): add, expire at 600 s, a missing or corrupt file reads as empty.
- CLI (`tests/profile_cli.rs`, isolated `NEBULA_DATA_DIR` and `NEBULA_RUNTIME_DIR`): `profile list --json` shape; `profile pick --json` shape and exit 3; `--dry-run` writes no hold; two picks in a row return different profiles when readings tie; `reserve` and `unreserve` round trip in config.json.
- Windows e2e (`e2e_windows/accounts.rs`, registered in its `TESTS`, every flow named `accounts_*`): two Claude profiles with usage files at 20 and 70 percent, the 20 percent one reserved; `kete orchestrator spawn` with no `--profile` lands on the 70 percent one and its card shows `claude@<name>`; a quick-prompt launch from the TUI still takes the reserved default.
- TUI draw test of the accounts overlay with a reserved row at 80x24 and 120x40; input parity for the toggle (key and click).
- Desktop vitest for the switch's model in `lib/accounts`.

## Checks

`cargo fmt --all`; `cargo clippy -p nebula-core -p nebula -p nebula-tui --all-targets -- -D warnings`; `cargo test -p nebula-core`; `cargo test -p nebula-tui`; `KETE_E2E_SKIP=1 cargo test -p nebula --test profile_cli`; `cargo test -p nebula --test e2e_windows -- accounts_`; `cd desktop && npm run check && npm test`; `cd desktop/src-tauri && cargo clippy -- -D warnings`.

## Stop condition

A green PR against `release/0.60`, its body per Lane rule 14, with the docs/profiles.md sections named and the toggle key for docs/keys.md.
