# Lane renew: K9, orchestrator renewal

Branch `release/0.60-renew`, worktree `C:\Code\bro\.claude\worktrees\lane-060-renew`, from `origin/release/0.60` after the foundation, roles and accounts lanes merged. Opus lead, at most 2 Sonnet helpers. Wave 2. Read the plan's Lane rules first; they bind this brief.

Spec: section 13 (K9 renewal), correction 8, decision 3, section 4.1 (`RenewOrchestrator`, `OrchestratorRenewal`, `RenewState`), section 16 questions 4 and 8. The permission profiles half of K9 is the roles lane's and has merged before you start.

## Goal

The orchestrator, or Jay from his own shell or the apps, asks for a renewal with a handover note. The daemon waits for the current turn to end, starts a fresh orchestrator on the same project with the same harness, model, effort, role card and account (or the ones asked for), gives it the note as its first prompt, moves the role across once the new session reports, archives the old session, and tells every client. A failure leaves the old orchestrator in charge.

## Files owned

- `crates/nebula-daemon/src/orchestrator_renew.rs` (placeholder from the foundation)
- `crates/nebula/src/orchestrator_renew.rs` (placeholder)
- `crates/nebula-tui/src/renew_view.rs` (placeholder)
- `desktop/src/lib/orchestrator/RenewNotice.svelte`, `RenewForm.svelte`, `renew.ts` (placeholders)
- `desktop/src-tauri/src/renew.rs` (placeholder)
- `crates/nebula/tests/e2e_windows/renew.rs`
- `desktop/e2e/renew.e2e.mjs` (placeholder)
- `docs/orchestrator.md`

## Files linked (read only)

`crates/nebula-daemon/src/orchestrator.rs` (`start_orchestrator`, `require_orchestrator`, `orchestrator_home`, `StartOrchestratorSpec` with the foundation's `role_card` and `starting_prompt`), `server.rs` (`check_caller_peer`, `check_provisioning_peer`, the arm that calls you), `registry/roles.rs` (`role_card_of`), `registry/lifecycle.rs` and `store.rs` (archive and stop paths), `orchestrator/tests.rs` (peer fixtures); `crates/nebula-core/src/accounts.rs` (`pick_for`, `Launcher::Automation`); `crates/nebula-tui/src/orchestrator_view.rs`; `desktop/src/lib/orchestrator/{ReviewNotices,OrchestratorPanel}.svelte`, `desktop/src/lib/orch.svelte.ts`.

## Interfaces consumed

`Daemon::renew_orchestrator(self: &Arc<Self>, req: RenewRequest) -> Result<RenewState, String>` (placeholder you fill), `RenewRequest`, `ClientRequest::RenewOrchestrator`, `ServerEvent::OrchestratorRenewal`, `studio::wire::RenewState`, `StartOrchestratorSpec.{role_card, starting_prompt}`, `Daemon::{start_orchestrator, require_orchestrator, check_caller_peer, check_provisioning_peer, role_card_of}`, `accounts::pick_for` with `Launcher::Automation`, `ipc_studio::renew_orchestrator`, `RenewArgs`, `renew_view::{open, submit, on_renewal}` signatures, `PromptKind::RenewNote`, `api.orchestratorRenew`, the Tauri event `orchestrator-renewal`, the `renew_orchestrator` verb.

## Interfaces produced

- Daemon renewal per spec section 13: who may ask; note checks (inside `<data dir>/orchestrator` or a registered project, UTF-8, 1 to 65,536 bytes); `Scheduled` reply; the swap after the old session's turn ends (at most 300 s) or at once with `now`; the starting prompt text exactly as in the spec; the role moved when the new session's first hook arrives (at most 60 s) or a rollback with `Failed { reason }`; the old session archived and stopped unless `keep_old`; a line in `<data dir>/orchestrator-renewals.jsonl` (`{ at, old, new, note, state, reason }`, the board lane reads the last line); `OrchestratorRenewal` with `req_id: None` to every 66 peer on `Done` and `Failed`.
- CLI `kete orchestrator renew --from <note> [--profile P | --pick] [--model M] [--keep-old] [--now] [--json]`; `--pick` refuses with the reason when no account is eligible.
- TUI: `on_renewal` makes the orchestrator view follow the new session and flashes "Orchestrator renewed from <note>" or the failure; `open` shows the `RenewNote` prompt, `submit` sends it.
- Desktop: `RenewNotice` (listens to `orchestrator-renewal`: renewed from <note>, with Open note; or the failure reason), `RenewForm` (note path, account with the pick offered, Keep old session, Renew now), `openRenew()`.
- docs/orchestrator.md: a Renewal section; `kete orchestrator renew` in Agent-facing commands; the permission model lines for the new verb (who may ask, the note path rule); Follow-ups updated (idle notes shipped in K2, with the line the queue lane gives in its PR; renewal shipped).

## Tests

- Daemon unit: the orchestrator may renew; a worker refused; a call with no caller from outside sessions allowed; a call with no caller from inside a session refused; a note outside the allowed folders, over 65,536 bytes, missing, or not UTF-8 refused before any change; the swap waits for `Finished` and gives up at 300 s with `Failed`; a new session that never reports within 60 s rolled back (old keeps the role); `keep_old` leaves the old session running as a worker; the role card and profile carried over.
- Windows e2e (`e2e_windows/renew.rs`, every flow named `renew_*`): a fake orchestrator started with `kete orchestrator start`; a note written to its workspace; `kete orchestrator renew --from <note> --now` from outside sessions; the new orchestrator's first prompt (agent log) holds the note text; `kete orchestrator status` names the new session; `kete sessions` shows the old one archived; a second run with `--keep-old` keeps the old one as a worker.
- TUI: a draw test of the flash; the view follows the new orchestrator on `Done`.
- Desktop e2e (`e2e/renew.e2e.mjs`): the session menu's Renew orchestrator opens the form; submitting with a fixture note shows the notice and the orchestrator view shows the new session.

## Checks

`cargo fmt --all`; `cargo clippy -p nebula-daemon -p nebula-tui -p nebula --all-targets -- -D warnings`; `cargo test -p nebula-daemon`; `cargo test -p nebula-tui`; `KETE_E2E_SKIP=1 cargo test -p nebula`; `cargo test -p nebula --test e2e_windows -- renew_`; `cd desktop && npm run check && npm test`; `cd desktop/src-tauri && cargo clippy -- -D warnings`; `cd desktop && npm run e2e:prepare && node --test --test-concurrency=1 e2e/renew.e2e.mjs`.

## Stop condition

A green PR against `release/0.60`, its body per Lane rule 14, with a sample renewals line and the docs/orchestrator.md sections changed.
