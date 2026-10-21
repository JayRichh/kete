# Lane board: K4, the studio board

Branch `release/0.60-board`, worktree `C:\Code\bro\.claude\worktrees\lane-060-board`, from `origin/release/0.60` after the foundation, roles and ledger lanes merged. Opus lead, at most 2 Sonnet helpers. Wave 2. Read the plan's Lane rules first; they bind this brief.

Spec: section 8 (K4), decisions 11 and 12, sections 4.3 to 4.6.

## Goal

One page per studio repo, in both apps: a What needs you strip, then one column per department with its queue, lead, runners, last verdict, blocked-on-Jay count and today's spend, all read from the repo's files and the session data clients already hold. Nothing is copied or written.

## Files owned

- `crates/nebula-core/src/studio/board.rs` (placeholder from the foundation)
- `crates/nebula-tui/src/studio_board.rs`, `crates/nebula-tui/src/ui/studio_board.rs` (placeholders)
- `desktop/src/lib/studio/board/**` (`Board.svelte` placeholder; any new components and `board.svelte.ts`)
- `desktop/src-tauri/src/studio_board.rs` (placeholder)
- `desktop/e2e/studio.e2e.mjs` (placeholder)
- `docs/studio-board.md` (new)

## Files linked (read only)

`crates/nebula-core/src/studio/{manifest,inbox,verdicts,settings,refusals,wire}.rs`; `desktop/src/lib/studio/StudioView.svelte` (the foundation's page: repo picker, the queue lane's header, then your `Board`); `crates/nebula-tui/src/{issues,view_jobs,filter_list,list_hit}.rs`, `ui/rows.rs`; `desktop/src/lib/Board.svelte` (card and column styling to match), `desktop/src/lib/status.ts`; `C:\Code\wheako\Design\Production\DEPARTMENTS.md` and `Design\Studio\inbox` (real shapes for synthetic fixtures; read only).

## Interfaces consumed

`studio::manifest::{load, slug}`, `studio::inbox::{read_dir, InboxNote}`, `studio::verdicts::newest`, `studio::settings::for_repo`, `studio::refusals::read_last`, `<data dir>/orchestrator-renewals.jsonl` (last line, format in the renew brief), `App.session_roles` and `App.session_spend` (TUI), `app.roles` and `app.spend` (desktop), the session list and statuses both apps hold, `SessionContexts` (context against budget), `studio_queue_view::header_line` (TUI header), `gauntlet_view::open_on` (TUI) and `nav.go({ view: 'gauntlet', repo, item: '<kind>/<id>' })` (desktop), `studio_board::{open, handle_key, handle_mouse}` and `ui::studio_board::draw` signatures, `Overlay::StudioBoard(BoardView)`, `api.studioBoard`, the `studio_board` verb.

## Interfaces produced

- `studio::board::{Department { slug, label }, departments(markdown) -> Vec<Department>, Board { needs_you: Vec<NeedsYou>, columns: Vec<Column> }, Column { department, queue: Vec<InboxNote>, lead: Option<SessionCell>, runners: Vec<SessionCell>, last_verdict: Option<Verdict>, blocked: u32, spend: SpendSum }, SessionCell { agent, label, status, model, profile, context_percent_of_budget }, build(inputs: &BoardInputs, now) -> Board, BUDGET_AMBER = 60, BUDGET_RED = 90}`. Departments come from the manifest's `departments` file: the first markdown table, first column, one per row. Sessions with no department go in "Unassigned", last.
- Tauri `studio_board(repo)` returns the file-derived half (departments, notes, verdicts, refusals, the last renewal); the frontend joins live sessions, roles and spend from the store.
- Desktop `Board { repo: string }`: columns 240 px wide in a horizontal scroll, each with the cells in spec section 8; status dots with glyphs; budget amber at 60 percent and red at 90; a note opens in Kete's file view, a session opens the session, a verdict opens the gauntlet view on it; refresh every 10 s while visible and on window focus; an empty state naming `.kete/studio.toml` when the repo has none.
- TUI `BoardView`: one row per department (name, queue, lead dot, runners, last verdict, blocked, spend) that fits 80 columns; Enter expands a department to its notes and sessions; Enter on a session attaches it, on a note opens it in the file view, on a verdict calls `gauntlet_view::open_on`; background reads every 10 s while open; the header line comes from `studio_queue_view::header_line`.
- Menu rows on a department row and a session cell, state-aware (Open lead, Review, Spawn lead is not offered: the manager spawns), listed in the PR.

## Tests

- Unit (`studio::board`): departments from the real DEPARTMENTS.md table shape (12 rows, slugs as in spec 4.4), a missing file, a table with no rows, a second table ignored; `build` from fixtures: queue per slug, lead and runners by tier, Unassigned, blocked counts, the needs-you strip order (needs-you sessions, then notes to Jay, then refusals, then a failed renewal); budget colours at 59, 60, 89 and 90 percent.
- TUI draw tests at 80x24 and 120x40 (collapsed and expanded); input parity for row activation and expansion.
- Desktop vitest for the column model and the budget colour rule.
- Desktop e2e (`e2e/studio.e2e.mjs`): a fixture repo with a manifest, a DEPARTMENTS-style table and notes; the Studio page shows its columns and counts; clicking a note opens the file view.

## Checks

`cargo fmt --all`; `cargo clippy -p nebula-core -p nebula-tui --all-targets -- -D warnings`; `cargo test -p nebula-core`; `cargo test -p nebula-tui`; `cd desktop && npm run check && npm test`; `cd desktop/src-tauri && cargo clippy -- -D warnings`; `cd desktop && npm run e2e:prepare && node --test --test-concurrency=1 e2e/studio.e2e.mjs`.

## Stop condition

A green PR against `release/0.60`, its body per Lane rule 14, with the menu rows added and rejected. Screenshots at each width are the visual-qa lane's job.
