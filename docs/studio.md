# Studio mode

<sub>[← README](../README.md) · [Keys](keys.md) · [Commands](commands.md) · [Sessions](sessions.md) · [Configuration](configuration.md) · [How it works](how-it-works.md)</sub>

Studio mode lets a repo keep a queue of notes that the [orchestrator](orchestrator.md) works through between your visits. You write notes into an inbox folder in the repo. While the orchestrator sits idle, Kete hands it a short `[from kete studio]` note that names the new inbox notes and says which accounts have room. The orchestrator decides what to do with them. Kete never spawns work on its own.

Two things switch it on:

- The repo describes itself in `.kete/studio.toml`, a file you commit. It names the inbox folder, the departments and the folders of verdict files.
- You allow it on this machine with `kete studio enable <repo>`. That writes an entry for the repo in `config.local.json`, at autonomy A1.

A repo with a manifest and no entry on this machine is not in studio mode. The manifest says what the repo is. Only the entry on your machine says what Kete may do with it.

```sh
kete studio enable my-game               # inbox from the manifest, autonomy A1 (Plan)
kete studio status                       # every studio repo: level, inbox, counts, queue state
kete studio autonomy my-game A2          # raise it; only you do this, never a session
kete studio queue --project my-game     # what the runner would send now
```

## When Kete sends an idle note

A note goes to the orchestrator only when all of these hold:

