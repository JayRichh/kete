# Studio queue

<sub>[← README](../README.md) · [Studio mode](studio.md) · [Orchestrator](orchestrator.md) · [Commands](commands.md) · [Configuration](configuration.md) · [Keys](keys.md)</sub>

The queue is how a repo's inbox reaches the orchestrator. You write notes. When the orchestrator is idle and you are not typing to it, Kete tells it what is new, in one short note. Autonomy sets how far the studio may go on its own, and at A0 nothing new starts.

## What studio mode is

Studio mode lets a repo keep an inbox folder of notes that the [orchestrator](orchestrator.md) works through between your visits. The repo describes itself in `.kete/studio.toml`. You allow it on this machine. Kete never spawns work on its own: it only tells the orchestrator what is waiting. [Studio mode](studio.md) has the manifest and the note format.

## Turn it on

```sh
kete studio enable <repo> [--inbox DIR]
kete studio disable <repo>
```

`enable` writes an entry for the repo on this machine at autonomy A1 (Plan). The inbox is the manifest's `inbox` unless `--inbox` names a folder, relative to the repo root. `disable` removes the entry. `<repo>` is a project's name or id.

## Autonomy

| Level | Name | What Kete enforces |
|---|---|---|
| A0 | Hold | No idle notes. New orchestrator and sibling spawns into the repo are refused with "autonomy A0 (Hold) for <repo>". Sessions already running carry on, but each shell command they run is checked against the A1 list. |
| A1 | Plan | Idle notes go out. Sessions the orchestrator spawns in the repo, and role sessions, are denied `git push`, `gh pr`, `gh release`, `git tag`, `kete ship` and `kete ado pr`. |
| A2 | Deliver | Idle notes go out. Those sessions are denied `git tag`, `gh release`, `git push --tags`, `git push --follow-tags` and pushes of `refs/tags/`. |
| A3 | Release | Idle notes go out. Autonomy adds no denials. |

A1 is the default and nothing raises it for you. Force-push stays denied at every level.

Only you change the level. Run `kete studio autonomy <repo> A2` from your own shell, or use the control in the TUI or the desktop app. The daemon refuses the change from inside an agent session, and a file in the repo can never raise it. `kete studio autonomy <repo>` with no level prints the current one.

### What a level change reaches

A0 stops idle notes and refuses new spawns at once. Sessions already running keep going. Their deny rules are the ones they started with, but `kete _guard` reads the repo's live level on every shell command, so a lowered level stops their pushes and tags from the next command. Stopping a running session stays your own action.

## The idle note

Kete writes an idle note to the orchestrator only when all four conditions hold:

| # | Condition |
|---|---|
| 1 | The repo is at A1 or higher, and its inbox has eligible notes not yet announced. |
| 2 | The orchestrator is finished or fresh, and has been unchanged for `studio_idle_after`. |
| 3 | Nobody has typed to the orchestrator for 120 seconds. |
| 4 | Fewer than 6 idle notes went out in the last 60 minutes. |

`studio_idle_after` is a General setting in the Studio group. The choices are `30s`, `45s`, `60s` and `120s`. The default is `45s`.

A note is at most 1,200 bytes and lists at most 5 inbox notes, highest priority first. When more are waiting, the note says "and N more" and the rest go in later notes. When the manifest lists `recipients`, only notes addressed to one of them are announced; without the list, every addressed note is announced except those to `jay`, which are yours.

```
[from kete studio] 14:05 · wheako A2 · 2 new inbox notes
1. high · world-and-weather · notes/inbox/2026-10-21-storm-sky.md · Storm sky banding at dusk
2. normal · orchestrator · notes/inbox/2026-10-21-ledger.md · Shift ledger
Accounts: max2 5h 34% (resets 3:10 pm); jayrich reserved.
Act within A2 (Deliver). Set each note's status when you take it.
```

### Announced once

Kete announces a note once for each modification time of its file. Editing a note announces it again. An open note you leave alone is never repeated. A daemon restart repeats nothing, because the announced notes are kept on disk.

### The context nudge

When the orchestrator's context reaches 60 percent of its role card's context budget, or of its model's window when it has no card, the note ends with a request to write a handover note and run `kete orchestrator renew --from <note>`. That reminder goes out alone, under the same four conditions, at most once every 30 minutes.

### Leave a note out

```sh
kete studio exclude <repo> <id-or-about>
kete studio include <repo> <id-or-about>
```

`exclude` leaves out one note by its id, or every note whose `about` matches. `include` hands it back.

## See the queue

```sh
kete studio status [--json]
kete studio queue [--project P] [--now] [--json]
```

`status` shows, for each studio repo, the autonomy level, the inbox, the open notes and the notes for you, the next notes, why the runner is holding and when the last idle note went out.

`queue` shows what the runner would send now. `--now` sends it if all four conditions hold. When one does not, it prints the reason from the table below and sends nothing.

## The controls

| Where | How |
|---|---|
| TUI | The studio board's header shows the level and the runner's state, for example `A1 Plan · idle note at 14:05` or `A1 Plan · holding: you typed 40 s ago`. On the board, `]` or `+` raises one level and `[` or `-` lowers one. The repo row's **Autonomy…** menu row opens the control and prints these keys. |
| Desktop | The board's header has four segments, A0 to A3, each with its name. A **Send now** button runs the same check as `kete studio queue --now`. |

Lowering applies at once. Raising to A1 applies at once. Raising to A2 or A3 asks first. In the TUI the header reads `Raise to A2 Deliver? y confirm · n cancel`: `y` or Enter confirms, `n` or Esc cancels, and any other key is taken and does nothing. In the desktop app the control asks in place.

## Why the runner is holding

When the runner would not send, the header and `kete studio queue` give one of these reasons.

| Reason | What it means |
|---|---|
| studio is off | The repo has no entry on this machine. Run `kete studio enable <repo>`. |
| autonomy A0 (Hold) | The repo is at A0. Raise it to send idle notes. |
| no orchestrator is running | There is nobody to tell. Start one. |
| the orchestrator is working | Its turn is running. The note waits for it to finish. |
| the orchestrator needs you | It is waiting on a question for you. Answer it first. |
| you typed 40 s ago | You typed to the orchestrator less than 120 seconds ago. The number is how long ago. |
| the orchestrator went idle 20 s ago | It finished less than `studio_idle_after` ago. The number is how long it has been idle. |
| 6 idle notes in the last hour | The hourly limit is reached. |
| nothing new | Every eligible note has been announced at its current modification time. |

When nothing holds, the TUI reads `A1 Plan · ready: 3 new notes` until the next note goes out.

## Where the state lives

| Where | What it holds |
|---|---|
| `config.local.json`, key `studio` | One entry per repo: its inbox, its autonomy and the notes to leave out. Machine-local: never exported, never sent over `kete ssh`. |
| `<data dir>/studio/queue-state.json` | The notes already announced, when the last idle note went out (overall and for each repo) and the times of the idle notes in the last hour, and when the last context nudge went out. It survives a restart. |
| `<data dir>/studio/refusals.jsonl` | One line for each spawn refused at A0, and for each refused unattended launch. |

See [Configuration](configuration.md#the-studio-map) for the `studio` map.
