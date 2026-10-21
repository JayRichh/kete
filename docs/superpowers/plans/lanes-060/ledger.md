# Lane ledger: K7, the usage ledger

Branch `release/0.60-ledger`, worktree `C:\Code\bro\.claude\worktrees\lane-060-ledger`, from `origin/release/0.60` after the foundation merged. Opus lead, at most 2 Sonnet helpers. Wave 1. Read the plan's Lane rules first; they bind this brief.

Spec: section 11 (K7), correction 7, decision 14, sections 4.1 and 4.2. Confirm the transcript layout (assistant lines, `message.id`, `message.usage` fields, where subagent transcripts are written) against the Claude Code docs and Kete's own transcript reader before building; build the test fixtures by hand, never from a real transcript.

## Goal

Every Claude session's tokens, estimated cost, tool uses and turns are added up per local day at each turn end, kept in SQLite, pushed to clients as `SessionSpend`, and exported by `kete usage --csv`, so the studio can commit a usage ledger and the board can show cost per lane.

## Files owned

- `crates/nebula-core/src/usage_ledger.rs` (placeholder from the foundation)
- `crates/nebula-daemon/src/registry/ledger.rs` (placeholder)
- `crates/nebula/src/usage.rs` (placeholder)
- `crates/nebula-tui/src/ledger_export.rs` (placeholder)
- `desktop/src/lib/ledger/**` (`SpendLine.svelte`, `export.ts` placeholders)
- `desktop/src-tauri/src/ledger.rs` (placeholder)
- `crates/nebula/tests/e2e_windows/ledger.rs`, `crates/nebula/tests/e2e_windows/agent.rs` (the fake agent; no other 0.60 lane edits it)
- `docs/usage-ledger.md` (new)

## Files linked (read only)

`crates/nebula-daemon/src/registry/usage.rs` (`on_statusline`, which calls you with the raw JSON), `registry/status.rs` (turn end and PostToolUse, which call you), the daemon's transcript map and reader (`transcripts`, the History tab's reader), `store.rs` (`add_spend`, `spend_rows`, `SpendDelta`); `crates/nebula-core/src/usage.rs`; `crates/nebula/tests/e2e_windows/harness.rs`, `flows.rs` (the wake flow's statusline step).

## Interfaces consumed

`Daemon::{ledger_on_statusline, ledger_on_tool_use, ledger_on_turn_end, usage_ledger}` (placeholders you fill; `usage_ledger` already calls `store.spend_rows`), `Store::{add_spend, spend_rows}`, `SpendDelta`, `studio::wire::{SpendRow, LedgerRow}`, `ServerEvent::SessionSpend`, `ClientRequest::UsageLedger`, `ServerEvent::UsageLedgerRows`, `Config::usage_ledger()`, `ipc_studio::usage_ledger`, `UsageArgs`, `api.usageLedger`, `api.usageExport`, `app.spend` (desktop store), `App.session_spend` (TUI).

## Interfaces produced

- `usage_ledger::{TranscriptTally { offset, seen_ids }, Tally { input_tokens, output_tokens, cache_read_tokens, cache_write_tokens }, TranscriptTally::read_new(&mut self, path) -> Tally, cost_delta(prev: Option<f64>, now: f64) -> f64, local_day(epoch_secs) -> String, write_csv(rows: &[LedgerRow], out: &mut impl Write)}`. `cost_delta` counts a drop (a `/clear`) as the new value whole.
- Daemon: per-session state (tally, last cost, tool uses this turn), `add_spend` at each turn end, `SessionSpend` to 66 peers; nothing when `usage_ledger` is off.
- CLI `kete usage` per spec section 11, CSV columns exactly as listed there.
- Fake agent step `usage:<input>,<output>,<cost>`: appends an assistant line with a fresh `message.id` and that usage to its transcript, then posts a statusLine whose `cost.total_cost_usd` is `<cost>` (the session's total after that turn, as Claude reports it).
- TUI `ledger_export::run(app)` writes `<data dir>/exports/usage-<YYYY-MM-DD>.csv` (today's local date) and flashes the path.
- Desktop `SpendLine { agent: string }` ("today: 412k tokens, est. US$3.10, 58 tool uses, 9 turns"; nothing when there is no row), `exportUsage()` (writes the same file and shows it in Explorer), `usage_ledger` and `usage_export` Tauri commands.

## Tests

- Unit (`usage_ledger`): a synthetic transcript with a duplicated `message.id`, a truncated last line (not counted until complete), and a subagent file beside it; `read_new` picks up only appended lines; `cost_delta` across a `/clear`; `local_day` at 23:59 and 00:00 local; CSV quoting of a session name with a comma and a quote.
- Daemon unit: two turns sum into one `(agent, day)` row; a turn across local midnight lands on the new day; `usage_ledger = off` records nothing; PostToolUse counts reset per turn.
- Windows e2e (`e2e_windows/ledger.rs`, every flow named `ledger_*`): a fake agent scripted `usage:1000,200,0.05` then `usage:500,100,0.08`; after both turns `kete usage --csv` prints one row with 1,500 input tokens, 300 output tokens, cost 0.08 and 2 turns; with `usage_ledger` set to `off` a third turn adds nothing.
- Desktop vitest for `SpendLine`'s wording (thousands as `k`, two decimals of USD, nothing for no row).

## Checks

`cargo fmt --all`; `cargo clippy -p nebula-core -p nebula-daemon -p nebula-tui -p nebula --all-targets -- -D warnings`; `cargo test -p nebula-core`; `cargo test -p nebula-daemon`; `cargo test -p nebula-tui`; `KETE_E2E_SKIP=1 cargo test -p nebula`; `cargo test -p nebula --test e2e_windows` (the whole suite once, since you changed the fake agent); `cd desktop && npm run check && npm test`; `cd desktop/src-tauri && cargo clippy -- -D warnings`.

## Stop condition

A green PR against `release/0.60`, its body per Lane rule 14, with a sample CSV from the e2e and the transcript facts you confirmed (with the doc link and the date read).
