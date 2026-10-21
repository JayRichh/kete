# Studio board

<sub>[← README](../README.md) · [Studio mode](studio.md) · [Gauntlet](gauntlet.md) · [Roles](roles.md) · [Usage ledger](usage-ledger.md) · [Keys](keys.md) · [Commands](commands.md)</sub>

The studio board is one page for a studio repo. It has one column for each department. A column shows the department's queue of notes, its lead, its runners, its last gauntlet verdict, what is waiting on you and what it cost today. Above the columns sits a strip called What needs you.

The board writes nothing. It reads the repo's files and the sessions Kete already holds, and it keeps no copy of either.

The board shows for a repo in [studio mode](studio.md). In the desktop app it is the Studio entry in the rail. In the TUI it is the `studio_board` action. The action has no key by default. It sits on the Repo and Global menus and in the command palette. Bind it in [Keys](keys.md) if you want one.

## What a column shows

| Cell | Source | Rule |
|---|---|---|
| Name | The manifest's `departments` file | The first table, first column. See below. |
| Queue count and top notes | The inbox folder | Open notes whose `to`, turned into a slug, is the department's slug. Open means `new` or `open`. High priority first, then oldest `at` first. A note with no `at` comes after the dated ones. |
| Lead | Live sessions and their [role cards](roles.md) | A session whose card names this department and has tier 1 or 2. The lowest tier wins. On a tie the first by name wins. |
| Runners | Live sessions and their role cards | Every other session in the department, by tier, then by name. |
| Last verdict | The manifest's `signoffs` folder and `[kinds]` map | The newest verdict over the folders that `[kinds]` maps to this department's slug. Several folders may map to one department. |
| Blocked on Jay | The inbox and live sessions | Open notes `to: jay` whose `about` is this department, plus the department's sessions that need feedback. |
| Today's spend | The [usage ledger](usage-ledger.md) | Input plus output tokens, and the estimated cost, of the lead and the runners for the current local day. |
| Context | A session's context window and its role card | Tokens now, as a percent of the card's `context_budget`. Shown only when the card has a budget and Kete has a reading. |

The source of each rule is `crates/nebula-core/src/studio/board.rs`. The file half is read by `read_files_in` (`board.rs:141`). The join with live sessions is `build` (`board.rs:367`).

## Where the inbox comes from

The inbox folder is the `studio_inbox` of this machine's `studio` entry for the repo. When the entry has none, it is the manifest's `inbox` (`board.rs:164`). Only the notes with status `new` or `open` are read. A note with status `done`, `closed`, `resolved` or `rejected` is not on the board. See [Inbox notes](studio.md#inbox-notes) for the file format and its limits.

## The departments table

The manifest's `departments` key names a markdown file. The board reads its first table (`board.rs:54`). Each row is one department. The department is the first column. A second table in the file is ignored.

The label is cleaned before it is shown (`board.rs:88`):

- Markdown emphasis is dropped: `**Art and 3D**` becomes `Art and 3D`.
- Backticks are dropped.
- A trailing parenthetical is dropped: `Animation (sub-department of Art and 3D)` becomes `Animation`.

The slug comes from the cleaned label. It is the label in lower case, with each run of other characters turned into one `-`.

| Cell in the table | Label | Slug |
|---|---|---|
| `Audio` | Audio | `audio` |
| `**Art and 3D**` | Art and 3D | `art-and-3d` |
| `Animation (sub-department of Art and 3D)` | Animation | `animation` |
| `Web, community, release` | Web, community, release | `web-community-release` |

A row with an empty first cell is skipped. So is a row whose slug was seen already. The columns follow the table's order. The file is read up to 262,144 bytes. A missing or larger file gives no departments.

A note is `to` a department when its `to` value, turned into a slug, equals the department's slug. `to: Art and 3D` and `to: art-and-3d` reach the same column.

## The Unassigned column

A live session goes in Unassigned when its role card names no department, when it has no role card, or when the department it names is not in the table. The column comes last. It shows only when at least one session lands in it.

Unassigned has no queue and no lead. Every session in it is a runner.

## Lead and runners

A department has at most one lead. The lead is the session with tier 1 or 2 that has the lowest tier. On a tie the first by name wins. Every other session in the department is a runner, ordered by tier and then by name. A session with no tier comes last.

Each session is one cell: a status dot, the role's label (the session's name when it has no card), its model and account, and its context against its budget.

