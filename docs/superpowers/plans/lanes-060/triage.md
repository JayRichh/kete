# Lane triage: K6, the triage desk

Branch `release/0.60-triage`, worktree `C:\Code\bro\.claude\worktrees\lane-060-triage`, from `origin/release/0.60` after the foundation and roles lanes merged. Opus lead, at most 2 Sonnet helpers. Wave 2. Read the plan's Lane rules first; they bind this brief.

Spec: section 10 (K6), sections 4.4 and 4.5, decision 9.

## Goal

One line typed under the orchestrator (or `kete studio note` from a shell) becomes a filed inbox note, refused if it duplicates an open one, and the repo's triage desk routes it in one turn; the strip shows the route.

## Files owned

- `crates/nebula-core/src/studio/inbox_write.rs` (placeholder from the foundation)
- `crates/nebula/src/studio_note.rs` (placeholder)
- `crates/nebula-tui/src/triage_strip.rs` (placeholder)
- `desktop/src/lib/triage/**` (`TriageStrip.svelte`, `focus.ts` placeholders; `triage.svelte.ts` new)
- `desktop/src-tauri/src/triage.rs` (placeholder)
- `crates/nebula/tests/e2e_windows/triage.rs`
- `desktop/e2e/triage.e2e.mjs` (placeholder)
- `docs/triage.md` (new)

## Files linked (read only)

`crates/nebula-core/src/studio/{inbox,manifest,settings}.rs`; `crates/nebula-daemon/src/session_context.rs` (how `AskSession` waits and reads back); `crates/nebula-tui/src/orchestrator_view.rs` (the row the foundation carved for you, chip mode), `text_input.rs`, `ipc.rs` (the `ask` and `read` clients, `SpawnArgs.role_card`); `desktop/src/lib/QuickPrompt.svelte` (field and target chip styling to match), `desktop/src/lib/orch.svelte.ts` (`sendToOrchestrator`); `crates/nebula/tests/e2e_windows/{harness,agent}.rs`.

## Interfaces consumed

`studio::inbox::{parse, read_dir, InboxNote, NoteStatus}`, `studio::manifest::load`, `studio::settings::for_repo` (the inbox path), `CreateAgent { role_card: Some("triage") }` through the existing create clients, `ClientRequest::{AskSession, ReadSession}`, `App.session_roles` and `app.roles` (finding a live desk: role card `triage`, same repo), `SessionContexts` (the 60 percent rule), `triage_strip::{rows, open, handle_key, draw}` signatures, `NoteArgs`, `api.{triageSubmit, triageTail}`, `focusTriage()`, the `triage_note` verb.

## Interfaces produced

- `studio::inbox_write::{NewNote { text, to, about, priority, by }, Written { id, path }, Duplicate { existing: PathBuf, reason }, write(dir, &NewNote, now) -> Result<Written, Duplicate>, slug(text, words: usize) -> String, title_key(title) -> String}`: id `YYYY-MM-DD-<slug of the first 6 words>` (a `-2` suffix only when a resolved note holds the id), frontmatter per spec section 4.5 with `status: new` and `to: triage` unless given, written tmp-then-rename.
- The flow (spec section 10): write, then one labelled turn to the desk (a live `triage` role session in the repo's root worktree, else a new one named "triage desk" when the roles file defines `triage`, else the orchestrator with `[triage]`); wait up to 120 s for the reply line; a desk at or over 60 percent context is replaced for the next note. The prompt asks the desk to set `to`, `about`, `priority`, `status: open` and answer in one line, and names the note path.
- CLI `kete studio note "<text>" [--project P] [--to R] [--about A] [--priority high|normal|low] [--triage] [--json]`; exit 3 on a duplicate, printing the existing note.
- TUI: chip mode `t` opens the one-line field on the carved row; Enter files and sends; Esc closes; the row shows the route line until the next key.
- Desktop `TriageStrip`: the field, the target chip (Triage desk or Orchestrator), the repo it files into (the orchestrator's last repo, or a picker over `app.studioRepos`), the route line, the last 3 notes filed as links; `focusTriage()`; `triage_submit(repo, text, target)` and `triage_tail(repo)` Tauri commands.

## Tests

- Unit (`inbox_write`): ids and slugs (punctuation, te reo macrons kept as letters, 6-word cut); a duplicate by id and by the first 5 normalised title words; a resolved note does not block; the written file parses back through `inbox::parse` with every field; the temp file never left behind on an error.
- Windows e2e (`e2e_windows/triage.rs`, every flow named `triage_*`): a repo with a manifest and a committed roles file defining `triage`; `kete studio note "storm sky bands at dusk" --triage --project <repo>` writes the note and starts the desk, whose first prompt (agent log) names the note path; the same line again exits 3 naming the first note; with no `triage` role the turn goes to the fake orchestrator labelled `[triage]`.
- TUI draw test of the strip row at 80x24 (open, typing, route shown); input parity for opening it.
- Desktop vitest for the target chip and the busy wording; desktop e2e (`e2e/triage.e2e.mjs`): type a line in the strip under a fake orchestrator, the note file appears in the fixture repo's inbox.

## Checks

`cargo fmt --all`; `cargo clippy -p nebula-core -p nebula-tui -p nebula --all-targets -- -D warnings`; `cargo test -p nebula-core`; `cargo test -p nebula-tui`; `KETE_E2E_SKIP=1 cargo test -p nebula`; `cargo test -p nebula --test e2e_windows -- triage_`; `cd desktop && npm run check && npm test`; `cd desktop/src-tauri && cargo clippy -- -D warnings`; `cd desktop && npm run e2e:prepare && node --test --test-concurrency=1 e2e/triage.e2e.mjs`.

## Stop condition

A green PR against `release/0.60`, its body per Lane rule 14, with the desk prompt text you settled on and a sample note file.