- A repo has studio mode on at A1 or higher, and its inbox holds eligible notes that have not been announced yet.
- The orchestrator is finished or fresh, and has been for at least `studio_idle_after` (45 seconds unless you change it, see [Configuration](configuration.md#every-setting)).
- You have not typed to the orchestrator for 120 seconds.
- Fewer than 6 idle notes went out in the last 60 minutes.

A note names at most 5 inbox notes, highest priority first, and is at most 1,200 bytes:

```
[from kete studio] 14:05 · my-game A2 · 3 new inbox notes
1. high · art-and-3d · notes/inbox/2026-10-21-storm-sky.md · Storm sky banding at dusk
2. normal · orchestrator · notes/inbox/2026-10-21-ledger.md · Shift ledger
Accounts: max2 5h 34% (resets 3:10 pm); home reserved.
Act within A2 (Deliver). Set each note's status when you take it.
```

Kete announces a note once for each time the file changes. Editing a note announces it again. An open note you leave alone is not repeated. A restart of the daemon does not repeat a note either.

When the orchestrator's context reaches 60 percent of its role card's `context_budget` (or of its model's window when it has no card), the note ends with a request to write a handover note and run `kete orchestrator renew --from <note>`. That reminder goes out alone, and at most once every 30 minutes.

## `.kete/studio.toml`

The manifest sits at `.kete/studio.toml` in the repo root. Commit it. Every key is optional except `version`.

```toml
version = 1
inbox = "notes/inbox"                      # repo-relative; the inbox `kete studio enable` uses
departments = "docs/DEPARTMENTS.md"        # first markdown table, first column, one department per row
signoffs = "reviews/signoffs"              # <signoffs>/<kind>/<id>.json verdict files
evidence = "reviews/evidence"              # folders a verdict's `evidence` field names
studio_url = "http://127.0.0.1:5173"       # a local review app the pages link into
recipients = ["orchestrator", "manager"]   # `to:` values announced to the orchestrator

[kinds]   # verdict folder -> department slug, for the board's last-gauntlet cell
art = "art-and-3d"
sound = "audio"

[links]   # verdict folder -> route on studio_url; {id} is replaced
art = "/art?id={id}"
sound = "/sound?id={id}"
```

| Key | What it is |
|---|---|
| `version` | Required. `1`. |
| `inbox` | The inbox folder, relative to the repo root. `kete studio enable` uses it unless you pass `--inbox`. |
| `departments` | A markdown file. The first table in it is read, one department per row, taken from its first column. |
| `signoffs` | The folder of verdict files, one folder per kind: `<signoffs>/<kind>/<id>.json`. The [gauntlet](gauntlet.md) reads it. |
| `evidence` | The folder holding the evidence a verdict names. |
| `studio_url` | The address of a local review app. The pages link into it. Without it there are no Open in Studio buttons. |
| `recipients` | The `to:` values announced to the orchestrator. |
| `[kinds]` | Maps a verdict folder to a department slug, so the [board](studio-board.md) can show each department's last verdict. |
| `[links]` | Maps a verdict folder to a route on `studio_url`. `{id}` becomes the verdict's id. |

The file is read up to 64 KiB (65,536 bytes). A larger file is refused. A parse error names the line.

A department slug is the label in lower case, with each run of other characters turned into one `-`. "Art and 3D" is `art-and-3d`. "Web, community, release" is `web-community-release`.

## Inbox notes

A note is a Markdown file in the inbox with a short frontmatter block. The body is free text.

```
---
id: 2026-10-21-storm-sky-banding
status: open
to: orchestrator
about: World and weather
priority: high
title: Storm sky banding at dusk
at: 2026-10-21T05:52:13Z
by: jay
---
Body text.
```

| Key | Meaning | Default |
|---|---|---|
| `id` | The note's name everywhere else. | The file name without `.md`. |
| `status` | `new`, `open`, `resolved`, `done`, `closed` or `rejected`. | `new` |
| `to` | A recipient, a department slug, or `jay`. | none |
| `about` | What the note is about. `kete studio exclude` can match it. | none |
| `priority` | `high`, `normal` or `low`. | `normal` |
| `title` | The line shown in lists and idle notes. | The first heading, then the first line of the body, then the id. |
| `at` | When it was filed, in UTC. | none |
| `by` | Who filed it. | none |

Kete reads these limits:

- Only the first 4,096 bytes of a note are read. Keep the frontmatter at the top.
- A folder lists at most 2,000 notes.
- A file over 1 MiB is skipped.

An idle note names a note when its status is `new` or `open`, it has a `to`, and it is not excluded. They are ordered by priority, high first, then oldest first. `kete studio exclude <repo> <id-or-about>` leaves a note, or every note with that `about`, out of the queue. `kete studio include` hands it back. To file a note from the shell, run `kete studio note "<text>"`, see [Triage](triage.md).

## Autonomy

Autonomy is how much the studio may do in a repo. There are four levels. A1 is the default.

| Level | Name | What Kete enforces |
|---|---|---|
| A0 | Hold | No idle notes. New orchestrator and sibling spawns into the repo are refused with "autonomy A0 (Hold) for <repo>". Sessions already running carry on, but every shell command they run is checked against the A1 list, so their pushes and tags stop. |
| A1 | Plan | Idle notes go out. Sessions the orchestrator spawns in the repo, and role sessions, are denied `git push`, `gh pr`, `gh release`, `git tag`, `kete ship` and `kete ado pr` (and the same through `bro`). |
| A2 | Deliver | Idle notes go out. Those sessions are denied `git tag`, `gh release`, `git push --tags`, `git push --follow-tags` and pushes of `refs/tags/`. |
| A3 | Release | Idle notes go out. Autonomy adds no denials. |

These apply at every level: no force-push in any form, and no edits to Kete's own data and config folders. With a [roles file](roles.md), its governance paths are protected too.

Sessions you start yourself without a role are never restricted by autonomy.

Only you change the level. Run `kete studio autonomy <repo> A2` from your own shell, or use the autonomy control in the apps. The daemon refuses the change from a client running inside an agent session, and a file in the repo can never raise it. A level change reaches new spawns at once. It does not stop a session that is already running. Stopping one stays your own action.

## Files Kete keeps

Studio mode keeps its state on this machine. Nothing here is exported or sent over `kete ssh`.

| Where | What it holds |
|---|---|
| `config.local.json`, key `studio` | One entry per repo: its inbox, its autonomy level and the notes to leave alone. Written only by `kete studio` and the apps. See [Configuration](configuration.md#the-studio-map). |
| `<data dir>/studio/queue-state.json` | The notes already announced and when the last idle note went out, so a restart never repeats one. |
| `<data dir>/studio/refusals.jsonl` | One line for each refused unattended launch and each spawn refused at A0. The board's What needs you strip shows the last 20. It stays under 1 MiB: past that the oldest half is dropped. |
| `<data dir>/sessions/<agent id>/claude-settings.json` | The settings file generated for a role session: its status line, its permission rules and the guard hook. Removed with the session. |
| `<data dir>/orchestrator-renewals.jsonl` | One line for each orchestrator renewal: when, the old session, the new one, the note and the result. |
| `<data dir>/profiles/.picks.json` | The 10-minute holds that `kete profile pick` records, so back-to-back picks spread over your accounts. |
| `nebula.db`, `agents.role_card` | The role card a session launched with, so a resume or restart applies the same card. |
| `nebula.db`, `session_spend` | The [usage ledger](usage-ledger.md): one row for each session for each local day. |

## More

- [Studio queue](studio-queue.md): the idle notes, the autonomy control and the queue runner's state.
- [Studio board](studio-board.md): one page with each department's queue, lead, runners and last verdict.
- [State map](state-map.md): what the repo holds and who is on it, as a tree, with the slice an agent reads.
- [Gauntlet](gauntlet.md): every verdict by kind, with its evidence.
- [Triage](triage.md): file a note from one line and have the triage desk route it.
- [Roles](roles.md): role cards, permission profiles and the guard.
- [Usage ledger](usage-ledger.md): tokens, estimated cost and turns per session per day.
- [Profiles](profiles.md): accounts, reserved accounts and scripted picks.
- [Orchestrator](orchestrator.md): the session that works the queue, and renewal.
