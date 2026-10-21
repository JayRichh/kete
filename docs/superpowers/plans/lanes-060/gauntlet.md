# Lane gauntlet: K5, the gauntlet view

Branch `release/0.60-gauntlet`, worktree `C:\Code\bro\.claude\worktrees\lane-060-gauntlet`, from `origin/release/0.60` after the foundation merged. Opus lead, at most 2 Sonnet helpers. Wave 2 (the first wave 2 lane to start; it waits on nothing but the foundation). Read the plan's Lane rules first; they bind this brief.

Spec: section 9 (K5), decisions 11 and 12, sections 4.4 and 4.6, section 16 question 6.

## Goal

A read-only view of every verdict file in a repo, by kind, newest first, with its note, tags, reconciled file and evidence one click away, and a link that opens the repo's review app on that verdict. No write actions this train.

## Files owned

- `crates/nebula-core/src/studio/gauntlet.rs` (placeholder from the foundation)
- `crates/nebula-tui/src/gauntlet_view.rs`, `crates/nebula-tui/src/ui/gauntlet_view.rs` (placeholders)
- `desktop/src/lib/gauntlet/**` (`GauntletView.svelte` placeholder; any new components and `gauntlet.svelte.ts`)
- `desktop/src-tauri/src/gauntlet.rs` (placeholder)
- `desktop/e2e/gauntlet.e2e.mjs` (placeholder)
- `docs/gauntlet.md` (new)

## Files linked (read only)

`crates/nebula-core/src/studio/{manifest,verdicts}.rs`; `crates/nebula-tui/src/issues.rs` (the reference list modal), `view_jobs.rs`, `filter_list.rs`, `ui/rows.rs`; `desktop/src-tauri/src/editors.rs` (`checkout`, `inside`, `blocking`) and `tree.rs` (reading a checkout from the backend); `desktop/src/lib/files/` (Open in Kete); `desktop/e2e/harness.mjs`; `C:\Code\wheako\Design\Reviews\signoffs` (real verdict shapes to copy into synthetic fixtures; read only, never edit).

## Interfaces consumed

`studio::manifest::{load, StudioManifest}`, `studio::verdicts::{Verdict, kinds, read_kind}`, `Overlay::Gauntlet(GauntletView)`, the placeholder signatures `gauntlet_view::{open, open_on, handle_key, handle_mouse}` and `ui::gauntlet_view::draw`, View `'gauntlet'` with `place.item` = `<kind>/<id>`, `api.{gauntletList, gauntletEvidence, gauntletOpenStudio}`, the `gauntlet_view` verb.

## Interfaces produced

- `studio::gauntlet::{KindVerdicts { kind, department: Option<String>, verdicts: Vec<Verdict> }, Tone { Done, Failed, NeedsYou, Plain }, tone(word) -> Tone, studio_link(manifest, kind, id) -> Option<String>, list(repo_root, manifest) -> Vec<KindVerdicts>, evidence(repo_root, manifest, ev_id) -> Vec<EvidenceEntry { name, path, is_dir, bytes }>}` (at most 200 entries; paths kept inside the repo).
- Tauri `gauntlet_list(repo)`, `gauntlet_evidence(repo, ev)`, `gauntlet_open_studio(repo, kind, id)` (opens the link with the system browser; refuses anything but `http://127.0.0.1` and `http://localhost` origins).
- Desktop `GauntletView`: kind tabs, filters by verdict word and age (24 h, 7 days, all), rows (id, verdict word with its tone and glyph, by, at, note's first line), a detail pane (note, tags, reconciled file, evidence listing with Open in Kete and Show in Explorer), Open in Studio when `studio_url` is set. `place.item` selects a verdict.
- TUI `GauntletView`: kind tabs (`Tab`, `⇧Tab`), rows, Enter for the detail, `o` opens the Studio link, `/` filters; `open_on(app, repo, kind, id)` selects one (the board lane calls it).
- Verb rows: none new beyond the foundation's `gauntlet_view`; a verdict row's own menu (Open in Studio, Copy id, Copy path, Show evidence) is state-aware (no Studio row without `studio_url`, no evidence row without `evidence`).

## Tests

- Unit (`studio::gauntlet`): tone for every listed word and an unknown one; `studio_link` with and without `{id}`, with no `studio_url`, with a kind that has no link; `list` groups a reconciled pair once; `evidence` refuses a `..` path.
- TUI draw tests at 80x24 and 120x40 (tabs, rows, detail); input parity for row activation (key and click).
- Desktop vitest for filters and the tone glyphs.
- Desktop e2e (`e2e/gauntlet.e2e.mjs`): a fixture repo with `.kete/studio.toml`, two kinds, a reconciled pair and an evidence folder; the view lists both kinds, the filter narrows, the detail shows the evidence entries.

## Checks

`cargo fmt --all`; `cargo clippy -p nebula-core -p nebula-tui --all-targets -- -D warnings`; `cargo test -p nebula-core`; `cargo test -p nebula-tui`; `cd desktop && npm run check && npm test`; `cd desktop/src-tauri && cargo clippy -- -D warnings`; `cd desktop && npm run e2e:prepare && node --test --test-concurrency=1 e2e/gauntlet.e2e.mjs`.

## Stop condition

A green PR against `release/0.60`, its body per Lane rule 14, with the menu rows added and rejected, and a note of the write actions left for question 6.