## Budget colours

The context figure turns colour against the role card's `context_budget` (`board.rs:30`).

| Context of budget | Colour |
|---|---|
| Under 60 percent | Normal |
| 60 to 89 percent | Amber |
| 90 percent and over | Red |

## What needs you

The strip lists everything that waits on you, in this order (`board.rs:487`):

| Order | Entry | Source |
|---|---|---|
| 1 | Sessions that need feedback: a prompt or a question | Live sessions in this repo |
| 2 | Open notes `to: jay` | The inbox |
| 3 | Refusals: an unattended launch or an A0 spawn that Kete refused | `<data dir>/studio/refusals.jsonl`, this repo's lines, newest first, at most 20 |
| 4 | A failed orchestrator renewal | The last line of `<data dir>/orchestrator-renewals.jsonl`, shown only when its state is a failure |

A note to Jay appears on the strip. It also counts in the blocked cell of the department its `about` names.

## Refresh

The board reads the repo's files fresh each time. It reads again:

| Where | When |
|---|---|
| Desktop | Every 10 seconds while the page is visible, and when the window takes focus. |
| TUI | Every 10 seconds while the board is open. |

Sessions, roles and spend are not read again. Both apps already hold them and update them as they change.

## In the desktop app

Open Studio in the rail. Pick the repo at the top of the page when more than one repo has studio mode. The columns are 240 px wide. The row scrolls sideways when there are more departments than fit.

| Click | What happens |
|---|---|
| A note | Opens the note in the file view. |
| A session | Opens the session. |
| A verdict | Opens the [gauntlet](gauntlet.md) view on that verdict. |

Right-click for a menu.

| On | Row | What it does |
|---|---|---|
| A column header | Open lead | Opens the department's lead session. |
| A column header | Review last verdict | Opens the gauntlet view on the department's last verdict. |
| A column header | Open top note | Opens the first note in the queue in the file view. |
| A session | The usual session menu | The same rows as on a session anywhere else in the app. |

A repo with studio mode and no `.kete/studio.toml` shows an empty state that names the file. The board has no columns until the manifest names a `departments` file with a table in it.

## In the TUI

Run the `studio_board` action. It opens as an overlay over the screen. It shows one row for each department. A row holds the name, the queue count, the lead's dot, the runners, the last verdict, the blocked count and today's spend. It fits 80 columns.

| Key | What it does |
|---|---|
| `↑` `↓`, `j` `k`, `PgUp` `PgDn`, `Home` `End` | Move between rows. |
| `Enter` on a department | Expands it to its notes (up to 10), sessions and last verdict. Again collapses it. |
| `Enter` on What needs you | Expands the strip to its entries. |
| `Enter` on a session | Attaches it. |
| `Enter` on a note | Opens it in the file view. |
| `Enter` on a verdict | Opens the gauntlet view on it. |
| `l` | Attaches the lead of the department under the cursor. |
| `v` | Opens the gauntlet view on that department's last verdict. |
| `n` | Opens that department's top note. |
| `m`, or right-click | Opens the row menu. |
| `r` | Reads the files again now. |
| `Esc`, `q` | Closes the board. |

A click on a row is `Enter` on it. The row menu lists only what applies to the row now:

| Row | Menu rows |
|---|---|
| A department | Expand or Collapse; Open lead, when it has one; Review last verdict, when it has one; Open top note, when its queue has notes; Reload |
| A session | Open session; Open lead, unless it is the lead; Review last verdict; Open top note; Reload |
| A note | Open note; Open lead; Review last verdict; Reload |
| A verdict | Review verdict; Open lead; Open top note; Reload |
| What needs you | Expand or Collapse, or the entry's own row (Open session, Open note, Show reason); Reload |

Spawn lead is not offered anywhere. The manager spawns.

## More

- [Studio mode](studio.md): the manifest, the inbox, autonomy and the files Kete keeps.
- [Gauntlet](gauntlet.md): every verdict by kind, with its evidence.
- [Roles](roles.md): the role cards whose department, tier and `context_budget` place a session on the board.
- [Usage ledger](usage-ledger.md): where today's tokens and estimated cost come from.
