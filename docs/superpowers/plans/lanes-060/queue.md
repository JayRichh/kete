# Lane queue: K2, studio inbox, autonomy and idle notes

Branch `release/0.60-queue`, worktree `C:\Code\bro\.claude\worktrees\lane-060-queue`, from `origin/release/0.60` after the foundation merged. Opus lead, at most 2 Sonnet helpers. Wave 1. Read the plan's Lane rules first; they bind this brief.

Spec: section 6 (K2), corrections 8 and 9, decisions 4, 5, 9 and 10, section 4.3 to 4.5, section 14, section 16 questions 2 and 7.

## Goal

Jay switches a repo's studio on, sets its autonomy, and from then on an idle orchestrator is told about new inbox notes in one short note, never while he is typing to it, never more than 6 times an hour, and never at A0, where spawns into the repo are refused too.

## Files owned

- `crates/nebula-core/src/studio/queue.rs` (placeholder from the foundation)
- `crates/nebula-daemon/src/registry/studio_queue.rs`, `crates/nebula-daemon/src/registry/studio_policy.rs` (placeholders)
- `crates/nebula/src/studio.rs` (placeholder; `kete studio note` is the triage lane's `studio_note.rs`)
- `crates/nebula-tui/src/studio_queue_view.rs` (placeholder)
- `desktop/src/lib/studio/queue/**` (`QueueHeader.svelte`, `autonomy.ts` placeholders; `AutonomyControl.svelte`, `queue.svelte.ts` new)
- `desktop/src-tauri/src/studio_queue.rs` (placeholder)
- `crates/nebula/tests/e2e_windows/queue.rs`
- `docs/studio-queue.md` (new)

## Files linked (read only)

`crates/nebula-daemon/src/session_context.rs` (`write_prompt`, `is_idle`), `orchestrator.rs` (`orchestrator_tell` for the labelled-write pattern), `registry/usage.rs` (`UsageBook.contexts` for the context nudge), `lib.rs` (the 30 s tick that calls you), `server.rs` (the arms that call you); `crates/nebula-core/src/studio/{inbox,settings,autonomy,manifest,refusals,wire}.rs`; `crates/nebula-core/src/{profiles,usage,usage_window}.rs` (headroom line and reset wording); docs/orchestrator.md (the idle-note follow-up; the renew lane edits that file).

## Interfaces consumed

`studio_queue::tick(&Arc<Daemon>)`, `Daemon::{studio_set, studio_queue, studio_check_spawn}` (placeholders you fill), `Daemon::last_input_at`, `session_context::write_prompt`, `Store::orchestrator`, `studio::inbox::{read_dir, eligible, InboxNote}`, `studio::settings::{load_all, for_repo, write, StudioSettings}`, `studio::manifest::load`, `studio::autonomy::Autonomy`, `studio::refusals::append`, `studio::wire::{StudioQueueRow, QueueNote}`, `ClientRequest::{SetStudio, StudioQueue}`, `ServerEvent::StudioQueueState`, `Config::studio_idle_after()`, `Profile.reserved`, `usage_window` (reset wording), `ipc_studio::{set_studio, studio_queue}`, `StudioCommand` variants, `api.studioQueue`, `api.studioSet`.

## Interfaces produced

- `studio::queue::{QueueState { announced, last_note_at, notes_this_hour }, Holding { Off, AutonomyHold, NoOrchestrator, Busy, NeedsYou, Typing { secs }, NotIdleLongEnough { secs }, HourlyCap, NothingNew }, decide(now, inputs: &TickInputs) -> Result<Vec<&InboxNote>, Holding>, compose_note(now_local, repo_name, autonomy, notes, headroom, nudge: Option<ContextNudge>) -> String, MAX_NOTE_BYTES = 1200, MAX_LISTED = 5, MAX_PER_HOUR = 6, TYPING_QUIET_SECS = 120, NUDGE_PERCENT = 60, NUDGE_REPEAT_SECS = 1800}`.
- `<data dir>/studio/queue-state.json` (spec section 6), read at boot.
- `Daemon::studio_set` (refused inside sessions by the foundation's arm; validates the inbox path is inside the repo), `Daemon::studio_queue` (`run: false` reports, `run: true` ticks now), `Daemon::studio_check_spawn` (A0 refusal text "autonomy A0 (Hold) for <repo>", plus a refusals line), and `studio_queue::tick`.
- CLI: `kete studio enable|disable|status|autonomy|exclude|include|queue` per spec section 6.
- TUI `studio_queue_view::{header_line, open_autonomy, handle_key}`; raising to A2 or A3 asks for a confirm.
- Desktop `QueueHeader { repo: string }` with `AutonomyControl` (four segments, names visible, raising past A1 confirms), the runner state line, Send now; `openAutonomy(repo)` focuses the control.

## Tests

- Unit (`studio::queue`): eligible ordering by priority then `at`; an edited note (new mtime) re-announced, an untouched one never; exclusion by id and by about; each `Holding` reason; the 6 per hour cap; a note over 1,200 bytes trimmed to 5 listed plus "and N more"; the context nudge at 59 and 60 percent and its 30-minute repeat; A0 never injects.
- Daemon unit: `SetStudio` refused from inside a session (the peer fixture in `orchestrator/tests.rs`); an inbox path outside the repo refused; `queue-state.json` survives a restart so nothing repeats; `studio_check_spawn` refuses at A0 only.
- Windows e2e (`e2e_windows/queue.rs`, every flow named `queue_*`): a repo with three notes (high, normal, a `to: jay`), `kete studio enable <repo>`, an idle fake orchestrator; `kete studio queue --now` writes `[from kete studio]` with the high note first into the orchestrator's pane (the fake agent's echo log); a second `--now` sends nothing; touching one note re-announces it; while the harness types into the orchestrator, `--now` holds with "typing"; `kete studio autonomy <repo> A0` then `kete orchestrator spawn --project <repo> "x"` is refused with the A0 reason; `kete studio status --json` shows the counts.
- TUI draw test of the header line at 80x24; input parity for the autonomy step keys.
- Desktop vitest for the control's confirm rule and the state line wording.

## Checks

`cargo fmt --all`; `cargo clippy -p nebula-core -p nebula-daemon -p nebula-tui -p nebula --all-targets -- -D warnings`; `cargo test -p nebula-core`; `cargo test -p nebula-daemon`; `cargo test -p nebula-tui`; `KETE_E2E_SKIP=1 cargo test -p nebula`; `cargo test -p nebula --test e2e_windows -- queue_`; `cd desktop && npm run check && npm test`; `cd desktop/src-tauri && cargo clippy -- -D warnings`.

## Stop condition

A green PR against `release/0.60`, its body per Lane rule 14, with the exact idle note the e2e produced and the one line for docs/orchestrator.md's Follow-ups that the renew lane should write.
