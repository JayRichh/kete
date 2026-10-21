# Lane visual-qa: every new surface, last

Branch `release/0.60-visual-qa`, worktree `C:\Code\bro\.claude\worktrees\lane-060-visual-qa`, from `origin/release/0.60` after every other lane merged. Opus lead, at most 2 Sonnet helpers. Wave 3, alone. Read the plan's Lane rules first; they bind this brief.

Spec: sections 5 to 13 (the TUI and Desktop paragraphs), PRODUCT.md (principles, accessibility), and the Workshop styling rules in Lane rule 10.

## Goal

Every surface 0.60 added looks and behaves like the rest of Kete at every supported size, in light and dark, by keyboard and by mouse, and nothing 0.59 had regressed. Fix what is wrong; list what was checked.

## Files owned

Any file the 0.60 train added or changed, for fixes only, each listed in the PR, plus a screenshot helper in `desktop/e2e/harness.mjs` (W3C Take Screenshot) if it has none. No new features, no protocol or schema changes. Screenshots go to `.superpowers/sdd/2026-10-21-release-060/visual/` (machine-local ledger, not committed).

## Files linked (read only)

The spec; every lane's PR body (rows added and rejected, keys bound); `desktop/e2e/harness.mjs` and `webdriver.mjs`; `crates/nebula-tui/src/event_loop/tests/visual.rs` (`SIZES`, `draw`, `shows`, `frames_close`).

## Interfaces consumed

Everything the lanes produced; nothing new is produced.

## What to check

- **Desktop**, at 640, 900 and 1280 px wide, light and dark, through the e2e harness (WebDriver only, Lane rule 9): the Studio page (repo picker, queue header with the autonomy control, the needs-you strip, columns, scroll), the Gauntlet view (tabs, filters, detail, evidence), the triage strip under the orchestrator, the renew form and notice, role chips on rows, cards and the session header, the role picker in the quick prompt and Start orchestrator, Reserve for me in Accounts, the spend line in session details, the six new verbs in the palette and `?` menu, the rail entries (hidden with no studio repo).
- **TUI**, at 80x24 and 120x40, both themes: the board overlay (collapsed and expanded), the gauntlet overlay, the triage row in the orchestrator view, the renew prompt and flashes, the role field and chip, the reserved account row; draw tests added to `event_loop/tests/visual.rs` for each.
- **Rules**: a visible affordance and a tooltip naming its key for every control; focus order and Esc; status never by colour alone; ember only for needs you; no em dashes in copy; menus state-aware (each lane's rows exercised in both states).
- **Regression**: the 0.59 visual tests still pass; the orchestrator view, the quick prompt and the accounts view look as they did when no studio repo exists.

## Tests

The new TUI draw tests above; any desktop vitest a fix needs; the desktop e2e suite once.

## Checks

`cargo fmt --all`; `cargo clippy -p nebula-tui --all-targets -- -D warnings` (and any other crate a fix touched); `cargo test -p nebula-tui`; `cd desktop && npm run check && npm test`; `cd desktop && npm run e2e`.

## Stop condition

A green PR against `release/0.60`, its body per Lane rule 14, with a table of every surface by size and theme (checked, fixed, or left with a reason) and the screenshot folder's path.
